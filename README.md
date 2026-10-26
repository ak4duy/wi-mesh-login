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

# Check once, log in if offline (up to 3 attempts), then exit
wi-mesh-login --auto --once --retry-after 10 --attempts 3 --no-logs
```

Use `wi-mesh-login --help` to see command-line options. The usage is:

```text
wi-mesh-login [OPTIONS] <USERNAME> <PASSWORD>
wi-mesh-login [OPTIONS] --auto [--interval SECONDS] [--retry-after SECONDS]
wi-mesh-login [OPTIONS] --auto --once [--retry-after SECONDS] [--attempts N]
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
| `--once`            | Check once, log in if offline, retry, then exit (no polling)     |
| `--attempts`        | Login attempts before `--once` gives up (default 3)              |
| `--ssid`            | Allowed Wi-Fi name for `--auto`, repeatable (default: Wi-MESH)   |
| `--current-ssid`    | Connected Wi-Fi name, passed by the caller (required on Android) |
| `--state-dir`       | Directory for shop credentials, cached card and device IDs       |

## Android

The [`android/`](android) app makes it fully automatic: save your shop account once, then it logs in
whenever the phone joins `Wi-MESH 2.4G` or `Wi-MESH 5G`. No terminal, no Tasker.

- **Battery:** nothing runs between connections. Android wakes the app only when Wi-Fi connects,
  and it runs `--auto --once` once.
- **Storage:** plain Java on framework APIs only (no AndroidX or Kotlin), shrunk with R8, arm64 only.
  The bundled CLI is most of the APK.
- **Permissions:** allow location "all the time". Android only shows the Wi-Fi name to apps with
  location access, and without it every run is skipped.
- **Credentials** are encrypted with Android Keystore. They are decrypted into the app's private
  `--state-dir` only while the CLI runs.

Download the APK from the `wi-mesh-login-android` artifact of a CI run, or build it yourself with
the Android SDK, NDK and `rustup target add aarch64-linux-android` installed:

```sh
cd android && ./gradlew assembleRelease
# android/build/outputs/apk/release/wi-mesh-login-android-release.apk
```

The release APK is signed with your local debug key. That is fine for sideloading, not for Play.

### Using the CLI from another Android caller

```sh
wi-mesh-login --auto --once --current-ssid "$SSID" --state-dir "$DIR" --network-handle "$HANDLE"
```

- If `--current-ssid` is not in the `--ssid` allowlist, it exits 0 without any network request.
  Without `--current-ssid`, it does nothing on Android.
- `--network-handle` (Android only) takes `Network.getNetworkHandle()`. It sends every request over
  that Wi-Fi even while mobile data is the default route.
- Android has no OS keyring backend, so shop credentials are read from `credentials`
  (`PHONE\nPASSWORD`) in `--state-dir` (or `WI_MESH_LOGIN_STATE_DIR`). Linux and Windows still use
  the OS credential store.

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