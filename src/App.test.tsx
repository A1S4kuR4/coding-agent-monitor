// @vitest-environment jsdom
import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import App from "./App";
import type { AgentUsage, DailyUsage, TokenBreakdown, UsageSummary } from "./types/usage";

/** Shared Tauri mocks. `focus` / `tray` capture the handlers App registers;
 *  `focusUnlisten` / `trayUnlisten` are the spy unregister fns App's cleanup
 *  releases; `fetch` backs `fetchUsageSummary`.
 *
 *  Registration is a real async call, so the mocks expose two modes. By default
 *  each registration resolves immediately with a spy unlisten fn (like the live
 *  webview listener at rest). Setting `deferFocus` / `deferTray` makes the
 *  registration stay pending until the test calls `resolveFocus` / `resolveTray`
 *  — the only way to exercise the race where `onFocusChanged()` / `listen()`'
 *  promise resolves *after* the component has unmounted. */
const tauri = vi.hoisted(() => ({
  focus: undefined as
    | ((event: { payload: boolean }) => void)
    | undefined,
  tray: undefined as
    | ((event: { payload: UsageSummary }) => void)
    | undefined,
  focusUnlisten: undefined as (() => void) | undefined,
  trayUnlisten: undefined as (() => void) | undefined,
  fetch: vi.fn(),
  deferFocus: false,
  deferTray: false,
  resolveFocus: undefined as ((fn: () => void) => void) | undefined,
  resolveTray: undefined as ((fn: () => void) => void) | undefined,
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: (_name: string, handler: (event: { payload: UsageSummary }) => void) => {
    tauri.tray = handler;
    return new Promise<() => void>((resolve) => {
      tauri.resolveTray = resolve;
      if (!tauri.deferTray) resolve((tauri.trayUnlisten = vi.fn()));
    });
  },
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    onFocusChanged: (handler: (event: { payload: boolean }) => void) => {
      tauri.focus = handler;
      return new Promise<() => void>((resolve) => {
        tauri.resolveFocus = resolve;
        if (!tauri.deferFocus) resolve((tauri.focusUnlisten = vi.fn()));
      });
    },
  }),
}));

vi.mock("./lib/usage-api", () => ({
  fetchUsageSummary: () => tauri.fetch(),
}));

function breakdownFor(tokenTotal: number): TokenBreakdown {
  const inputTokens = Math.floor(tokenTotal * 0.3);
  const outputTokens = Math.floor(tokenTotal * 0.1);
  const cacheReadTokens = Math.floor(tokenTotal * 0.6);
  const cacheCreationTokens = 0;
  const otherTokens =
    tokenTotal - inputTokens - outputTokens - cacheReadTokens - cacheCreationTokens;
  return {
    inputTokens,
    outputTokens,
    cacheReadTokens,
    cacheCreationTokens,
    otherTokens,
  };
}

function day(date: string, totalTokens: number, agents: AgentUsage[] = []): DailyUsage {
  return {
    date,
    totalTokens,
    tokenBreakdown: breakdownFor(totalTokens),
    estimatedCostUsd: totalTokens > 0 ? 0 : null,
    cacheReadShare: totalTokens > 0 ? 0.6 : null,
    agents,
  };
}

