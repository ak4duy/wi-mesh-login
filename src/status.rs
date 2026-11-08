use anyhow::{Context, Result, anyhow, bail};
use clap::Args;
use reqwest::blocking::Client;
use scraper::{Html, Selector};
use serde::Serialize;

const STATUS_URL: &str = "https://login.net.vn/status";

const BYTE_UNITS: [(&str, f64); 5] = [
    ("B", 1.0),
    ("KiB", 1024.0),
    ("MiB", 1024.0 * 1024.0),
    ("GiB", 1024.0 * 1024.0 * 1024.0),
    ("TiB", 1024.0 * 1024.0 * 1024.0 * 1024.0),
];

const DURATION_UNITS: [(char, u64); 5] = [
    ('w', 604_800),
    ('d', 86_400),
    ('h', 3_600),
    ('m', 60),
    ('s', 1),
];

#[derive(Debug, Args)]
#[command(about = "Show the current Wi-MESH session from the captive portal status page")]
pub struct StatusArgs {
    /// Network interface name to bind requests to
    #[arg(long)]
    interface: Option<String>,

    /// Request timeout in seconds
    #[arg(long, default_value_t = 30)]
    timeout_seconds: u64,

    /// Print the status as JSON
    #[arg(long)]
    json: bool,
}

#[derive(Debug, PartialEq, Serialize)]
pub struct Status {
    pub user: String,
    pub ip: String,
    pub uploaded_bytes: u64,
    pub downloaded_bytes: u64,
    pub connected_seconds: u64,
    pub remaining_seconds: u64,
}

pub fn run(args: StatusArgs) -> Result<()> {
    let client = crate::client_for(args.interface.as_deref(), args.timeout_seconds)?;
    let status = fetch(&client)?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&status)?);
    } else {
        print!("{}", render(&status));
    }
    Ok(())
}

pub fn fetch(client: &Client) -> Result<Status> {
    let page = client
        .get(STATUS_URL)
        .send()
        .context("requesting portal status page")?
        .error_for_status()
        .context("portal status page returned an HTTP error")?
        .text()
        .context("reading portal status page")?;
    parse(&page)
}

pub fn parse(page: &str) -> Result<Status> {
    let document = Html::parse_document(page);
    let selector = Selector::parse(".txtUser, .txtInfoCard").expect("valid selector");
    let lines: Vec<String> = document
        .select(&selector)
        .map(|element| element.text().collect::<String>().trim().to_owned())
        .collect();
    let field = |label: &str| {
        lines
            .iter()
            .find_map(|line| line.strip_prefix(label))
            .map(str::trim)
    };

    let user = field("Xin Chào:")
        .filter(|user| !user.is_empty())
        .ok_or_else(|| anyhow!("status page has no account info; is this device logged in?"))?;
    let ip = field("IP address:").context("status page has no IP address")?;
    let (uploaded, downloaded) = field("Dung Lượng Sử Dụng Up/Down:")
        .and_then(|value| value.split_once('/'))
        .context("status page has no data usage")?;
    let (connected, remaining) = field("Thời Gian Kết Nối/Còn Lại:")
        .and_then(|value| value.split_once('/'))
        .context("status page has no session time")?;

    Ok(Status {
        user: user.to_owned(),
        ip: ip.to_owned(),
        uploaded_bytes: parse_bytes(uploaded)?,
        downloaded_bytes: parse_bytes(downloaded)?,
        connected_seconds: parse_duration(connected)?,
        remaining_seconds: parse_duration(remaining)?,
    })
}

fn parse_bytes(text: &str) -> Result<u64> {
    let text = text.trim();
    let (number, unit) = text
        .split_once(char::is_whitespace)
        .with_context(|| format!("invalid data size '{text}'"))?;
    let number: f64 = number
        .parse()
        .with_context(|| format!("invalid data size '{text}'"))?;
    let (_, factor) = BYTE_UNITS
        .iter()
        .find(|(name, _)| *name == unit.trim())
        .with_context(|| format!("unknown data unit in '{text}'"))?;
    Ok((number * factor).round() as u64)
}

fn parse_duration(text: &str) -> Result<u64> {
    let text = text.trim();
    if text.is_empty() {
        bail!("empty duration");
    }
    let mut total = 0;
    let mut digits = String::new();
    for character in text.chars() {
        if character.is_ascii_digit() {
            digits.push(character);
            continue;
        }
        let (_, seconds) = DURATION_UNITS
            .iter()
            .find(|(unit, _)| *unit == character)
            .with_context(|| format!("invalid duration '{text}'"))?;
        let count: u64 = digits
            .parse()
            .with_context(|| format!("invalid duration '{text}'"))?;
        total += count * seconds;
        digits.clear();
    }
    if !digits.is_empty() {
        bail!("invalid duration '{text}'");
    }
    Ok(total)
}

