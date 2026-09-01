use crate::{collector::worker_runner, error::AppError, usage::UsageSummary};

/// Runs the production collection: ONE batch snapshot worker (the product EXE
/// itself, `--cam-internal-collector-worker-v1`) over the full agent registry,
/// folded into the public `UsageSummary` by the shared adapter. Executes on a
/// background runtime thread so the window never blocks on the subprocess.
///
/// Phase 5: the external ccusage sidecar supply chain (binaries, staging,
/// fetch scripts, packaging, runner) has been removed; this command's only
/// collection path is the batch worker.
#[tauri::command]
pub async fn get_usage_summary() -> Result<UsageSummary, AppError> {
    worker_runner::collect_usage()
}
