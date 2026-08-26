import { test, expect, type Page } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";

// ---------------------------------------------------------------------------
// Programmatic-only browser E2E for the dashboard. Runs against the real React
// app through e2e.html with mocked Tauri IPC and the fixed fixture. All checks
// are text/geometry/DOM — no screenshots, screenshots config is "off", and the
// axe tree is analysed as JSON. The config channel is the system Edge.
// ---------------------------------------------------------------------------

/** Boot the harness at a given viewport and wait for the fixture to render. */
async function boot(page: Page, width: number, height: number) {
  await page.setViewportSize({ width, height });
  await page.goto("/e2e.html");
  await expect(page.locator(".trend-day")).toHaveCount(7);
  await expect(page.locator(".total")).toContainText("93.89M");
}

/** Resolve a theme token to its computed rgb() string so tests compare colors
 * against the same normalization the browser reports, never raw syntax. */
async function cssColor(page: Page, token: string): Promise<string> {
  return page.evaluate((t) => {
    const value = getComputedStyle(document.documentElement)
      .getPropertyValue(t)
      .trim();
    const probe = document.createElement("span");
    probe.style.background = value || "transparent";
    document.body.appendChild(probe);
    const rgb = getComputedStyle(probe).backgroundColor;
    probe.remove();
    return rgb;
  }, token);
}

const rgbOf = (locator: ReturnType<Page["locator"]>) =>
  locator.evaluate((el) => getComputedStyle(el).backgroundColor);

/** Text colour (the `color` property), for elements like the delta badge whose
 *  value is paint, not an element background. */
const colorOf = (locator: ReturnType<Page["locator"]>) =>
  locator.evaluate((el) => getComputedStyle(el).color);

test.describe("single column (800x1200)", () => {
  test("renders one column without horizontal overflow; refresh fully visible", async ({ page }) => {
    await boot(page, 800, 1200);

    // Single-column grid.
    const cols = await page
      .locator(".dash-grid")
      .evaluate((el) => getComputedStyle(el).gridTemplateColumns.split(" ").length);
    expect(cols).toBe(1);

    // No horizontal overflow at the document level or inside the chart.
    const noDocOverflow = await page.evaluate(
      () => document.documentElement.scrollWidth <= document.documentElement.clientWidth,
    );
    expect(noDocOverflow).toBe(true);
    const noChartOverflow = await page
      .locator(".trend")
      .evaluate((el) => el.scrollWidth <= el.clientWidth);
    expect(noChartOverflow).toBe(true);

    // Refresh bounding box is fully within the viewport.
    const refresh = await page.locator(".refresh-btn").boundingBox();
    expect(refresh).not.toBeNull();
    expect(refresh!.x).toBeGreaterThanOrEqual(0);
    expect(refresh!.y).toBeGreaterThanOrEqual(0);
    expect(refresh!.x + refresh!.width).toBeLessThanOrEqual(800);
    expect(refresh!.y + refresh!.height).toBeLessThanOrEqual(1200);

    // No chip is clipped horizontally.
    const chips = page.locator(".filter-chip");
    for (let i = 0; i < (await chips.count()); i++) {
      const box = await chips.nth(i).boundingBox();
      expect(box!.width).toBeGreaterThan(0);
      expect(box!.x + box!.width).toBeLessThanOrEqual(800);
    }
  });

  test("sticky header keeps Refresh in view while scrolling", async ({ page }) => {
    // Tall single-col page so it actually scrolls.
    await boot(page, 800, 700);

    await page.evaluate(() => window.scrollTo(0, document.body.scrollHeight));
    const scrollTop = await page.evaluate(() => document.documentElement.scrollTop);
    expect(scrollTop).toBeGreaterThan(0);

    const header = await page.locator(".app-header").boundingBox();
    const vh = 700;
    expect(header!.y).toBeGreaterThanOrEqual(0);
    expect(header!.y).toBeLessThan(2); // stuck flush to the viewport top
    expect(header!.y + header!.height).toBeLessThanOrEqual(vh);

    const refresh = await page.locator(".refresh-btn").boundingBox();
    expect(refresh!.x).toBeGreaterThanOrEqual(0);
    expect(refresh!.y).toBeGreaterThanOrEqual(0);
    expect(refresh!.x + refresh!.width).toBeLessThanOrEqual(800);
    expect(refresh!.y + refresh!.height).toBeLessThanOrEqual(vh);

    // Body content is still readable below the stuck header.
    const belowVisible = await page.evaluate((hb) => {
      for (const sel of [".trend-day", ".agent-row", ".breakdown-row", ".total"]) {
        for (const el of document.querySelectorAll(sel)) {
          const r = el.getBoundingClientRect();
          if (r.bottom > hb && r.top < window.innerHeight) return true;
        }
      }
      return false;
    }, header!.y + header!.height);
    expect(belowVisible).toBe(true);
  });
});

