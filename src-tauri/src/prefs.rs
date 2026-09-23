//! Minimal, versioned local preferences (T05).
//!
//! One CAM-owned JSON file in the app data directory holds exactly what the
//! v0.4 plan allows: launch behaviour (start with Windows, start hidden to
//! tray), the persisted language choice (`system`/`zh-CN`/`en`), whether the
//! first-close tray notice was acknowledged, and the window's normal
//! (restored) geometry. No settings centre, no sync, no generic config
//! framework.
//!
//! Safety rules:
//! - a missing, corrupt, unknown-version or partially unknown file falls back
//!   to defaults (unknown *language* values fall back to `system` per field)
//!   and never blocks startup;
//! - saving is atomic (write temp file, then rename over the target);
//! - window geometry is only restored when it demonstrably overlaps a current
//!   monitor by a usable amount, so removed displays, DPI changes and corrupt
//!   values can never strand the window off-screen;
//! - a minimized window is never recorded as the user's normal geometry (the
//!   callers skip geometry capture while minimized/maximized).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::lang::{system_language, Language};

/// Current preferences schema version. A file written for another version is
/// discarded in favour of defaults rather than guessed at.
pub const SCHEMA_VERSION: u32 = 1;
const PREFERENCES_FILE: &str = "preferences.json";

/// A monitor's physical-pixel bounds, kept as a plain struct so geometry
/// validation is a pure function testable without a Tauri runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonitorRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// The window's normal (restored) geometry in physical pixels. A minimized
/// window's coordinates are never stored here; a maximized window stores its
/// last known normal rect with `maximized: true`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowGeometry {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub maximized: bool,
}

/// Persisted language choice. `System` resolves at startup through the same
/// shared boundary the tray and the webview use ([`crate::lang`] on the Rust
/// side, `src/features/usage/i18n.ts` on the TypeScript side).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LanguagePref {
    #[default]
    System,
    ZhCn,
    En,
}

impl LanguagePref {
    /// Resolves the preference to the concrete tray/window language. The
    /// `system` default reads the Windows UI language once.
    pub fn resolved(self) -> Language {
        match self {
            LanguagePref::System => system_language(),
            LanguagePref::ZhCn => Language::ZhCn,
            LanguagePref::En => Language::En,
        }
    }
}

impl Serialize for LanguagePref {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let tag = match self {
            LanguagePref::System => "system",
            LanguagePref::ZhCn => "zh-CN",
            LanguagePref::En => "en",
        };
        serializer.serialize_str(tag)
    }
}

impl<'de> Deserialize<'de> for LanguagePref {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let tag = String::deserialize(deserializer)?;
        // Unknown or damaged values fall back to `system`; they must never
        // block startup or discard the rest of the file (v0.4 plan §4.5).
        Ok(match tag.as_str() {
            "zh-CN" => LanguagePref::ZhCn,
            "en" => LanguagePref::En,
            _ => LanguagePref::System,
        })
    }
}

/// The whole preference file, version 1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Preferences {
    pub version: u32,
    pub language: LanguagePref,
    /// Start when Windows starts. Default off: a first manual launch must
    /// surface the main window.
    pub start_with_windows: bool,
    /// Hide to the tray when the app starts. Default off.
    pub start_hidden_to_tray: bool,
    /// The first close-to-tray explanation was acknowledged; later closes
    /// hide silently.
    pub close_notice_acknowledged: bool,
    /// Last known normal window geometry, `None` until the first save.
    pub window: Option<WindowGeometry>,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            version: SCHEMA_VERSION,
            language: LanguagePref::System,
            start_with_windows: false,
            start_hidden_to_tray: false,
            close_notice_acknowledged: false,
            window: None,
        }
    }
}

/// A partial update from the window. `None` fields stay unchanged.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreferencesPatch {
    pub language: Option<LanguagePref>,
    pub start_with_windows: Option<bool>,
    pub start_hidden_to_tray: Option<bool>,
    pub close_notice_acknowledged: Option<bool>,
}

