//! Preference commands (T05).
//!
//! Updates are serialized by the preference manager. Startup registration is
//! applied before the atomic save and rolled back if saving fails. Language
//! and window hiding happen only after persistence succeeds.

use tauri::{AppHandle, Manager};

use crate::{
    autostart,
    error::AppError,
    prefs::{Preferences, PreferencesManager, PreferencesPatch},
    tray,
    usage::CollectionCoordinator,
};

/// Emitted when the user closes the main window for the first time and the
/// "still running in the tray" explanation has not been acknowledged yet.
pub const CLOSE_NOTICE_EVENT: &str = "close-notice-requested";

/// Returns the current persisted preferences.
#[tauri::command]
pub fn get_preferences(app: AppHandle) -> Preferences {
    app.state::<PreferencesManager>().get()
}

/// Applies a partial preference update and persists it atomically.
///
/// Order of operations per field:
/// - `startWithWindows` writes the per-user Run key FIRST; a registry failure
///   aborts the whole update so the stored preference can never claim an
///   autostart the OS does not have;
/// - a language change re-resolves the tray language and re-renders the tray
///   text immediately, keeping the tray and the window in one language.
#[tauri::command]
pub fn update_preferences(
    app: AppHandle,
    patch: PreferencesPatch,
) -> Result<Preferences, AppError> {
    let manager = app.state::<PreferencesManager>();
    let previous = manager.get();
    let updated = manager.update_with_startup(patch, autostart::set_enabled)?;

    if updated.language != previous.language {
        tray::set_language(updated.language.resolved());
        let state = app.state::<CollectionCoordinator>().current_state();
        tray::apply_collection_state(&app, &state);
    }
    Ok(updated)
}

/// The user acknowledged the first-close tray explanation; the preference is
/// remembered and the window hides to the tray now.
#[tauri::command]
pub fn acknowledge_close_notice(app: AppHandle) -> Result<Preferences, AppError> {
    let manager = app.state::<PreferencesManager>();
    let updated = manager.update(PreferencesPatch {
        close_notice_acknowledged: Some(true),
        ..Default::default()
    })?;
    crate::hide_main_window(&app);
    Ok(updated)
}

/// Hides the main window to the tray without acknowledging the notice.
#[tauri::command]
pub fn hide_main_window(app: AppHandle) {
    crate::hide_main_window(&app);
}
