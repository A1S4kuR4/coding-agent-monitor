use std::{
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::Duration,
};

use tauri::{
    menu::{Menu, MenuBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager,
};

/// Event name for a successful tray refresh; the payload is the same
/// `UsageSummary` the window renders, so a long-open window updates without
/// starting a second collector child.
const USAGE_UPDATED_EVENT: &str = "usage-updated";

use crate::collector::worker_runner;

/// How many of today's top agents to fold into the tray summary. The adapter
/// sorts agents by tokens descending, so taking the first few gives the biggest
/// contributors without letting an endless agent list overflow the menu.
const TRAY_AGENT_LIMIT: usize = 2;

const TRAY_ID: &str = "main-tray";
const SUMMARY_MENU_ID: &str = "today-summary";
const SHOW_MENU_ID: &str = "show-dashboard";
const QUIT_MENU_ID: &str = "quit";
/// Fallback text shown until the first collection succeeds.
const SUMMARY_PLACEHOLDER: &str = "Today: —";
/// Low-frequency summary refresh so the tray stays current without churn.
const REFRESH_INTERVAL: Duration = Duration::from_secs(300);

/// Runtime flag so the periodic refresher stops once the app begins to exit.
static REFRESHER_STOPPED: AtomicBool = AtomicBool::new(false);
/// Single-flight guard: only one tray refresh runs at a time. A refresh that
/// lands while one is already in flight is dropped, so fast clicks or the
/// periodic timer can never pile up collection runs or refresher threads.
static REFRESH_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

struct RefreshGuard;

impl Drop for RefreshGuard {
    fn drop(&mut self) {
        REFRESH_IN_PROGRESS.store(false, Ordering::SeqCst);
    }
}

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let menu = build_menu(app, SUMMARY_PLACEHOLDER)?;
    let mut tray = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("Coding Agent Monitor")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            SHOW_MENU_ID => show_dashboard(app),
            QUIT_MENU_ID => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                // `show_dashboard` performs the refresh; no separate call here
                // so a click triggers exactly one refresh, not two.
                let handle = tray.app_handle();
                show_dashboard(handle);
            }
        });

    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;

    refresh(app);
    spawn_periodic_refresher(app);

    Ok(())
}

/// Applies current usage to the tray menu text and tooltip. Work runs on a
/// background thread so opening the tray or a refresh never blocks the window.
fn refresh(app: &AppHandle) {
    if REFRESHER_STOPPED.load(Ordering::Relaxed) {
        return;
    }
    // Single-flight: if a refresh already is running, drop this one entirely.
    // Prevents unbounded refresher threads / collection runs on rapid clicks.
    if REFRESH_IN_PROGRESS.swap(true, Ordering::SeqCst) {
        return;
    }
    let handle = app.clone();
    let spawned = thread::Builder::new()
        .name("tray-refresh".into())
        .spawn(move || {
            let _guard = RefreshGuard;
            let summary = match worker_runner::collect_usage() {
                Ok(summary) => summary,
                // Keep the last-known text; a transient collection failure should
                // not turn the tray into an error banner.
                Err(_) => return,
            };
            // Show today's total plus the top contributors, bounded so an
            // unusually long agent list can never overflow the menu. The
            // adapter already sorts agents by tokens descending.
            let mut menu_text = format!("Today: {}", format_tokens(summary.today.total_tokens));
            let mut tooltip = format!(
                "Coding Agent Monitor — Today {}",
                format_tokens(summary.today.total_tokens)
            );
            for agent in summary.today.agents.iter().take(TRAY_AGENT_LIMIT) {
                menu_text.push_str(&format!(
                    " · {} {}",
                    agent.display_name,
                    format_tokens(agent.tokens)
                ));
                tooltip.push_str(&format!(
                    " ({} {})",
                    agent.display_name,
                    format_tokens(agent.tokens)
                ));
            }

            let menu = match build_menu(&handle, &menu_text) {
                Ok(menu) => menu,
                Err(_) => return,
            };
            if let Some(tray) = handle.tray_by_id(TRAY_ID) {
                let _ = tray.set_menu(Some(menu));
                let _ = tray.set_tooltip(Some(tooltip));
            }
            // Push the same successful snapshot to any open window so it stays
            // current without a competing frontend fetch.
            let _ = handle.emit(USAGE_UPDATED_EVENT, &summary);
        });
    if spawned.is_err() {
        REFRESH_IN_PROGRESS.store(false, Ordering::SeqCst);
    }
}

/// A single, low-frequency loop keeps the tray text current even when the user
/// never interacts. It exits cleanly once the app starts shutting down.
fn spawn_periodic_refresher(app: &AppHandle) {
    let handle = app.clone();
    let _ = thread::Builder::new()
        .name("tray-refresher".into())
        .spawn(move || loop {
            thread::sleep(REFRESH_INTERVAL);
            if REFRESHER_STOPPED.load(Ordering::Relaxed) {
                return;
            }
            refresh(&handle);
        });
}

fn build_menu(app: &AppHandle, summary_text: &str) -> tauri::Result<Menu<tauri::Wry>> {
    MenuBuilder::new(app)
        .text(SUMMARY_MENU_ID, summary_text)
        .separator()
        .text(SHOW_MENU_ID, "Open dashboard")
        .text(QUIT_MENU_ID, "Exit")
        .build()
}

fn show_dashboard(app: &AppHandle) {
    refresh(app);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Stops the periodic refresher on exit. Called from the app run loop.
pub fn stop_refresher() {
    REFRESHER_STOPPED.store(true, Ordering::Relaxed);
}

/// Mirrors the frontend `formatTokens` so tray text and the dashboard show the
/// same token strings for the same numbers.
fn format_tokens(tokens: u64) -> String {
    if tokens >= 1_000_000_000 {
        compact(tokens, 1_000_000_000, "B")
    } else if tokens >= 1_000_000 {
        compact(tokens, 1_000_000, "M")
    } else if tokens >= 1_000 {
        compact(tokens, 1_000, "K")
    } else {
        tokens.to_string()
    }
}

fn compact(tokens: u64, divisor: u64, suffix: &str) -> String {
    let mut text = format!("{:.2}", tokens as f64 / divisor as f64);
    if text.ends_with(".00") {
        text.truncate(text.len() - 3);
    }
    let bytes = text.as_bytes();
    let len = bytes.len();
    if len >= 3
        && bytes[len - 1] == b'0'
        && bytes[len - 2].is_ascii_digit()
        && bytes[len - 3] == b'.'
    {
        text.truncate(len - 1);
    }
    format!("{text}{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_tokens_like_the_frontend() {
        assert_eq!(format_tokens(0), "0");
        assert_eq!(format_tokens(999), "999");
        assert_eq!(format_tokens(1_234), "1.23K");
        assert_eq!(format_tokens(5_000_000), "5M");
        assert_eq!(format_tokens(18_449_180), "18.45M");
        assert_eq!(format_tokens(13_863_680), "13.86M");
        assert_eq!(format_tokens(32_312_860), "32.31M");
        assert_eq!(format_tokens(1_000_000_000), "1B");
    }
}
