mod autostart;
pub mod collector;
mod commands;
mod db;
mod error;
mod lang;
mod prefs;
pub mod shutdown;
pub mod single_instance;
// The sidecar/usage modules are pub so the shadow harness (dev/test only) can
// drive both collection paths and compare their UsageSummary outputs.
pub mod sidecar;
mod tray;
pub mod usage;

use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, RunEvent, WindowEvent};

use crate::prefs::{geometry_is_restorable, MonitorRect, PreferencesManager, WindowGeometry};
use crate::usage::FileSnapshotStore;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .setup(|app| {
            db::initialize(app.handle())?;

            let data_dir = app
                .path()
                .app_data_dir()
                .map_err(|error| error.to_string())?;
            std::fs::create_dir_all(&data_dir).map_err(|error| error.to_string())?;
            app.manage(PreferencesManager::new(&data_dir));

            // Cross-restart last-success cache (T06): the coordinator owns the
            // versioned atomic snapshot file and restores the previous success
            // into the initial state, so a restart can display the last data
            // (with its original dates and freshness) before the first
            // background refresh completes.
            let coordinator = usage::CollectionCoordinator::with_store(Arc::new(
                FileSnapshotStore::new(&data_dir),
            ));
            coordinator.restore_from_store();
            app.manage(coordinator);

            // Restore the persisted (validated) window placement and honour
            // "start hidden to tray" before anything becomes visible.
            restore_window_placement(app.handle());

            // The tray speaks the persisted language for the whole session
            // (`system` resolves once through the Windows UI language).
            let prefs = app.state::<PreferencesManager>().get();
            tray::set_language(prefs.language.resolved());
            tray::setup(app.handle())?;

            // Project the restored snapshot into the tray immediately (T06):
            // before the startup refresh finishes, the menu/tooltip show the
            // last success under its original date with its real freshness.
            tray::apply_collection_state(
                app.handle(),
                &app.state::<usage::CollectionCoordinator>().current_state(),
            );

            // A duplicate launch signals this event to bring the dashboard
            // forward (see `single_instance`; the collector worker bypasses
            // this path entirely in `main`).
            let handle = app.handle().clone();
            single_instance::spawn_activation_listener(move || {
                tray::show_dashboard(&handle);
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let app = window.app_handle();
                if app
                    .state::<PreferencesManager>()
                    .get()
                    .close_notice_acknowledged
                {
                    hide_main_window(app);
                } else {
                    // First close: explain tray residency inside the window
                    // instead of hiding silently. The notice acknowledges
                    // (remembered) or hides for now.
                    let _ = app.emit(commands::preferences::CLOSE_NOTICE_EVENT, ());
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::usage::get_usage_state,
            commands::usage::get_usage_history,
            commands::usage::refresh_usage_state,
            commands::preferences::get_preferences,
            commands::preferences::update_preferences,
            commands::preferences::acknowledge_close_notice,
            commands::preferences::hide_main_window
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        if let RunEvent::ExitRequested { .. } = event {
            // Persist the last window placement, stop tray refreshes and
            // cancel any in-flight collector child (worker or, historically,
            // sidecar) so nothing survives the app.
            save_window_geometry(app_handle);
            tray::stop_refresher();
            shutdown::begin_shutdown();
        }
    });
}

/// Applies the persisted, validated window placement before the window is
/// first shown. The window config starts with `visible: false`, so this is
/// also what makes the window appear: the "start hidden to tray" preference
/// keeps it hidden, and any placement that no current monitor shows a usable
/// slice of (removed display, DPI change, corrupt values) is ignored in
/// favour of the configured centred default.
fn restore_window_placement(app: &AppHandle) {
    let prefs = app.state::<PreferencesManager>().get();
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let start_hidden = prefs.start_hidden_to_tray;

    if let Some(geometry) = prefs.window {
        let monitors = window
            .available_monitors()
            .map(|monitors| {
                monitors
                    .iter()
                    .map(|monitor| MonitorRect {
                        x: monitor.position().x,
                        y: monitor.position().y,
                        width: monitor.size().width as i32,
                        height: monitor.size().height as i32,
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if geometry_is_restorable(&geometry, &monitors) {
            let _ = window.set_size(PhysicalSize::new(
                geometry.width.max(0) as u32,
                geometry.height.max(0) as u32,
            ));
            let _ = window.set_position(PhysicalPosition::new(geometry.x, geometry.y));
            // Maximizing a hidden window is deferred to the first show.
            if geometry.maximized && !start_hidden {
                let _ = window.maximize();
            }
        }
    }

    if !start_hidden {
        let _ = window.show();
    }
}

/// Records the window's current NORMAL geometry and persists the preference
/// file. A minimized window's OS coordinates are never stored (its previous
/// normal placement is kept), and a maximized window only flags `maximized`
/// on top of its last known normal rect.
pub(crate) fn save_window_geometry(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let manager = app.state::<PreferencesManager>();
    // If the minimized/maximized state cannot be queried, saving nothing is
    // the safe outcome.
    if window.is_minimized().unwrap_or(true) {
        return;
    }
    let mut prefs = manager.get();
    if window.is_maximized().unwrap_or(false) {
        if let Some(existing) = prefs.window.as_mut() {
            existing.maximized = true;
            manager.set_window_geometry(prefs.window);
        }
        return;
    }
    if let (Ok(position), Ok(size)) = (window.outer_position(), window.inner_size()) {
        prefs.window = Some(WindowGeometry {
            x: position.x,
            y: position.y,
            width: size.width as i32,
            height: size.height as i32,
            maximized: false,
        });
        manager.set_window_geometry(prefs.window);
    }
}

/// Hides the main window to the tray, saving the normal geometry first.
pub(crate) fn hide_main_window(app: &AppHandle) {
    save_window_geometry(app);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
}
