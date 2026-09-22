use anyhow::{Context, Result, bail};
use clap::Args;
use md5::{Digest, Md5};
use reqwest::{blocking::Client, redirect::Policy};
use serde_json::{Value, json};
use std::{
    fs,
    io::{self, IsTerminal, Write},
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const API: &str = "https://shop.login.net.vn/apimobi/v1";
const CREDENTIAL_SERVICE: &str = "wi-mesh-login";
const CREDENTIAL_USER: &str = "shop";

#[derive(Debug, Args)]
#[command(about = "Log in to the Wi-MESH shop or claim points")]
pub struct ShopArgs {
    /// Log in to shop.login.net.vn
    #[arg(
        long,
        required_unless_present_any = ["reward", "forget"],
        conflicts_with_all = ["reward", "forget"]
    )]
    login: bool,
    /// Claim daily task
    #[arg(long, conflicts_with = "forget")]
    reward: bool,
    /// Delete saved credentials
    #[arg(long)]
    forget: bool,

    /// Directory for per-account device IDs
    #[arg(long)]
    state_dir: Option<PathBuf>,
}

fn post(
    client: &Client,
    endpoint: &str,
    device: &str,
    token: &str,
    fields: &[(&str, &str)],
) -> Result<Value> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_millis()
        .to_string();
    let response = client
        .post(format!("{API}/{endpoint}"))
        .header("accept", "application/json")
        .header("origin", "https://shop.login.net.vn")
        .header("referer", "https://shop.login.net.vn/webview/reward")
        .header("wm-device", device)
        .header("wm-token", token)
        .header("wm-time", now)
        .form(fields)
        .send()
        .with_context(|| format!("requesting {endpoint}"))?
        .error_for_status()
        .with_context(|| format!("HTTP error from {endpoint}"))?;
    serde_json::from_str(&response.text()?).context("invalid JSON response from shop")
}

fn data(reply: &Value) -> Result<&Value> {
    if reply["result"] != true {
        bail!(
            "shop rejected request (code {}): {}",
            reply["code"],
            reply["msg"].as_str().unwrap_or("unknown error")
        );
    }
    reply.get("data").context("response missing data")
}

fn token(value: &Value) -> Result<&str> {
    value["Token"]
        .as_str()
        .filter(|s| !s.is_empty())
        .context("response missing authentication token")
}

fn finished(task: &Value) -> Result<bool> {
    Ok(task["FinishAt"]
        .as_u64()
        .context("task missing valid FinishAt")?
        > 0)
}

fn device_id(directory: &std::path::Path, phone: &str) -> Result<String> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(directory)?;
    let path = directory.join(format!("{:x}.device", Md5::digest(phone.as_bytes())));
    if path.exists() {
        let value = fs::read_to_string(&path)?;
        if value.len() != 32 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
            bail!("invalid stored device ID");
        }
        return Ok(value);
    }
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes)
        .map_err(|error| anyhow::anyhow!("generating device ID: {error}"))?;
    let value: String = bytes.iter().map(|b| format!("{b:02X}")).collect();
    use std::io::Write;
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)?.write_all(value.as_bytes())?;
    Ok(value)
}

fn default_state_dir() -> Result<PathBuf> {
    #[cfg(windows)]
    let root = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .context("set --state-dir when LOCALAPPDATA is unavailable")?;
    #[cfg(not(windows))]
    let root = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))
        .context("set --state-dir when HOME/XDG_STATE_HOME are unavailable")?;
    Ok(root.join("wi-mesh-login").join("reward"))
}

fn credential_entry() -> Result<keyring::Entry> {
    keyring::Entry::new(CREDENTIAL_SERVICE, CREDENTIAL_USER)
        .context("opening the operating system credential store")
}

fn prompt_credentials() -> Result<(String, String)> {
    if !io::stdin().is_terminal() {
        bail!("shop login requires an interactive terminal");
    }
    print!("Username: ");
    io::stdout().flush()?;
    let mut phone = String::new();
    io::stdin()
        .read_line(&mut phone)
        .context("reading shop username")?;
    let phone = phone.trim().to_owned();
    if phone.is_empty() {
        bail!("shop phone number is empty");
    }
    let password =
        rpassword::prompt_password("Password: ").context("reading hidden shop password")?;
    if password.is_empty() {
        bail!("shop password is empty");
    }
    Ok((phone, password))
}

