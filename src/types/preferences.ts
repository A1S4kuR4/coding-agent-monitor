/** CAM-owned minimal local preferences (T05). Mirrors
 * `src-tauri/src/prefs.rs::Preferences` (schema version 1); the Rust and
 * TypeScript shapes must stay in sync. A corrupt or unknown persisted value
 * never reaches this contract: Rust falls back to defaults (unknown language
 * values fall back to `system`) before serializing. */
export type LanguagePreference = "system" | "zh-CN" | "en";

/** The window's last known NORMAL geometry in physical pixels. Never carries
 * a minimized window's coordinates; `maximized` flags the restored state. */
export interface WindowGeometry {
  x: number;
  y: number;
  width: number;
  height: number;
  maximized: boolean;
}

export interface AppPreferences {
  version: number;
  /** `system` resolves once at startup on both surfaces (v0.4 plan §4.5). */
  language: LanguagePreference;
  /** Off by default: a first manual launch must surface the main window. */
  startWithWindows: boolean;
  startHiddenToTray: boolean;
  closeNoticeAcknowledged: boolean;
  window: WindowGeometry | null;
}

/** Partial update; omitted fields stay unchanged on the Rust side. */
export interface PreferencesPatch {
  language?: LanguagePreference;
  startWithWindows?: boolean;
  startHiddenToTray?: boolean;
  closeNoticeAcknowledged?: boolean;
}