test.describe("two column (1400x900)", () => {
  test("lays Today/Breakdown left and filter/chart right without overflow", async ({ page }) => {
    await boot(page, 1400, 900);

    const cols = await page
      .locator(".dash-grid")
      .evaluate((el) => getComputedStyle(el).gridTemplateColumns.split(" ").length);
    expect(cols).toBe(2);

    const leftBox = await page.locator(".dash-left").boundingBox();
    const rightBox = await page.locator(".dash-right").boundingBox();
    expect(leftBox!.x).toBeLessThan(rightBox!.x);

    // Today's figure + breakdown live in the left column; filter + chart in the right.
    const totals = await page.locator(".dash-left .total, .dash-left .breakdown-section").count();
    expect(totals).toBe(2);
    const rightContent = await page.locator(".dash-right .trend-filter, .dash-right .trend").count();
    expect(rightContent).toBe(2);

    const totalBox = await page.locator(".total").boundingBox();
    const trendBox = await page.locator(".trend").boundingBox();
    expect(totalBox!.x).toBeLessThan(trendBox!.x);

    // Chart height is in the expected ~320px range.
    expect(trendBox!.height).toBeGreaterThanOrEqual(280);
    expect(trendBox!.height).toBeLessThanOrEqual(360);

    const noOverflow = await page.evaluate(
      () => document.documentElement.scrollWidth <= document.documentElement.clientWidth,
    );
    expect(noOverflow).toBe(true);
  });
});

test.describe("stacked All-mode chart", () => {
  test("stacks agents bottom-up with the canonical colors and a single top rounding", async ({ page }) => {
    await boot(page, 1400, 900);

    // 08/25 (index 6) has all four agents non-zero.
    const day = page.locator(".trend-day").nth(6);
    const segments = day.locator(".bar-segment");
    expect(await segments.count()).toBe(4);

    const claude = await cssColor(page, "--agent-claude");
    const codex = await cssColor(page, "--agent-codex");
    const antigravity = await cssColor(page, "--agent-antigravity");
    const opencode = await cssColor(page, "--agent-opencode");

    // DOM order is bottom-up = canonical: claude, codex, antigravity, opencode.
    expect(await rgbOf(segments.nth(0))).toBe(claude);
    expect(await rgbOf(segments.nth(1))).toBe(codex);
    expect(await rgbOf(segments.nth(2))).toBe(antigravity);
    expect(await rgbOf(segments.nth(3))).toBe(opencode);

    // Bar height = dayTotal / windowMax => 08/25 is the max day => ~100%.
    const sum = await segments.evaluateAll((els) =>
      els.reduce((s, el) => s + parseFloat((el as HTMLElement).style.height), 0),
    );
    expect(Math.abs(sum - 100)).toBeLessThanOrEqual(0.5);

    // Only the topmost (last) segment is rounded at the top.
    expect((await rgbOf(segments.nth(3)))?.length).toBeGreaterThan(0);
    const topRadius = await segments
      .nth(3)
      .evaluate((el) => getComputedStyle(el).borderRadius);
    expect(topRadius).toContain("4px");
    const otherRadius = await segments
      .nth(0)
      .evaluate((el) => getComputedStyle(el).borderRadius);
    expect(otherRadius).toBe("0px");
  });

  test("a zero-token day renders no fake bar but stays focusable with a 0 tooltip", async ({ page }) => {
    await boot(page, 1400, 900);
    expect(await page.locator(".trend-day").nth(0).locator(".bar-segment").count()).toBe(0);

    // Hover the zero day -> tooltip reports a 0 total.
    await page.locator(".trend-day").nth(0).hover();
    await expect(page.locator(".chart-tooltip")).toBeVisible();
    await expect(page.locator(".chart-tooltip")).toContainText("0 tokens total");
  });
});