function summary(total: number): UsageSummary {
  const agents: AgentUsage[] = [
    {
      id: "claude",
      displayName: "Claude Code",
      tokens: 8_420_000,
      models: [
        {
          modelName: "deepseek-v2-lite",
          modelDisplayName: "deepseek-v2-lite",
          inputTokens: 2_174_608,
          outputTokens: 101_150,
          cacheReadTokens: 6_144_242,
          cacheCreationTokens: 0,
          totalTokens: 8_420_000,
        },
      ],
    },
    {
      id: "codex",
      displayName: "Codex",
      tokens: 5_170_000,
      models: [
        {
          modelName: "gpt-5.6-sol",
          modelDisplayName: "gpt-5.6-sol",
          inputTokens: 1_600_000,
          outputTokens: 100_000,
          cacheReadTokens: 3_300_000,
          cacheCreationTokens: 0,
          totalTokens: 5_000_000,
        },
        {
          modelName: "gpt-5.6-luna",
          modelDisplayName: "gpt-5.6-luna",
          inputTokens: 120_000,
          outputTokens: 30_000,
          cacheReadTokens: 20_000,
          cacheCreationTokens: 0,
          totalTokens: 170_000,
        },
      ],
    },
  ];
  const today: DailyUsage = {
    date: "2026-08-24",
    totalTokens: total,
    tokenBreakdown: breakdownFor(total),
    estimatedCostUsd: 12.0,
    cacheReadShare: 0.7,
    agents,
  };
  return {
    collectedAt: "2026-08-24T12:00:00Z",
    today,
    last7Days: [
      day("2026-08-18", 0),
      day("2026-08-19", 100),
      day("2026-08-20", 2000),
      day("2026-08-21", 300),
      day("2026-08-22", 4000),
      day("2026-08-23", 500),
      // The window's last day mirrors today's agents so the trend filter can
      // attribute them (same way the Rust adapter dedups a single logical day).
      { ...today },
    ],
  };
}

/** Waits until the core `.total` figure shows `text`. Scoped to that node
 *  because the today trend-bar label renders the same formatted number. */
async function waitTotal(text: string) {
  await waitFor(() => {
    expect(document.querySelector(".total")?.textContent).toContain(text);
  });
}

/** The agent-toggle button that expands/collapses `agentId`'s per-model
 *  detail. Disambiguated from same-named trend filter chips by its
 *  `aria-controls` id. */
function agentToggle(agentId: string): HTMLElement {
  const btn = screen
    .getAllByRole("button", { name: /\S/ })
    .find((b) => b.getAttribute("aria-controls") === `agent-models-${agentId}`);
  if (!btn) throw new Error(`agent toggle ${agentId} not found`);
  return btn;
}

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  tauri.focus = undefined;
  tauri.tray = undefined;
  tauri.focusUnlisten = undefined;
  tauri.trayUnlisten = undefined;
  tauri.deferFocus = false;
  tauri.deferTray = false;
  tauri.resolveFocus = undefined;
  tauri.resolveTray = undefined;
});

