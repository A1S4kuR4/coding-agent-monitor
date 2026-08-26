import ReactDOM from "react-dom/client";
import { mockWindows, mockIPC } from "@tauri-apps/api/mocks";
import App from "../App";
import { e2eFixture } from "./fixture";

/**
 * Browser test hook so the real dashboard can boot under Playwright without a
 * Tauri webview. It installs the official @tauri-apps IPC mocks before the app
 * renders, and answers `get_usage_summary` with a fixture injected by the test
 * as `window.__E2E_FIXTURE__` (JSON), or with the bundled default otherwise.
 *
 * Production never takes this path: the module is only reachable from the
 * `e2e.html` entry, and the mocks replace only the IPC layer, not the real
 * data-fetching or persistence logic.
 */
const injected = (window as unknown as { __E2E_FIXTURE__?: unknown })
  .__E2E_FIXTURE__;
const fixture = injected ?? e2eFixture;

mockWindows("main");
mockIPC(
  (cmd: string) => {
    if (cmd === "get_usage_summary") return fixture;
    return null;
  },
  { shouldMockEvents: true },
);

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <App />,
);