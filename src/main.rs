use std::{
    fs,
    net::IpAddr,
    path::PathBuf,
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};

mod reward;
mod usage;
use if_addrs::get_if_addrs;
use reqwest::{blocking::Client, header, redirect::Policy};
use scraper::{Html, Selector};
use url::{Url, form_urlencoded};

const ENTRY_URL: &str = "http://connectivitycheck.gstatic.com/generate_204";
const LOGOUT_URL: &str = "https://login.net.vn/logout";
const USER_AGENT: &str = "Mozilla/5.0";
const WI_MESH_SSIDS: [&str; 2] = ["Wi-MESH 2.4G", "Wi-MESH 5G"];

#[derive(Debug, Parser)]
#[command(
    about = "Log in to Wi-MESH or claim shop rewards",
    subcommand_negates_reqs = true,
    args_conflicts_with_subcommands = true
)]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,

    /// Captive portal detection URL (https://en.wikipedia.org/wiki/Captive_portal#Detection)
    #[arg(long, default_value = ENTRY_URL)]
    entry_url: Url,

    /// Network interface name to bind requests to
    #[arg(long)]
    interface: Option<String>,

    /// Card pin
    #[arg(required_unless_present_any = ["logout", "auto"], conflicts_with_all = ["logout", "auto"])]
    username: Option<String>,

    /// Card password
    #[arg(required_unless_present_any = ["logout", "auto"], conflicts_with_all = ["logout", "auto"])]
    password: Option<String>,

    /// Logout of Wi-MESH
    #[arg(long)]
    logout: bool,

    /// Log in with the first card in use from the saved shop account
    #[arg(long, conflicts_with = "logout")]
    auto: bool,

    /// Seconds between internet checks in auto mode [default: 60]
    #[arg(long, value_name = "SECONDS", conflicts_with_all = ["username", "logout"])]
    interval: Option<u64>,

    /// Seconds to wait before retrying a failed auto login [default: 10]
    #[arg(long, value_name = "SECONDS", conflicts_with_all = ["username", "logout"])]
    retry_after: Option<u64>,

    /// Check once, log in if offline, retry on failure, then exit (no polling)
    #[arg(long, requires = "auto", conflicts_with = "interval")]
    once: bool,

    /// Login attempts before `--once` gives up [default: 3]
    #[arg(
        long,
        value_name = "N",
        requires = "once",
        value_parser = clap::value_parser!(u32).range(1..)
    )]
    attempts: Option<u32>,

    /// Wi-Fi name auto mode may act on (repeatable)
    #[arg(long = "ssid", value_name = "NAME", default_values = WI_MESH_SSIDS)]
    ssids: Vec<String>,

    /// Wi-Fi name the device is connected to, passed by the caller (required on Android)
    #[arg(long, value_name = "NAME", requires = "auto")]
    current_ssid: Option<String>,

    /// Directory for saved shop credentials, cached card and device IDs
    #[arg(long, value_name = "DIR", requires = "auto")]
    state_dir: Option<PathBuf>,

    /// Post login without logs, debug artifacts, or connectivity checks
    #[arg(long, conflicts_with = "logout")]
    no_logs: bool,

    /// Request timeout in seconds
    #[arg(long, default_value_t = 30)]
    timeout_seconds: u64,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Shop account login and rewards
    Shop(reward::ShopArgs),
    /// Query MyWIFI card usage
    Usage(usage::UsageArgs),
}

struct Artifacts {
    directory: PathBuf,
    log: PathBuf,
    headers: PathBuf,
    page: PathBuf,
    response: PathBuf,
}

impl Artifacts {
    fn create() -> Result<Self> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("system clock is before Unix epoch")?
            .as_secs();
        let directory = std::env::temp_dir().join(format!("wi-mesh-login-{stamp}"));
        fs::create_dir_all(&directory).context("creating debug-artifact directory")?;
        let result = Self {
            log: directory.join("login.log"),
            headers: directory.join("portal-headers.txt"),
            page: directory.join("portal-page.html"),
            response: directory.join("login-response.html"),
            directory,
        };
        fs::write(&result.log, "")?;
        Ok(result)
    }

    fn write_log(&self, message: impl AsRef<str>) {
        let message = message.as_ref();
        println!("{message}");
        if let Ok(mut file) = fs::OpenOptions::new().append(true).open(&self.log) {
            use std::io::Write;
            let _ = writeln!(file, "{message}");
        }
    }
}

