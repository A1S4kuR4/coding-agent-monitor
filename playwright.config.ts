import { defineConfig } from "@playwright/test";

/**
 * Browser E2E for the dashboard. Reuses the Vite dev server already pinned to
 * the Tauri port (strictPort 1420) and drives the real React app through the
 * `e2e.html` harness, which boots it with mocked Tauri IPC against a fixed
 * fixture. Tests force the dark color scheme so the dark-warm presentation is
 * deterministic.
 *
 * Uses the system Edge/Chrome channel so no Playwright browser download is
 * needed on Windows.
 */
export default defineConfig({
  testDir: "./e2e",
  timeout: 45_000,
  fullyParallel: true,
  reporter: [["list"]],
  use: {
    baseURL: "http://127.0.0.1:1420",
    colorScheme: "dark",
    channel: "msedge",
    // Text-only verification only: never emit screenshot/video/trace artifacts,
    // including on failure.
    screenshot: "off",
    video: "off",
    trace: "off",
  },
  webServer: {
    command: "pnpm dev",
    url: "http://127.0.0.1:1420",
    reuseExistingServer: true,
    timeout: 60_000,
  },
});