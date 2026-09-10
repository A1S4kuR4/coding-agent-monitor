import { invoke } from "@tauri-apps/api/core";
import type { AppPreferences, PreferencesPatch } from "../types/preferences";

export function getPreferences(): Promise<AppPreferences> {
  return invoke<AppPreferences>("get_preferences");
}

/** Persists a partial update. Rejections keep the last saved preference;
 * the Rust boundary returns only a safe error, never a filesystem path. */
export function updatePreferences(
  patch: PreferencesPatch,
): Promise<AppPreferences> {
  return invoke<AppPreferences>("update_preferences", { patch });
}

/** Hides the main window to the tray (the "hide once" close-notice action;
 * window show/hide is a native-side operation). */
export function hideMainWindow(): Promise<void> {
  return invoke<void>("hide_main_window");
}

export function acknowledgeCloseNotice(): Promise<AppPreferences> {
  return invoke<AppPreferences>("acknowledge_close_notice");
}
