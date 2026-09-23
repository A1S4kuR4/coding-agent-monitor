import { test, expect, type Page } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";
import { e2eFixture } from "../src/e2e/fixture";
import type { UsageCollectionState } from "../src/types/usage";

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

async function bootWithState(
  page: Page,
  state: UsageCollectionState,
  width = 800,
  height = 700,
) {
  await page.setViewportSize({ width, height });
  await page.addInitScript((fixture) => {
    (window as unknown as { __E2E_STATE__: UsageCollectionState }).__E2E_STATE__ =
      fixture;
  }, state);
  await page.goto("/e2e.html");
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
    const chips = page.locator(".filter-tab");
    for (let i = 0; i < (await chips.count()); i++) {
      const box = await chips.nth(i).boundingBox();
      expect(box!.width).toBeGreaterThan(0);
      expect(box!.x + box!.width).toBeLessThanOrEqual(800);
    }
  });

  test("the default 680x700 window shows the whole core summary plus the main trend", async ({ page }) => {
    await boot(page, 680, 700);

    const noDocOverflow = await page.evaluate(
      () => document.documentElement.scrollWidth <= document.documentElement.clientWidth,
    );
    expect(noDocOverflow).toBe(true);
    // Refresh stays reachable inside the default window.
    const refresh = await page.locator(".refresh-btn").boundingBox();
    expect(refresh).not.toBeNull();
    expect(refresh!.x + refresh!.width).toBeLessThanOrEqual(680);
    // Agent rows are also fully inside the window.
    for (const el of await page.locator(".agent-row").all()) {
      const box = await el.boundingBox();
      expect(box).not.toBeNull();
      expect(box!.x + box!.width).toBeLessThanOrEqual(680);
    }

    // First-screen hierarchy: with 1-4 agents and the details collapsed, the
    // today total, the agent list AND the seven-day trend are all visible in
    // the 700px viewport — no scrolling required to answer "how much today,
    // who mostly, and how has it been going".
    const vh = 700;
    const totalBox = await page.locator(".total").boundingBox();
    expect(totalBox!.y + totalBox!.height).toBeLessThanOrEqual(vh);
    const agentListBox = await page.locator(".agent-list").boundingBox();
    expect(agentListBox!.y + agentListBox!.height).toBeLessThanOrEqual(vh);
    const trendBox = await page.locator(".trend").boundingBox();
    expect(trendBox!.y + trendBox!.height).toBeLessThanOrEqual(vh);
    // The collapsed detail tier starts below the fold — it is on demand.
    const breakdown = await page.locator("details.breakdown-section").boundingBox();
    expect(breakdown!.y).toBeGreaterThanOrEqual(vh);

    // Freshness answers first: the last-success time sits in the sticky
    // header, never in a footer at the bottom of the scroll.
    const headerTime = await page.locator(".app-header time").boundingBox();
    expect(headerTime).not.toBeNull();
    expect(headerTime!.y + headerTime!.height).toBeLessThanOrEqual(vh);
    await expect(page.locator(".app-header time")).toHaveAttribute(
      "datetime",
      "2026-08-25T07:00:00.000Z",
    );
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

test.describe("collection state", () => {
  test("first failure is recoverable and never renders fixture totals", async ({ page }) => {
    const failed = structuredClone(e2eFixture);
    failed.revision = 2;
    failed.snapshot = null;
    failed.lastAttempt = {
      ...failed.lastAttempt!,
      outcome: "failed",
      failure: "failed",
    };
    failed.freshness = {
      ...failed.freshness,
      status: "unknown",
      reason: "noSnapshot",
    };
    await bootWithState(page, failed);
    await expect(page.getByRole("heading", { name: "Usage unavailable" })).toBeVisible();
    await expect(page.getByRole("button", { name: "Try again" })).toBeVisible();
    await expect(page.locator(".total")).toHaveCount(0);
  });

  test("old-day failed data keeps its date and last-success timestamp", async ({ page }) => {
    const stale = structuredClone(e2eFixture);
    stale.revision = 4;
    stale.lastAttempt = {
      ...stale.lastAttempt!,
      outcome: "failed",
      failure: "failed",
      trigger: "periodic",
    };
    stale.freshness = {
      ...stale.freshness,
      status: "stale",
      reason: "dateChanged",
      currentDate: "2026-08-26",
    };
    await bootWithState(page, stale, 420, 560);
    await expect(
      page.getByRole("heading", { name: "Usage for 2026-08-25", exact: true }),
    ).toBeVisible();
    await expect(page.getByText(/out-of-date data for 2026-08-25/i)).toBeVisible();
    await expect(page.locator("time")).toHaveAttribute(
      "datetime",
      "2026-08-25T07:00:00.000Z",
    );
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= document.documentElement.clientWidth,
      ),
    ).toBe(true);
  });

  // T06: what Rust produces right after `restore_from_store` on a restart —
  // the persisted success with NO refresh attempt in this process yet. The
  // restored data must show immediately under its original date and original
  // success time, labelled old, never as "today" and never as a failure.
  test("a restored cross-restart snapshot shows its original date and time, not today", async ({ page }) => {
    const restored = structuredClone(e2eFixture);
    restored.revision = 1;
    restored.lastAttempt = null;
    restored.refreshing = false;
    restored.freshness = {
      ...restored.freshness,
      status: "stale",
      reason: "dateChanged",
      checkedAt: "2026-08-26T07:00:00Z",
      currentDate: "2026-08-26",
    };
    await bootWithState(page, restored, 680, 700);
    await expect(
      page.getByRole("heading", { name: "Usage for 2026-08-25", exact: true }),
    ).toBeVisible();
    // The stale banner explains the date; no refresh-failure copy may appear.
    await expect(page.getByText(/out-of-date data for 2026-08-25/i)).toBeVisible();
    await expect(page.getByText(/refresh failed/i)).toHaveCount(0);
    // The success time is the restored original, never rewritten.
    await expect(page.locator("time")).toHaveAttribute(
      "datetime",
      "2026-08-25T07:00:00.000Z",
    );
    // The numbers themselves still render (no error page, no empty state).
    await expect(page.locator(".total")).toContainText("93.89M");
  });

  test("refreshing preserves the visible snapshot and disables Refresh", async ({ page }) => {
    const running = structuredClone(e2eFixture);
    running.revision = 3;
    running.refreshing = true;
    running.lastAttempt = {
      ...running.lastAttempt!,
      outcome: "inProgress",
      finishedAt: null,
      failure: null,
      trigger: "manual",
    };
    await bootWithState(page, running);
    await expect(page.locator(".total")).toContainText("93.89M");
    await expect(page.getByRole("button", { name: "Refreshing…" })).toBeDisabled();
  });
});

