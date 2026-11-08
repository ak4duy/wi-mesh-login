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

# Keep running: check internet every 60s, log in with the in-use cards when logged out
wi-mesh-login --auto --interval 60 --retry-after 10 --no-logs

# Also claim the daily shop reward once per day (uses saved shop credentials)
wi-mesh-login --auto --claim-reward --no-logs
```

Use `wi-mesh-login --help` to see command-line options. The usage is:

```text
wi-mesh-login [OPTIONS] <USERNAME> <PASSWORD>
wi-mesh-login [OPTIONS] --auto [--claim-reward] [--interval SECONDS] [--retry-after SECONDS] [--max-retry-after SECONDS]
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
| `--auto`            | Keep running, re-login with in-use cards when logged out         |
| `--interval`        | Seconds between internet checks in `--auto` mode (default 60)    |
| `--retry-after`     | Seconds before the first retry of a failed `--auto` login, doubled on each failure (default 10) |
| `--max-retry-after` | Longest wait between failed `--auto` retries (default 300)       |
| `--claim-reward`    | Claim the daily shop reward once per day (see the auto login service below) |

### Auto mode

When the internet check fails, `--auto` asks `https://login.net.vn/status` what is wrong before logging in:

- **Logged out** (no `Xin Chào: <user>` on the status page): log in with a saved card.
- **Still logged in**: the upstream is down, so it waits instead of logging in again.
- **Portal unreachable**: no link to the Wi-MESH network, so it waits.

Failures back off exponentially from `--retry-after` up to `--max-retry-after`, with each wait randomly shortened by up to half. A success resets the backoff.

Cards come from the saved shop account. `--auto` keeps using the card that last worked and moves to the next in-use card when it is exhausted or rejected, wrapping around the list.

## Session status

```sh
wi-mesh-login status

wi-mesh-login status --json

wi-mesh-login status --interface wlan0 --timeout-seconds 10
```

Reads `https://login.net.vn/status` from the logged-in IP without a shop login, and prints the account, IP address, uploaded/downloaded data, connected time, and remaining time. It fails if the device is not logged in. Remaining time is the card's remaining validity, not a per-session timer: logging out and in again does not reset it.

| Option              | Purpose                                                                                            |
| ------------------- | -------------------------------------------------------------------------------------------------- |
| `--interface`       | Adapter name to use                                                                                |
| `--timeout-seconds` | HTTP timeout (default 30)                                                                          |
| `--json`            | Print `user`, `ip`, `uploaded_bytes`, `downloaded_bytes`, `connected_seconds`, `remaining_seconds` |

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

Shop requests recover from an expired session on their own. On HTTP 405 or an authorization failure (code 2), the request renews the shop token and retries once, then logs in again with the saved credentials and retries once more. Run `shop --login` again only when that re-login fails.

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

## Card MAC groups

> [!IMPORTANT]  
> **Run `wi-mesh-login shop --login` once to save your shop credentials before using these commands**

```sh
# List masked MAC groups seen on every in-use card
wi-mesh-login devices

# List masked MAC groups seen on specific cards, as JSON
wi-mesh-login devices --json CARD_PIN_1 CARD_PIN_2
```

> [!NOTE]  
> **The shop has no live device list.** `devices` groups the card's bandwidth sessions by the masked MAC label the shop returns (for example `30:C5:******40:31`), with sessions, last seen, online time, download and upload per group. It shows what was seen on the card, not what is connected right now. If a session lists several MACs separated by commas, semicolons or spaces, it counts for each.

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

## Auto login service

```sh
cp systemd/wi-mesh-login.service ~/.config/systemd/user/
systemctl --user enable --now wi-mesh-login.service
```

`--claim-reward` in the unit claims the daily reward from inside the login loop, so `wi-mesh-reward.timer` is optional. The last claim date (UTC+7) is stored in `last-claim` under the state directory with private permissions. A failed claim is logged and retried every 30 minutes without interrupting auto login. Run `wi-mesh-login shop --login` once beforehand.

To claim on a schedule without `--auto`, use the standalone timer instead:

```sh
cp systemd/wi-mesh-reward.{service,timer} ~/.config/systemd/user/
systemctl --user enable --now wi-mesh-reward.timer
```
