use crate::{error::AppError, sidecar::collect_usage, usage::UsageSummary};

/// Runs the real ccusage sidecar (Claude and Codex nightly reports) and folds
/// the results through the adapter. Executes on a background runtime thread so
/// the window never blocks on the subprocesses.
#[tauri::command]
pub async fn get_usage_summary() -> Result<UsageSummary, AppError> {
    collect_usage()
}
