use crate::{collector::worker_runner, error::AppError, usage::UsageSummary};

/// Runs the production collection: ONE batch snapshot worker (the product EXE
/// itself, `--cam-internal-collector-worker-v1`) over the full agent registry,
/// folded into the public `UsageSummary` by the shared adapter. Executes on a
/// background runtime thread so the window never blocks on the subprocess.
///
/// Phase 4B: this command no longer looks up, spawns or falls back to any
/// ccusage sidecar; the sidecar runner stays only for shadow/rollback until
/// Phase 5 removes it.
#[tauri::command]
pub async fn get_usage_summary() -> Result<UsageSummary, AppError> {
    worker_runner::collect_usage()
}
