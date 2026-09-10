use tauri::{AppHandle, Emitter, Manager};

use crate::{
    tray,
    usage::{CollectionCoordinator, RefreshTrigger, UsageCollectionState},
};

pub const USAGE_STATE_UPDATED_EVENT: &str = "usage-state-updated";

/// Returns current in-memory state without starting a worker. A window reads
/// this after subscribing, so it never depends on an earlier tray event.
#[tauri::command]
pub fn get_usage_state(app: AppHandle) -> UsageCollectionState {
    app.state::<CollectionCoordinator>().current_state()
}

/// Starts or joins the process-wide stateful refresh. Both the in-progress and
/// final states use the same event and tray projection as background refreshes.
#[tauri::command]
pub async fn refresh_usage_state(app: AppHandle, trigger: RefreshTrigger) -> UsageCollectionState {
    refresh_and_publish(&app, trigger)
}

pub(crate) fn refresh_and_publish(
    app: &AppHandle,
    trigger: RefreshTrigger,
) -> UsageCollectionState {
    let coordinator = app.state::<CollectionCoordinator>();
    coordinator.refresh(trigger, |state| publish_state(app, state))
}

fn publish_state(app: &AppHandle, state: &UsageCollectionState) {
    tray::apply_collection_state(app, state);
    let _ = app.emit(USAGE_STATE_UPDATED_EVENT, state);
}

/// Only a user history action calls this command. Collection stays off the UI
/// runtime; errors expose a stable safe category only.
#[tauri::command]
pub async fn get_usage_history(days: u8) -> Result<crate::usage::HistoryUsage, &'static str> {
    if days != 7 && days != 30 {
        return Err("invalidRange");
    }
    tauri::async_runtime::spawn_blocking(move || {
        let today = chrono::Local::now().date_naive();
        let scope = crate::usage::UsageScope {
            start_date: (today - chrono::Duration::days(i64::from(days) - 1)).to_string(),
            end_date: today.to_string(),
            time_zone: crate::usage::state::system_time_zone(),
        };
        crate::collector::worker_runner::collect_history_for_scope(&scope).map_err(|_| "failed")
    })
    .await
    .map_err(|_| "failed")?
}
