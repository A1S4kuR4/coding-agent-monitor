pub mod collector;
mod commands;
mod db;
mod error;
mod shutdown;
// The sidecar/usage modules are pub so the shadow harness (dev/test only) can
// drive both collection paths and compare their UsageSummary outputs.
pub mod sidecar;
mod tray;
pub mod usage;

use tauri::{RunEvent, WindowEvent};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .setup(|app| {
            db::initialize(app.handle())?;
            tray::setup(app.handle())?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![commands::usage::get_usage_summary])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|_app_handle, event| {
        if let RunEvent::ExitRequested { .. } = event {
            // Stop tray refreshes and kill any in-flight sidecar processes so
            // nothing survives the app. Kill-all is best-effort.
            tray::stop_refresher();
            sidecar::begin_shutdown();
        }
    });
}
