use std::{
    fs,
    net::IpAddr,
    path::PathBuf,
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};
use clap::Parser;
use if_addrs::get_if_addrs;
use reqwest::{blocking::Client, header, redirect::Policy};
use scraper::{Html, Selector};
use url::{Url, form_urlencoded};

const ENTRY_URL: &str = "http://connectivitycheck.gstatic.com/generate_204";
const LOGOUT_URL: &str = "https://login.net.vn/logout";
const USER_AGENT: &str = "Mozilla/5.0";

#[derive(Debug, Parser)]
#[command(about = "Log in to ex.login.net.vn through CLI")]
struct Args {
    /// Captive portal detection URL (https://en.wikipedia.org/wiki/Captive_portal#Detection)
    #[arg(long, default_value = ENTRY_URL)]
    entry_url: Url,

    /// Network interface name to bind requests to
    #[arg(long)]
    interface: Option<String>,

    /// Wi-MESH username
    #[arg(required_unless_present = "logout", conflicts_with = "logout")]
    username: Option<String>,

    /// Wi-MESH password
    #[arg(required_unless_present = "logout", conflicts_with = "logout")]
    password: Option<String>,

    /// Logout of Wi-MESH
    #[arg(long)]
    logout: bool,

    /// Request timeout in seconds
    #[arg(long, default_value_t = 30)]
    timeout_seconds: u64,
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
    let artifacts = Artifacts::create()?;

    artifacts.write_log("[0/5] Captive portal login");
    artifacts.write_log(format!(
        "interface: {}",
        args.interface.as_deref().unwrap_or("default route")
    ));
    artifacts.write_log(format!("artifacts: {}", artifacts.directory.display()));

    let client = build_client(&args)?;

    if args.logout {
        return logout(&client, &artifacts);
    }

    let username = args
        .username
        .as_deref()
        .expect("clap requires a username");
    let password = args
        .password
        .as_deref()
        .expect("clap requires a password");

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logout() {
        let args = Args::try_parse_from(["wi-mesh-login", "--logout"]).unwrap();
        assert!(args.logout);
        assert_eq!(args.username, None);
        assert_eq!(args.password, None);
    }

    #[test]
    fn login() {
        assert!(Args::try_parse_from(["wi-mesh-login", "alice"]).is_err());
    }

    #[test]
    fn builds_payload_and_resolves_relative_action() {
        let page = r#"<form id="login-user" action="/login"><input name="token" value="abc"><input name="username"><input type="password" name="password"></form>"#;
        let (action, payload) = build_login_payload(
            page,
            &Url::parse("https://portal.test/start").unwrap(),
            "alice@example.test",
            "secret",
        )
        .unwrap();
        assert_eq!(action.as_str(), "https://portal.test/login");
        assert_eq!(
            payload,
            "token=abc&username=alice%40example.test&password=secret&popup=true"
        );
    }
}