fn format_bytes(bytes: u64) -> String {
    let (name, factor) = BYTE_UNITS
        .iter()
        .rev()
        .find(|(_, factor)| bytes as f64 >= *factor)
        .unwrap_or(&BYTE_UNITS[0]);
    if *factor == 1.0 {
        format!("{bytes} B")
    } else {
        format!("{:.1} {name}", bytes as f64 / factor)
    }
}

fn format_duration(seconds: u64) -> String {
    let mut remaining = seconds;
    let mut parts = Vec::new();
    for (unit, size) in DURATION_UNITS.iter().filter(|(unit, _)| *unit != 'w') {
        let count = remaining / size;
        remaining %= size;
        if count > 0 {
            parts.push(format!("{count}{unit}"));
        }
    }
    if parts.is_empty() {
        "0s".to_owned()
    } else {
        parts.join(" ")
    }
}

fn render(status: &Status) -> String {
    format!(
        "User:       {}\nIP address: {}\nUploaded:   {}\nDownloaded: {}\nConnected:  {}\nRemaining:  {}\n",
        status.user,
        status.ip,
        format_bytes(status.uploaded_bytes),
        format_bytes(status.downloaded_bytes),
        format_duration(status.connected_seconds),
        format_duration(status.remaining_seconds),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = include_str!("../tests/fixtures/status.html");

    #[test]
    fn parses_status_page() {
        assert_eq!(
            parse(PAGE).unwrap(),
            Status {
                user: "ABCD1234".to_owned(),
                ip: "10.0.0.2".to_owned(),
                uploaded_bytes: 29_464_986,
                downloaded_bytes: 449_734_246,
                connected_seconds: 18 * 60 + 48,
                remaining_seconds: 28 * 86_400 + 19 * 3_600 + 59 * 60 + 57,
            }
        );
    }

    #[test]
    fn rejects_page_without_account_info() {
        let page =
            "<html><body><form action=\"/login\"><input name=\"username\"></form></body></html>";
        let error = parse(page).unwrap_err().to_string();
        assert!(error.contains("logged in"), "{error}");
    }

    #[test]
    fn rejects_page_with_missing_time() {
        let page = PAGE.replace("Thời Gian Kết Nối/Còn Lại", "Thời Gian");
        let error = parse(&page).unwrap_err().to_string();
        assert!(error.contains("session time"), "{error}");
    }

    #[test]
    fn parses_durations() {
        assert_eq!(parse_duration("1m4s").unwrap(), 64);
        assert_eq!(parse_duration("2h5m59s").unwrap(), 7_559);
        assert_eq!(parse_duration("28d20h20m27s").unwrap(), 2_492_427);
        assert_eq!(parse_duration("1w1d").unwrap(), 691_200);
        assert_eq!(parse_duration("0s").unwrap(), 0);
    }

    #[test]
    fn rejects_malformed_durations() {
        for text in ["", "12", "m5", "5x", "1h30"] {
            assert!(parse_duration(text).is_err(), "{text}");
        }
    }

    #[test]
    fn parses_data_sizes() {
        assert_eq!(parse_bytes("512 B").unwrap(), 512);
        assert_eq!(parse_bytes("1.5 KiB").unwrap(), 1_536);
        assert_eq!(parse_bytes("39.1 MiB").unwrap(), 40_999_322);
        assert_eq!(parse_bytes("2.0 GiB").unwrap(), 2_147_483_648);
        assert!(parse_bytes("5 parsecs").is_err());
        assert!(parse_bytes("MiB").is_err());
    }

    #[test]
    fn renders_human_output() {
        let output = render(&parse(PAGE).unwrap());
        assert_eq!(
            output,
            "User:       ABCD1234\nIP address: 10.0.0.2\nUploaded:   28.1 MiB\nDownloaded: 428.9 MiB\nConnected:  18m 48s\nRemaining:  28d 19h 59m 57s\n"
        );
    }

    #[test]
    fn formats_small_and_zero_values() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1023), "1023 B");
        assert_eq!(format_duration(0), "0s");
        assert_eq!(format_duration(3_600), "1h");
    }
}
