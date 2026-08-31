// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Internal collector-worker entry (Phase 3): must be recognized BEFORE any
    // Tauri, single-instance, window, tray, app-database, refresh or sidecar
    // initialization. The worker flag is undocumented and internal; when it is
    // present the process NEVER reaches any parent-path code that could spawn
    // another worker, so recursion is impossible by construction.
    if coding_agent_monitor_lib::collector::worker::is_worker_invocation() {
        std::process::exit(coding_agent_monitor_lib::collector::worker::run_worker_stdio());
    }
    coding_agent_monitor_lib::run()
}
