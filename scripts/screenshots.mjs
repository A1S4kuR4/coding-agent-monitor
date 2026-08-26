// Capture dashboard screenshots at fixed viewports for visual regression/for
// the colorblind sim, against the running `vite` dev server at e2e.html.
// Usage: node scripts/screenshots.mjs <outDir>
import { chromium } from "@playwright/test";
import { mkdirSync } from "node:fs";
import { join } from "node:path";

const outDir = process.argv[2] || "screenshots";
const base = process.env.E2E_BASE_URL || "http://127.0.0.1:1420";
mkdirSync(outDir, { recursive: true });

const viewports = [
  { name: "narrow", width: 800, height: 1200 },
  { name: "wide", width: 1400, height: 900 },
];

const browser = await chromium.launch({ channel: "msedge", headless: true });

for (const vp of viewports) {
  const ctx = await browser.newContext({
    viewport: { width: vp.width, height: vp.height },
    colorScheme: "dark",
  });
  const page = await ctx.newPage();
  await page.goto(`${base}/e2e.html`, { waitUntil: "networkidle" });
  await page.locator(".total").waitFor();
  // brief settle for fonts/layout
  await page.waitForTimeout(250);
  await page.screenshot({
    path: join(outDir, `before-${vp.name}.png`),
    fullPage: true,
  });
  await ctx.close();
}

await browser.close();
console.log(`screenshots written to ${outDir}`);