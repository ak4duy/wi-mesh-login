use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Args;
use reqwest::blocking::multipart::Form;
use serde_json::Value;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::reward::{self, AuthenticatedSession};

const API_URL: &str = "https://mywifi.vn/apimobi/v1/card-check";

#[derive(Debug, Args)]
#[command(about = "Show card usage from the MyWIFI app API")]
pub struct UsageArgs {
    /// Card PINs
    #[arg(required = true, num_args = 1.., value_name = "PIN")]
    pins: Vec<String>,

    /// Show individual bandwidth sessions
    #[arg(long)]
    sessions: bool,

    /// Directory for per-account device IDs
    #[arg(long)]
    state_dir: Option<PathBuf>,
}

pub fn run(args: UsageArgs) -> Result<()> {
    let session = reward::authenticate_saved(args.state_dir.as_deref())?;

    let mut failures = 0;
    for (index, pin) in args.pins.iter().enumerate() {
        if pin.is_empty() {
            eprintln!("Card {}: PIN is empty.", index + 1);
            failures += 1;
            continue;
        }

        match card_check(&session, pin) {
            Ok(data) => print_card(index + 1, &data, args.sessions),
            Err(error) => {
                eprintln!("Card {}: {error:#}", index + 1);
                failures += 1;
            }
        }

        if index + 1 < args.pins.len() {
            println!();
        }
    }

    if failures > 0 {
        bail!("{failures} card request(s) failed");
    }
    Ok(())
}

fn card_check(session: &AuthenticatedSession, pin: &str) -> Result<Value> {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .context("system clock is before the Unix epoch")?
        .as_millis()
        .to_string();

    let response = session
        .client
        .post(API_URL)
        .query(&[("key", pin)])
        .header("wm-token", &session.token)
        .header("wm-device", &session.device)
        .header("wm-time", now_ms)
        .multipart(Form::new())
        .send()
        .context("requesting card usage")?
        .error_for_status()
        .context("MyWIFI API returned an HTTP error")?;
    let reply: Value =
        serde_json::from_str(&response.text().context("reading MyWIFI API response")?)
            .context("MyWIFI API returned invalid JSON")?;

    if reply.get("result").and_then(Value::as_bool) != Some(true) {
        let code = reply.get("code").and_then(Value::as_i64);
        match code {
            Some(2) => {
                bail!("authorization failed (code 2); run `wi-mesh-login shop --login` again")
            }
            Some(3) => {
                bail!("card PIN is incorrect, unactivated, or not yet reporting usage (code 3)")
            }
            _ => {
                let message = reply
                    .get("msg")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown API error");
                bail!(
                    "MyWIFI API rejected the request (code {}): {message}",
                    code.map_or_else(|| "unknown".to_owned(), |value| value.to_string())
                );
            }
        }
    }

    reply
        .get("data")
        .filter(|data| data.is_object())
        .cloned()
        .context("successful MyWIFI response is missing data")
}

fn print_card(number: usize, data: &Value, show_sessions: bool) {
    println!("Card {number}");
    print_field("Type", data, "TypeName");
    print_field("Status", data, "Status");
    print_field("Project", data, "ProjectName");
    println!("  Activated:   {}", timestamp(data.get("ActivedAt")));
    println!("  Expires:     {}", timestamp(data.get("ExpiredAt")));
    print_field("Total usage", data, "BWTotal");

    if show_sessions {
        print_sessions(data.get("BWStatics"));
    }
}

fn print_field(label: &str, data: &Value, key: &str) {
    println!("  {label:<12} {}", display_value(data.get(key)));
}

fn timestamp(value: Option<&Value>) -> String {
    let Some(raw) = value.and_then(Value::as_i64) else {
        return display_value(value);
    };
    if raw <= 0 {
        return "—".to_owned();
    }
    let nanoseconds = if raw >= 100_000_000_000 {
        i128::from(raw) * 1_000_000
    } else {
        i128::from(raw) * 1_000_000_000
    };
    OffsetDateTime::from_unix_timestamp_nanos(nanoseconds)
        .ok()
        .and_then(|date| date.format(&Rfc3339).ok())
        .unwrap_or_else(|| raw.to_string())
}

fn print_sessions(value: Option<&Value>) {
    let Some(sessions) = value.and_then(Value::as_array) else {
        println!("  Sessions:    —");
        return;
    };
    println!("  Sessions:    {}", sessions.len());
    if sessions.is_empty() {
        return;
    }

    let headers = ["#", "Start", "Total time", "Download", "Upload", "MACs"];
    let rows: Vec<[String; 6]> = sessions
        .iter()
        .enumerate()
        .map(|(index, session)| {
            [
                (index + 1).to_string(),
                timestamp(session.get("StartAt")),
                display_value(session.get("TotalTime")),
                display_value(session.get("Download")),
                display_value(session.get("Upload")),
                display_value(session.get("MACs")),
            ]
        })
        .collect();
    let mut widths = std::array::from_fn::<_, 6, _>(|index| headers[index].len());
    for row in &rows {
        for (index, cell) in row.iter().enumerate() {
            widths[index] = widths[index].max(cell.chars().count());
        }
    }

    print!("    ");
    print_row(headers.iter().copied(), &widths);
    let separators: [String; 6] = std::array::from_fn(|index| "-".repeat(widths[index]));
    print!("    ");
    print_row(separators.iter().map(String::as_str), &widths);
    for row in &rows {
        print!("    ");
        print_row(row.iter().map(String::as_str), &widths);
    }
}

fn print_row<'a>(cells: impl IntoIterator<Item = &'a str>, widths: &[usize; 6]) {
    for (index, cell) in cells.into_iter().enumerate() {
        if index > 0 {
            print!("  ");
        }
        print!("{cell:<width$}", width = widths[index]);
    }
    println!();
}

fn display_value(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => "—".to_owned(),
        Some(Value::String(value)) if value.is_empty() => "—".to_owned(),
        Some(Value::String(value)) => value.clone(),
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| match value {
                Value::String(value) => value.clone(),
                _ => value.to_string(),
            })
            .collect::<Vec<_>>()
            .join(", "),
        Some(value) => value.to_string(),
    }
}