test.describe("two column (1400x900)", () => {
  test("lays Today left and filter/chart right, with the detail tier spanning below", async ({ page }) => {
    await boot(page, 1400, 900);

    const cols = await page
      .locator(".dash-grid")
      .evaluate((el) => getComputedStyle(el).gridTemplateColumns.split(" ").length);
    expect(cols).toBe(2);

    const leftBox = await page.locator(".dash-left").boundingBox();
    const rightBox = await page.locator(".dash-right").boundingBox();
    expect(leftBox!.x).toBeLessThan(rightBox!.x);

    // Today's figure lives in the left column; filter + chart in the right.
    const totals = await page.locator(".dash-left .total").count();
    expect(totals).toBe(1);
    const rightContent = await page.locator(".dash-right .trend-filter, .dash-right .trend").count();
    expect(rightContent).toBe(2);

    const totalBox = await page.locator(".total").boundingBox();
    const trendBox = await page.locator(".trend").boundingBox();
    expect(totalBox!.x).toBeLessThan(trendBox!.x);

    // Chart height is in the expected ~320px range.
    expect(trendBox!.height).toBeGreaterThanOrEqual(280);
    expect(trendBox!.height).toBeLessThanOrEqual(360);

    // The on-demand detail tier spans both columns below the two of them.
    const detailsBox = await page.locator(".dash-details").boundingBox();
    expect(detailsBox!.x).toBeLessThanOrEqual(leftBox!.x);
    expect(detailsBox!.width).toBeGreaterThanOrEqual(rightBox!.x + rightBox!.width - detailsBox!.x - 1);
    // Breakdown is collapsed by default there.
    await expect(page.locator("details.breakdown-section")).not.toHaveAttribute("open");

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
  test("shows the neutral +66.2% delta against yesterday's full day", async ({ page }) => {
    await boot(page, 1400, 900);
    const delta = page.locator(".total-delta");
    // Direction is carried by the arrow + sign and the explicit time basis
    // (today's running total vs yesterday's FULL day) — never by colour.
    await expect(delta).toHaveText(/▲ \+66\.2% vs yesterday \(full day\)/);
    expect(await colorOf(delta)).toBe(await cssColor(page, "--ink-2"));
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

  test("the full-row toggle expands and collapses with the keyboard and has a generous target", async ({ page }) => {
    await boot(page, 1400, 900);

    const claudeToggle = page
      .locator(".agent-block", { hasText: "Claude Code" })
      .locator("button.agent-toggle");
    await expect(claudeToggle).toHaveAttribute("aria-expanded", "false");

    // The whole row is the target: much taller and wider than the 24px minimum.
    const box = await claudeToggle.boundingBox();
    expect(box!.height).toBeGreaterThanOrEqual(24);
    expect(box!.width).toBeGreaterThanOrEqual(300);

    // Keyboard-only expand/collapse.
    await claudeToggle.focus();
    await page.keyboard.press("Enter");
    await expect(claudeToggle).toHaveAttribute("aria-expanded", "true");
    await expect(
      page.locator(".agent-block", { hasText: "Claude Code" }).locator(".model-list"),
    ).toBeVisible();
    await page.keyboard.press("Enter");
    await expect(claudeToggle).toHaveAttribute("aria-expanded", "false");
    await expect(
      page.locator(".agent-block", { hasText: "Claude Code" }).locator(".model-list"),
    ).toHaveCount(0);
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
    // 08/24 vs 08/23 are both full days -> "previous day" basis.
    await expect(tip).toContainText("vs previous day");

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
  test("row sigil, filter tab sigil, chart segment and tooltip sigil all echo --agent-codex", async ({ page }) => {
    await boot(page, 1400, 900);
    const codex = await cssColor(page, "--agent-codex");

    // Identity sigils share one stroke-currentColor SVG across every surface;
    // the span's colour IS the mark colour (--mark-color ← --agent-*).
    const rowSigil = page
      .locator(".agent-block", { hasText: "Codex" })
      .locator(".agent-row .sigil");
    await expect(rowSigil).toHaveCount(1);
    await expect(rowSigil.locator("svg")).toHaveCount(1);
    expect(await rowSigil.evaluate((el) => getComputedStyle(el).color)).toBe(codex);

    // Filter tab sigil.
    const tabSigil = page
      .locator(".filter-tab", { hasText: "Codex" })
      .locator(".sigil");
    await expect(tabSigil).toHaveCount(1);
    expect(await tabSigil.evaluate((el) => getComputedStyle(el).color)).toBe(codex);

    // Chart segment on a Codex-heavy day keeps the colour echo.
    const segment = page.locator(".trend-day").nth(5).locator(".bar-segment").nth(1);
    expect(await rgbOf(segment)).toBe(codex);

    // Tooltip legend sigil, once open. Scoped to the hover overlay: the
    // default-pinned today panel renders the same DayDetailContent.
    await page.locator(".trend-day").nth(5).hover();
    await expect(page.locator(".chart-tooltip")).toBeVisible();
    const tipSigil = page
      .locator(".chart-tooltip .tooltip-agents li", { hasText: "Codex" })
      .locator(".sigil");
    expect(await tipSigil.evaluate((el) => getComputedStyle(el).color)).toBe(codex);
    await expect(tipSigil.locator("svg")).toHaveCount(1);
  });

  test("filter tabs carry the identity underline only when active", async ({ page }) => {
    await boot(page, 1400, 900);
    const codex = await cssColor(page, "--agent-codex");
    const codexTab = page.getByRole("button", { name: "Codex", exact: true });
    // Rest: no underline (a transparent 2px bottom border).
    const restBorder = await codexTab.evaluate((el) =>
      getComputedStyle(el).borderBottomColor,
    );
    expect(restBorder).toBe("rgba(0, 0, 0, 0)");
    // Active: the underline takes the agent colour. toHaveCSS polls past the
    // 120ms underline transition instead of sampling it mid-flight.
    await codexTab.click();
    await expect(codexTab).toHaveAttribute("aria-pressed", "true");
    await expect(codexTab).toHaveCSS("border-bottom-color", codex);
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

// Dark theme in the same browser: prefers-color-scheme flips the CSS tokens,
// so the dark palette gets the same axe contrast audit as the light one.
test.describe("dark theme", () => {
  test.use({ colorScheme: "dark" });

  test("dark tokens apply and axe finds zero contrast violations", async ({ page }) => {
    await boot(page, 800, 700);

    const paper = await page.evaluate(() =>
      getComputedStyle(document.documentElement).backgroundColor,
    );
    expect(paper).toBe("rgb(25, 22, 18)");

    const results = await new AxeBuilder({ page }).analyze();
    const contrast = results.violations.filter((v) => v.id === "color-contrast");
    expect(contrast).toHaveLength(0);
    // The neutral delta keeps one ink colour in dark mode too.
    expect(await colorOf(page.locator(".total-delta"))).toBe(
      await cssColor(page, "--ink-2"),
    );
  });
});

test.describe("statistics transparency (T02)", () => {
  test("a missing-price day demotes to the short mark and keeps the reason on the tooltip", async ({ page }) => {
    const state = structuredClone(e2eFixture);
    state.snapshot!.summary.today.estimatedCostUsd = null;
    state.snapshot!.summary.today.costUnknownReason = "missingModelPricing";
    await bootWithState(page, state);
    // A2: a quiet short mark in the meta slot; the concrete reason stays on
    // the title tooltip. Never a fake $0.00.
    const mark = page.locator(".meta-cost");
    await expect(mark).toHaveText("Cost n/a ⓘ");
    await expect(mark).toHaveAttribute(
      "title",
      "Est. cost unavailable — missing model prices",
    );
    await expect(page.locator(".meta")).not.toContainText("$0.00");
  });

  test("a no-usage day shows N/A cost and local-check guidance, not a fake zero", async ({ page }) => {
    const state = structuredClone(e2eFixture);
    const zeroDay = (day: { totalTokens: number; agents: unknown[]; estimatedCostUsd: number | null; costUnknownReason: string | null; cacheReadShare: number | null; tokenBreakdown: Record<string, number> }) => {
      day.totalTokens = 0;
      day.agents = [];
      day.estimatedCostUsd = null;
      day.costUnknownReason = null;
      day.cacheReadShare = null;
      day.tokenBreakdown = {
        inputTokens: 0,
        outputTokens: 0,
        cacheReadTokens: 0,
        cacheCreationTokens: 0,
        reasoningTokens: 0,
        unclassifiedTokens: 0,
      };
    };
    zeroDay(state.snapshot!.summary.today);
    for (const day of state.snapshot!.summary.last7Days) {
      zeroDay(day);
    }
    await bootWithState(page, state);
    // A2 short mark; the title carries the N/A reason.
    const mark = page.locator(".meta-cost");
    await expect(mark).toHaveText("Cost n/a ⓘ");
    await expect(mark).toHaveAttribute("title", "Est. cost N/A — no usage");
    await expect(page.locator(".empty-state")).toContainText(
      "No agent usage was found for",
    );
    await page.locator(".empty-help summary").click();
    await expect(page.locator(".empty-help")).toContainText("Windows user account");
    // No-usage never claims an agent is uninstalled.
    await expect(page.locator(".empty-help")).not.toContainText(/not installed/i);
  });

  test("skipped-record diagnostics surface as a coverage banner without paths", async ({ page }) => {
    const state = structuredClone(e2eFixture);
    state.snapshot!.summary.coverage = {
      status: "possiblyIncomplete",
      diagnostics: [
        {
          kind: "corruptRecord",
          agentId: "claude",
          agentDisplayName: "Claude Code",
          count: 2,
        },
        {
          kind: "databaseError",
          agentId: "codex",
          agentDisplayName: "Codex",
          count: 1,
        },
      ],
    };
    await bootWithState(page, state);
    const banner = page.locator(".coverage-banner");
    await expect(banner).toContainText("Coverage may be incomplete");
    await expect(banner).toContainText(
      "Claude Code: 2 records were malformed and skipped",
    );
    await expect(banner).toContainText("Codex: 1 database read failed");
    // Accepted totals stay visible alongside the coverage warning.
    await expect(page.locator(".total")).toContainText("93.89M");
    // Sanitized payload: no paths anywhere in the rendered document.
    const body = await page.evaluate(() => document.body.textContent ?? "");
    expect(body).not.toContain("C:\\");
    expect(body).not.toContain("/Users/");
  });

  test("the statistics explainer defines the scope, token types, cache denominator and cost estimate", async ({ page }) => {
    await boot(page, 1400, 900);
    const about = page.locator(".about-stats");
    await expect(about).toBeVisible();
    await about.locator("summary").click();
    await expect(about).toContainText("Windows user account");
    await expect(about).toContainText("Cache read");
    await expect(about).toContainText("Cache creation");
    await expect(about).toContainText("Reasoning");
    await expect(about).toContainText("Unclassified");
    await expect(about).toContainText(
      "cache read ÷ (input + cache read + cache creation)",
    );
    await expect(about).toContainText("reference estimate");
    await expect(about).toContainText("not a bill, subscription charge, or account credit");
    await expect(about).toContainText("no usage, cost is N/A");
  });
});

// The language resolves from the browser locale at mount (system-language
// default); `locale` makes navigator.language zh-CN, mirroring a Chinese
// Windows install. No mixed-language surface may appear in either language.
test.describe("Chinese UI (zh-CN system locale)", () => {
  test.use({ locale: "zh-CN" });

  test("renders headings, filter, delta and dates in Chinese without mixing", async ({ page }) => {
    await boot(page, 800, 700);
    await expect(page.getByRole("heading", { name: "今日", exact: true })).toBeVisible();
    await expect(page.getByRole("button", { name: "刷新" })).toBeVisible();
    await expect(page.locator(".filter-tab", { hasText: "全部" })).toBeVisible();
    await expect(page.locator(".total-delta")).toHaveText(/▲ \+66\.2% 较昨日全天/);
    // The tooltip date is a Chinese full date; agent names stay untranslated.
    await page.locator(".trend-day").nth(5).hover();
    await expect(page.locator(".chart-tooltip")).toContainText("2026年8月24日");
    await expect(page.locator(".chart-tooltip")).toContainText("Claude Code");
    // No leftover English status wording in the Chinese surface.
    const body = await page.evaluate(() => document.body.textContent ?? "");
    expect(body).not.toContain("vs yesterday");
    expect(body).not.toContain("Today");
  });

  test("stale, refresh-failure and cost states render in Chinese", async ({ page }) => {
    const stale = structuredClone(e2eFixture);
    stale.revision = 4;
    stale.lastAttempt = {
      ...stale.lastAttempt!,
      outcome: "failed",
      failure: "failed",
      trigger: "periodic",
    };
    stale.freshness = {
      ...stale.freshness,
      status: "stale",
      reason: "dateChanged",
      currentDate: "2026-08-26",
    };
    await bootWithState(page, stale);
    const banner = page.locator(".stale-banner");
    await expect(banner).toContainText("刷新失败");
    await expect(banner).toContainText("旧数据");
    await expect(banner).toContainText("2026-08-25");
    await expect(page.getByRole("button", { name: "重试" })).toBeVisible();
  });

  test("missing-price cost and coverage banner render in Chinese without paths", async ({ page }) => {
    const state = structuredClone(e2eFixture);
    state.snapshot!.summary.today.estimatedCostUsd = null;
    state.snapshot!.summary.today.costUnknownReason = "missingModelPricing";
    state.snapshot!.summary.coverage = {
      status: "possiblyIncomplete",
      diagnostics: [
        {
          kind: "corruptRecord",
          agentId: "claude",
          agentDisplayName: "Claude Code",
          count: 2,
        },
      ],
    };
    await bootWithState(page, state);
    // A2: 短标记 + tooltip 保留完整原因。
    const mark = page.locator(".meta-cost");
    await expect(mark).toHaveText("成本 n/a ⓘ");
    await expect(mark).toHaveAttribute("title", "预估成本不可用 — 缺少模型价格");
    const banner = page.locator(".coverage-banner");
    await expect(banner).toContainText("覆盖可能不完整");
    await expect(banner).toContainText("Claude Code: 2 条记录格式错误已跳过");
    const body = await page.evaluate(() => document.body.textContent ?? "");
    expect(body).not.toContain("C:\\");
  });
});

// ---------------------------------------------------------------------------
// T04 — first-screen hierarchy, day selection and multi-agent readability.
// ---------------------------------------------------------------------------

test.describe("day selection (T04)", () => {
  test("today is pinned by default, other days pin on click, re-click unpins", async ({ page }) => {
    await boot(page, 1400, 900);
    const detail = page.locator(".day-detail");
    // C1: the current window's last day (today) starts pinned, so the day
    // detail interaction and the model breakdown are discoverable with zero
    // clicks.
    await expect(detail).toBeVisible();
    await expect(detail).toContainText("August 25, 2026");
    await expect(page.locator(".trend-day").nth(6)).toHaveAttribute("aria-pressed", "true");

    await page.locator(".trend-day").nth(5).click();
    await expect(detail).toBeVisible();
    await expect(detail).toContainText("August 24, 2026");
    await expect(page.locator(".trend-day").nth(5)).toHaveAttribute("aria-pressed", "true");

    // The pinned day suppresses its redundant hover tooltip; other days still
    // show theirs.
    await page.locator(".trend-day").nth(5).hover();
    await expect(page.locator(".chart-tooltip")).toHaveCount(0);
    await page.locator(".trend-day").nth(4).hover();
    await expect(page.locator(".chart-tooltip")).toBeVisible();
    await page.mouse.move(0, 0);

    // Clicking another day switches the pinned detail deterministically.
    await page.locator(".trend-day").nth(6).click();
    await expect(detail).toContainText("August 25, 2026");

    // Clicking the pinned day again unpins it.
    await page.locator(".trend-day").nth(6).click();
    await expect(detail).toHaveCount(0);
  });

  test("the pinned panel's close button dismisses it and restores focus to the bar", async ({ page }) => {
    await boot(page, 1400, 900);
    await page.locator(".trend-day").nth(5).click();
    const detail = page.locator(".day-detail");
    await expect(detail).toContainText("August 24, 2026");
    // C2: the dismissal lives in the panel itself as a fixed control.
    await detail.locator(".dd-close").click();
    await expect(detail).toHaveCount(0);
    await expect(page.locator(".trend-day").nth(5)).toBeFocused();
  });

  test("the pinned detail survives a resize, stays in flow and keeps its content", async ({ page }) => {
    await boot(page, 1400, 900);
    await page.locator(".trend-day").nth(5).click();
    const detail = page.locator(".day-detail");
    await expect(detail).toContainText("August 24, 2026");

    await page.setViewportSize({ width: 800, height: 900 });
    // The panel is in the document flow, not a fixed overlay: it reflows with
    // the layout and keeps the same day's content.
    await expect(detail).toBeVisible();
    await expect(detail).toContainText("August 24, 2026");
    await expect(detail).toContainText("56.49M");
    const box = await detail.boundingBox();
    expect(box).not.toBeNull();
    expect(box!.x).toBeGreaterThanOrEqual(0);
    expect(box!.x + box!.width).toBeLessThanOrEqual(800);
  });

  test("keyboard Enter pins a day and Escape clears selection and tooltip", async ({ page }) => {
    await boot(page, 1400, 900);
    const day5 = page.locator(".trend-day").nth(5);
    await day5.focus();
    await page.keyboard.press("Enter");
    await expect(page.locator(".day-detail")).toBeVisible();
    await expect(day5).toHaveAttribute("aria-pressed", "true");

    await page.keyboard.press("Escape");
    await expect(page.locator(".day-detail")).toHaveCount(0);
    await expect(page.locator(".chart-tooltip")).toHaveCount(0);
    await expect(day5).toHaveAttribute("aria-pressed", "false");
  });

  test("single-agent pin shows the agent's value, share and day basis", async ({ page }) => {
    await boot(page, 1400, 900);
    await page.getByRole("button", { name: "Codex", exact: true }).click();
    await page.locator(".trend-day").nth(5).click();
    const detail = page.locator(".day-detail");
    await expect(detail).toContainText("Codex");
    await expect(detail).toContainText("26.18M");
    await expect(detail).toContainText("of day");
  });
});

test.describe("filter scope and scale semantics (T04)", () => {
  test("the single context line shows the scale note unfiltered and the scope note filtered", async ({ page }) => {
    await boot(page, 1400, 900);
    // A1: one context line, two states. Without a filter it states the
    // scaling basis.
    await expect(page.locator(".trend-hint")).toHaveText(
      "Bar height scales to the busiest day of the window; filtering rescales the chart.",
    );
    await page.getByRole("button", { name: "Codex", exact: true }).click();
    // With a filter it states that the filter only affects the trend.
    await expect(page.locator(".trend-hint")).toHaveText(
      "Filter affects the trend only — today's summary above is always complete.",
    );
    // Today's headline figure is untouched by the trend filter.
    await expect(page.locator(".total")).toContainText("93.89M");
    // The Claude Code agent row is still listed in full.
    await expect(page.locator(".agent-block", { hasText: "Claude Code" })).toBeVisible();
  });

  test("a selected agent that has left the trend window falls back to All deterministically", async ({ page }) => {
    // Antigravity keeps today's usage but is absent from every trend day —
    // selecting its chip must not blank the chart or stick the pressed state.
    const state = structuredClone(e2eFixture);
    state.snapshot!.summary.last7Days = state.snapshot!.summary.last7Days.map((d) => {
      const agents = d.agents.filter((a) => a.id !== "antigravity");
      return {
        ...d,
        agents,
        totalTokens: agents.reduce((s, a) => s + a.tokens, 0),
      };
    });
    await bootWithState(page, state, 1400, 900);

    const chip = page.getByRole("button", { name: "Antigravity", exact: true });
    await chip.click();
    // The effective filter is All: the All chip stays pressed, the bars stay
    // stacked, and the chip itself does not get stuck selected.
    await expect(page.getByRole("button", { name: "All" })).toHaveAttribute("aria-pressed", "true");
    await expect(chip).toHaveAttribute("aria-pressed", "false");
    await expect(page.locator(".trend-day").nth(6).locator(".bar-segment")).toHaveCount(3);
    // Today's summary keeps listing Antigravity — the filter never touches it.
    await expect(page.locator(".agent-block", { hasText: "Antigravity" })).toBeVisible();
  });
});

test.describe("Claude Desktop", () => {
  /** The base fixture plus a second Claude surface: Desktop sessions are their
   * own agent, and the app must show them as a distinct row rather than hiding
   * them or merging them into Claude Code. */
  function claudeDesktopState(): UsageCollectionState {
    const state = structuredClone(e2eFixture);
    const desktop = () => ({
      id: "claude-desktop",
      displayName: "Claude Desktop",
      tokens: 12_000_000,
      reasoningTokens: 0,
      unclassifiedTokens: 0,
      models: [],
    });
    state.snapshot!.summary.today.agents.push(desktop());
    state.snapshot!.summary.today.totalTokens += 12_000_000;
    for (const day of state.snapshot!.summary.last7Days.slice(-2)) {
      day.agents.push(desktop());
      day.totalTokens += 12_000_000;
    }
    return state;
  }

  test("is listed as its own agent row next to Claude Code", async ({ page }) => {
    await bootWithState(page, claudeDesktopState(), 1400, 900);

    // A row of its own: not hidden, and not merged into the Claude Code row.
    const rows = page.locator(".agent-block");
    expect(await rows.count()).toBe(5);
    const desktopRow = page.locator(".agent-block", { hasText: "Claude Desktop" });
    const claudeRow = page.locator(".agent-block", { hasText: "Claude Code" });
    await expect(desktopRow).toHaveCount(1);
    await expect(claudeRow).toHaveCount(1);
    await expect(desktopRow.locator(".agent-tokens")).toHaveText("12M");

    // Its own identity: no drawn mark exists for Claude Desktop yet, so it
    // carries the neutral diamond sigil in its OWN colour — never Claude
    // Code's coral spark.
    const desktopSigil = desktopRow.locator(".agent-row .sigil");
    await expect(desktopSigil).toHaveCount(1);
    const desktopColor = await cssColor(page, "--agent-claude-desktop");
    const claudeColor = await cssColor(page, "--agent-claude");
    expect(desktopColor).not.toBe(claudeColor);
    expect(
      await desktopSigil.evaluate((el) => getComputedStyle(el).color),
    ).toBe(desktopColor);
  });

  test("filters the trend to its own colour, and stays out of the overflow menu", async ({ page }) => {
    await bootWithState(page, claudeDesktopState(), 680, 700);

    // Five known agents: the inline capacity must not hide this one, since it
    // is the newest agent a user is looking for.
    const chip = page.getByRole("button", { name: "Claude Desktop", exact: true });
    await expect(chip).toBeVisible();
    await chip.click();
    await expect(chip).toHaveAttribute("aria-pressed", "true");

    const segments = page.locator(".trend-day").nth(6).locator(".bar-segment");
    await expect(segments).toHaveCount(1);
    expect(await rgbOf(segments.nth(0))).toBe(await cssColor(page, "--agent-claude-desktop"));

    const noOverflow = await page.evaluate(
      () => document.documentElement.scrollWidth <= document.documentElement.clientWidth,
    );
    expect(noOverflow).toBe(true);
  });
});

/** A 17-agent presentation fixture (clearly synthetic test data): the four
 * known agents plus 13 unknown long-named agents on the last four days. The
 * unknowns are small, so the All chart's top-three members stay stable and the
 * rest fold into the labelled others group. */
function seventeenAgentState(): UsageCollectionState {
  const state = structuredClone(e2eFixture);
  const extras = Array.from({ length: 13 }, (_, i) => {
    const nn = String(i + 1).padStart(2, "0");
    return {
      id: `team-agent-${nn}`,
      displayName: `Team Sync Agent ${nn} — long agent display name`,
      tokens: 40_000 + (i + 1) * 1_000,
      reasoningTokens: 0,
      unclassifiedTokens: 0,
      models: [],
    };
  });
  const extrasTotal = extras.reduce((s, a) => s + a.tokens, 0);
  for (const day of state.snapshot!.summary.last7Days.slice(-4)) {
    day.agents.push(...extras.map((a) => structuredClone(a)));
    day.totalTokens += extrasTotal;
  }
  state.snapshot!.summary.today.agents.push(...extras.map((a) => structuredClone(a)));
  state.snapshot!.summary.today.totalTokens += extrasTotal;
  return state;
}

test.describe("seventeen agents (T04)", () => {
  test("the today list scrolls naturally and the filter row stays compact behind More agents", async ({ page }) => {
    await bootWithState(page, seventeenAgentState(), 680, 700);

    // All 17 agent rows exist; the page grows vertically (natural scroll).
    expect(await page.locator(".agent-block").count()).toBe(17);
    // Only All + four inline tabs; the rest are behind the disclosure.
    expect(await page.locator(".trend-filter > .filter-tab").count()).toBe(5);
    const more = page.locator(".filter-more");
    await expect(more.locator("summary")).toHaveText(/More agents \(13\)/);
    await expect(more).not.toHaveAttribute("open");

    const noDocOverflow = await page.evaluate(
      () => document.documentElement.scrollWidth <= document.documentElement.clientWidth,
    );
    expect(noDocOverflow).toBe(true);
  });

  test("More agents is keyboard-operable: open, select a hidden agent, menu closes", async ({ page }) => {
    await bootWithState(page, seventeenAgentState(), 1400, 900);
    const more = page.locator(".filter-more");
    const summary = more.locator("summary");

    await summary.focus();
    await page.keyboard.press("Enter");
    await expect(more).toHaveAttribute("open", "");
    // A hidden agent inside the menu is reachable and selectable.
    const hidden = more.getByRole("button", { name: /Team Sync Agent 05/ });
    await expect(hidden).toHaveAttribute("aria-pressed", "false");
    await hidden.focus();
    await page.keyboard.press("Enter");

    // Selection applied: the selected overflow agent is swapped inline and
    // pressed (it no longer lives inside the menu), the menu is closed, and
    // the chart is monochrome unknown.
    const selectedChip = page
      .locator(".trend-filter > .filter-tab", { hasText: "Team Sync Agent 05" });
    await expect(selectedChip).toHaveAttribute("aria-pressed", "true");
    await expect(more).not.toHaveAttribute("open");
    const unknown = await cssColor(page, "--agent-unknown");
    const segments = page.locator(".trend .bar-segment");
    const allUnknown = await segments.evaluateAll(
      (els, expected) =>
        els.every((el) => getComputedStyle(el).backgroundColor === expected),
      unknown,
    );
    expect(allUnknown).toBe(true);

    // The selected overflow agent is swapped inline deterministically.
    await expect(
      page.locator(".trend-filter > .filter-tab", { hasText: "Team Sync Agent 05" }),
    ).toBeVisible();
    await expect(summary).toHaveText(/More agents \(13\)/);
  });

  test("the All chart groups non-top agents into a labelled others segment with the sum preserved", async ({ page }) => {
    await bootWithState(page, seventeenAgentState(), 1400, 900);

    // Window top three: Claude Code, Codex, OpenCode. Day 6 (today) renders
    // claude + codex + opencode + one others segment.
    const day6 = page.locator(".trend-day").nth(6);
    const segments = day6.locator(".bar-segment");
    expect(await segments.count()).toBe(4);
    const othersColor = await cssColor(page, "--agent-others");
    expect(await rgbOf(segments.nth(3))).toBe(othersColor);

    // Sum invariant: today is the window max, so its rendered bar fills 100%.
    const sum = await day6
      .locator(".bar-track")
      .evaluate((track) =>
        Array.from(track.querySelectorAll<HTMLElement>(".bar-segment")).reduce(
          (s, el) => s + parseFloat(el.style.height),
          0,
        ),
      );
    expect(sum).toBeCloseTo(100, 0);

    // Hover tooltip on a grouped day: labelled others row plus the FULL
    // composition — every agent stays visible in the detail.
    await page.locator(".trend-day").nth(5).hover();
    const tip = page.locator(".chart-tooltip");
    await expect(tip).toContainText("Other agents (14)");
    const body = (await tip.textContent()) ?? "";
    for (const a of ["Claude Code", "Codex", "Antigravity"]) {
      expect(body).toContain(a);
    }
    expect(body).toContain("Team Sync Agent 01");
    expect(body).toContain("Team Sync Agent 13");
  });

  test("the pinned day detail shows the full composition for a grouped day", async ({ page }) => {
    await bootWithState(page, seventeenAgentState(), 1400, 900);
    // Today (index 6) is already pinned by default; pin a different grouped
    // day instead of toggling the default pin off.
    await page.locator(".trend-day").nth(5).click();
    const detail = page.locator(".day-detail");
    await expect(detail).toContainText("Other agents (14)");
    await expect(detail).toContainText("Team Sync Agent 07");
    await expect(detail).toContainText("Antigravity");
  });

  test("unknown long-named agents keep the neutral diamond sigil and never overflow", async ({ page }) => {
    await bootWithState(page, seventeenAgentState(), 1400, 900);
    // Unknown agents share the neutral diamond sigil; identity comes from the
    // name, the shape stays quiet, and the colour is the shared neutral.
    const rowSigil = page
      .locator(".agent-block", { hasText: "Team Sync Agent 01" })
      .locator(".agent-row .sigil");
    await expect(rowSigil).toHaveCount(1);
    await expect(rowSigil.locator("svg")).toHaveCount(1);
    expect(await rowSigil.evaluate((el) => getComputedStyle(el).color)).toBe(
      await cssColor(page, "--agent-unknown"),
    );
    // Long names wrap inside the row; the row itself never overflows.
    const row = page
      .locator(".agent-block", { hasText: "Team Sync Agent 01" })
      .locator(".agent-row");
    const box = await row.boundingBox();
    expect(box).not.toBeNull();
    expect(box!.x + box!.width).toBeLessThanOrEqual(1400);
  });
});

// ---------------------------------------------------------------------------
// Tray residency + minimal preferences (T05). The IPC mock answers the
// preference commands with the shipped defaults (or window.__E2E_PREFS__),
// echoing patches like the real Rust command; window show/hide stays native
// and is not part of the browser harness.
// ---------------------------------------------------------------------------

test.describe("minimal preferences (T05)", () => {
  test("the preferences section lives in the on-demand tier below the 680x700 first screen", async ({ page }) => {
    await boot(page, 680, 700);

    // Collapsed by default and starting below the fold, like the other
    // on-demand details — the first screen answers data questions, not
    // settings.
    const settings = page.locator(".settings-section");
    await expect(settings).toHaveCount(1);
    const box = await settings.boundingBox();
    expect(box).not.toBeNull();
    expect(box!.y).toBeGreaterThanOrEqual(700);

    // No horizontal overflow introduced anywhere.
    const noDocOverflow = await page.evaluate(
      () => document.documentElement.scrollWidth <= document.documentElement.clientWidth,
    );
    expect(noDocOverflow).toBe(true);

    // Shipped defaults: system language, both launch behaviours off.
    await page.locator(".settings-section summary").click();
    await expect(page.locator(".settings-section select")).toHaveValue("system");
    const checkboxes = page.locator('.settings-section input[type="checkbox"]');
    await expect(checkboxes).toHaveCount(2);
    for (let i = 0; i < 2; i++) {
      expect(await checkboxes.nth(i).isChecked()).toBe(false);
    }
  });

  test("switching the language re-renders the whole surface and persists", async ({ page }) => {
    await boot(page, 800, 700);
    await page.locator(".settings-section summary").click();
    await page.locator(".settings-section select").selectOption("zh-CN");

    // Runtime switching is allowed (v0.4 plan §4.5): every label, including
    // the settings section itself, uses the same resolved language.
    await expect(page.getByRole("heading", { name: "今日", exact: true })).toBeVisible();
    await expect(page.getByRole("button", { name: "刷新" })).toBeVisible();
    await expect(page.locator(".settings-section summary")).toHaveText("偏好设置");
    await expect(page.locator(".settings-section select")).toHaveValue("zh-CN");
  });

  test("an explicit persisted language preference overrides the system language", async ({ page }) => {
    await page.setViewportSize({ width: 800, height: 700 });
    await page.addInitScript(() => {
      (window as unknown as { __E2E_PREFS__: unknown }).__E2E_PREFS__ = {
        version: 1,
        language: "zh-CN",
        startWithWindows: true,
        startHiddenToTray: false,
        closeNoticeAcknowledged: true,
        window: null,
      };
    });
    await page.goto("/e2e.html");
    await expect(page.locator(".total")).toContainText("93.89M");

    await expect(page.getByRole("heading", { name: "今日", exact: true })).toBeVisible();
    // The persisted launch choice is reflected truthfully.
    await page.locator(".settings-section summary").click();
    await expect(page.locator(".settings-section summary")).toHaveText("偏好设置");
    await expect(
      page.locator('.settings-section input[type="checkbox"]').first(),
    ).toBeChecked();
  });
});


test("T07 30-day chart scrolls inside minimum window, keeps today's total and keyboard details", async ({ page }) => {
  const base = e2eFixture.snapshot!;
  const days = Array.from({ length: 30 }, (_, i) => ({
    ...base.summary.last7Days[i % 7],
    date: new Date(Date.UTC(2026, 7, 24 - 29 + i)).toISOString().slice(0, 10),
  }));
  await page.addInitScript(history => {
    (window as unknown as { __E2E_HISTORY__: unknown }).__E2E_HISTORY__ = history;
  }, { scope: { startDate: days[0].date, endDate: days[29].date, timeZone: "UTC" },
    collectedAt: base.summary.collectedAt, days, coverage: base.summary.coverage, estimatedCostUsd: null });
  await boot(page, 420, 560);
  const total = await page.locator(".total").innerText();
  await page.getByRole("button", { name: "30 days", exact: true }).click();
  // B2: the 30-day window defaults to the week aggregation — five buckets,
  // structurally identical to the 7-day view, no horizontal scroll.
  await expect(page.locator(".trend-day")).toHaveCount(5);
  await expect(page.locator(".total")).toHaveText(total);
  const weekGeometry = await page.evaluate(() => {
    const chart = document.querySelector(".history-chart-scroll")!;
    return { windowFits: document.documentElement.scrollWidth <= innerWidth,
      chartScrolls: chart.scrollWidth > chart.clientWidth };
  });
  expect(weekGeometry).toEqual({ windowFits: true, chartScrolls: false });
  // Day mode keeps the existing horizontal-scroll behaviour for drilling.
  await page.getByRole("button", { name: "By day", exact: true }).click();
  await expect(page.locator(".trend-day")).toHaveCount(30);
  const geometry = await page.evaluate(() => {
    const chart = document.querySelector(".history-chart-scroll")!;
    return { windowFits: document.documentElement.scrollWidth <= innerWidth,
      chartScrolls: chart.scrollWidth > chart.clientWidth };
  });
  expect(geometry).toEqual({ windowFits: true, chartScrolls: true });
  await page.locator(".trend-day").last().focus();
  await page.keyboard.press("Enter");
  await expect(page.locator(".day-detail")).toBeVisible();
  await expect(page.locator(".day-detail .breakdown-list").first()).toBeVisible();
  // C2: the pin's dismissal lives inside the panel as a fixed close button.
  await page.getByRole("button", { name: "Close details" }).click();
  await expect(page.locator(".day-detail")).toHaveCount(0);
  await page.getByRole("button", { name: "7 days", exact: true }).click();
  await expect(page.locator(".trend-day")).toHaveCount(7);
});


test("T07 pending history gives immediate feedback, 7-day return rejects the delayed 30-day result", async ({ page }) => {
  await page.addInitScript(() => {
    Object.assign(window, { __E2E_HISTORY__: { days: Array(30).fill(null) }, __E2E_HISTORY_DELAY__: 1500 });
  });
  await boot(page, 680, 700);
  const feedbackMs = await page.evaluate(async () => {
    const button = Array.from(document.querySelectorAll("button")).find(b => b.textContent === "30 days")!;
    const started = performance.now(); button.click();
    await new Promise<void>(resolve => requestAnimationFrame(() => resolve()));
    if (!document.querySelector(".history-status")?.textContent?.includes("Loading")) throw new Error("no loading feedback");
    return performance.now() - started;
  });
  console.log(`T07 click-to-frame feedback: ${feedbackMs.toFixed(1)}ms`);
  await page.getByRole("button", { name: "7 days", exact: true }).click();
  await expect(page.locator(".trend-day")).toHaveCount(7);
  await page.waitForTimeout(1600); // Deliberately deliver the superseded IPC response.
  await expect(page.locator(".trend-day")).toHaveCount(7);
  await expect(page.getByRole("heading", { name: "Last 7 days" })).toBeVisible();
});
