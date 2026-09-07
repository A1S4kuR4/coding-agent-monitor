//! Preference commands (T05).
//!
//! The webview reads the persisted preference once at mount and patches named
//! fields when the user changes them. Side effects that touch the OS (the
//! autostart Run key, the tray language, hiding the window) are applied
//! before the value is persisted, so a failed side effect is never recorded
//! as a preference the system does not honour.

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
    if let Some(enable) = patch.start_with_windows {
        autostart::set_enabled(enable)?;
    }
    let manager = app.state::<PreferencesManager>();
    let previous = manager.get();
    let updated = manager.update(patch);

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
pub fn acknowledge_close_notice(app: AppHandle) -> Preferences {
    let manager = app.state::<PreferencesManager>();
    let updated = manager.update(PreferencesPatch {
        close_notice_acknowledged: Some(true),
        ..Default::default()
    });
    crate::hide_main_window(&app);
    updated
}

/// Hides the main window to the tray without acknowledging the notice.
#[tauri::command]
pub fn hide_main_window(app: AppHandle) {
    crate::hide_main_window(&app);
}
