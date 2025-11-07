mod glpi;
mod state;

use crate::glpi::{GlpiClient, Ticket};
use crate::state::{load_state, save_state, SeenState};

use anyhow::{anyhow, Result};
use dotenvy::dotenv;
use log::{error, info, warn};
use once_cell::sync::OnceCell;
use std::env;
use std::process::Command;
use std::{thread, time::Duration};

// URL template (e.g. https://your-glpi/front/ticket.form.php?id={id})
static URL_TEMPLATE: OnceCell<Option<String>> = OnceCell::new();

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    env_logger::init();
    dotenv().ok(); // loads .env if present in current directory

    // Read optional link template for the button
    let _ = URL_TEMPLATE.set(env::var("GLPI_TICKET_URL_TEMPLATE").ok());

    // Best effort: create Start Menu shortcut (AUMID) so SnoreToast buttons show up
    ensure_snore_shortcut("GlpiNotifier");

    // Manual test of a toast
    if env::args().any(|a| a == "--test-toast") {
        let dummy =
            Ticket { id: 12345, name: "Notification test".to_string(), requester: Some("Example User".to_string()) };
        if let Err(e) = show_toast(&dummy) {
            eprintln!("Toast error: {e:#}");
        }
        return Ok(());
    }

    // Configuration from .env
    let base_url = env::var("GLPI_BASE_URL").unwrap_or_default().trim().trim_end_matches('/').to_string();
    let app_token = env::var("GLPI_APP_TOKEN").ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let user_token = env::var("GLPI_USER_TOKEN").unwrap_or_default().trim().to_string();
    let poll_secs: u64 = env::var("POLL_SECONDS").ok().and_then(|s| s.trim().parse().ok()).unwrap_or(60);
    let verify_ssl = env::var("VERIFY_SSL").map(|s| s.to_lowercase() == "true").unwrap_or(true);
    let first_run_notify = env::var("FIRST_RUN_NOTIFY").map(|s| s.to_lowercase() == "true").unwrap_or(false);
    let debug_list = env::var("DEBUG_LIST").map(|s| s.to_lowercase() == "true").unwrap_or(false);

    if base_url.is_empty() || user_token.is_empty() {
        error!("Please set GLPI_BASE_URL and GLPI_USER_TOKEN in .env (no quotes, no extra spaces).");
        return Ok(());
    }

    info!("GLPI notifier starting (interval: {}s)", poll_secs);

    main_loop_with_flags(
        || false,
        first_run_notify,
        debug_list,
        base_url,
        app_token,
        user_token,
        poll_secs,
        verify_ssl,
    )
    .await;

    Ok(())
}

/// Main loop used by the console build (and previously by the Service build).
pub async fn main_loop_with_flags<F: Fn() -> bool>(
    stop_flag: F,
    mut first_run_notify: bool,
    debug_list: bool,
    base_url: String,
    app_token: Option<String>,
    user_token: String,
    poll_secs: u64,
    verify_ssl: bool,
) {
    // Attempt to read the link template even if running under Scheduled Task
    let _ = URL_TEMPLATE.get_or_init(|| env::var("GLPI_TICKET_URL_TEMPLATE").ok());
    ensure_snore_shortcut("GlpiNotifier");

    let mut client = match GlpiClient::new(base_url, app_token, user_token, verify_ssl).await {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to create GLPI client: {e:#}");
            write_heartbeat(false, 0);
            return;
        }
    };

    // Resolve field ids (includes requester)
    let (id_id, name_id, status_id, requester_id) = match async {
        client.init_session().await?;
        let ids = client
            .resolve_field_ids(&["Ticket.id", "Ticket.name", "Ticket.status", "Ticket._users_id_recipient"])
            .await?;
        let id_id = *ids.get("Ticket.id").ok_or_else(|| anyhow!("field id not found"))?;
        let name_id = *ids.get("Ticket.name").ok_or_else(|| anyhow!("field name not found"))?;
        let status_id = *ids.get("Ticket.status").ok_or_else(|| anyhow!("field status not found"))?;
        let requester_id = ids.get("Ticket._users_id_recipient").copied();
        Ok::<(i64, i64, i64, Option<i64>), anyhow::Error>((id_id, name_id, status_id, requester_id))
    }
    .await
    {
        Ok(v) => v,
        Err(e) => {
            error!("Failed to resolve fields: {e:#}");
            write_heartbeat(false, 0);
            return;
        }
    };

    let mut st: SeenState = match load_state() {
        Ok(s) => s,
        Err(e) => {
            warn!("Could not load state: {e:#}");
            SeenState::default()
        }
    };
    let mut first_run = st.seen_ticket_ids.is_empty();

    loop {
        if stop_flag() {
            let _ = client.kill_session().await;
            break;
        }

        match tick(
            &mut client,
            id_id,
            name_id,
            status_id,
            requester_id,
            &mut st,
            &mut first_run,
            &mut first_run_notify,
            debug_list,
        )
        .await
        {
            Ok(new_count) => {
                write_heartbeat(true, new_count);
            }
            Err(e) => {
                warn!("Tick error: {e:#}. Will re-authenticate on next iteration.");
                write_heartbeat(false, 0);
                let _ = client.kill_session().await;
            }
        }

        for _ in 0..poll_secs {
            if stop_flag() {
                let _ = client.kill_session().await;
                break;
            }
            thread::sleep(Duration::from_secs(1));
        }
    }
}

