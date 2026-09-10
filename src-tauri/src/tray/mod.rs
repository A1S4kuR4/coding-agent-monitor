use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        RwLock,
    },
    thread,
    time::Duration,
};

use chrono::{DateTime, Local};
use tauri::{
    menu::{Menu, MenuBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager,
};

use crate::{
    commands::usage::refresh_and_publish,
    lang::{system_language, Language},
    usage::{FreshnessStatus, RefreshOutcome, RefreshTrigger, UsageCollectionState, UsageSnapshot},
};

const TRAY_AGENT_LIMIT: usize = 2;
const TRAY_ID: &str = "main-tray";
const SUMMARY_MENU_ID: &str = "today-summary";
const REFRESH_MENU_ID: &str = "refresh-now";
const SHOW_MENU_ID: &str = "show-dashboard";
const QUIT_MENU_ID: &str = "quit";
/// Recorded refresh policy: five minutes. The shared state uses a ten-minute
/// stale threshold, leaving room for one delayed/failed cycle.
const REFRESH_INTERVAL: Duration = Duration::from_secs(300);

static REFRESHER_STOPPED: AtomicBool = AtomicBool::new(false);
static TRAY_REFRESH_IN_PROGRESS: AtomicBool = AtomicBool::new(false);
/// The tray language is seeded once at startup from the persisted preference
/// (`system` resolves through the Windows UI language) so the tray and the
/// main window speak one language for the whole session; an explicit
/// preference change updates it (see `commands::preferences`).
static TRAY_LANGUAGE: RwLock<Option<Language>> = RwLock::new(None);

/// Seeds the tray language for this session.
pub(crate) fn set_language(lang: Language) {
    *TRAY_LANGUAGE
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(lang);
}

fn language() -> Language {
    TRAY_LANGUAGE
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .unwrap_or_else(system_language)
}

/// Bounded, localized tray strings. Never carries errors, paths or diagnostics.
struct TrayText {
    unavailable: &'static str,
    refreshing: &'static str,
    refresh_failed: &'static str,
    waiting: &'static str,
    today: &'static str,
    last_success: &'static str,
    stale: &'static str,
    refresh_now: &'static str,
    open_dashboard: &'static str,
    exit: &'static str,
    unknown_time: &'static str,
}

fn tray_text(lang: Language) -> TrayText {
    match lang {
        Language::En => TrayText {
            unavailable: "Usage unavailable",
            refreshing: "Refreshing…",
            refresh_failed: "Refresh failed",
            waiting: "Waiting to refresh",
            today: "Today",
            last_success: "Last success",
            stale: "Stale",
            refresh_now: "Refresh now",
            open_dashboard: "Open dashboard",
            exit: "Exit",
            unknown_time: "unknown",
        },
        Language::ZhCn => TrayText {
            unavailable: "无法获取用量",
            refreshing: "刷新中…",
            refresh_failed: "刷新失败",
            waiting: "等待刷新",
            today: "今日",
            last_success: "最近成功",
            stale: "旧数据",
            refresh_now: "立即刷新",
            open_dashboard: "打开主界面",
            exit: "退出",
            unknown_time: "未知",
        },
    }
}

struct TrayRefreshGuard;

impl Drop for TrayRefreshGuard {
    fn drop(&mut self) {
        TRAY_REFRESH_IN_PROGRESS.store(false, Ordering::SeqCst);
    }
}

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let text = tray_text(language());
    let menu = build_menu(app, text.unavailable, &text)?;
    let mut tray = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip(format!("Coding Agent Monitor — {}", text.unavailable))
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            REFRESH_MENU_ID => request_refresh(app, RefreshTrigger::Tray),
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
                show_dashboard(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;

    request_refresh(app, RefreshTrigger::Startup);
    spawn_periodic_refresher(app);
    Ok(())
}

fn request_refresh(app: &AppHandle, trigger: RefreshTrigger) {
    if REFRESHER_STOPPED.load(Ordering::Relaxed) {
        return;
    }
    // Bound native click/timer pressure before spawning a waiter thread. The
    // coordinator remains the process-wide single-flight shared with commands.
    if TRAY_REFRESH_IN_PROGRESS.swap(true, Ordering::SeqCst) {
        return;
    }
    let handle = app.clone();
    let spawned = thread::Builder::new()
        .name("tray-refresh".into())
        .spawn(move || {
            let _guard = TrayRefreshGuard;
            refresh_and_publish(&handle, trigger);
        });
    if spawned.is_err() {
        TRAY_REFRESH_IN_PROGRESS.store(false, Ordering::SeqCst);
    }
}

fn spawn_periodic_refresher(app: &AppHandle) {
    let handle = app.clone();
    let _ = thread::Builder::new()
        .name("tray-refresher".into())
        .spawn(move || loop {
            thread::sleep(REFRESH_INTERVAL);
            if REFRESHER_STOPPED.load(Ordering::Relaxed) {
                return;
            }
            request_refresh(&handle, RefreshTrigger::Periodic);
        });
}

fn build_menu(
    app: &AppHandle,
    summary_text: &str,
    text: &TrayText,
) -> tauri::Result<Menu<tauri::Wry>> {
    MenuBuilder::new(app)
        .text(SUMMARY_MENU_ID, summary_text)
        .separator()
        .text(REFRESH_MENU_ID, text.refresh_now)
        .text(SHOW_MENU_ID, text.open_dashboard)
        .text(QUIT_MENU_ID, text.exit)
        .build()
}

/// Shows and focuses the main window (tray menu, tray click, and the
/// single-instance activation listener all land here) and triggers a tray
/// refresh so the numbers on screen are current.
pub(crate) fn show_dashboard(app: &AppHandle) {
    request_refresh(app, RefreshTrigger::Tray);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Projects shared state into bounded, sanitized tray text. Menu/tooltip
/// failures never suppress the state event published by the caller.
pub(crate) fn apply_collection_state(app: &AppHandle, state: &UsageCollectionState) {
    let text = tray_text(language());
    let (menu_text, tooltip) = tray_strings(state, &text);
    if let Ok(menu) = build_menu(app, &menu_text, &text) {
        if let Some(tray) = app.tray_by_id(TRAY_ID) {
            let _ = tray.set_menu(Some(menu));
            let _ = tray.set_tooltip(Some(tooltip));
        }
    }
}

fn tray_strings(state: &UsageCollectionState, text: &TrayText) -> (String, String) {
    let Some(snapshot) = state.snapshot.as_ref() else {
        let status = if state.refreshing {
            text.refreshing
        } else if last_attempt_failed(state) {
            text.refresh_failed
        } else {
            text.waiting
        };
        return (
            format!("{} · {status}", text.unavailable),
            format!("Coding Agent Monitor — {} — {status}", text.unavailable),
        );
    };

    let date_label = if snapshot.scope.end_date == state.freshness.current_date
        && snapshot.scope.time_zone == state.freshness.current_time_zone
    {
        text.today.to_string()
    } else {
        snapshot.scope.end_date.clone()
    };
    let success = successful_time(snapshot, text.unknown_time);
    let mut menu = format!(
        "{date_label}: {} · {} {success}",
        format_tokens(snapshot.summary.today.total_tokens),
        text.last_success
    );
    for agent in snapshot.summary.today.agents.iter().take(TRAY_AGENT_LIMIT) {
        menu.push_str(&format!(
            " · {} {}",
            agent.display_name,
            format_tokens(agent.tokens)
        ));
    }

    let mut statuses = Vec::new();
    if state.refreshing {
        statuses.push(text.refreshing);
    } else if last_attempt_failed(state) {
        statuses.push(text.refresh_failed);
    }
    if state.freshness.status != FreshnessStatus::Fresh {
        statuses.push(text.stale);
    }
    if !statuses.is_empty() {
        menu.push_str(" · ");
        menu.push_str(&statuses.join(" · "));
    }

    let tooltip = format!(
        "Coding Agent Monitor — {date_label} {} — {} {success}{}",
        format_tokens(snapshot.summary.today.total_tokens),
        text.last_success,
        if statuses.is_empty() {
            String::new()
        } else {
            format!(" — {}", statuses.join(" — "))
        }
    );
    (menu, tooltip)
}

fn last_attempt_failed(state: &UsageCollectionState) -> bool {
    state
        .last_attempt
        .as_ref()
        .is_some_and(|attempt| attempt.outcome == RefreshOutcome::Failed)
}

fn successful_time(snapshot: &UsageSnapshot, unknown_label: &str) -> String {
    DateTime::parse_from_rfc3339(&snapshot.summary.collected_at)
        .map(|value| {
            value
                .with_timezone(&Local)
                .format(
                    if snapshot.scope.end_date == Local::now().format("%Y-%m-%d").to_string() {
                        "%H:%M"
                    } else {
                        "%Y-%m-%d %H:%M"
                    },
                )
                .to_string()
        })
        .unwrap_or_else(|_| unknown_label.to_string())
}

pub fn stop_refresher() {
    REFRESHER_STOPPED.store(true, Ordering::Relaxed);
}

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
    use crate::usage::{
        DailyUsage, FreshnessInfo, FreshnessReason, RefreshAttempt, RefreshFailureKind,
        TokenBreakdown, UsageScope, UsageSnapshot, UsageSummary,
    };

    fn state(date: &str, current_date: &str) -> UsageCollectionState {
        let day = DailyUsage {
            date: date.to_string(),
            total_tokens: 1_234,
            token_breakdown: TokenBreakdown {
                input_tokens: 1_234,
                output_tokens: 0,
                cache_read_tokens: 0,
                cache_creation_tokens: 0,
                reasoning_tokens: 0,
                unclassified_tokens: 0,
            },
            estimated_cost_usd: None,
            cost_unknown_reason: None,
            cache_read_share: None,
            agents: vec![],
        };
        let scope = UsageScope {
            start_date: date.to_string(),
            end_date: date.to_string(),
            time_zone: "UTC".to_string(),
        };
        UsageCollectionState {
            revision: 2,
            refreshing: false,
            snapshot: Some(UsageSnapshot {
                scope: scope.clone(),
                summary: UsageSummary {
                    collected_at: format!("{date}T12:00:00Z"),
                    today: day.clone(),
                    last7_days: vec![day],
                    coverage: crate::usage::CoverageInfo::complete(),
                },
            }),
            last_attempt: Some(RefreshAttempt {
                id: 1,
                trigger: RefreshTrigger::Manual,
                scope,
                started_at: format!("{date}T11:59:59Z"),
                finished_at: Some(format!("{date}T12:00:00Z")),
                outcome: RefreshOutcome::Succeeded,
                failure: None,
            }),
            freshness: FreshnessInfo {
                status: if date == current_date {
                    FreshnessStatus::Fresh
                } else {
                    FreshnessStatus::Stale
                },
                reason: if date == current_date {
                    FreshnessReason::Current
                } else {
                    FreshnessReason::DateChanged
                },
                checked_at: format!("{current_date}T12:00:00Z"),
                current_date: current_date.to_string(),
                current_time_zone: "UTC".to_string(),
                stale_after_seconds: 600,
            },
        }
    }

    #[test]
    fn formats_tokens_like_the_frontend() {
        assert_eq!(format_tokens(0), "0");
        assert_eq!(format_tokens(999), "999");
        assert_eq!(format_tokens(1_234), "1.23K");
        assert_eq!(format_tokens(5_000_000), "5M");
        assert_eq!(format_tokens(1_000_000_000), "1B");
    }

    #[test]
    fn old_day_uses_its_date_and_failure_is_sanitized() {
        let mut state = state("2026-09-05", "2026-09-06");
        let attempt = state.last_attempt.as_mut().unwrap();
        attempt.outcome = RefreshOutcome::Failed;
        attempt.failure = Some(RefreshFailureKind::Failed);
        let (menu, tooltip) = tray_strings(&state, &tray_text(Language::En));
        assert!(menu.starts_with("2026-09-05: 1.23K"));
        assert!(menu.contains("Refresh failed"));
        assert!(menu.contains("Stale"));
        assert!(tooltip.contains("2026-09-05"));
        assert!(!format!("{menu}{tooltip}").contains("secret"));
    }

    #[test]
    fn recent_failure_and_staleness_are_independent() {
        let mut state = state("2026-09-06", "2026-09-06");
        state.last_attempt.as_mut().unwrap().outcome = RefreshOutcome::Failed;
        let (menu, _) = tray_strings(&state, &tray_text(Language::En));
        assert!(menu.contains("Refresh failed"));
        assert!(!menu.contains("Stale"));
    }

    #[test]
    fn chinese_strings_stay_bounded_and_sanitized() {
        let mut state = state("2026-09-05", "2026-09-06");
        let attempt = state.last_attempt.as_mut().unwrap();
        attempt.outcome = RefreshOutcome::Failed;
        attempt.failure = Some(RefreshFailureKind::Failed);
        let (menu, tooltip) = tray_strings(&state, &tray_text(Language::ZhCn));
        assert!(menu.starts_with("2026-09-05: 1.23K"));
        assert!(menu.contains("刷新失败"));
        assert!(menu.contains("旧数据"));
        assert!(tooltip.contains("2026-09-05"));
        // No raw error or path ever enters the tray text in either language.
        assert!(!format!("{menu}{tooltip}").contains("secret"));
        assert!(!format!("{menu}{tooltip}").contains("C:\\"));
    }

    #[test]
    fn every_language_carries_the_full_bounded_menu_vocabulary() {
        for lang in [Language::En, Language::ZhCn] {
            let text = tray_text(lang);
            for value in [
                text.unavailable,
                text.refreshing,
                text.refresh_failed,
                text.waiting,
                text.today,
                text.last_success,
                text.stale,
                text.refresh_now,
                text.open_dashboard,
                text.exit,
                text.unknown_time,
            ] {
                assert!(!value.is_empty());
            }
        }
    }

    #[test]
    fn no_snapshot_states_use_the_resolved_language() {
        let mut state = state("2026-09-06", "2026-09-06");
        state.snapshot = None;
        let (menu, tooltip) = tray_strings(&state, &tray_text(Language::En));
        assert!(menu.contains("Usage unavailable"));
        assert!(menu.contains("Waiting to refresh"));
        assert!(tooltip.contains("Usage unavailable"));
        let (menu_zh, tooltip_zh) = tray_strings(&state, &tray_text(Language::ZhCn));
        assert!(menu_zh.contains("无法获取用量"));
        assert!(menu_zh.contains("等待刷新"));
        assert!(tooltip_zh.contains("无法获取用量"));
    }
}
