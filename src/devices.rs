use std::{
    io::{self, BufRead, Write},
    path::PathBuf,
};

use anyhow::{Context, Result, bail};
use clap::Args;
use serde_json::{Value, json};

use crate::{reward, usage};

#[derive(Debug, Args)]
#[command(about = "List devices seen on cards, grouped by MAC address")]
pub struct DevicesArgs {
    /// Card PINs [default: every in-use card of the saved shop account]
    #[arg(num_args = 0.., value_name = "PIN")]
    pins: Vec<String>,

    /// Print JSON instead of a table
    #[arg(long)]
    json: bool,

    /// Directory for per-account device IDs
    #[arg(long)]
    state_dir: Option<PathBuf>,
}

#[derive(Debug, Args)]
#[command(about = "Log every device off a card")]
pub struct KickArgs {
    /// Card PIN
    #[arg(value_name = "PIN")]
    pin: String,

    /// Skip the confirmation prompt
    #[arg(long, short)]
    yes: bool,

    /// Print JSON instead of text
    #[arg(long)]
    json: bool,

    /// Directory for per-account device IDs
    #[arg(long)]
    state_dir: Option<PathBuf>,
}

#[derive(Debug, PartialEq)]
struct Device {
    mac: String,
    sessions: usize,
    last_seen: String,
    online_seconds: u64,
}