describe("App", () => {
  it("shows the error page when the first fetch fails", async () => {
    tauri.fetch.mockRejectedValueOnce(new Error("no sidecar"));
    render(<App />);
    expect(await screen.findByText("Usage unavailable")).toBeTruthy();
    expect(screen.getByText("Try again")).toBeTruthy();
  });

  it("renders a successful load and updates on manual refresh", async () => {
    tauri.fetch.mockResolvedValueOnce(summary(13_590_000));
    render(<App />);
    await waitTotal("13.59M");

    tauri.fetch.mockResolvedValueOnce(summary(14_000_000));
    await act(async () => {
      screen.getByText("Refresh").click();
    });
    await waitTotal("14M");
    expect(tauri.fetch).toHaveBeenCalledTimes(2);
  });

  it("expands an agent into its per-model detail and collapses it again", async () => {
    tauri.fetch.mockResolvedValueOnce(summary(13_590_000));
    render(<App />);
    await waitTotal("13.59M");

    // Model detail is collapsed by default.
    expect(screen.queryByText("gpt-5.6-sol")).toBeNull();

    // The agent-toggle for Codex, picked out of the two "Codex" buttons (the
    // other is the trend filter chip) by its aria-controls id.
    const toggle = screen
      .getAllByRole("button", { name: /Codex/ })
      .find((b) => b.getAttribute("aria-controls") === "agent-models-codex");
    if (!toggle) throw new Error("Codex agent toggle not found");
    expect(toggle.getAttribute("aria-expanded")).toBe("false");

    await act(async () => {
      toggle.click();
    });
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    expect(screen.getByText("gpt-5.6-sol")).toBeTruthy();
    expect(screen.getByText("gpt-5.6-luna")).toBeTruthy();
    // A codex-only model's own figure renders in its row.
    expect(screen.getByText("5M")).toBeTruthy();

    // Collapsing hides the detail again.
    await act(async () => {
      toggle.click();
    });
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    expect(screen.queryByText("gpt-5.6-sol")).toBeNull();
  });

  it("marks data stale when a later refresh fails, keeping the last snapshot", async () => {
    tauri.fetch.mockResolvedValueOnce(summary(13_590_000));
    render(<App />);
    await waitTotal("13.59M");

    tauri.fetch.mockRejectedValueOnce(new Error("offline"));
    await act(async () => {
      screen.getByText("Refresh").click();
    });
    // Old data stays, stale banner appears with a Retry entry.
    expect(await screen.findByText(/showing last known data/i)).toBeTruthy();
    await waitTotal("13.59M");
    expect(screen.getByText("Retry")).toBeTruthy();
  });

  it("does not start a fetch when a tray usage-updated event arrives", async () => {
    tauri.fetch.mockResolvedValueOnce(summary(13_590_000));
    render(<App />);
    await waitTotal("13.59M");
    expect(tauri.fetch).toHaveBeenCalledTimes(1);

    await act(async () => {
      tauri.tray?.({ payload: summary(9_000_000) });
    });
    // The event applied the snapshot directly; still only the one mount fetch.
    await waitTotal("9M");
    expect(tauri.fetch).toHaveBeenCalledTimes(1);
  });

  it("unmounts without leaking listeners or updating state afterwards", async () => {
    tauri.fetch.mockResolvedValueOnce(summary(13_590_000));
    const { unmount, container } = render(<App />);
    await waitTotal("13.59M");
    // Wait for the async registrations to resolve so unlisten spies are set.
    await waitFor(() => {
      expect(tauri.focusUnlisten).toBeDefined();
      expect(tauri.trayUnlisten).toBeDefined();
    });

    unmount();
    // Cleanup released both webview listeners.
    expect(tauri.focusUnlisten).toHaveBeenCalledTimes(1);
    expect(tauri.trayUnlisten).toHaveBeenCalledTimes(1);

    // Focus/event firing after unmount is a no-op: no throw, no state update.
    await act(async () => {
      tauri.focus?.({ payload: true });
      tauri.tray?.({ payload: summary(5_000_000) });
    });
    expect(container.querySelector(".total")).toBeNull();
    expect(tauri.fetch).toHaveBeenCalledTimes(1);
  });

  it("releases a focus/tray listener whose async registration resolves after unmount", async () => {
    // Defer the registrations so they resolve only after the component unmounts,
    // reproducing the race where `onFocusChanged()` / `listen()`'s promise
    // resolves late.
    tauri.deferFocus = true;
    tauri.deferTray = true;
    tauri.fetch.mockResolvedValueOnce(summary(13_590_000));
    const { unmount } = render(<App />);

    // Registration pending: no unlisten spies yet.
    expect(tauri.focusUnlisten).toBeUndefined();
    expect(tauri.trayUnlisten).toBeUndefined();

    unmount();

    // Now the late registration promises resolve with their unlisten fns. The
    // app must release them immediately, not leak or update state.
    const focusUnlisten = vi.fn();
    const trayUnlisten = vi.fn();
    tauri.resolveFocus?.(focusUnlisten);
    tauri.resolveTray?.(trayUnlisten);
    await act(async () => {});

    expect(focusUnlisten).toHaveBeenCalledTimes(1);
    expect(trayUnlisten).toHaveBeenCalledTimes(1);
  });

  it("renders a zero-token day with no data bar but a visible track", async () => {
    tauri.fetch.mockResolvedValueOnce(summary(13_590_000));
    const { container } = render(<App />);
    await screen.findByText("Today");
    const tracks = Array.from(container.querySelectorAll(".bar-track"));
    expect(tracks).toHaveLength(7);
    // last7Days[0] is the zero day: no fake bar segment, just the faint track.
    expect(tracks[0]!.querySelectorAll(".bar-segment")).toHaveLength(0);
    // last7Days[6] today (13.59M) is non-zero and its segment fills the window-max bar.
    const segmentsToday = tracks[6]!.querySelectorAll(".bar-segment") as unknown as HTMLElement[];
    expect(segmentsToday.length).toBeGreaterThan(0);
    expect(parseFloat(segmentsToday[0]!.style.height)).toBeGreaterThan(0);
  });

  it("renders the token breakdown with per-type share and hides empty types", async () => {
    tauri.fetch.mockResolvedValueOnce(summary(100));
    render(<App />);
    await screen.findByText("Token Breakdown");

    // 100 total → components 30/10/60, no other (sums exactly to 100).
    expect(screen.getByText("Input")).toBeTruthy();
    expect(screen.getByText("30%")).toBeTruthy();
    expect(screen.getByText("10%")).toBeTruthy();
    expect(screen.getByText("60%")).toBeTruthy();
    // cacheCreation is 0 and otherTokens is 0, so neither row renders.
    expect(screen.queryByText("Cache creation")).toBeNull();
    expect(screen.queryAllByText("Other")).toHaveLength(0);
  });

  it("folds unattributable tokens into a per-agent Other row", async () => {
    // Claude's models account for only 300 of its 10_000_000 tokens, so the
    // expandable detail must surface the residual as an "Other" row.
    const s = summary(100);
    s.today.agents[0] = {
      id: "claude",
      displayName: "Claude Code",
      tokens: 10_000_000,
      models: [
        {
          modelName: "deepseek-v2-lite",
          modelDisplayName: "deepseek-v2-lite",
          inputTokens: 100,
          outputTokens: 100,
          cacheReadTokens: 100,
          cacheCreationTokens: 0,
          totalTokens: 300,
        },
      ],
    };
    tauri.fetch.mockResolvedValueOnce(s);
    render(<App />);
    await screen.findByText("Today");

    const toggle = agentToggle("claude");
    await act(async () => {
      toggle.click();
    });
    // Model composition line renders the in/out/cache-read figures and the
    // model's own total.
    expect(screen.getByText("100 in · 100 out · 100 cache read")).toBeTruthy();
    // The residual (10_000_000 − 300) renders as its own "Other" row, distinct
    // from the agent's own 10M figure (scoped to the Other row to avoid the
    // duplicate). Only Claude has a residual, so there is exactly one.
    expect(screen.getAllByText("Other")).toHaveLength(1);
    expect(document.querySelector(".model-other dd")?.textContent).toBe("10M");
  });

  it("trend filter switching updates the bar series and aggregate", async () => {
    tauri.fetch.mockResolvedValueOnce(summary(13_590_000));
    render(<App />);
    await waitTotal("13.59M");

    // All: the series is the whole-day totals, and the aggregate is their sum.
    // 13_590_000 (today) + 100 + 2000 + 300 + 4000 + 500 = 13_596_900.
    expect(screen.getByText("Total 13.6M")).toBeTruthy();

    // Select the Claude Code filter chip (disambiguated from the same-named
    // agent-toggle, which owns an aria-controls id). The trend value drops to 0
    // on the days Claude had no entries, since only today carries agents.
    const chip = screen
      .getAllByRole("button", { name: "Claude Code" })
      .find((b) => !b.getAttribute("aria-controls"));
    if (!chip) throw new Error("Claude Code filter chip not found");
    await act(async () => {
      chip.click();
    });
    // Only today contributes (8_420_000) → aggregate reflects just that day.
    expect(screen.getByText("Total 8.42M")).toBeTruthy();
    const claudeSeries = Array.from(
      document.querySelectorAll(".trend-day .trend-value"),
    ).map((el) => el.textContent);
    // Seven days, with only the last non-zero.
    expect(claudeSeries).toHaveLength(7);
    expect(claudeSeries!.slice(0, 6).every((t) => t === "0")).toBe(true);
    expect(claudeSeries![6]).toBe("8.42M");
  });
});