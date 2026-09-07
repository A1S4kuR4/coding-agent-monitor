import { invoke } from "@tauri-apps/api/core";
import type { AppPreferences, PreferencesPatch } from "../types/preferences";

export function getPreferences(): Promise<AppPreferences> {
  return invoke<AppPreferences>("get_preferences");
}

/** Applies a partial update. The Rust side applies OS effects (the Windows
 * Run key, tray language) before persisting, so a rejection means nothing
 * was recorded and callers can revert their optimistic state. */
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
