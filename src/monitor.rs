use std::{collections::HashMap, fs, io, path::PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use clap::Args;
use reqwest::{blocking::Client, header};
use serde::Deserialize;
use serde_json::json;

use crate::{
    reward::{self, AuthenticatedSession},
    usage,
};

const DEFAULT_MESSAGE: &str = "**MY-WIFI DATA ALERT!!!** {mention}\n\
    Card: **{pin}** ({type})\n\
    Total usage: **{used} GB / {quota} GB**\n\
    Remaining: **{remaining} GB**";

#[derive(Debug, Args)]
#[command(about = "Send a Discord alert when a card is running out of data")]
pub struct MonitorArgs {
    /// Config file [default: ~/.config/wi-mesh-login/config.toml]
    #[arg(long)]
    config: Option<PathBuf>,
}

#[derive(Deserialize)]
struct Config {
    monitor: MonitorConfig,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MonitorConfig {
    webhook: String,
    #[serde(default)]
    pins: Vec<String>,
    quota_gb: Option<f64>,
    #[serde(default = "default_thresholds")]
    thresholds_gb: Vec<f64>,
    #[serde(default)]
    mention_users: Vec<String>,
    #[serde(default)]
    mention_roles: Vec<String>,
    #[serde(default = "default_message")]
    message: String,
}

fn default_thresholds() -> Vec<f64> {
    vec![10.0, 5.0, 2.0, 1.0, 0.0]
}

fn default_message() -> String {
    DEFAULT_MESSAGE.to_owned()
}

fn default_config() -> Result<PathBuf> {
    #[cfg(windows)]
    let root = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .context("set --config when APPDATA is unavailable")?;
    #[cfg(not(windows))]
    let root = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .context("set --config when HOME/XDG_CONFIG_HOME are unavailable")?;
    Ok(root.join("wi-mesh-login").join("config.toml"))
}

pub fn run(args: MonitorArgs) -> Result<()> {
    let path = match args.config {
        Some(path) => path,
        None => default_config()?,
    };
    let text = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let config = toml::from_str::<Config>(&text)
        .with_context(|| format!("parsing {}", path.display()))?
        .monitor;

    let pins = if config.pins.is_empty() {
        reward::fetch_active_cards(None)?
            .into_iter()
            .map(|(pin, _)| pin)
            .collect()
    } else {
        config.pins.clone()
    };

    let state_path = reward::default_state_dir()?.join("monitor.json");
    let mut state: HashMap<String, f64> = match fs::read_to_string(&state_path) {
        Ok(text) => serde_json::from_str(&text).context("parsing monitor state")?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => HashMap::new(),
        Err(error) => return Err(error).context("reading monitor state"),
    };

    let session = reward::authenticate_saved(None)?;
    let mut failures = 0;
    for pin in &pins {
        match check_card(&config, &session, pin, state.get(pin).copied()) {
            Ok(Some(threshold)) => {
                state.insert(pin.clone(), threshold);
                reward::write_private(&state_path, &serde_json::to_string(&state)?)?;
            }
            Ok(None) => {}
            Err(error) => {
                eprintln!("{pin}: {error:#}");
                failures += 1;
            }
        }
    }
    if failures == pins.len() {
        bail!("every card check failed");
    }
    Ok(())
}

fn check_card(
    config: &MonitorConfig,
    session: &AuthenticatedSession,
    pin: &str,
    last_alert: Option<f64>,
) -> Result<Option<f64>> {
    let data = usage::card_check(session, pin)?;
    let used = data["BWTotal"]
        .as_str()
        .and_then(parse_gb)
        .with_context(|| format!("unexpected usage value {}", data["BWTotal"]))?;
    let kind = data["TypeName"].as_str().unwrap_or("");
    let Some(quota) = config.quota_gb.or_else(|| quota_from_type(kind)) else {
        println!("{pin}: {kind} has no data quota, skipped");
        return Ok(None);
    };
    let remaining = (quota - used).max(0.0);
    let Some(threshold) = next_alert(remaining, &config.thresholds_gb, last_alert) else {
        println!("{pin}: used {used:.2} GB of {quota} GB, no new alert");
        return Ok(None);
    };

    let mention = config
        .mention_users
        .iter()
        .map(|id| format!("<@{id}>"))
        .chain(config.mention_roles.iter().map(|id| format!("<@&{id}>")))
        .collect::<Vec<_>>()
        .join(" ");
    let message = config
        .message
        .replace("{mention}", &mention)
        .replace("{pin}", pin)
        .replace("{type}", kind)
        .replace("{used}", &format!("{used:.2}"))
        .replace("{quota}", &quota.to_string())
        .replace("{remaining}", &format!("{remaining:.2}"))
        .replace("{threshold}", &threshold.to_string());
    let body = json!({
        "content": message,
        "allowed_mentions": {"users": config.mention_users, "roles": config.mention_roles},
    });
    // The webhook URL is a secret, so errors must not echo it.
    Client::new()
        .post(&config.webhook)
        .header(header::CONTENT_TYPE, "application/json")
        .body(body.to_string())
        .send()
        .and_then(|response| response.error_for_status())
        .map_err(|error| anyhow!("posting Discord alert: {}", error.without_url()))?;
    println!("{pin}: Discord alert sent, remaining {remaining:.2} GB");
    Ok(Some(threshold))
}

fn parse_gb(text: &str) -> Option<f64> {
    let text = text.trim();
    let split = text.find(|c: char| c.is_ascii_alphabetic())?;
    let value: f64 = text[..split].trim().parse().ok()?;
    let scale = match text[split..].trim().to_ascii_uppercase().as_str() {
        "KB" => 1e-6,
        "MB" => 1e-3,
        "GB" => 1.0,
        "TB" => 1e3,
        _ => return None,
    };
    Some(value * scale)
}

fn quota_from_type(kind: &str) -> Option<f64> {
    kind.split_whitespace()
        .find_map(|word| word.strip_suffix("GB")?.parse().ok())
}

fn next_alert(remaining: f64, thresholds: &[f64], last_alert: Option<f64>) -> Option<f64> {
    thresholds
        .iter()
        .copied()
        .filter(|&threshold| remaining <= threshold && last_alert.is_none_or(|last| threshold < last))
        .reduce(f64::min)
}

#[test]
fn parses_usage_and_quota() {
    assert_eq!(parse_gb("152.4 GB"), Some(152.4));
    assert_eq!(parse_gb("500 MB"), Some(0.5));
    assert_eq!(parse_gb("—"), None);
    assert_eq!(quota_from_type("Thẻ Tháng 150GB"), Some(150.0));
    assert_eq!(quota_from_type("Thẻ Ngày"), None);
}

#[test]
fn alerts_once_per_threshold() {
    let thresholds = [10.0, 5.0, 1.0];
    assert_eq!(next_alert(20.0, &thresholds, None), None);
    assert_eq!(next_alert(4.0, &thresholds, None), Some(5.0));
    assert_eq!(next_alert(4.0, &thresholds, Some(5.0)), None);
    assert_eq!(next_alert(0.5, &thresholds, Some(5.0)), Some(1.0));
}