fn main() -> Result<()> {
    let args = Args::parse();
    match args.command {
        Some(Command::Shop(shop_args)) => return reward::run(shop_args),
        Some(Command::Usage(usage_args)) => return usage::run(usage_args),
        None => {}
    }

    if args.logout {
        let artifacts = start_artifacts(&args)?;
        return logout(&build_client(&args)?, &artifacts);
    }

    if args.auto {
        if let Some(state_dir) = &args.state_dir {
            reward::set_state_dir(state_dir.clone());
        }
        if !ssid_allowed(&args) {
            return Ok(());
        }
        let retry_after = Duration::from_secs(args.retry_after.unwrap_or(10));
        if args.once {
            return auto_once(&args, retry_after, args.attempts.unwrap_or(3));
        }
        let interval = Duration::from_secs(args.interval.unwrap_or(60));
        let client = build_client(&args)?;
        let _ = reward::fetch_active_card(None);
        println!(
            "Auto login running, checking every {}s. Press Ctrl+C to stop.",
            interval.as_secs()
        );
        loop {
            if internet_ok(&client) {
                thread::sleep(interval);
                continue;
            }
            match auto_login(&args) {
                Ok(()) => thread::sleep(interval),
                Err(error) => {
                    eprintln!(
                        "Auto login failed: {error:#}. Retrying in {}s...",
                        retry_after.as_secs()
                    );
                    thread::sleep(retry_after);
                }
            }
        }
    }

    let username = args.username.as_deref().expect("clap requires a username");
    let password = args.password.as_deref().expect("clap requires a password");
    login(&args, username, password)
}

/// Gate auto mode on the Wi-Fi name; without `--current-ssid` only Android skips
fn ssid_allowed(args: &Args) -> bool {
    let Some(current) = args.current_ssid.as_deref() else {
        if cfg!(target_os = "android") {
            println!("No --current-ssid given, skipping auto login.");
            return false;
        }
        return true;
    };
    // Android's WifiInfo.getSSID() wraps the name in quotes
    let current = current
        .strip_prefix('"')
        .and_then(|name| name.strip_suffix('"'))
        .unwrap_or(current);
    if args.ssids.iter().any(|ssid| ssid == current) {
        return true;
    }
    println!("Wi-Fi '{current}' is not in the --ssid list, skipping auto login.");
    false
}

/// One-shot auto login for event-driven callers such as an Android NetworkCallback
fn auto_once(args: &Args, retry_after: Duration, attempts: u32) -> Result<()> {
    let client = build_client(args)?;
    for attempt in 1..=attempts {
        if internet_ok(&client) {
            println!("Internet connectivity already works.");
            return Ok(());
        }
        match auto_login(args) {
            Ok(()) => return Ok(()),
            Err(error) if attempt < attempts => {
                eprintln!(
                    "Auto login failed ({attempt}/{attempts}): {error:#}. Retrying in {}s...",
                    retry_after.as_secs()
                );
                thread::sleep(retry_after);
            }
            Err(error) => return Err(error.context(format!("auto login failed {attempts} times"))),
        }
    }
    unreachable!("attempts is at least 1")
}

fn auto_login(args: &Args) -> Result<()> {
    let (card, cached) = match reward::fetch_active_card(None) {
        Ok(card) => (card, false),
        Err(error) => {
            eprintln!("Shop unreachable ({error:#}), using cached card.");
            (reward::cached_card()?, true)
        }
    };
    login(args, &card.0, &card.1)?;
    if cached {
        let _ = reward::fetch_active_card(None);
    }
    Ok(())
}

fn start_artifacts(args: &Args) -> Result<Artifacts> {
    let artifacts = Artifacts::create()?;
    artifacts.write_log("[0/5] Captive portal login");
    artifacts.write_log(format!(
        "interface: {}",
        args.interface.as_deref().unwrap_or("default route")
    ));
    artifacts.write_log(format!("artifacts: {}", artifacts.directory.display()));
    Ok(artifacts)
}

fn login(args: &Args, username: &str, password: &str) -> Result<()> {
    if args.no_logs {
        let client = build_client(args)?;
        let response = client
            .get(args.entry_url.clone())
            .send()
            .context("loading captive portal page")?;
        let page_url = response.url().clone();
        let page = response.text().context("reading captive portal page")?;
        let (action, payload) = build_login_payload(&page, &page_url, username, password)?;
        client
            .post(action)
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header(header::ORIGIN, "https://login.net.vn")
            .header(header::REFERER, "https://login.net.vn/login")
            .body(payload)
            .send()
            .context("posting login form")?
            .error_for_status()
            .context("login request returned an HTTP error")?;
        return Ok(());
    }

    let artifacts = start_artifacts(args)?;
    let client = build_client(args)?;

    artifacts.write_log("[1/5] Checking internet connectivity...");
    if internet_ok(&client) {
        artifacts.write_log("Internet connectivity already works.");
        return Ok(());
    }

    artifacts.write_log("[2/5] Loading captive portal page...");
    let response = client.get(args.entry_url.clone()).send();
    let response = match response {
        Ok(response) => response,
        Err(error) => bail!("could not load captive portal page: {error}"),
    };
    let page_url = response.url().clone();
    let headers = format_headers(response.headers());
    let page = response.text().context("reading captive portal page")?;
    fs::write(&artifacts.headers, headers)?;
    fs::write(&artifacts.page, &page)?;

    artifacts.write_log("[3/5] Building login request...");
    let (action, payload) = build_login_payload(&page, &page_url, username, password)?;
    artifacts.write_log(format!("login action: {action}"));

    artifacts.write_log("[4/5] Posting login form...");
    let response = client
        .post(action)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(header::ORIGIN, "https://login.net.vn")
        .header(header::REFERER, "https://login.net.vn/login")
        .body(payload)
        .send()
        .context("posting login form")?;
    let login_body = response.text().context("reading login response")?;
    fs::write(&artifacts.response, &login_body)?;

    artifacts.write_log("[5/5] Testing internet connectivity...");
    thread::sleep(Duration::from_secs(2));
    if internet_ok(&client) {
        artifacts.write_log("Internet connectivity works.");
        Ok(())
    } else if login_body.contains("Total traffic limit reached!") {
        bail!(
            "Card has reached the limits, please use another one. Debug artifacts: {}",
            artifacts.directory.display()
        )
    } else {
        bail!(
            "Login was posted, but internet connectivity still failed. Debug artifacts: {}",
            artifacts.directory.display()
        )
    }
}