/// Single poll iteration: fetch New tickets, notify unseen ones. Returns number of new notifications.
async fn tick(
    client: &mut GlpiClient,
    id_id: i64,
    name_id: i64,
    status_id: i64,
    requester_id: Option<i64>,
    st: &mut SeenState,
    first_run: &mut bool,
    first_run_notify: &mut bool,
    debug_list: bool,
) -> Result<usize> {
    let tickets = client.search_new_tickets(id_id, name_id, status_id, requester_id, 200).await?;

    if debug_list {
        info!("DEBUG: {} ticket(s) with status=New", tickets.len());
        for t in tickets.iter().take(10) {
            info!("DEBUG: New -> #{} {} (by {})", t.id, t.name, t.requester.as_deref().unwrap_or("?"));
        }
    }

    if tickets.is_empty() && debug_list {
        if let Ok(recent) = client.search_recent_tickets(id_id, name_id, 10).await {
            info!("DEBUG: recent tickets (any status): {}", recent.len());
            for t in recent.iter().take(10) {
                info!("DEBUG: Recent -> #{} {}", t.id, t.name);
            }
        }
    }

    let current_ids: Vec<i64> = tickets.iter().map(|t| t.id).collect();

    if *first_run && !*first_run_notify {
        st.seen_ticket_ids.extend(current_ids);
        save_state(st)?;
        *first_run = false;
        info!("First run: marked {} 'New' tickets as seen. (FIRST_RUN_NOTIFY=false)", st.seen_ticket_ids.len());
        return Ok(0);
    } else if *first_run && *first_run_notify {
        info!("First run WITH notifications (FIRST_RUN_NOTIFY=true).");
        *first_run = false;
        *first_run_notify = false; // only notify on first iteration once
    }

    // Filter unseen -> newest first
    let mut fresh: Vec<&Ticket> = tickets.iter().filter(|t| !st.seen_ticket_ids.contains(&t.id)).collect();
    fresh.sort_by_key(|t| -t.id);

    for t in &fresh {
        show_toast(t)?;
        st.seen_ticket_ids.insert(t.id);
    }

    if !fresh.is_empty() {
        save_state(st)?;
        info!("Notified {} new ticket(s): {:?}", fresh.len(), fresh.iter().map(|t| t.id).collect::<Vec<_>>());
    }

    Ok(fresh.len())
}

/// Build and show a toast (title + subject + requester, and an optional "Open" button).
fn show_toast(t: &Ticket) -> Result<()> {
    let title = format!("GLPI: New ticket #{}", t.id);
    let requester = t.requester.as_deref().unwrap_or("Unknown");
    let msg = if t.name.is_empty() {
        format!("New ticket\nBy: {}", requester)
    } else {
        format!("{}\nBy: {}", t.name, requester)
    };

    // Build URL from template if configured
    let open_url = URL_TEMPLATE.get().and_then(|tpl| tpl.as_ref()).map(|tpl| tpl.replace("{id}", &t.id.to_string()));

    show_toast_snoretoast("GlpiNotifier", &title, &msg, t.id, open_url.as_deref())
}

