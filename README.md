[![Release (Windows)](https://github.com/yourname/glpi-notifier-rs/actions/workflows/release.yml/badge.svg)](https://github.com/yourname/glpi-notifier-rs/actions/workflows/release.yml)
[![CI](https://github.com/yourname/glpi-notifier-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/yourname/glpi-notifier-rs/actions/workflows/ci.yml)

# glpi-notifier-rs

Windows toast notifier for **GLPI** "New" tickets.
Runs as a **user-mode background app** via **Windows Task Scheduler** so notifications show in Action Center.

> Why Scheduled Task instead of a Windows Service? Services run in **Session 0** and cannot show toasts to the logged-on user.

## Features
- Polls GLPI `/search/Ticket` for **status = New**.
- Windows toasts via **SnoreToast**, with an **Open** button to your GLPI ticket page.
- Shows **requester** on the toast.
- **Heartbeat** file written to `%LOCALAPPDATA%\GlpiNotifier\heartbeat.json` every cycle.
- Persists **seen ticket IDs** to avoid duplicate notifications.
- Optional logo on the toast (`logo.png`).
- Zero OpenSSL hassles: uses `reqwest` with **rustls** TLS backend.

## Build
Prereqs (Windows):
- Rust stable with MSVC toolchain: `rustup toolchain install stable-x86_64-pc-windows-msvc`
- Build Tools for Visual Studio (C++): only needed for some crates; this project uses rustls to avoid OpenSSL.
- (Optional) `winres` to embed an app icon (.ico).

Build:
```powershell
cargo build --release
```

## Configure
Create a `.env` next to the EXE (the installer does this from `.env.template`):
```
GLPI_BASE_URL=https://your-domain/apirest.php
GLPI_APP_TOKEN=
GLPI_USER_TOKEN=
POLL_SECONDS=60
VERIFY_SSL=true
FIRST_RUN_NOTIFY=true
DEBUG_LIST=true
GLPI_TICKET_URL_TEMPLATE=https://your-glpi/front/ticket.form.php?id={id}
# Optional: force a toast image
# GLPI_LOGO_PATH=C:\Users\you\Pictures\logo.png
```

## Install (Scheduled Task, user-mode)
Use the helper script:
```powershell
Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass
.\scripts\install.ps1
```
This installs to `%LOCALAPPDATA%\Programs\GlpiNotifier\`, registers the Scheduled Task **GlpiNotifier** (At logon), starts it, and fires a `--test-toast`.

**SnoreToast**: place `snoretoast.exe` next to the installed EXE (the script copies it if present at repo root).

## Verify
```powershell
# one-shot status + last 80 log lines + heartbeat
& "$env:LOCALAPPDATA\Programs\GlpiNotifier\health.ps1"

# live tail
& "$env:LOCALAPPDATA\Programs\GlpiNotifier\health.ps1" -Tail
```

`heartbeat.json` example:
```json
{"ts": 1730970000, "ok": true, "new": 1}
```

## CLI
```
glpi-notifier-rs --test-toast
    Shows a sample toast (installs Start Menu shortcut/AUMID if needed)
```

## Toast image / icon
- Toast image: local **PNG**, ≤ 1024×1024, ≤ 200 KB.  
  Put `assets\logo.png` or set `GLPI_LOGO_PATH`.
- EXE icon: add `assets\app.ico` and a `build.rs` like:
```rust
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/app.ico");
        res.set("ProductName", "GlpiNotifier");
        res.set("FileDescription", "GLPI notifier for Windows");
        res.compile().expect("Failed to embed icon");
    }
}
```

## Troubleshooting
- No button on toast? Ensure Start Menu shortcut / AUMID exists. The app tries to install it at startup; log off/on once if needed.
- No toasts when running as a **Service**: by design. Use the Scheduled Task.
- GLPI 30x during `initSession`: the client follows 30x once and updates `base_url`.
- `verify_ssl=false` to accept self-signed certs (only if you understand the risks).

## License
MIT © 2025 Your Name
