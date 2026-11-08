use std::{thread, time::Duration};

use anyhow::{Context, Result, bail};
use reqwest::blocking::Client;
use scraper::{Html, Selector};

use crate::{
    Args, build_client, internet_ok, login,
    night::{Night, Tick},
    portal_online, reward,
};

const STATUS_URL: &str = "https://login.net.vn/status";
const PROBE_TIMEOUT: Duration = Duration::from_secs(8);
const DEFAULT_INTERVAL: u64 = 60;
const DEFAULT_RETRY_AFTER: u64 = 10;
const DEFAULT_MAX_RETRY_AFTER: u64 = 300;

enum Portal {
    Unreachable(String),
    LoggedIn(String),
    LoggedOut,
}

struct Backoff {
    base: Duration,
    cap: Duration,
    failures: u32,
}

impl Backoff {
    fn new(base: Duration, cap: Duration) -> Self {
        Self {
            base,
            cap: cap.max(base),
            failures: 0,
        }
    }

    fn next_delay(&mut self) -> Duration {
        let delay = backoff_delay(self.base, self.cap, self.failures, jitter_fraction());
        self.failures = self.failures.saturating_add(1);
        delay
    }

    fn reset(&mut self) {
        self.failures = 0;
    }
}

#[derive(Default)]
struct Rotation {
    current: Option<String>,
}

impl Rotation {
    fn order(&self, mut cards: Vec<(String, String)>) -> Vec<(String, String)> {
        let start = self
            .current
            .as_ref()
            .and_then(|current| cards.iter().position(|(pin, _)| pin == current))
            .unwrap_or(0);
        cards.rotate_left(start);
        cards
    }
}

pub fn run(args: &Args) -> Result<()> {
    let mut night = args.night_pause.map(Night::new).transpose()?;
    let interval = Duration::from_secs(args.interval.unwrap_or(DEFAULT_INTERVAL));
    let mut backoff = Backoff::new(
        Duration::from_secs(args.retry_after.unwrap_or(DEFAULT_RETRY_AFTER)),
        Duration::from_secs(args.max_retry_after.unwrap_or(DEFAULT_MAX_RETRY_AFTER)),
    );
    let client = build_client(args)?;
    let mut rotation = Rotation::default();
    let _ = reward::fetch_active_cards(None);
    println!(
        "Auto login running, checking every {}s. Press Ctrl+C to stop.",
        interval.as_secs()
    );
    loop {
        if let Some(night) = night.as_mut() {
            match night.tick(rotation.current.as_deref()) {
                Tick::Proceed => {}
                Tick::Paused(remaining) => {
                    thread::sleep(remaining.min(interval));
                    continue;
                }
                Tick::Failed(error) => {
                    let delay = backoff.next_delay();
                    eprintln!(
                        "Night pause check failed: {error:#}. Retrying in {}s...",
                        delay.as_secs()
                    );
                    thread::sleep(delay);
                    continue;
                }
            }
        }
        if internet_ok(&client) {
            backoff.reset();
            thread::sleep(interval);
            continue;
        }
        println!("No internet, checking the portal...");
        match recover(args, &client, &mut rotation) {
            Ok(()) => {
                backoff.reset();
                println!(
                    "Auto login succeeded. Next check in {}s.",
                    interval.as_secs()
                );
                thread::sleep(interval);
            }
            Err(error) => {
                let delay = backoff.next_delay();
                eprintln!(
                    "Auto login failed: {error:#}. Retrying in {}s...",
                    delay.as_secs()
                );
                thread::sleep(delay);
            }
        }
    }
}

fn recover(args: &Args, client: &Client, rotation: &mut Rotation) -> Result<()> {
    match probe_portal(client) {
        Portal::Unreachable(reason) => {
            bail!("portal unreachable ({reason}), no link to the network")
        }
        Portal::LoggedIn(user) => {
            bail!("session {user} is still logged in, waiting for the upstream to come back")
        }
        Portal::LoggedOut => {
            println!("Logged out of the portal, trying to login...");
            login_with_rotation(args, rotation)
        }
    }
}

fn probe_portal(client: &Client) -> Portal {
    let page = client
        .get(STATUS_URL)
        .timeout(PROBE_TIMEOUT)
        .send()
        .and_then(|response| response.text());
    match page {
        Ok(page) => status_user(&page).map_or(Portal::LoggedOut, Portal::LoggedIn),
        Err(error) => Portal::Unreachable(error.to_string()),
    }
}

fn status_user(page: &str) -> Option<String> {
    let selector = Selector::parse(".txtUser").expect("static selector is valid");
    let document = Html::parse_document(page);
    let text = document
        .select(&selector)
        .next()?
        .text()
        .collect::<String>();
    let user = text.split_once(':')?.1.trim();
    (!user.is_empty()).then(|| user.to_owned())
}