test.describe("agent filter chip", () => {
  test("Codex chip isolates a single monochrome series and All restores stacking", async ({ page }) => {
    await boot(page, 1400, 900);

    await page.getByRole("button", { name: "Codex", exact: true }).click();
    await expect(page.getByRole("button", { name: "Codex", exact: true })).toHaveAttribute("aria-pressed", "true");

    // Only Codex data segments remain: 6 non-zero days, no stacked bars.
    const segments = page.locator(".trend .bar-segment");
    expect(await segments.count()).toBe(6);
    const codex = await cssColor(page, "--agent-codex");
    const allCodex = await segments.evaluateAll(
      (els, expected) =>
        els.every((el) => getComputedStyle(el).backgroundColor === expected),
      codex,
    );
    expect(allCodex).toBe(true);

    // 08/24 hover tooltip reflects Codex-specific semantics.
    await page.locator(".trend-day").nth(5).hover();
    await expect(page.locator(".chart-tooltip")).toBeVisible();
    await expect(page.locator(".chart-tooltip")).toContainText("Codex");
    await expect(page.locator(".chart-tooltip")).toContainText("26.18M");
    await expect(page.locator(".chart-tooltip")).toContainText("of day");

    // Switching back to All restores stacking.
    await page.getByRole("button", { name: "All" }).click();
    expect(await page.locator(".trend-day").nth(6).locator(".bar-segment").count()).toBe(4);
  });
});

test.describe("header delta", () => {
  test("shows +66.2% vs yesterday in the --delta-up colour", async ({ page }) => {
    await boot(page, 1400, 900);
    const delta = page.locator(".total-delta");
    await expect(delta).toHaveText(/▲ \+66\.2% vs 昨日/);
    expect(await colorOf(delta)).toBe(await cssColor(page, "--delta-up"));
  });
});

test.describe("agent rows and caret", () => {
  test("model-carrying agents toggle; model-less agents keep a non-interactive placeholder", async ({ page }) => {
    await boot(page, 1400, 900);

    const claudeBlock = page.locator(".agent-block", { hasText: "Claude Code" });
    const claudeToggle = claudeBlock.locator("button.agent-toggle");
    await expect(claudeToggle).toHaveCount(1);
    await expect(claudeToggle).toHaveAttribute("aria-expanded", "false");
    await claudeToggle.click();
    await expect(claudeToggle).toHaveAttribute("aria-expanded", "true");

    // OpenCode has no models -> no interactive caret, only a placeholder.
    const opencodeBlock = page.locator(".agent-block", { hasText: "OpenCode" });
    await expect(opencodeBlock.locator("button.agent-toggle")).toHaveCount(0);
    await expect(opencodeBlock.locator(".chevron-placeholder")).toHaveCount(1);

    // Name start is aligned whether or not a caret is present.
    const claudeName = await claudeBlock.locator(".agent-name").boundingBox();
    const opencodeName = await opencodeBlock.locator(".agent-name").boundingBox();
    expect(Math.abs(claudeName!.x - opencodeName!.x)).toBeLessThan(1);
  });
});

