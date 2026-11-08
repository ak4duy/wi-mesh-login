use std::{
    fs, io,
    path::PathBuf,
    str::FromStr,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow};
use time::{OffsetDateTime, UtcOffset};

use crate::reward;

const SECONDS_PER_DAY: u32 = 86_400;
const PAUSE_RETRY: Duration = Duration::from_secs(600);
const PAUSE_WEEKS: &str = "1";

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NightWindow {
    start: u32,
    end: u32,
}

impl NightWindow {
    fn contains(self, second: u32) -> bool {
        if self.start < self.end {
            (self.start..self.end).contains(&second)
        } else {
            second >= self.start || second < self.end
        }
    }

    fn seconds_until_end(self, second: u32) -> u32 {
        (self.end + SECONDS_PER_DAY - second) % SECONDS_PER_DAY
    }
}

fn parse_time(text: &str) -> Option<u32> {
    let (hours, minutes) = text.split_once(':')?;
    let hours: u32 = hours.parse().ok().filter(|hours| *hours < 24)?;
    let minutes: u32 = minutes.parse().ok().filter(|minutes| *minutes < 60)?;
    Some(hours * 3600 + minutes * 60)
}

impl FromStr for NightWindow {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let invalid = || format!("expected HH:MM-HH:MM with different times, got '{text}'");
        let (start, end) = text.split_once('-').ok_or_else(invalid)?;
        let start = parse_time(start).ok_or_else(invalid)?;
        let end = parse_time(end).ok_or_else(invalid)?;
        if start == end {
            return Err(invalid());
        }
        Ok(Self { start, end })
    }
}

pub enum Tick {
    Proceed,
    Paused(Duration),
    Failed(anyhow::Error),
}

pub struct Night {
    window: NightWindow,
    offset: UtcOffset,
    retry_pause_at: Option<Instant>,
}

impl Night {
    pub fn new(window: NightWindow) -> Result<Self> {
        let offset = UtcOffset::current_local_offset()
            .context("cannot determine the local time zone for --night-pause")?;
        Ok(Self {
            window,
            offset,
            retry_pause_at: None,
        })
    }

    fn second_of_day(&self) -> u32 {
        let now = OffsetDateTime::now_utc().to_offset(self.offset);
        u32::from(now.hour()) * 3600 + u32::from(now.minute()) * 60 + u32::from(now.second())
    }

    pub fn tick(&mut self, current_pin: Option<&str>) -> Tick {
        match self.step(current_pin) {
            Ok(tick) => tick,
            Err(error) => Tick::Failed(error),
        }
    }

    fn step(&mut self, current_pin: Option<&str>) -> Result<Tick> {
        let second = self.second_of_day();
        let held = load_paused()?;
        if !self.window.contains(second) {
            self.retry_pause_at = None;
            if !held.is_empty() {
                resume(&held)?;
            }
            return Ok(Tick::Proceed);
        }
        if !held.is_empty() {
            return Ok(Tick::Paused(self.until_end(second)));
        }
        if self
            .retry_pause_at
            .is_some_and(|retry_at| Instant::now() < retry_at)
        {
            return Ok(Tick::Proceed);
        }
        match pause(current_pin) {
            Ok(pin) => {
                println!("Night window started, card {pin} paused.");
                Ok(Tick::Paused(self.until_end(second)))
            }
            Err(error) => {
                eprintln!(
                    "Night pause failed: {error:#}. Staying online, retrying in {}s.",
                    PAUSE_RETRY.as_secs()
                );
                self.retry_pause_at = Some(Instant::now() + PAUSE_RETRY);
                Ok(Tick::Proceed)
            }
        }
    }

    fn until_end(&self, second: u32) -> Duration {
        Duration::from_secs(u64::from(self.window.seconds_until_end(second)))
    }
}

fn paused_file() -> Result<PathBuf> {
    Ok(reward::default_state_dir()?.join("night-paused"))
}

fn load_paused() -> Result<Vec<String>> {
    match fs::read_to_string(paused_file()?) {
        Ok(text) => Ok(text.lines().map(str::to_owned).collect()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error).context("reading paused cards"),
    }
}

fn save_paused(pins: &[String]) -> Result<()> {
    let path = paused_file()?;
    if pins.is_empty() {
        return match fs::remove_file(path) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => {
                Err(error).context("clearing paused cards")
            }
            _ => Ok(()),
        };
    }
    reward::write_private(&path, &pins.join("\n"))
}

fn card_request(endpoint: &str, fields: &[(&str, &str)]) -> Result<()> {
    let session = reward::authenticate_saved(None)?;
    let reply = reward::post(
        &session.client,
        endpoint,
        &session.device,
        &session.token,
        fields,
    )?;
    reward::data(&reply).map(|_| ())
}

fn pause(current_pin: Option<&str>) -> Result<String> {
    let pin = match current_pin {
        Some(pin) => pin.to_owned(),
        None => reward::fetch_active_cards(None)?
            .into_iter()
            .next()
            .map(|(pin, _)| pin)
            .ok_or_else(|| anyhow!("no card to pause"))?,
    };
    card_request("card-pause", &[("key", &pin), ("week", PAUSE_WEEKS)])
        .with_context(|| format!("pausing card {pin}"))?;
    save_paused(std::slice::from_ref(&pin))?;
    Ok(pin)
}

fn resume(pins: &[String]) -> Result<()> {
    let mut remaining = Vec::new();
    let mut last_error = None;
    for pin in pins {
        match card_request("card-resume", &[("key", pin)]) {
            Ok(()) => println!("Night window ended, card {pin} resumed."),
            Err(error) => {
                remaining.push(pin.clone());
                last_error = Some(error.context(format!("resuming card {pin}")));
            }
        }
    }
    save_paused(&remaining)?;
    match last_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(text: &str) -> NightWindow {
        text.parse().unwrap()
    }

    #[test]
    fn window_within_one_day() {
        let night = window("01:00-05:30");
        assert!(!night.contains(3599));
        assert!(night.contains(3600));
        assert!(night.contains(5 * 3600 + 29 * 60));
        assert!(!night.contains(5 * 3600 + 30 * 60));
    }

    #[test]
    fn window_wraps_midnight() {
        let night = window("23:00-06:00");
        assert!(night.contains(23 * 3600));
        assert!(night.contains(0));
        assert!(night.contains(6 * 3600 - 1));
        assert!(!night.contains(6 * 3600));
        assert!(!night.contains(12 * 3600));
    }

    #[test]
    fn seconds_until_end_wraps_midnight() {
        let night = window("23:00-06:00");
        assert_eq!(night.seconds_until_end(23 * 3600), 7 * 3600);
        assert_eq!(night.seconds_until_end(5 * 3600), 3600);
    }

    #[test]
    fn rejects_malformed_windows() {
        for text in [
            "",
            "22:00",
            "22:00-22:00",
            "24:00-05:00",
            "22:60-05:00",
            "a-b",
        ] {
            assert!(text.parse::<NightWindow>().is_err(), "{text}");
        }
    }
}
