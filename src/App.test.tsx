// @vitest-environment jsdom
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import App from "./App";
import type { AppPreferences } from "./types/preferences";
import type {
  AgentUsage,
  DailyUsage,
  TokenBreakdown,
  UsageCollectionState,
  UsageSummary,
} from "./types/usage";

/** Shared Tauri mocks. `focus` / `tray` capture the handlers App registers;
 *  `focusUnlisten` / `trayUnlisten` are the spy unregister fns App's cleanup
 *  releases; `fetch` backs both state IPC calls.
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
    | ((event: { payload: UsageCollectionState }) => void)
    | undefined,
  closeNotice: undefined as
    | ((event: { payload: unknown }) => void)
    | undefined,
  focusUnlisten: undefined as (() => void) | undefined,
  trayUnlisten: undefined as (() => void) | undefined,
  noticeUnlisten: undefined as (() => void) | undefined,
  fetch: vi.fn(),
  history: vi.fn(),
  getPreferences: vi.fn(),
  updatePreferences: vi.fn(),
  hideMainWindow: vi.fn(),
  deferFocus: false,
  deferTray: false,
  resolveFocus: undefined as ((fn: () => void) => void) | undefined,
  resolveTray: undefined as ((fn: () => void) => void) | undefined,
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: (name: string, handler: (event: { payload: unknown }) => void) => {
    if (name === "close-notice-requested") {
      tauri.closeNotice = handler;
      return Promise.resolve((tauri.noticeUnlisten = vi.fn()));
    }
    tauri.tray = handler as (event: { payload: UsageCollectionState }) => void;
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
  fetchUsageState: () => tauri.fetch(),
  fetchUsageHistory: () => tauri.history(),
  refreshUsageState: () => tauri.fetch(),
}));

vi.mock("./lib/preferences-api", () => ({
  getPreferences: () => tauri.getPreferences(),
  updatePreferences: (patch: unknown) => tauri.updatePreferences(patch),
  hideMainWindow: () => tauri.hideMainWindow(),
  acknowledgeCloseNotice: async () => {
    const saved = await tauri.updatePreferences({ closeNoticeAcknowledged: true });
    await tauri.hideMainWindow();
    return saved;
  },
}));

export const defaultPreferences: AppPreferences = {
  version: 1,
  language: "system",
  startWithWindows: false,
  startHiddenToTray: false,
  closeNoticeAcknowledged: false,
  window: null,
};

function breakdownFor(tokenTotal: number): TokenBreakdown {
  const inputTokens = Math.floor(tokenTotal * 0.3);
  const outputTokens = Math.floor(tokenTotal * 0.1);
  const cacheReadTokens = Math.floor(tokenTotal * 0.6);
  const cacheCreationTokens = 0;
  const unclassifiedTokens =
    tokenTotal - inputTokens - outputTokens - cacheReadTokens - cacheCreationTokens;
  return {
    inputTokens,
    outputTokens,
    cacheReadTokens,
    cacheCreationTokens,
    reasoningTokens: 0,
    unclassifiedTokens,
  };
}

function day(date: string, totalTokens: number, agents: AgentUsage[] = []): DailyUsage {
  return {
    date,
    totalTokens,
    tokenBreakdown: breakdownFor(totalTokens),
    estimatedCostUsd: totalTokens > 0 ? 0 : null,
    costUnknownReason: null,
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
      reasoningTokens: 0,
      unclassifiedTokens: 0,
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
      reasoningTokens: 0,
      unclassifiedTokens: 0,
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
    costUnknownReason: null,
    cacheReadShare: 0.7,
    agents,
  };
  return {
    collectedAt: "2026-08-24T12:00:00Z",
    today,
    coverage: { status: "complete", diagnostics: [] },
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

function collectionState(
  value: UsageSummary | null,
  overrides: Partial<UsageCollectionState> = {},
): UsageCollectionState {
  const date = value?.today.date ?? "2026-08-24";
  const scope = {
    startDate: "2026-08-18",
    endDate: date,
    timeZone: "UTC",
  };
  return {
    revision: 2,
    refreshing: false,
    snapshot: value ? { scope, summary: value } : null,
    lastAttempt: {
      id: 1,
      trigger: "startup",
      scope,
      startedAt: "2026-08-24T11:59:59Z",
      finishedAt: "2026-08-24T12:00:00Z",
      outcome: "succeeded",
      failure: null,
    },
    freshness: {
      status: value ? "fresh" : "unknown",
      reason: value ? "current" : "noSnapshot",
      checkedAt: "2026-08-24T12:00:00Z",
      currentDate: date,
      currentTimeZone: "UTC",
      staleAfterSeconds: 600,
    },
    ...overrides,
  };
}

function failedState(
  value: UsageSummary | null,
  stale = false,
  revision = 4,
): UsageCollectionState {
  const base = collectionState(value, { revision });
  return {
    ...base,
    lastAttempt: {
      ...base.lastAttempt!,
      id: 2,
      trigger: "manual",
      finishedAt: "2026-08-24T12:05:00Z",
      outcome: "failed",
      failure: "failed",
    },
    freshness: {
      ...base.freshness,
      status: stale ? "stale" : "fresh",
      reason: stale ? "expired" : "current",
      checkedAt: "2026-08-24T12:05:00Z",
    },
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

/** Preference mocks default to the shipped defaults before every test;
 * individual tests override with mockResolvedValueOnce / mockImplementation. */
