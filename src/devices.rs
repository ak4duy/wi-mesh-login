use std::{
    io::{self, BufRead, Write},
    path::PathBuf,
};

use anyhow::{Context, Result, bail};
use clap::Args;
use serde_json::{Value, json};

use crate::{reward, usage};

#[derive(Debug, Args)]
#[command(about = "List masked MAC groups seen on cards. The shop has no live device list")]
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
struct MacGroup {
    mac: String,
    sessions: usize,
    last_seen: String,
    online_seconds: u64,
    download_bytes: u64,
    upload_bytes: u64,
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
            Ok(data) => cards.push((pin, mac_groups_from(&data))),
            Err(error) => {
                eprintln!("Card {pin}: {error:#}");
                failures += 1;
            }
        }
    }

    if args.json {
        let output: Vec<Value> = cards
            .iter()
            .map(|(pin, groups)| {
                json!({
                    "pin": pin,
                    "mac_groups": groups.iter().map(mac_group_json).collect::<Vec<_>>(),
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        for (index, (pin, groups)) in cards.iter().enumerate() {
            if index > 0 {
                println!();
            }
            print_mac_groups(pin, groups);
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
    let message = kick_message(&reply)?;
    if args.json {
        let output = json!({"pin": args.pin, "result": true, "msg": message});
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!("Card {}: {message}", args.pin);
    }
    Ok(())
}

fn kick_message(reply: &Value) -> Result<String> {
    if reply["result"] != true {
        bail!(
            "shop rejected request (code {}): {}",
            reply["code"],
            reply["msg"].as_str().unwrap_or("unknown error")
        );
    }
    Ok(reply["msg"].as_str().unwrap_or("Success").to_owned())
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

fn mac_groups_from(data: &Value) -> Vec<MacGroup> {
    let mut groups: Vec<MacGroup> = Vec::new();
    let sessions = data
        .get("BWStatics")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    for session in sessions {
        let seconds = session["TotalTime"].as_str().map_or(0, parse_duration);
        let download = session["Download"].as_str().map_or(0, parse_size);
        let upload = session["Upload"].as_str().map_or(0, parse_size);
        for label in mac_labels(session["MACs"].as_str().unwrap_or_default()) {
            let index = match groups.iter().position(|group| group.mac == label) {
                Some(index) => index,
                None => {
                    groups.push(MacGroup {
                        mac: label,
                        sessions: 0,
                        last_seen: session["StartAt"].as_str().unwrap_or("-").to_owned(),
                        online_seconds: 0,
                        download_bytes: 0,
                        upload_bytes: 0,
                    });
                    groups.len() - 1
                }
            };
            let group = &mut groups[index];
            group.sessions += 1;
            group.online_seconds += seconds;
            group.download_bytes += download;
            group.upload_bytes += upload;
        }
    }
    groups
}

fn mac_labels(text: &str) -> Vec<String> {
    let labels: Vec<String> = text
        .split(|c: char| c == ',' || c == ';' || c.is_whitespace())
        .filter(|label| !label.is_empty())
        .map(str::to_owned)
        .collect();
    if labels.is_empty() {
        vec!["unknown".to_owned()]
    } else {
        labels
    }
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

fn parse_size(text: &str) -> u64 {
    let mut parts = text.split_whitespace();
    let (Some(number), Some(unit), None) = (parts.next(), parts.next(), parts.next()) else {
        return 0;
    };
    let Ok(number) = number.parse::<f64>() else {
        return 0;
    };
    let scale = match unit.to_ascii_uppercase().as_str() {
        "B" => 1.0,
        "KB" => 1024.0,
        "MB" => 1024.0 * 1024.0,
        "GB" => 1024.0 * 1024.0 * 1024.0,
        "TB" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => return 0,
    };
    (number * scale).round() as u64
}

fn format_duration(seconds: u64) -> String {
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds % 3600 / 60,
        seconds % 60
    )
}

fn format_size(bytes: u64) -> String {
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    let value = bytes as f64;
    if value >= GB {
        format!("{:.2} GB", value / GB)
    } else {
        format!("{:.2} MB", value / MB)
    }
}

fn mac_group_json(group: &MacGroup) -> Value {
    json!({
        "mac": group.mac,
        "sessions": group.sessions,
        "last_seen": group.last_seen,
        "online_seconds": group.online_seconds,
        "download_bytes": group.download_bytes,
        "upload_bytes": group.upload_bytes,
    })
}

fn print_mac_groups(pin: &str, groups: &[MacGroup]) {
    println!("Card {pin}");
    if groups.is_empty() {
        println!("  No sessions recorded.");
        return;
    }
    let rows: Vec<[String; 6]> = groups
        .iter()
        .map(|group| {
            [
                group.mac.clone(),
                group.sessions.to_string(),
                group.last_seen.clone(),
                format_duration(group.online_seconds),
                format_size(group.download_bytes),
                format_size(group.upload_bytes),
            ]
        })
        .collect();
    let headers = [
        "Masked MAC",
        "Sessions",
        "Last seen",
        "Online time",
        "Download",
        "Upload",
    ];
    let widths: Vec<usize> = (0..headers.len())
        .map(|column| {
            rows.iter()
                .map(|row| row[column].chars().count())
                .chain([headers[column].len()])
                .max()
                .unwrap_or(0)
        })
        .collect();
    let print_row = |cells: [&str; 6]| {
        let line: Vec<String> = cells
            .iter()
            .zip(&widths)
            .map(|(cell, width)| format!("{cell:<width$}"))
            .collect();
        println!("  {}", line.join("  ").trim_end());
    };
    print_row(headers);
    for row in &rows {
        print_row([&row[0], &row[1], &row[2], &row[3], &row[4], &row[5]]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_sessions_by_mac_keeping_newest_last_seen() {
        let data = json!({"BWStatics": [
            {"MACs": "30:C5:******40:31", "StartAt": "08/11/2026 15:36:50", "TotalTime": "00:10:00", "Download": "1.50 GB", "Upload": "10.00 MB"},
            {"MACs": "82:C7:******A2:D9", "StartAt": "08/11/2026 00:38:39", "TotalTime": "11:56:36", "Download": "1.09 GB", "Upload": "44.28 MB"},
            {"MACs": "30:C5:******40:31", "StartAt": "07/11/2026 20:31:56", "TotalTime": "04:07:35", "Download": "512.00 MB", "Upload": "6.00 MB"},
        ]});
        assert_eq!(
            mac_groups_from(&data),
            vec![
                MacGroup {
                    mac: "30:C5:******40:31".into(),
                    sessions: 2,
                    last_seen: "08/11/2026 15:36:50".into(),
                    online_seconds: 600 + 4 * 3600 + 7 * 60 + 35,
                    download_bytes: 2 * 1024 * 1024 * 1024,
                    upload_bytes: 16 * 1024 * 1024,
                },
                MacGroup {
                    mac: "82:C7:******A2:D9".into(),
                    sessions: 1,
                    last_seen: "08/11/2026 00:38:39".into(),
                    online_seconds: 11 * 3600 + 56 * 60 + 36,
                    download_bytes: (1.09_f64 * 1024.0 * 1024.0 * 1024.0).round() as u64,
                    upload_bytes: (44.28_f64 * 1024.0 * 1024.0).round() as u64,
                },
            ]
        );
    }

    #[test]
    fn session_with_several_macs_counts_for_each() {
        let data = json!({"BWStatics": [
            {"MACs": "AA:**:01, BB:**:02;CC:**:03", "StartAt": "s1", "TotalTime": "00:00:10", "Download": "1.00 MB", "Upload": "1.00 MB"},
            {"MACs": "BB:**:02", "StartAt": "s0", "TotalTime": "00:00:05", "Download": "1.00 MB", "Upload": "1.00 MB"},
            {"Download": "1.00 MB"},
        ]});
        let groups = mac_groups_from(&data);
        let summary: Vec<(&str, usize, u64)> = groups
            .iter()
            .map(|group| (group.mac.as_str(), group.sessions, group.online_seconds))
            .collect();
        assert_eq!(
            summary,
            vec![
                ("AA:**:01", 1, 10),
                ("BB:**:02", 2, 15),
                ("CC:**:03", 1, 10),
                ("unknown", 1, 0),
            ]
        );
    }

    #[test]
    fn card_without_sessions_has_no_mac_groups() {
        assert!(mac_groups_from(&json!({"BWStatics": null})).is_empty());
        assert!(mac_groups_from(&json!({})).is_empty());
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
    fn malformed_values_count_as_zero() {
        assert_eq!(parse_duration("01:02:03"), 3723);
        assert_eq!(parse_duration("garbage"), 0);
        assert_eq!(parse_duration("1:2"), 0);
        assert_eq!(parse_size("2.00 KB"), 2048);
        assert_eq!(parse_size("12"), 0);
        assert_eq!(parse_size("1.0 PB"), 0);
    }

    #[test]
    fn kick_reply_success_and_rejection() {
        assert_eq!(
            kick_message(&json!({"result": true, "msg": "Done"})).unwrap(),
            "Done"
        );
        assert_eq!(kick_message(&json!({"result": true})).unwrap(), "Success");
        let error = kick_message(&json!({"result": false, "code": 2, "msg": "denied"}))
            .unwrap_err()
            .to_string();
        assert!(error.contains("code 2") && error.contains("denied"));
    }
}