fn login_with_rotation(args: &Args, rotation: &mut Rotation) -> Result<()> {
    let (cards, cached) = match reward::fetch_active_cards(None) {
        Ok(cards) => (cards, false),
        Err(error) => {
            eprintln!("Shop unreachable ({error:#}), using cached cards.");
            (reward::cached_cards()?, true)
        }
    };
    let mut last_error = None;
    for (pin, pass) in &rotation.order(cards) {
        let result = login(args, pin, pass).and_then(|()| {
            if args.no_logs && !portal_online(&build_client(args)?, &args.entry_url) {
                bail!("login was posted, but internet connectivity still failed");
            }
            Ok(())
        });
        match result {
            Ok(()) => {
                if rotation
                    .current
                    .as_ref()
                    .is_some_and(|current| current != pin)
                {
                    println!("Switched to card {pin}.");
                }
                rotation.current = Some(pin.clone());
                if cached {
                    let _ = reward::fetch_active_cards(None);
                }
                return Ok(());
            }
            Err(error) => {
                eprintln!("Card {pin} failed: {error:#}");
                last_error = Some(error);
            }
        }
    }
    Err(last_error.context("no card to try")?).context("every card failed")
}

fn backoff_delay(base: Duration, cap: Duration, failures: u32, fraction: f64) -> Duration {
    let ceiling = (base.as_secs_f64() * 2f64.powi(failures.min(30) as i32)).min(cap.as_secs_f64());
    Duration::from_secs_f64(ceiling * (0.5 + 0.5 * fraction))
}

fn jitter_fraction() -> f64 {
    let mut bytes = [0u8; 4];
    if getrandom::getrandom(&mut bytes).is_err() {
        return 1.0;
    }
    f64::from(u32::from_le_bytes(bytes)) / f64::from(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECOND: Duration = Duration::from_secs(1);

    #[test]
    fn backoff_doubles_then_caps() {
        let (base, cap) = (10 * SECOND, 100 * SECOND);
        let delays: Vec<u64> = (0..6)
            .map(|failures| backoff_delay(base, cap, failures, 1.0).as_secs())
            .collect();
        assert_eq!(delays, [10, 20, 40, 80, 100, 100]);
    }

    #[test]
    fn backoff_jitter_stays_within_half_to_full() {
        let (base, cap) = (10 * SECOND, 100 * SECOND);
        assert_eq!(backoff_delay(base, cap, 2, 0.0), 20 * SECOND);
        assert_eq!(backoff_delay(base, cap, 2, 0.5), 30 * SECOND);
        assert_eq!(backoff_delay(base, cap, 2, 1.0), 40 * SECOND);
    }

    #[test]
    fn backoff_survives_huge_failure_counts() {
        let delay = backoff_delay(10 * SECOND, 300 * SECOND, u32::MAX, 1.0);
        assert_eq!(delay, 300 * SECOND);
    }

    #[test]
    fn backoff_resets_after_success() {
        let mut backoff = Backoff::new(10 * SECOND, 300 * SECOND);
        for _ in 0..4 {
            backoff.next_delay();
        }
        backoff.reset();
        assert!(backoff.next_delay() <= 10 * SECOND);
    }

    #[test]
    fn cap_never_undercuts_base() {
        let mut backoff = Backoff::new(60 * SECOND, 5 * SECOND);
        assert!(backoff.next_delay() >= 30 * SECOND);
    }

    fn cards(pins: &[&str]) -> Vec<(String, String)> {
        pins.iter()
            .map(|pin| ((*pin).to_owned(), format!("pass-{pin}")))
            .collect()
    }

    fn pins(cards: &[(String, String)]) -> Vec<&str> {
        cards.iter().map(|(pin, _)| pin.as_str()).collect()
    }

    #[test]
    fn rotation_starts_at_current_card_and_wraps() {
        let rotation = Rotation {
            current: Some("B".to_owned()),
        };
        let ordered = rotation.order(cards(&["A", "B", "C"]));
        assert_eq!(pins(&ordered), ["B", "C", "A"]);
    }

    #[test]
    fn rotation_falls_back_to_first_card() {
        let ordered = Rotation::default().order(cards(&["A", "B"]));
        assert_eq!(pins(&ordered), ["A", "B"]);
        let gone = Rotation {
            current: Some("Z".to_owned()),
        };
        assert_eq!(pins(&gone.order(cards(&["A", "B"]))), ["A", "B"]);
    }

    #[test]
    fn status_page_reveals_logged_in_user() {
        let page = r#"<div><span class="txtUser">Xin Chào: WGBH1XRR</span></div>"#;
        assert_eq!(status_user(page).as_deref(), Some("WGBH1XRR"));
    }

    #[test]
    fn login_page_is_not_a_session() {
        assert_eq!(
            status_user("<html><form name=\"login\"></form></html>"),
            None
        );
        assert_eq!(
            status_user(r#"<span class="txtUser">Xin Chào: </span>"#),
            None
        );
    }
}