beforeEach(() => {
  tauri.getPreferences.mockResolvedValue({ ...defaultPreferences });
  tauri.updatePreferences.mockImplementation(
    async (patch: Partial<AppPreferences>) => ({ ...defaultPreferences, ...patch }),
  );
  tauri.hideMainWindow.mockResolvedValue(undefined);
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  tauri.fetch.mockReset();
  tauri.history.mockReset();
  tauri.getPreferences.mockReset();
  tauri.updatePreferences.mockReset();
  tauri.hideMainWindow.mockReset();
  tauri.focus = undefined;
  tauri.tray = undefined;
  tauri.closeNotice = undefined;
  tauri.focusUnlisten = undefined;
  tauri.trayUnlisten = undefined;
  tauri.noticeUnlisten = undefined;
  tauri.deferFocus = false;
  tauri.deferTray = false;
  tauri.resolveFocus = undefined;
  tauri.resolveTray = undefined;
  restoreNavigatorLanguage();
});

/** Overrides the navigator language App resolves at mount (jsdom defaults to
 * "en-US"). `languages` takes priority in real browsers and in App's resolver,
 * so both own properties are stubbed; they are deleted afterwards so the
 * prototype getters return again. */
function setNavigatorLanguage(value: string) {
  Object.defineProperty(window.navigator, "language", {
    value,
    configurable: true,
  });
  Object.defineProperty(window.navigator, "languages", {
    value: [value],
    configurable: true,
  });
}

function restoreNavigatorLanguage() {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  delete (window.navigator as any).language;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  delete (window.navigator as any).languages;
}