impl Preferences {
    fn apply_patch(&mut self, patch: PreferencesPatch) {
        if let Some(language) = patch.language {
            self.language = language;
        }
        if let Some(value) = patch.start_with_windows {
            self.start_with_windows = value;
        }
        if let Some(value) = patch.start_hidden_to_tray {
            self.start_hidden_to_tray = value;
        }
        if let Some(value) = patch.close_notice_acknowledged {
            self.close_notice_acknowledged = value;
        }
    }
}

/// Loads preferences from `path`, falling back to defaults for a missing,
/// unreadable, corrupt or unknown-version file.
fn load_from_file(path: &Path) -> Preferences {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .filter(|prefs: &Preferences| prefs.version == SCHEMA_VERSION)
        .unwrap_or_default()
}

/// In-process owner of the one preference file. Load happens once at startup;
/// every mutation is followed by an atomic save.
pub struct PreferencesManager {
    inner: Mutex<Preferences>,
    path: PathBuf,
}

impl PreferencesManager {
    pub fn new(app_data_dir: &Path) -> Self {
        Self {
            inner: Mutex::new(load_from_file(&app_data_dir.join(PREFERENCES_FILE))),
            path: app_data_dir.join(PREFERENCES_FILE),
        }
    }

    /// Test seam: manager over an explicit file path.
    #[cfg(test)]
    fn at(path: PathBuf) -> Self {
        Self {
            inner: Mutex::new(load_from_file(&path)),
            path,
        }
    }