/// Call snoretoast.exe to display a Windows toast with optional button and image.
fn show_toast_snoretoast(app_id: &str, title: &str, body: &str, ticket_id: i64, open_url: Option<&str>) -> Result<()> {
    let snore =
        find_snoretoast().ok_or_else(|| anyhow!("snoretoast.exe not found (place it next to the .exe or in PATH)"))?;

    let mut cmd = Command::new(snore);
    cmd.arg("-appID")
        .arg(app_id)
        .arg("-id")
        .arg(ticket_id.to_string())
        .arg("-t")
        .arg(title)
        .arg("-m")
        .arg(body)
        .arg("-d")
        .arg("short");

    if let Some(img) = ensure_logo_file() {
        log::info!("SnoreToast: attaching image {}", img);
        cmd.arg("-p").arg(img);
    }
    if open_url.is_some() {
        cmd.arg("-b").arg("Open");
    }

    let out = cmd.output()?;
    let code = out.status.code().unwrap_or(-1);

    // Accept all documented statuses
    if (0..=5).contains(&code) {
        if code == 4 {
            // ButtonPressed
            if let Some(url) = open_url {
                if let Err(e) = open_url_windows(url) {
                    warn!("Failed to open ticket URL: {e:#}");
                }
            }
        }
        let label = match code {
            0 => "Success",
            1 => "Hidden",
            2 => "Dismissed",
            3 => "TimedOut",
            4 => "ButtonPressed",
            5 => "TextEntered",
            _ => "Unknown",
        };
        log::debug!("SnoreToast: {}", label);
        return Ok(());
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    Err(anyhow!("snoretoast failed (code {:?}). STDOUT:\n{}\nSTDERR:\n{}", out.status.code(), stdout, stderr))
}

fn open_url_windows(url: &str) -> Result<()> {
    // 'start' needs an empty title "" after /C
    Command::new("cmd").args(&["/C", "start", "", url]).spawn()?;
    Ok(())
}

/// Try to locate snoretoast.exe in common places (next to exe, default install dir, PATH).
fn find_snoretoast() -> Option<String> {
    // 1) next to the notifier exe
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let cand = dir.join("snoretoast.exe");
            if cand.exists() {
                return Some(cand.to_string_lossy().into_owned());
            }
        }
    }
    // 2) typical Program Files location
    if let Ok(pf) = std::env::var("ProgramFiles") {
        let cand = std::path::Path::new(&pf).join("SnoreToast").join("snoretoast.exe");
        if cand.exists() {
            return Some(cand.to_string_lossy().into_owned());
        }
    }
    // 3) let PATH resolve it
    Some("snoretoast.exe".to_string())
}

/// Ensure a Start Menu shortcut exists with an AUMID so SnoreToast shows buttons.
fn ensure_snore_shortcut(app_id: &str) {
    if let Ok(exe) = std::env::current_exe() {
        let exe_str = exe.to_string_lossy().into_owned();
        if let Some(snore) = find_snoretoast() {
            let _ = std::process::Command::new(&snore)
                .arg("-install")
                .arg("GlpiNotifier") // shortcut name
                .arg(&exe_str) // executable path
                .arg(app_id) // AUMID
                .status();
        }
    }
}

/// Return the path to the heartbeat JSON.
fn heartbeat_path() -> Option<std::path::PathBuf> {
    let dir = dirs::data_dir()?;
    let p = dir.join("GlpiNotifier").join("heartbeat.json");
    let _ = std::fs::create_dir_all(p.parent().unwrap());
    Some(p)
}

/// Write an always-on heartbeat file with UNIX timestamp and last result.
fn write_heartbeat(ok: bool, new_count: usize) {
    use std::time::{SystemTime, UNIX_EPOCH};
    if let Some(p) = heartbeat_path() {
        let ts = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let payload = format!(r#"{{\"ts\": {ts}, \"ok\": {ok}, \"new\": {new_count}}}"#);
        let _ = std::fs::write(p, payload);
    }
}

/// Resolve a toast image to use:
/// 1) GLPI_LOGO_PATH (.env) if valid PNG
/// 2) assets/logo.png next to the exe
/// 3) %LOCALAPPDATA%/GlpiNotifier/logo.png
/// If none found, no image is attached.
fn ensure_logo_file() -> Option<String> {
    use std::path::Path;

    // 1) explicit path from .env
    if let Ok(p) = std::env::var("GLPI_LOGO_PATH") {
        let p = p.trim().to_string();
        if !p.is_empty() && Path::new(&p).exists() {
            return Some(p);
        }
    }

    // 2) assets/logo.png next to exe
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let cand1 = dir.join("assets").join("logo.png");
            if cand1.exists() {
                return Some(cand1.to_string_lossy().into_owned());
            }
            let cand2 = dir.join("logo.png");
            if cand2.exists() {
                return Some(cand2.to_string_lossy().into_owned());
            }
        }
    }

    // 3) LOCALAPPDATA cache
    if let Some(ld) = dirs::data_dir() {
        let cand = ld.join("GlpiNotifier").join("logo.png");
        if cand.exists() {
            return Some(cand.to_string_lossy().into_owned());
        }
    }

    None
}