describe("App", () => {
  it("shows the error page when the first fetch fails", async () => {
    tauri.fetch.mockRejectedValueOnce(new Error("no sidecar"));
    render(<App />);
    expect(await screen.findByText("Usage unavailable")).toBeTruthy();
    expect(screen.getByText("Try again")).toBeTruthy();
  });

  it("shows a recoverable, sanitized first collection failure", async () => {
    const failure = failedState(null);
    tauri.fetch
      .mockResolvedValueOnce(failure)
      // Initialization safely retries; the 2-second Rust backoff returns the
      // same state rather than exposing a raw failure.
      .mockResolvedValueOnce(failure);
    render(<App />);
    expect(await screen.findByText("Usage unavailable")).toBeTruthy();
    expect(screen.getByText(/could not be refreshed/i)).toBeTruthy();
    expect(document.body.textContent).not.toContain("C:\\secret");
  });

  it("renders a successful load and updates on manual refresh", async () => {
    tauri.fetch.mockResolvedValueOnce(collectionState(summary(13_590_000)));
    render(<App />);
    await waitTotal("13.59M");

    tauri.fetch.mockResolvedValueOnce(collectionState(summary(14_000_000), { revision: 4 }));
    await act(async () => {
      screen.getByText("Refresh").click();
    });
    await waitTotal("14M");
    expect(tauri.fetch).toHaveBeenCalledTimes(2);
  });

  it("refreshes through the shared command when the window regains focus", async () => {
    tauri.fetch
      .mockResolvedValueOnce(collectionState(summary(10_000)))
      .mockResolvedValueOnce(collectionState(summary(20_000), { revision: 4 }));
    render(<App />);
    await waitTotal("10K");
    await act(async () => {
      tauri.focus?.({ payload: false });
      tauri.focus?.({ payload: true });
    });
    await waitTotal("20K");
    expect(tauri.fetch).toHaveBeenCalledTimes(2);
  });

  it("expands an agent into its per-model detail and collapses it again", async () => {
    tauri.fetch.mockResolvedValueOnce(collectionState(summary(13_590_000)));
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

  it("shows a refresh failure separately while keeping a recent snapshot", async () => {
    const initial = summary(13_590_000);
    tauri.fetch.mockResolvedValueOnce(collectionState(initial));
    render(<App />);
    await waitTotal("13.59M");

    tauri.fetch.mockResolvedValueOnce(failedState(initial));
    await act(async () => {
      screen.getByText("Refresh").click();
    });
    expect(await screen.findByText(/data below is still recent/i)).toBeTruthy();
    await waitTotal("13.59M");
    expect(screen.getByText("Retry")).toBeTruthy();
  });

  it("keeps an old-day snapshot labelled with its date after a failed refresh", async () => {
    const oldSummary = summary(13_590_000);
    const failed = failedState(oldSummary, true);
    failed.freshness = {
      ...failed.freshness,
      reason: "dateChanged",
      currentDate: "2026-08-25",
    };
    tauri.fetch
      .mockResolvedValueOnce(failed)
      .mockImplementationOnce(() => new Promise<UsageCollectionState>(() => {}));
    render(<App />);

    expect(await screen.findByText("Usage for 2026-08-24")).toBeTruthy();
    expect(screen.queryByRole("heading", { name: "Today" })).toBeNull();
    expect(screen.getByText(/out-of-date data for 2026-08-24/i)).toBeTruthy();
    expect(document.querySelector("time")?.getAttribute("datetime")).toBe(
      oldSummary.collectedAt,
    );
  });

  it("recovers from an initial failure when Retry succeeds", async () => {
    const failed = failedState(null);
    tauri.fetch
      .mockResolvedValueOnce(failed)
      .mockResolvedValueOnce(failed)
      .mockResolvedValueOnce(collectionState(summary(7_000), { revision: 6 }));
    render(<App />);
    await screen.findByText("Usage unavailable");
    await waitFor(() => expect(tauri.fetch).toHaveBeenCalledTimes(2));
    await act(async () => {
      screen.getByText("Try again").click();
    });
    await waitTotal("7K");
  });

  it("does not start a fetch when a tray state event arrives", async () => {
    tauri.fetch.mockResolvedValueOnce(collectionState(summary(13_590_000)));
    render(<App />);
    await waitTotal("13.59M");
    expect(tauri.fetch).toHaveBeenCalledTimes(1);

    await act(async () => {
      tauri.tray?.({ payload: collectionState(summary(9_000_000), { revision: 4 }) });
    });
    // The event applied the snapshot directly; still only the one mount fetch.
    await waitTotal("9M");
    expect(tauri.fetch).toHaveBeenCalledTimes(1);
  });

  it("applies periodic failure and later success events without dropping data", async () => {
    const initial = summary(10_000);
    tauri.fetch.mockResolvedValueOnce(collectionState(initial));
    render(<App />);
    await waitTotal("10K");

    const failed = failedState(initial, false, 4);
    failed.lastAttempt = { ...failed.lastAttempt!, trigger: "periodic" };
    await act(async () => tauri.tray?.({ payload: failed }));
    expect(screen.getByText(/data below is still recent/i)).toBeTruthy();
    await waitTotal("10K");

    await act(async () => {
      tauri.tray?.({
        payload: collectionState(summary(30_000), { revision: 6 }),
      });
    });
    await waitTotal("30K");
    expect(screen.queryByText(/refresh failed/i)).toBeNull();
  });

  it("rejects a late initialization read after a newer event", async () => {
    let resolveRead: ((state: UsageCollectionState) => void) | undefined;
    tauri.fetch.mockImplementationOnce(
      () =>
        new Promise<UsageCollectionState>((resolve) => {
          resolveRead = resolve;
        }),
    );
    render(<App />);
    await waitFor(() => expect(tauri.fetch).toHaveBeenCalledTimes(1));

    await act(async () => {
      tauri.tray?.({
        payload: collectionState(summary(20_000), { revision: 6 }),
      });
    });
    await waitTotal("20K");
    await act(async () => {
      resolveRead?.(collectionState(summary(10_000), { revision: 2 }));
    });
    await waitTotal("20K");
  });

  it("rejects a late manual response after a newer periodic result", async () => {
    let resolveManual: ((state: UsageCollectionState) => void) | undefined;
    tauri.fetch
      .mockResolvedValueOnce(collectionState(summary(10_000)))
      .mockImplementationOnce(
        () =>
          new Promise<UsageCollectionState>((resolve) => {
            resolveManual = resolve;
          }),
      );
    render(<App />);
    await waitTotal("10K");
    await act(async () => {
      screen.getByText("Refresh").click();
    });
    await act(async () => {
      const periodic = collectionState(summary(30_000), { revision: 6 });
      periodic.lastAttempt = {
        ...periodic.lastAttempt!,
        trigger: "periodic",
      };
      tauri.tray?.({ payload: periodic });
    });
    await waitTotal("30K");
    await act(async () => {
      resolveManual?.(collectionState(summary(20_000), { revision: 4 }));
    });
    await waitTotal("30K");
  });

  it("coalesces rapid refresh clicks before the native running event arrives", async () => {
    tauri.fetch
      .mockResolvedValueOnce(collectionState(summary(10_000)))
      .mockImplementationOnce(() => new Promise<UsageCollectionState>(() => {}));
    render(<App />);
    await waitTotal("10K");
    const refresh = screen.getByText("Refresh");
    await act(async () => {
      refresh.click();
      refresh.click();
    });
    expect(tauri.fetch).toHaveBeenCalledTimes(2);
  });

  it("unmounts without leaking listeners or updating state afterwards", async () => {
    tauri.fetch.mockResolvedValueOnce(collectionState(summary(13_590_000)));
    const { unmount, container } = render(<App />);
    await waitTotal("13.59M");
    // Wait for the async registrations to resolve so unlisten spies are set.
    await waitFor(() => {
      expect(tauri.focusUnlisten).toBeDefined();
      expect(tauri.trayUnlisten).toBeDefined();
    });

    unmount();
    // Cleanup released all webview listeners (focus, state, close notice).
    expect(tauri.focusUnlisten).toHaveBeenCalledTimes(1);
    expect(tauri.trayUnlisten).toHaveBeenCalledTimes(1);
    expect(tauri.noticeUnlisten).toHaveBeenCalledTimes(1);

    // Focus/event firing after unmount is a no-op: no throw, no state update.
    await act(async () => {
      tauri.focus?.({ payload: true });
      tauri.tray?.({ payload: collectionState(summary(5_000_000), { revision: 4 }) });
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
    tauri.fetch.mockResolvedValueOnce(collectionState(summary(13_590_000)));
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
    tauri.fetch.mockResolvedValueOnce(collectionState(summary(13_590_000)));
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
    tauri.fetch.mockResolvedValueOnce(collectionState(summary(100)));
    render(<App />);
    await screen.findByText("Token Breakdown");

    // 100 total → components 30/10/60, with no residual categories.
    expect(screen.getByText("Input")).toBeTruthy();
    expect(screen.getByText("30%")).toBeTruthy();
    expect(screen.getByText("10%")).toBeTruthy();
    expect(screen.getByText("60%")).toBeTruthy();
    // Empty optional categories do not render.
    expect(screen.queryByText("Cache creation")).toBeNull();
    expect(screen.queryByText("Reasoning")).toBeNull();
    expect(screen.queryByText("Unclassified")).toBeNull();
  });

  it("describes agent-level reasoning without presenting it as a model", async () => {
    const s = summary(100);
    s.today.agents[0] = {
      id: "antigravity",
      displayName: "Antigravity",
      tokens: 1_000,
      reasoningTokens: 700,
      unclassifiedTokens: 0,
      models: [
        {
          modelName: "gemini-3.7-flash",
          modelDisplayName: "gemini-3.7-flash",
          inputTokens: 100,
          outputTokens: 100,
          cacheReadTokens: 100,
          cacheCreationTokens: 0,
          totalTokens: 300,
        },
      ],
    };
    tauri.fetch.mockResolvedValueOnce(collectionState(s));
    render(<App />);
    await screen.findByText("Today");

    const toggle = agentToggle("antigravity");
    await act(async () => {
      toggle.click();
    });
    // Model composition line renders the in/out/cache-read figures, the model's
    // own cached-input share, and the model's total.
    expect(
      screen.getByText("100 in · 100 out · 100 cache read · ~50% cached input"),
    ).toBeTruthy();
    expect(screen.getByText("Agent total includes 700 reasoning")).toBeTruthy();
    expect(screen.queryByText("Other")).toBeNull();
    expect(document.querySelectorAll(".model-row")).toHaveLength(1);
  });

  it("labels unknown residuals as unclassified at both scopes", async () => {
    const s = summary(1_000);
    s.today.tokenBreakdown = {
      inputTokens: 100,
      outputTokens: 100,
      cacheReadTokens: 100,
      cacheCreationTokens: 0,
      reasoningTokens: 400,
      unclassifiedTokens: 300,
    };
    s.today.agents[0] = {
      ...s.today.agents[0]!,
      tokens: 1_000,
      reasoningTokens: 0,
      unclassifiedTokens: 700,
      models: [{ ...s.today.agents[0]!.models[0]!, totalTokens: 300 }],
    };
    tauri.fetch.mockResolvedValueOnce(collectionState(s));
    render(<App />);
    await screen.findByText("Token Breakdown");

    expect(screen.getByText("Reasoning")).toBeTruthy();
    expect(screen.getByText("Unclassified")).toBeTruthy();
    await act(async () => {
      agentToggle("claude").click();
    });
    expect(screen.getByText("Agent total includes 700 unclassified tokens")).toBeTruthy();
  });

  it("trend filter switching updates the bar series and aggregate", async () => {
    tauri.fetch.mockResolvedValueOnce(collectionState(summary(13_590_000)));
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

  it("disables Refresh and still labels it while a refresh is in flight", async () => {
    const initial = summary(13_590_000);
    tauri.fetch
      .mockResolvedValueOnce(collectionState(initial))
      // The manual refresh never resolves, so the in-flight state is observable.
      .mockImplementationOnce(() => new Promise<UsageCollectionState>(() => {}));
    render(<App />);
    await waitTotal("13.59M");

    await act(async () => {
      screen.getByText("Refresh").click();
      const running = collectionState(initial, { revision: 3, refreshing: true });
      running.lastAttempt = {
        ...running.lastAttempt!,
        id: 2,
        trigger: "manual",
        finishedAt: null,
        outcome: "inProgress",
      };
      tauri.tray?.({ payload: running });
    });

    const btn = screen.getByRole("button", {
      name: /Refreshing…/,
    }) as HTMLButtonElement;
    // Still a visible text label + disabled, not animation-only state.
    expect(btn.disabled).toBe(true);
    expect(btn.classList.contains("refreshing")).toBe(true);
    expect(btn.querySelector(".refresh-icon")).toBeTruthy();
  });

  it("renders breakdown proportion bars with unrounded widths while text stays integer", async () => {
    const s = summary(30);
    // 7/30, 11/30, 12/30 — only the text % is rounded; the bar width must not be.
    s.today.tokenBreakdown = {
      inputTokens: 7,
      outputTokens: 11,
      cacheReadTokens: 12,
      cacheCreationTokens: 0,
      reasoningTokens: 0,
      unclassifiedTokens: 0,
    };
    tauri.fetch.mockResolvedValueOnce(collectionState(s));
    const { container } = render(<App />);
    await screen.findByText("Token Breakdown");

    const rows = Array.from(
      container.querySelectorAll(".breakdown-row"),
    ) as HTMLElement[];
    // Input / Output / Cache read only (all optional categories are 0 → hidden).
    expect(rows).toHaveLength(3);
    const inputRow = rows[0]!;
    const outputRow = rows[1]!;
    // Unrounded width from the true share…
    expect(inputRow.style.getPropertyValue("--bar-width")).toBe(`${(7 / 30) * 100}%`);
    expect(outputRow.style.getPropertyValue("--bar-width")).toBe(`${(11 / 30) * 100}%`);
    // …while the accessible text stays the rounded integer percent.
    expect(inputRow.querySelector(".breakdown-pct")?.textContent).toBe("23%");
    expect(outputRow.querySelector(".breakdown-pct")?.textContent).toBe("37%");
  });

  it("shows a token-weighted agent cached-input summary, never an arithmetic average", async () => {
    tauri.fetch.mockResolvedValueOnce(collectionState(summary(13_590_000)));
    render(<App />);
    await waitTotal("13.59M");

    await act(async () => {
      agentToggle("codex").click();
    });

    // All codex tokens are covered by its two models (no residual), so the agent
    // summary is trustworthy. Weighted: 3_320_000 / 5_040_000 ≈ 66%.
    expect(screen.getByText("~66% cached input across 2 models")).toBeTruthy();
    // An average of the model shares (67% and 14% → ~41%) must NOT appear.
    expect(screen.queryByText("~41% cached input across 2 models")).toBeNull();
  });

  it("hides the agent cached-input summary when model details do not cover the agent total", async () => {
    const s = summary(13_590_000);
    // Codex has two models but also tokens outside them, so the model subset must
    // not present itself as the whole agent.
    s.today.agents = s.today.agents.map((a) =>
      a.id === "codex"
        ? {
            ...a,
            tokens: a.models.reduce((sum, m) => sum + m.totalTokens, 0) + 100_000,
          }
        : a,
    );
    tauri.fetch.mockResolvedValueOnce(collectionState(s));
    render(<App />);
    await waitTotal("13.59M");

    await act(async () => {
      agentToggle("codex").click();
    });

    expect(document.querySelector(".model-agent-summary")).toBeNull();
    // Each model still reports its own cached-input share.
    const compositions = Array.from(
      document.querySelectorAll(".model-row .model-composition"),
    );
    expect(compositions).toHaveLength(2);
    expect(
      compositions.every((el) => /cached input/.test(el.textContent ?? "")),
    ).toBe(true);
  });
});

describe("App — cost, coverage and statistics transparency (T02)", () => {
  it("renders a priced zero as a real $0.00", async () => {
    const s = summary(1_000);
    s.today.estimatedCostUsd = 0;
    s.today.costUnknownReason = null;
    tauri.fetch.mockResolvedValueOnce(collectionState(s));
    render(<App />);
    await waitTotal("1K");
    expect(screen.getByText("Est. cost $0.00")).toBeTruthy();
  });

  it("shows a missing-price day as unavailable with the concrete reason, never $0.00", async () => {
    const s = summary(1_000);
    s.today.estimatedCostUsd = null;
    s.today.costUnknownReason = "missingModelPricing";
    tauri.fetch.mockResolvedValueOnce(collectionState(s));
    render(<App />);
    await waitTotal("1K");
    expect(
      screen.getByText("Est. cost unavailable — missing model prices"),
    ).toBeTruthy();
    expect(document.querySelector(".meta")?.textContent).not.toContain("$0.00");
  });

  it("marks a no-usage day as N/A, distinct from missing prices", async () => {
    const s = summary(0);
    // summary(0) keeps the agent rows but the adapter-derived zero day has
    // neither usage nor a price reason; mirror the zero-day shape.
    s.today.agents = [];
    s.today.estimatedCostUsd = null;
    s.today.costUnknownReason = null;
    s.today.cacheReadShare = null;
    tauri.fetch.mockResolvedValueOnce(collectionState(s));
    render(<App />);
    await waitTotal("0");
    expect(screen.getByText("Est. cost N/A — no usage")).toBeTruthy();
    expect(screen.queryByText(/missing model prices/)).toBeNull();
  });

  it("surfaces skipped-record diagnostics as a coverage banner without paths", async () => {
    const s = summary(13_590_000);
    s.coverage = {
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
    tauri.fetch.mockResolvedValueOnce(collectionState(s));
    render(<App />);
    await waitTotal("13.59M");
    const banner = document.querySelector(".coverage-banner");
    expect(banner?.textContent).toContain(
      "Coverage may be incomplete — Claude Code: 2 records were malformed and skipped.",
    );
    // Totals stay visible (accepted data is not hidden)…
    expect(document.querySelector(".total")?.textContent).toContain("13.59M");
    // …and no path or raw detail can leak through the sanitized payload.
    expect(document.body.textContent).not.toContain("C:\\");
  });

  it("explains scope, token types, the cache denominator and the estimate's meaning", async () => {
    tauri.fetch.mockResolvedValueOnce(collectionState(summary(13_590_000)));
    render(<App />);
    await waitTotal("13.59M");

    const about = document.querySelector(".about-stats") as HTMLDetailsElement;
    expect(about).toBeTruthy();
    about.open = true;
    const text = about.textContent ?? "";
    expect(text).toContain("Windows user account");
    expect(text).toContain("Cache read");
    expect(text).toContain("Cache creation");
    expect(text).toContain("Reasoning");
    expect(text).toContain("Unclassified");
    expect(text).toContain("cache read ÷ (input + cache read + cache creation)");
    expect(text).toContain("reference estimate");
    expect(text).toContain("not a bill, subscription charge, or account credit");
  });

  it("offers local-check guidance in the empty state without claiming an agent is uninstalled", async () => {
    const s = summary(0);
    s.today.agents = [];
    s.today.estimatedCostUsd = null;
    s.today.costUnknownReason = null;
    s.today.cacheReadShare = null;
    tauri.fetch.mockResolvedValueOnce(collectionState(s));
    render(<App />);
    await waitTotal("0");

    expect(screen.getByText("No agent usage was found for today.")).toBeTruthy();
    const help = document.querySelector(".empty-help") as HTMLDetailsElement;
    expect(help).toBeTruthy();
    help.open = true;
    const text = help.textContent ?? "";
    expect(text).toContain("Windows user account");
    // The empty state describes missing records, never "not installed".
    expect(text).not.toMatch(/not installed/i);
  });

  it("adds a local-check hint to the first-failure error state", async () => {
    const failure = failedState(null);
    tauri.fetch
      .mockResolvedValueOnce(failure)
      .mockResolvedValueOnce(failure);
    render(<App />);
    expect(await screen.findByText("Usage unavailable")).toBeTruthy();
    expect(
      screen.getByText(/check that a supported coding agent has been used here/i),
    ).toBeTruthy();
  });
});

describe("App — bilingual presentation (T03)", () => {
  it("renders the Chinese surface when the system language is Chinese", async () => {
    setNavigatorLanguage("zh-CN");
    tauri.fetch.mockResolvedValueOnce(collectionState(summary(13_590_000)));
    render(<App />);
    expect(await screen.findByText("今日")).toBeTruthy();
    expect(screen.getByRole("button", { name: "刷新" })).toBeTruthy();
    expect(screen.getByText("Token 构成")).toBeTruthy();
    expect(screen.getByText("最近 7 天")).toBeTruthy();
    // Dynamic agent/model data is never translated.
    await act(async () => {
      agentToggle("codex").click();
    });
    expect(screen.getByText("gpt-5.6-sol")).toBeTruthy();
  });

  it("renders the first-failure error page in Chinese", async () => {
    setNavigatorLanguage("zh-CN");
    const failure = failedState(null);
    tauri.fetch
      .mockResolvedValueOnce(failure)
      .mockResolvedValueOnce(failure);
    render(<App />);
    expect(await screen.findByText("无法获取用量")).toBeTruthy();
    expect(screen.getByText("重试")).toBeTruthy();
    expect(screen.getByText("无法刷新用量，可以重试。")).toBeTruthy();
  });

  it("states the full-day time basis on the header delta", async () => {
    tauri.fetch.mockResolvedValueOnce(collectionState(summary(13_590_000)));
    render(<App />);
    await waitTotal("13.59M");
    // Today is a running total compared against yesterday's FULL day.
    expect(document.querySelector(".total-delta")?.textContent).toContain(
      "vs yesterday (full day)",
    );
  });

  it("keeps the direction of a delta colour-neutral (arrow + sign only)", async () => {
    tauri.fetch.mockResolvedValueOnce(collectionState(summary(13_590_000)));
    render(<App />);
    await waitTotal("13.59M");
    // up/down/flat share one neutral ink class; the arrow carries direction.
    const delta = document.querySelector(".total-delta") as HTMLElement;
    expect(delta.className).toContain("delta up");
  });
});

describe("App — tray residency and minimal preferences (T05)", () => {
  function openSettings() {
    const section = document.querySelector(
      ".settings-section",
    ) as HTMLDetailsElement;
    if (!section) throw new Error("settings section not found");
    section.open = true;
    return section;
  }

  function languageSelect(): HTMLSelectElement {
    return document.querySelector(".settings-section select") as HTMLSelectElement;
  }

  function launchCheckboxes(): HTMLInputElement[] {
    return Array.from(
      document.querySelectorAll<HTMLInputElement>(
        '.settings-section input[type="checkbox"]',
      ),
    );
  }

  it("renders the minimal preference controls with the shipped defaults", async () => {
    tauri.fetch.mockResolvedValueOnce(collectionState(summary(13_590_000)));
    render(<App />);
    await waitTotal("13.59M");
    openSettings();

    expect(screen.getByText("Preferences")).toBeTruthy();
    expect(languageSelect().value).toBe("system");
    const checkboxes = launchCheckboxes();
    expect(checkboxes).toHaveLength(2);
    expect(checkboxes[0]!.checked).toBe(false); // start with Windows: off
    expect(checkboxes[1]!.checked).toBe(false); // start hidden to tray: off
  });

  it("an explicit persisted language overrides the system resolution", async () => {
    setNavigatorLanguage("en-US");
    tauri.getPreferences.mockResolvedValueOnce({
      ...defaultPreferences,
      language: "zh-CN",
    });
    tauri.fetch.mockResolvedValueOnce(collectionState(summary(13_590_000)));
    render(<App />);
    expect(await screen.findByText("今日")).toBeTruthy();
    expect(screen.getByRole("button", { name: "刷新" })).toBeTruthy();
  });

  it("switching the language updates every label and persists the choice", async () => {

    tauri.fetch.mockResolvedValueOnce(collectionState(summary(13_590_000)));
    render(<App />);
    await waitTotal("13.59M");
    openSettings();

    await act(async () => {
      fireEvent.change(languageSelect(), { target: { value: "zh-CN" } });
    });
    // Runtime switching is allowed (v0.4 plan §4.5): the whole surface,
    // including the settings section itself, re-renders in Chinese.
    expect(screen.getByText("今日")).toBeTruthy();
    expect(screen.getByText("偏好设置")).toBeTruthy();
    expect(tauri.updatePreferences).toHaveBeenCalledWith({
      language: "zh-CN",
    });
    expect(languageSelect().value).toBe("zh-CN");
  });

  it("toggling a launch preference persists the new value optimistically", async () => {

    tauri.fetch.mockResolvedValueOnce(collectionState(summary(13_590_000)));
    render(<App />);
    await waitTotal("13.59M");
    openSettings();

    await act(async () => {
      fireEvent.click(launchCheckboxes()[0]!);
    });
    expect(tauri.updatePreferences).toHaveBeenCalledWith({
      startWithWindows: true,
    });
    expect(launchCheckboxes()[0]!.checked).toBe(true);
    expect(document.querySelector(".settings-error")).toBeNull();
  });

  it("a failed preference save reverts the control and says so honestly", async () => {

    tauri.fetch.mockResolvedValueOnce(collectionState(summary(13_590_000)));
    render(<App />);
    await waitTotal("13.59M");
    openSettings();

    // The Rust command rejects BEFORE persisting when the OS effect fails
    // (e.g. the Run key write), so the checkbox must revert — never fake a
    // saved state.
    tauri.updatePreferences.mockRejectedValueOnce(new Error("registry denied"));
    await act(async () => {
      fireEvent.click(launchCheckboxes()[1]!);
    });
    expect(launchCheckboxes()[1]!.checked).toBe(false);
    expect(
      screen.getByText("This preference could not be saved."),
    ).toBeTruthy();
  });

  it("failed language persistence keeps the previous language and selection", async () => {
    tauri.fetch.mockResolvedValueOnce(collectionState(summary(13_590_000)));
    render(<App />);
    await waitTotal("13.59M");
    openSettings();
    tauri.updatePreferences.mockRejectedValueOnce(new Error("disk denied"));
    await act(async () => { fireEvent.change(languageSelect(), { target: { value: "zh-CN" } }); });
    expect(languageSelect().value).toBe("system");
    expect(screen.getByText("This preference could not be saved.")).toBeTruthy();
  });

  it("failed close acknowledgement remains visible and does not hide", async () => {
    tauri.fetch.mockResolvedValueOnce(collectionState(summary(13_590_000)));
    render(<App />);
    await waitTotal("13.59M");
    await act(async () => { tauri.closeNotice?.({ payload: undefined }); });
    tauri.updatePreferences.mockRejectedValueOnce(new Error("disk denied"));
    await act(async () => { fireEvent.click(screen.getByText("Got it — don’t show again")); });
    expect(screen.getByText("Still running in the tray")).toBeTruthy();
    expect(screen.getByRole("alert").textContent).toContain("could not be saved");
    expect(tauri.hideMainWindow).not.toHaveBeenCalled();
  });

  it("the first-close notice explains tray residency and remembers the acknowledgement", async () => {

    tauri.fetch.mockResolvedValueOnce(collectionState(summary(13_590_000)));
    render(<App />);
    await waitTotal("13.59M");

    await act(async () => {
      tauri.closeNotice?.({ payload: undefined });
    });
    expect(screen.getByText("Still running in the tray")).toBeTruthy();
    expect(
      screen.getByText(/keeps Coding Agent Monitor running in the system tray/i),
    ).toBeTruthy();

    await act(async () => {
      fireEvent.click(screen.getByText("Got it — don’t show again"));
    });
    expect(tauri.updatePreferences).toHaveBeenCalledWith({
      closeNoticeAcknowledged: true,
    });
    expect(screen.queryByText("Still running in the tray")).toBeNull();
    expect(tauri.hideMainWindow).toHaveBeenCalledTimes(1);
  });

  it("the notice’s hide-once action hides without acknowledging", async () => {

    tauri.fetch.mockResolvedValueOnce(collectionState(summary(13_590_000)));
    render(<App />);
    await waitTotal("13.59M");

    await act(async () => {
      tauri.closeNotice?.({ payload: undefined });
    });
    await act(async () => {
      fireEvent.click(screen.getByText("Hide now"));
    });
    expect(tauri.hideMainWindow).toHaveBeenCalledTimes(1);
    expect(tauri.updatePreferences).not.toHaveBeenCalledWith({
      closeNoticeAcknowledged: true,
    });
    expect(screen.queryByText("Still running in the tray")).toBeNull();
  });

  it("the close notice is reachable from the loading and error states too", async () => {
    tauri.fetch.mockRejectedValueOnce(new Error("no data"));
    render(<App />);
    expect(await screen.findByText("Usage unavailable")).toBeTruthy();

    await act(async () => {
      tauri.closeNotice?.({ payload: undefined });
    });
    expect(screen.getByText("Still running in the tray")).toBeTruthy();
  });
});


describe("T07 on-demand history", () => {
  function historyFixture(): import("./types/usage").HistoryUsage {
    const days = Array.from({ length: 30 }, (_, index) => {
      const date = new Date(Date.UTC(2026, 7, 24 - 29 + index)).toISOString().slice(0, 10);
      return day(date, 0);
    });
    return { scope: { startDate: days[0].date, endDate: days[29].date, timeZone: "UTC" },
      collectedAt: "2026-08-24T12:00:00Z", days, estimatedCostUsd: null,
      coverage: { status: "complete", diagnostics: [] } };
  }
  it("does no historical work on mount or tray refresh; refuses late results after 30/7/30", async () => {
    tauri.fetch.mockResolvedValue(collectionState(null));
    // Use an explicit empty but successful public summary.
    const h = historyFixture();
    tauri.fetch.mockResolvedValue(collectionState({ collectedAt: h.collectedAt,
      today: h.days[29], last7Days: h.days.slice(-7), coverage: h.coverage }));
    const pending: ((h: import("./types/usage").HistoryUsage) => void)[] = [];
    tauri.history.mockImplementation(() => new Promise(resolve => pending.push(resolve)));
    const { container } = render(<App />);
    await screen.findByRole("button", { name: "30 days" });
    expect(tauri.history).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "30 days" }));
    expect(screen.getByText("Loading 30-day history…")).toBeTruthy();
    expect(container.querySelectorAll(".trend-day")).toHaveLength(0);
    fireEvent.click(screen.getByRole("button", { name: "7 days" }));
    fireEvent.click(screen.getByRole("button", { name: "30 days" }));
    await act(async () => pending[0](h));
    expect(container.querySelectorAll(".trend-day")).toHaveLength(0);
    await act(async () => pending[1](h));
    expect(container.querySelectorAll(".trend-day")).toHaveLength(30);
    await act(async () => tauri.focus?.({ payload: true }));
    expect(tauri.history).toHaveBeenCalledTimes(2);
    expect(container.querySelectorAll(".trend-day")).toHaveLength(30);
    fireEvent.click(container.querySelector(".trend-day")!);
    expect(screen.getByRole("region", { name: "Selected day details" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Back to today" }));
    expect(container.querySelector(".day-detail")).toBeNull();
  });
  it("shows history failure without zero results and recovers on retry", async () => {
    const h = historyFixture();
    tauri.fetch.mockResolvedValue(collectionState({ collectedAt: h.collectedAt,
      today: h.days[29], last7Days: h.days.slice(-7), coverage: h.coverage }));
    tauri.history.mockRejectedValueOnce(new Error("private source text"));
    tauri.history.mockResolvedValueOnce(h);
    const { container } = render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "30 days" }));
    await screen.findByText("History could not be loaded.");
    expect(container.textContent).not.toContain("private source text");
    expect(container.querySelectorAll(".trend-day")).toHaveLength(0);
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    await waitFor(() => expect(container.querySelectorAll(".trend-day")).toHaveLength(30));
  });
});