    pub fn get(&self) -> Preferences {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Serialize mutation and disk replacement; publish only a saved value.
    pub fn update(&self, patch: PreferencesPatch) -> Result<Preferences, crate::error::AppError> {
        self.update_with_startup(patch, |_| Ok(()))
    }

    pub fn update_with_startup(
        &self,
        patch: PreferencesPatch,
        mut set_startup: impl FnMut(bool) -> Result<(), crate::error::AppError>,
    ) -> Result<Preferences, crate::error::AppError> {
        let mut inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        let mut next = inner.clone();
        next.apply_patch(patch);
        if let Some(enabled) = patch.start_with_windows {
            set_startup(enabled)?;
        }
        if self.store(&next).is_err() {
            if patch.start_with_windows.is_some() && set_startup(inner.start_with_windows).is_err()
            {
                return Err(crate::error::AppError {
                    code: "preferences_rollback_failed".into(),
                    message: "Preferences could not be saved or Windows startup restored.".into(),
                });
            }
            return Err(crate::error::AppError::filesystem(
                "Preferences could not be saved.".into(),
            ));
        }
        *inner = next.clone();
        Ok(next)
    }

    /// Geometry is best effort, but a failed save must not change memory.
    pub fn set_window_geometry(&self, geometry: Option<WindowGeometry>) {
        let mut inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        let mut next = inner.clone();
        next.window = geometry;
        if self.store(&next).is_ok() {
            *inner = next;
        }
    }

    fn store(&self, prefs: &Preferences) -> std::io::Result<()> {
        save_to_file(&self.path, prefs)
    }
}

/// Atomic save: write the sibling temp file first, then rename it over the
/// target so a crash mid-write can never leave a truncated preference file.
fn save_to_file(path: &Path, prefs: &Preferences) -> std::io::Result<()> {
    let text = serde_json::to_string_pretty(prefs).map_err(std::io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)
}

/// A saved geometry is restorable only when it is self-consistent and it
/// overlaps some current monitor by at least [`MIN_VISIBLE`] physical pixels
/// in both axes — enough of the title bar and edge to grab and move the
/// window. This is what makes removed displays, DPI shifts and corrupt values
/// safe: anything else falls back to the configured default placement.
const MIN_WINDOW_SIZE: i32 = 100;
const MIN_VISIBLE: (i32, i32) = (100, 60);

pub fn geometry_is_restorable(rect: &WindowGeometry, monitors: &[MonitorRect]) -> bool {
    if rect.width < MIN_WINDOW_SIZE || rect.height < MIN_WINDOW_SIZE {
        return false;
    }
    let window = MonitorRect {
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: rect.height,
    };
    monitors.iter().any(|monitor| {
        overlap_width(&window, monitor) >= MIN_VISIBLE.0
            && overlap_height(&window, monitor) >= MIN_VISIBLE.1
    })
}

fn overlap_width(a: &MonitorRect, b: &MonitorRect) -> i32 {
    (a.x + a.width).min(b.x + b.width) - a.x.max(b.x)
}

fn overlap_height(a: &MonitorRect, b: &MonitorRect) -> i32 {
    (a.y + a.height).min(b.y + b.height) - a.y.max(b.y)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(tag: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        std::env::temp_dir().join(format!("coding-agent-monitor-prefs-{tag}-{unique}.json"))
    }

    fn geometry(x: i32, y: i32, width: i32, height: i32) -> WindowGeometry {
        WindowGeometry {
            x,
            y,
            width,
            height,
            maximized: false,
        }
    }

    fn monitor(x: i32, y: i32, width: i32, height: i32) -> MonitorRect {
        MonitorRect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn missing_file_uses_defaults() {
        let path = temp_path("missing");
        let manager = PreferencesManager::at(path);
        let prefs = manager.get();
        assert_eq!(prefs, Preferences::default());
        assert!(!prefs.start_with_windows);
        assert!(!prefs.start_hidden_to_tray);
        assert!(!prefs.close_notice_acknowledged);
        assert_eq!(prefs.language, LanguagePref::System);
        assert!(prefs.window.is_none());
    }

    #[test]
    fn save_then_load_round_trips_every_field() {
        let path = temp_path("roundtrip");
        let manager = PreferencesManager::at(path.clone());
        let saved = manager
            .update(PreferencesPatch {
                language: Some(LanguagePref::ZhCn),
                start_with_windows: Some(true),
                start_hidden_to_tray: Some(true),
                close_notice_acknowledged: Some(true),
            })
            .expect("save preferences");
        manager.set_window_geometry(Some(geometry(64, -8, 1360, 1400)));

        let reloaded = PreferencesManager::at(path.clone()).get();
        let mut expected = saved;
        expected.window = Some(geometry(64, -8, 1360, 1400));
        assert_eq!(reloaded, expected);
        assert_eq!(reloaded.window, Some(geometry(64, -8, 1360, 1400)));
        assert_eq!(reloaded.language, LanguagePref::ZhCn);

        // Nothing temporary is left behind by the atomic rename.
        assert!(!path.with_extension("json.tmp").exists());

        std::fs::remove_file(&path).expect("remove test preference file");
    }

    #[test]
    fn failed_save_preserves_memory_and_disk_and_rolls_back_startup() {
        let path = temp_path("failed-save");
        let manager = PreferencesManager::at(path.clone());
        manager
            .update(PreferencesPatch {
                language: Some(LanguagePref::En),
                ..Default::default()
            })
            .unwrap();
        let original = fs::read(&path).unwrap();
        let tmp = path.with_extension("json.tmp");
        fs::create_dir(&tmp).unwrap();
        let mut calls = Vec::new();
        let result = manager.update_with_startup(
            PreferencesPatch {
                language: Some(LanguagePref::ZhCn),
                start_with_windows: Some(true),
                ..Default::default()
            },
            |enabled| {
                calls.push(enabled);
                Ok(())
            },
        );
        assert_eq!(
            result.unwrap_err().message,
            "Preferences could not be saved."
        );
        assert_eq!(calls, vec![true, false]);
        assert_eq!(manager.get().language, LanguagePref::En);
        assert_eq!(fs::read(&path).unwrap(), original);
        manager.set_window_geometry(Some(geometry(1, 1, 680, 700)));
        assert!(manager.get().window.is_none());
        fs::remove_dir(tmp).unwrap();
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn corrupt_file_falls_back_to_defaults() {
        let path = temp_path("corrupt");
        std::fs::write(&path, "{ not json ").expect("write corrupt fixture");
        assert_eq!(
            PreferencesManager::at(path.clone()).get(),
            Preferences::default()
        );
        std::fs::remove_file(&path).expect("remove corrupt fixture");
    }

    #[test]
    fn unknown_version_is_discarded_not_guessed() {
        let path = temp_path("version");
        std::fs::write(
            &path,
            r#"{ "version": 99, "startWithWindows": true, "language": "en" }"#,
        )
        .expect("write future fixture");
        assert_eq!(
            PreferencesManager::at(path.clone()).get(),
            Preferences::default()
        );
        std::fs::remove_file(&path).expect("remove future fixture");
    }

    #[test]
    fn unknown_language_value_falls_back_per_field_without_discarding_the_file() {
        let path = temp_path("language");
        std::fs::write(
            &path,
            r#"{ "version": 1, "language": "klingon", "startHiddenToTray": true }"#,
        )
        .expect("write unknown-language fixture");
        let prefs = PreferencesManager::at(path.clone()).get();
        assert_eq!(prefs.language, LanguagePref::System);
        assert!(prefs.start_hidden_to_tray);
        std::fs::remove_file(&path).expect("remove language fixture");
    }

    #[test]
    fn patch_changes_only_named_fields() {
        let mut prefs = Preferences::default();
        prefs.apply_patch(PreferencesPatch {
            start_hidden_to_tray: Some(true),
            ..Default::default()
        });
        assert!(!prefs.start_with_windows);
        assert!(prefs.start_hidden_to_tray);
        assert_eq!(prefs.language, LanguagePref::System);
        assert!(!prefs.close_notice_acknowledged);
    }

    #[test]
    fn language_pref_serializes_with_stable_tags() {
        assert_eq!(
            serde_json::to_value(LanguagePref::ZhCn).unwrap(),
            serde_json::json!("zh-CN")
        );
        assert_eq!(
            serde_json::to_value(LanguagePref::System).unwrap(),
            serde_json::json!("system")
        );
    }

    #[test]
    fn onscreen_geometry_is_restorable() {
        let monitors = vec![monitor(0, 0, 1920, 1080)];
        assert!(geometry_is_restorable(
            &geometry(10, 10, 1360, 1400),
            &monitors
        ));
    }

    #[test]
    fn fully_offscreen_geometry_is_rejected() {
        let monitors = vec![monitor(0, 0, 1920, 1080)];
        // Entirely to the right of the only display.
        assert!(!geometry_is_restorable(
            &geometry(3000, 10, 1360, 1400),
            &monitors
        ));
        // Entirely above it.
        assert!(!geometry_is_restorable(
            &geometry(10, -2000, 1360, 1400),
            &monitors
        ));
        // No monitor information at all: never trust the saved rect.
        assert!(!geometry_is_restorable(&geometry(10, 10, 1360, 1400), &[]));
    }

    #[test]
    fn tiny_sliver_of_visibility_is_rejected() {
        let monitors = vec![monitor(0, 0, 1920, 1080)];
        // Only 40px of width remain visible — not enough to grab the window.
        assert!(!geometry_is_restorable(
            &geometry(1880, 10, 1360, 1400),
            &monitors
        ));
        // Only 20px of height remain visible.
        assert!(!geometry_is_restorable(
            &geometry(10, 1070, 1360, 1400),
            &monitors
        ));
    }

    #[test]
    fn corrupt_or_degenerate_sizes_are_rejected() {
        let monitors = vec![monitor(0, 0, 1920, 1080)];
        assert!(!geometry_is_restorable(
            &geometry(10, 10, 0, 700),
            &monitors
        ));
        assert!(!geometry_is_restorable(
            &geometry(10, 10, -500, 700),
            &monitors
        ));
        assert!(!geometry_is_restorable(
            &geometry(10, 10, 680, 0),
            &monitors
        ));
    }

    #[test]
    fn geometry_on_a_second_monitor_is_restorable() {
        let primary = monitor(0, 0, 1920, 1080);
        let secondary = monitor(1920, 0, 2560, 1440);
        let monitors = vec![primary, secondary];
        assert!(geometry_is_restorable(
            &geometry(3000, 100, 1360, 1400),
            &monitors
        ));
    }
}