test.describe("tooltip interaction", () => {
  test("hover/focus/blur/Escape drive the tooltip, which stays inside the viewport", async ({ page }) => {
    await boot(page, 1400, 900);
    const tips = page.locator(".chart-tooltip");

    // Hover shows it; moving away hides it.
    await page.locator(".trend-day").nth(5).hover();
    await expect(tips).toBeVisible();
    await page.mouse.move(0, 0);
    await expect(tips).toHaveCount(0);

    // Focus shows; blur hides.
    await page.locator(".trend-day").nth(5).focus();
    await expect(tips).toBeVisible();
    await page.locator(".trend-day").nth(5).blur();
    await expect(tips).toHaveCount(0);

    // Focus then Escape closes it.
    await page.locator(".trend-day").nth(5).focus();
    await expect(tips).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(tips).toHaveCount(0);

    // Re-open and confirm it stays within the viewport.
    await page.locator(".trend-day").nth(5).hover();
    await expect(tips).toBeVisible();
    const box = await tips.boundingBox();
    expect(box!.x).toBeGreaterThanOrEqual(0);
    expect(box!.y).toBeGreaterThanOrEqual(0);
    expect(box!.x + box!.width).toBeLessThanOrEqual(1400);
    expect(box!.y + box!.height).toBeLessThanOrEqual(900);
  });

  test("All-mode tooltip carries the full date, total, agents and delta; aria-label is equivalent", async ({ page }) => {
    await boot(page, 1400, 900);
    const tip = page.locator(".chart-tooltip");

    await page.locator(".trend-day").nth(5).hover();
    await expect(tip).toBeVisible();
    await expect(tip).toContainText("August 24, 2026");
    await expect(tip).toContainText("56.49M");
    await expect(tip).toContainText("Claude Code");
    await expect(tip).toContainText("Codex");
    await expect(tip).toContainText("Antigravity");
    await expect(tip).toContainText("vs 昨日");

    const aria = await page.locator(".trend-day").nth(5).getAttribute("aria-label");
    expect(aria).toContain("August 24, 2026");
    expect(aria).toContain("56.49M");
    expect(aria).toContain("Claude Code");
    expect(aria).toContain("Codex");
    expect(aria).toContain("Antigravity");
  });

  test("repositions to stay inside the viewport after a 1400 -> 800 resize", async ({ page }) => {
    // Regression for the fix that made the tooltip re-position on window
    // resize/scroll instead of staying fixed at its old (now off-screen) spot.
    await boot(page, 1400, 900);
    const tips = page.locator(".chart-tooltip");

    // Open on a right-hand bar via focus (kept across the resize — no mouse
    // involvement, so no mouse-leave can close the overlay mid-test).
    await page.locator(".trend-day").nth(5).focus();
    await expect(tips).toBeVisible();

    // Shrink to single column; the overlay must be re-clamped inside the new
    // viewport rather than left stranded at its old 1400px x-position.
    await page.setViewportSize({ width: 800, height: 900 });
    await expect(tips).toBeVisible();
    await expect(async () => {
      const b = await tips.boundingBox();
      expect(b).not.toBeNull();
      expect(b!.x).toBeGreaterThanOrEqual(0);
      expect(b!.y).toBeGreaterThanOrEqual(0);
      expect(b!.x + b!.width).toBeLessThanOrEqual(800);
      expect(b!.y + b!.height).toBeLessThanOrEqual(900);
    }).toPass();
  });
});

test.describe("color carry-through", () => {
  test("row dot, chip dot, chart segment and tooltip dot all match --agent-codex", async ({ page }) => {
    await boot(page, 1400, 900);
    const codex = await cssColor(page, "--agent-codex");

    // Row dot.
    const rowDot = page.locator(".agent-block", { hasText: "Codex" }).locator(".agent-dot");
    expect(await rgbOf(rowDot)).toBe(codex);

    // Chip dot.
    const chipDot = page
      .locator(".filter-chip", { hasText: "Codex" })
      .locator(".chip-dot");
    expect(await rgbOf(chipDot)).toBe(codex);

    // Chart segment on a Codex-heavy day.
    const segment = page.locator(".trend-day").nth(5).locator(".bar-segment").nth(1);
    expect(await rgbOf(segment)).toBe(codex);

    // Tooltip legend dot, once open.
    await page.locator(".trend-day").nth(5).hover();
    await expect(page.locator(".chart-tooltip")).toBeVisible();
    const tipDot = page
      .locator(".tooltip-agents li", { hasText: "Codex" })
      .locator(".tooltip-dot");
    expect(await rgbOf(tipDot)).toBe(codex);
  });
});

test.describe("accessibility and console", () => {
  test("has zero WCAG contrast violations, no serious/critical findings, and no console errors", async ({ page }) => {
    const errors: string[] = [];
    page.on("console", (msg) => {
      if (msg.type() === "error") errors.push(msg.text());
    });
    page.on("pageerror", (err) => errors.push(err.message));

    await boot(page, 1400, 900);

    const results = await new AxeBuilder({ page }).analyze();
    const serious = results.violations.filter(
      (v) => v.impact === "serious" || v.impact === "critical",
    );
    const contrast = results.violations.filter((v) => v.id === "color-contrast");
    expect(contrast).toHaveLength(0);
    expect(serious).toHaveLength(0);
    expect(errors).toEqual([]);
  });
});