fn logout(client: &Client, artifacts: &Artifacts) -> Result<()> {
    artifacts.write_log("[logout] Ending captive portal session...");
    let response = client
        .get(LOGOUT_URL)
        .header(header::REFERER, "https://login.net.vn/status")
        .send()
        .context("requesting captive portal logout")?;
    let status = response.status();
    let final_url = response.url().clone();
    fs::write(&artifacts.headers, format_headers(response.headers()))?;
    fs::write(
        &artifacts.response,
        response.text().context("reading logout response")?,
    )?;
    artifacts.write_log(format!("[logout] Completed: {status} ({final_url})"));
    Ok(())
}

fn build_client(args: &Args) -> Result<Client> {
    let mut builder = Client::builder()
        .cookie_store(true)
        .redirect(Policy::limited(10))
        .timeout(Duration::from_secs(args.timeout_seconds))
        .user_agent(USER_AGENT);

    if let Some(interface) = &args.interface {
        let address = interface_address(interface)?;
        builder = builder.local_address(Some(address));
    }

    builder.build().context("building HTTP client")
}

fn interface_address(interface: &str) -> Result<IpAddr> {
    let addresses = get_if_addrs().context("listing network interfaces")?;
    addresses
        .into_iter()
        .find(|address| {
            address.name == interface
                && !address.is_loopback()
                && matches!(address.ip(), IpAddr::V4(_))
        })
        .map(|address| address.ip())
        .with_context(|| format!("no non-loopback IPv4 address found for interface '{interface}'"))
}

fn internet_ok(client: &Client) -> bool {
    client
        .get("https://example.com")
        .timeout(Duration::from_secs(8))
        .send()
        .and_then(|response| response.error_for_status())
        .is_ok()
}

fn build_login_payload(
    page: &str,
    page_url: &Url,
    username: &str,
    password: &str,
) -> Result<(Url, String)> {
    let document = Html::parse_document(page);
    let form_selector = Selector::parse("form#login-user").expect("valid static selector");
    let input_selector = Selector::parse("input[name]").expect("valid static selector");
    let form = document
        .select(&form_selector)
        .next()
        .context("could not find login-user form")?;
    let action = form
        .value()
        .attr("action")
        .context("login form has no action")?;
    let action = page_url
        .join(action)
        .context("resolving login form action")?;

    let mut fields = Vec::new();
    let mut has_username = false;
    let mut has_password = false;
    let mut has_popup = false;
    for input in form.select(&input_selector) {
        let value = input.value();
        let name = value.attr("name").expect("selector requires name");
        let input_type = value.attr("type").unwrap_or("text").to_ascii_lowercase();
        if matches!(
            input_type.as_str(),
            "submit" | "button" | "reset" | "image" | "file"
        ) {
            continue;
        }
        let field_value = match name.to_ascii_lowercase().as_str() {
            "username" => {
                has_username = true;
                username
            }
            "password" => {
                has_password = true;
                password
            }
            "popup" => {
                has_popup = true;
                value.attr("value").unwrap_or("")
            }
            _ => value.attr("value").unwrap_or(""),
        };
        fields.push((name, field_value));
    }
    if !has_username {
        fields.push(("username", username));
    }
    if !has_password {
        fields.push(("password", password));
    }
    if !has_popup {
        fields.push(("popup", "true"));
    }

    Ok((
        action,
        form_urlencoded::Serializer::new(String::new())
            .extend_pairs(fields)
            .finish(),
    ))
}

fn format_headers(headers: &header::HeaderMap) -> String {
    headers
        .iter()
        .map(|(name, value)| format!("{}: {}", name, value.to_str().unwrap_or("<non-UTF-8>")))
        .collect::<Vec<_>>()
        .join("\n")
}
