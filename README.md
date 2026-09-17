# wi-mesh-login

A Rust CLI to login into `ex.login.net.vn`

This is for personal use on my own Linux laptop/server, so I can SSH into it and log in to my dorm network captive portal when the machine is running headless.

It is not intended to bypass payment, authentication, access control, or network policy. It only automates the same login flow I would normally complete manually in a browser using my own account credentials.

## Requirements

- [Rust](https://www.rust-lang.org/tools/install) 1.85 or newer
- Account for `Wi-MESH (VN)`

## Usage

```sh
wi-mesh-login YOUR_USERNAME YOUR_PASSWORD
```

Use `wi-mesh-login --help` to see command-line options. The usage is:

```text
wi-mesh-login [OPTIONS] <USERNAME> <PASSWORD>
wi-mesh-login [OPTIONS] --logout
```

Options include:

| Option              | Purpose                        |
| ------------------- | ------------------------------ |
| `--interface`       | Adapter name to use            |
| `--entry-url`       | Captive-portal probe URL       |
| `--timeout-seconds` | HTTP timeout                   |
| `--logout`          | End the current portal session |