pub fn run_devices(args: DevicesArgs) -> Result<()> {
    let state_dir = args.state_dir.as_deref();
    let session = reward::authenticate_saved(state_dir)?;
    let pins = if args.pins.is_empty() {
        let reply = reward::post(
            &session.client,
            "mycards",
            &session.device,
            &session.token,
            &[("page", "1")],
        )?;
        let pins = in_use_pins(reward::data(&reply)?)?;
        if pins.is_empty() {
            bail!("no in-use card found in shop account");
        }
        pins
    } else {
        args.pins
    };

    let mut failures = 0;
    let mut cards = Vec::new();
    for pin in &pins {
        if pin.is_empty() {
            eprintln!("PIN is empty.");
            failures += 1;
            continue;
        }
        match usage::card_check(&session, pin) {
            Ok(data) => cards.push((pin, devices_from(&data))),
            Err(error) => {
                eprintln!("Card {pin}: {error:#}");
                failures += 1;
            }
        }
    }

    if args.json {
        let output: Vec<Value> = cards
            .iter()
            .map(|(pin, devices)| {
                json!({
                    "pin": pin,
                    "devices": devices.iter().map(device_json).collect::<Vec<_>>(),
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        for (index, (pin, devices)) in cards.iter().enumerate() {
            if index > 0 {
                println!();
            }
            print_devices(pin, devices);
        }
    }

    if failures > 0 {
        bail!("{failures} card request(s) failed");
    }
    Ok(())
}

pub fn run_kick(args: KickArgs) -> Result<()> {
    if args.pin.is_empty() {
        bail!("PIN is empty");
    }
    if !args.yes && !confirm(&args.pin)? {
        bail!("aborted, no device was logged off");
    }

    let session = reward::authenticate_saved(args.state_dir.as_deref())?;
    let reply = reward::post(
        &session.client,
        "card-logout",
        &session.device,
        &session.token,
        &[("key", &args.pin)],
    )?;
    if reply["result"] != true {
        bail!(
            "shop rejected request (code {}): {}",
            reply["code"],
            reply["msg"].as_str().unwrap_or("unknown error")
        );
    }

    let message = reply["msg"].as_str().unwrap_or("Success");
    if args.json {
        let output = json!({"pin": args.pin, "result": true, "msg": message});
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!("Card {}: {message}", args.pin);
    }
    Ok(())
}

fn confirm(pin: &str) -> Result<bool> {
    eprint!("Log every device off card {pin}? [y/N] ");
    io::stderr().flush()?;
    let mut answer = String::new();
    io::stdin()
        .lock()
        .read_line(&mut answer)
        .context("reading confirmation")?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

fn in_use_pins(cards: &Value) -> Result<Vec<String>> {
    cards
        .as_array()
        .context("mycards response missing card list")?
        .iter()
        .filter(|card| card["Status"] == "Thẻ Đang Sử Dụng")
        .map(|card| {
            card["Pin"]
                .as_str()
                .filter(|pin| !pin.is_empty())
                .map(str::to_owned)
                .context("card missing Pin")
        })
        .collect()
}

fn devices_from(data: &Value) -> Vec<Device> {
    let mut devices: Vec<Device> = Vec::new();
    let sessions = data
        .get("BWStatics")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    for session in sessions {
        let mac = session["MACs"].as_str().unwrap_or("unknown");
        let seconds = session["TotalTime"].as_str().map_or(0, parse_duration);
        match devices.iter_mut().find(|device| device.mac == mac) {
            Some(device) => {
                device.sessions += 1;
                device.online_seconds += seconds;
            }
            None => devices.push(Device {
                mac: mac.to_owned(),
                sessions: 1,
                last_seen: session["StartAt"].as_str().unwrap_or("-").to_owned(),
                online_seconds: seconds,
            }),
        }
    }
    devices
}

fn parse_duration(text: &str) -> u64 {
    let mut parts = text.split(':').map(|part| part.parse::<u64>());
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(Ok(hours)), Some(Ok(minutes)), Some(Ok(seconds)), None) => {
            hours * 3600 + minutes * 60 + seconds
        }
        _ => 0,
    }
}

fn format_duration(seconds: u64) -> String {
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds % 3600 / 60,
        seconds % 60
    )
}

fn device_json(device: &Device) -> Value {
    json!({
        "mac": device.mac,
        "sessions": device.sessions,
        "last_seen": device.last_seen,
        "online_seconds": device.online_seconds,
    })
}

fn print_devices(pin: &str, devices: &[Device]) {
    println!("Card {pin}");
    if devices.is_empty() {
        println!("  No devices recorded.");
        return;
    }
    let rows: Vec<[String; 4]> = devices
        .iter()
        .map(|device| {
            [
                device.mac.clone(),
                device.sessions.to_string(),
                device.last_seen.clone(),
                format_duration(device.online_seconds),
            ]
        })
        .collect();
    let headers = ["MAC", "Sessions", "Last seen", "Online time"];
    let widths: Vec<usize> = (0..headers.len())
        .map(|column| {
            rows.iter()
                .map(|row| row[column].chars().count())
                .chain([headers[column].len()])
                .max()
                .unwrap_or(0)
        })
        .collect();
    let print_row = |cells: [&str; 4]| {
        let line: Vec<String> = cells
            .iter()
            .zip(&widths)
            .map(|(cell, width)| format!("{cell:<width$}"))
            .collect();
        println!("  {}", line.join("  ").trim_end());
    };
    print_row(headers);
    for row in &rows {
        print_row([&row[0], &row[1], &row[2], &row[3]]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_sessions_by_mac_keeping_newest_last_seen() {
        let data = json!({"BWStatics": [
            {"MACs": "30:C5:******40:31", "StartAt": "08/11/2026 15:36:50", "TotalTime": "00:10:00"},
            {"MACs": "82:C7:******A2:D9", "StartAt": "08/11/2026 00:38:39", "TotalTime": "11:56:36"},
            {"MACs": "30:C5:******40:31", "StartAt": "07/11/2026 20:31:56", "TotalTime": "04:07:35"},
        ]});
        assert_eq!(
            devices_from(&data),
            vec![
                Device {
                    mac: "30:C5:******40:31".into(),
                    sessions: 2,
                    last_seen: "08/11/2026 15:36:50".into(),
                    online_seconds: 600 + 4 * 3600 + 7 * 60 + 35,
                },
                Device {
                    mac: "82:C7:******A2:D9".into(),
                    sessions: 1,
                    last_seen: "08/11/2026 00:38:39".into(),
                    online_seconds: 11 * 3600 + 56 * 60 + 36,
                },
            ]
        );
    }

    #[test]
    fn card_without_sessions_has_no_devices() {
        assert!(devices_from(&json!({"BWStatics": null})).is_empty());
        assert!(devices_from(&json!({})).is_empty());
    }

    #[test]
    fn in_use_pins_skips_unused_and_expired_cards() {
        let cards = json!([
            {"Pin": "AAA", "Status": "Thẻ Đang Sử Dụng"},
            {"Pin": "BBB", "Status": "Thẻ Chưa Sử Dụng"},
            {"Pin": "CCC", "Status": "Thẻ Hết Hạn"},
            {"Pin": "DDD", "Status": "Thẻ Đang Sử Dụng"},
        ]);
        assert_eq!(in_use_pins(&cards).unwrap(), vec!["AAA", "DDD"]);
        assert!(in_use_pins(&json!({})).is_err());
        assert!(in_use_pins(&json!([{"Status": "Thẻ Đang Sử Dụng"}])).is_err());
    }

    #[test]
    fn malformed_duration_counts_as_zero() {
        assert_eq!(parse_duration("01:02:03"), 3723);
        assert_eq!(parse_duration("garbage"), 0);
        assert_eq!(parse_duration("1:2"), 0);
    }
}
