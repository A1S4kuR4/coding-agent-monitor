import ReactDOM from "react-dom/client";
import { mockWindows, mockIPC } from "@tauri-apps/api/mocks";
import App from "../App";
import { e2eFixture } from "./fixture";
import type { AppPreferences } from "../types/preferences";

/**
 * Browser test hook so the real dashboard can boot under Playwright without a
 * Tauri webview. It installs the official @tauri-apps IPC mocks before the app
 * renders, and answers the state commands with a fixture injected by the test
 * as `window.__E2E_STATE__` (JSON), or with the bundled default otherwise.
 * Preferences commands answer `window.__E2E_PREFS__` or the shipped defaults,
 * echoing patches back like the real Rust command does.
 *
 * Production never takes this path: the module is only reachable from the
 * `e2e.html` entry, and the mocks replace only the IPC layer, not the real
 * data-fetching or persistence logic.
 */
const injected = (window as unknown as { __E2E_STATE__?: unknown })
  .__E2E_STATE__;
const fixture = injected ?? e2eFixture;

const defaultPreferences: AppPreferences = {
  version: 1,
  language: "system",
  startWithWindows: false,
  startHiddenToTray: false,
  closeNoticeAcknowledged: false,
  window: null,
};
const injectedPreferences: AppPreferences =
  (window as unknown as { __E2E_PREFS__?: AppPreferences }).__E2E_PREFS__ ??
  defaultPreferences;

mockWindows("main");
mockIPC(
  (cmd: string, args?: unknown) => {
    if (cmd === "get_usage_state" || cmd === "refresh_usage_state") return fixture;
    if (cmd === "get_usage_history") {
      const history = (window as unknown as { __E2E_HISTORY__?: unknown }).__E2E_HISTORY__;
      if (!history) return Promise.reject("fixture history unavailable");
      const delay = (window as unknown as { __E2E_HISTORY_DELAY__?: number }).__E2E_HISTORY_DELAY__ ?? 0;
      return new Promise(resolve => window.setTimeout(() => resolve(history), delay));
    }
    if (cmd === "get_preferences") return injectedPreferences;
    if (cmd === "update_preferences") {
      const patch =
        (args as { patch?: Partial<AppPreferences> } | undefined)?.patch ?? {};
      return { ...injectedPreferences, ...patch };
    }
    if (cmd === "hide_main_window") return null;
    return null;
  },
  { shouldMockEvents: true },
);

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <App />,
);