fn save_credentials(phone: &str, password: &str) -> Result<()> {
    credential_entry()?
        .set_password(&format!("{phone}\n{password}"))
        .context("saving shop credentials in the operating system credential store")
}

fn load_credentials() -> Result<(String, String)> {
    let stored = credential_entry()?
        .get_password()
        .context("loading saved shop credentials, run `wi-mesh-login shop --login` first")?;
    let (phone, password) = stored
        .split_once('\n')
        .context("saved shop credentials have an invalid format, run `shop --login` again")?;
    if phone.is_empty() || password.is_empty() {
        bail!("saved shop credentials are empty, run `shop --login` again");
    }
    Ok((phone.to_owned(), password.to_owned()))
}

pub(crate) struct AuthenticatedSession {
    pub(crate) client: Client,
    pub(crate) device: String,
    pub(crate) token: String,
}

fn authenticate(
    phone: &str,
    password: &str,
    state_dir: Option<&std::path::Path>,
) -> Result<AuthenticatedSession> {
    let directory = match state_dir {
        Some(path) => path.to_owned(),
        None => default_state_dir()?,
    };
    let device = device_id(&directory, phone)?;
    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .redirect(Policy::none())
        .user_agent("Mozilla/5.0")
        .build()?;
    let device_info = json!({"Id":"", "Code":"", "UserId":0, "DeviceName":"WEB_APP", "DeviceId":device,
        "OS":"WEB", "OSVersion":"", "AppVersion":"1.0", "Network":"WIFI", "Type":"LAPTOP", "Status":"",
        "CreatedAt":0, "UpdatedAt":0, "DeletedAt":0, "ExpiredAt":0}).to_string();
    let initial = post(
        &client,
        "token",
        &device,
        "",
        &[("deviceinfo", &device_info)],
    )?;
    let initial_token = token(data(&initial)?)?;
    let password_hash = format!("{:X}", Md5::digest(password.as_bytes()));
    let login = post(
        &client,
        "login",
        &device,
        initial_token,
        &[("phone", phone), ("password", &password_hash)],
    )?;
    let authenticated = token(&data(&login)?["Token"])?;
    Ok(AuthenticatedSession {
        client,
        device,
        token: authenticated.to_owned(),
    })
}

pub(crate) fn authenticate_saved(
    state_dir: Option<&std::path::Path>,
) -> Result<AuthenticatedSession> {
    let (phone, password) = load_credentials()?;
    authenticate(&phone, &password, state_dir)
}

pub fn run(args: ShopArgs) -> Result<()> {
    if args.forget {
        credential_entry()?
            .delete_credential()
            .context("deleting saved shop credentials")?;
        println!("Saved shop credentials deleted.");
        return Ok(());
    }
    let (phone, password) = if args.login {
        prompt_credentials()?
    } else {
        load_credentials()?
    };
    let session = authenticate(&phone, &password, args.state_dir.as_deref())?;
    if args.login {
        save_credentials(&phone, &password)?;
        #[cfg(target_os = "windows")]
        println!("Shop login successful, saved in Credential Manager");
        #[cfg(target_os = "linux")]
        println!("Shop login successful, saved in OS credential store");
        return Ok(());
    }
    let rewards = post(
        &session.client,
        "rewards",
        &session.device,
        &session.token,
        &[],
    )?;
    let tasks = data(&rewards)?["Tasks"]
        .as_array()
        .context("rewards response missing Tasks")?;
    for task in tasks {
        finished(task)?;
        if !finished(task)? && task["Key"].as_str().filter(|s| !s.is_empty()).is_none() {
            bail!("unclaimed task missing key");
        }
    }
    if tasks.is_empty() {
        println!("No reward tasks available.");
    }
    for task in tasks {
        let name = task["Name"].as_str().unwrap_or("Unnamed task");
        if finished(task)? {
            println!("Already claimed: {name}");
            continue;
        }
        let key = task["Key"]
            .as_str()
            .filter(|s| !s.is_empty())
            .context("task missing key")?;
        let reply = post(
            &session.client,
            "get-score",
            &session.device,
            &session.token,
            &[("key", key)],
        )?;
        if reply["result"] == false
            && reply["code"] == 2
            && reply["msg"] == "Bạn đã nhận điểm thưởng này rồi."
        {
            println!("Already claimed: {name}");
        } else {
            data(&reply)?;
            println!("{}", reply["msg"].as_str().unwrap_or("Reward claimed."));
        }
    }
    Ok(())
}
