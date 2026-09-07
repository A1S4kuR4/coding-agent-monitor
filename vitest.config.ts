import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

// Dedicated Vitest config. Scoping `include` to `src` keeps Playwright's
// `e2e/dashboard.spec.ts` (a sibling glob match under the default `**/*.spec.ts`
// pattern) out of the unit run — unit tests are "vitest", browser tests are
// "playwright", and the production Vite/dev config in vite.config.ts is left
// untouched for Tauri.
export default defineConfig({
  plugins: [react()],
  test: {
    include: ["src/**/*.{test,spec}.?(c|m)[jt]s?(x)"],
    // The WCAG contrast regression test reads the theme tokens out of App.css
    // via a `?raw` import; with CSS handling off, vitest stubs CSS imports as
    // empty strings and the test would see nothing.
    css: true,
  },
});