// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Internal collector-worker entry (Phase 3): must be recognized BEFORE any
    // Tauri, single-instance, window, tray, app-database, refresh or sidecar
    // initialization. The worker flag is undocumented and internal; when it is
    // present the process NEVER reaches any parent-path code that could spawn
    // another worker, so recursion is impossible by construction. The
    // single-instance guard (T05) runs strictly after this check, so internal
    // collector workers are never intercepted and can always coexist with the
    // main instance and with each other.
    if coding_agent_monitor_lib::collector::worker::is_worker_invocation() {
        std::process::exit(coding_agent_monitor_lib::collector::worker::run_worker_stdio());
    }
    // Single main-instance guard (T05): a duplicate launch signals the running
    // instance to show its dashboard and exits without touching any UI, tray
    // or database state.
    if !coding_agent_monitor_lib::single_instance::claim_main_instance() {
        return;
    }
    coding_agent_monitor_lib::run()
}
