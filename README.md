# wi-mesh-login

A Rust CLI to login into `ex.login.net.vn` and `shop.login.net.vn`

This is for personal use on my own Linux laptop/server, so I can SSH into it and log in to my dorm network captive portal when the machine is running headless.

It is not intended to bypass payment, authentication, access control, or network policy. It only automates the same login flow I would normally complete manually in a browser using my own account credentials.

## Requirements

- [Rust](https://www.rust-lang.org/tools/install) 1.85 or newer
- Account for `Wi-MESH (VN)`

## Usage

```sh
wi-mesh-login YOUR_USERNAME YOUR_PASSWORD

wi-mesh-login YOUR_USERNAME YOUR_PASSWORD --no-logs

# Keep running: check internet every 60s, log in with the first in-use card when offline
wi-mesh-login --auto --interval 60 --retry-after 10 --no-logs
```

Use `wi-mesh-login --help` to see command-line options. The usage is:

```text
wi-mesh-login [OPTIONS] <USERNAME> <PASSWORD>
wi-mesh-login [OPTIONS] --auto [--interval SECONDS] [--retry-after SECONDS]
wi-mesh-login [OPTIONS] --logout
```

Options include:

| Option              | Purpose                                                          |
| ------------------- | ---------------------------------------------------------------- |
| `--interface`       | Adapter name to use                                              |
| `--entry-url`       | Captive-portal probe URL                                         |
| `--timeout-seconds` | HTTP timeout                                                     |
| `--logout`          | End the current portal session                                   |
| `--no-logs`         | Post login without logs, debug artifacts, or connectivity checks |
| `--auto`            | Keep running, re-login with the first in-use card when offline   |
| `--interval`        | Seconds between internet checks in `--auto` mode (default 60)    |
| `--retry-after`     | Seconds before retrying a failed `--auto` login (default 10)     |

## Shop commands

> [!IMPORTANT]  
> **For `shop.login.net.vn` only**

Use `wi-mesh-login shop --help` to see command-line options. The usage is:

```sh
wi-mesh-login shop [OPTIONS]

# Log in to shop.login.net.vn
wi-mesh-login shop --login

# Claim daily task
wi-mesh-login shop --reward

# Delete saved credentials
wi-mesh-login shop --forget
```

Options include:

| Option                    | Purpose                                      |
| ------------------------- | -------------------------------------------- |
| `--login`                 | Authenticate and save credentials securely   |
| `--reward`                | Claim the daily task using saved credentials |
| `--forget`                | Delete saved shop credentials                |
| `--state-dir <STATE_DIR>` | Directory for per-account device IDs         |

> [!NOTE]  
> **Each account has randomly generated device ID under**

### On Linux

```sh
$XDG_STATE_HOME/wi-mesh-login/reward
~/.local/state/wi-mesh-login/reward
```

### On Windows

```sh
%LOCALAPPDATA%\wi-mesh-login\reward
```

You can change the folder by `shop --state-dir PATH/TO/FOLDER`

## Card usage

> [!IMPORTANT]  
> **Run `wi-mesh-login shop --login` once to save your shop credentials before using this command**

```sh
# Query one card with card pin
wi-mesh-login usage CARD_PIN

# Query multiple cards and include sessions.
wi-mesh-login usage --sessions CARD_PIN_1 CARD_PIN_2
```
## Data alerts

> [!IMPORTANT]  
> **Run `wi-mesh-login shop --login` once to save your shop credentials before using this command**

`wi-mesh-login monitor` checks every in-use card and posts a Discord webhook message when a card's remaining data drops below a threshold. Each threshold alerts once per card. The quota comes from the card type (`Thẻ Tháng 150GB` is 150 GB); cards without a GB quota are skipped.

Create `~/.config/wi-mesh-login/config.toml` (`%APPDATA%\wi-mesh-login\config.toml` on Windows):

```toml
[monitor]
webhook = "https://discord.com/api/webhooks/..."
# Remaining GB that trigger an alert (default [10, 5, 2, 1, 0])
thresholds_gb = [20, 10, 5, 1]
# Discord user or role IDs to ping
mention_users = ["123456789012345678"]
mention_roles = []
# Placeholders: {mention} {pin} {type} {used} {quota} {remaining} {threshold}
message = "{mention} card **{pin}** has **{remaining} GB** left ({used}/{quota} GB)"
# Optional: only watch these cards instead of every in-use card
# pins = ["CARD_PIN"]
# Optional: override the quota read from the card type
# quota_gb = 150
```

Run it every 10 minutes with the systemd timer:

```sh
cp systemd/wi-mesh-monitor.{service,timer} ~/.config/systemd/user/
systemctl --user enable --now wi-mesh-monitor.timer
```
