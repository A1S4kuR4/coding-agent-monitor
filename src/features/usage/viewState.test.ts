import { describe, expect, it } from "vitest";

import type { UsageSummary } from "../../types/usage";
import { viewReducer, type ViewAction, type ViewState } from "./viewState";

function summary(over: Partial<UsageSummary> = {}): UsageSummary {
  return {
    collectedAt: "2026-08-24T12:00:00Z",
    today: {
      date: "2026-08-24",
      totalTokens: 13_590_000,
      tokenBreakdown: {
        inputTokens: 4_077_000,
        outputTokens: 1_359_000,
        cacheReadTokens: 8_154_000,
        cacheCreationTokens: 0,
        reasoningTokens: 0,
        unclassifiedTokens: 0,
      },
      estimatedCostUsd: 12.0,
      cacheReadShare: 0.7,
      agents: [
        { id: "claude", displayName: "Claude Code", tokens: 8_420_000, reasoningTokens: 0, unclassifiedTokens: 0, models: [] },
        { id: "codex", displayName: "Codex", tokens: 5_170_000, reasoningTokens: 0, unclassifiedTokens: 0, models: [] },
      ],
    },
    last7Days: [],
    ...over,
  };
}

const start = (state: ViewState, action: ViewAction): ViewState =>
  viewReducer(state, action);

describe("viewReducer", () => {
  it("first load success lands on ready, not stale", () => {
    const next = start({ status: "loading" }, { type: "load-succeeded", summary: summary() });
    expect(next).toEqual({ status: "ready", summary: summary(), refreshing: false, stale: false });
  });

  it("first-load failure lands on the error page (no existing data)", () => {
    const next = start(
      { status: "loading" },
      { type: "load-failed", keepExisting: false, message: "boom" },
    );
    expect(next).toEqual({ status: "error", message: "boom" });
    // keepExisting is irrelevant when there is nothing to keep.
    const nextKeep = start(
      { status: "loading" },
      { type: "load-failed", keepExisting: true, message: "boom" },
    );
    expect(nextKeep).toEqual({ status: "error", message: "boom" });
  });

  it("refresh-started marks a ready view refreshing without dropping data", () => {
    const ready: ViewState = { status: "ready", summary: summary(), refreshing: false, stale: false };
    const next = start(ready, { type: "refresh-started" });
    expect(next).toMatchObject({ status: "ready", refreshing: true, stale: false });
    expect(next.status === "ready" && next.summary).toEqual(ready.status === "ready" && ready.summary);
  });

  it("refresh failure keeps last data and marks it stale (graceful degradation)", () => {
    const ready: ViewState = { status: "ready", summary: summary(), refreshing: false, stale: false };
    const next = start(
      ready,
      { type: "load-failed", keepExisting: true, message: "offline" },
    );
    expect(next).toMatchObject({ status: "ready", refreshing: false, stale: true });
    if (next.status !== "ready") throw new Error("expected ready");
    expect(next.summary).toEqual(summary());
  });

  it("refresh failure with no existing data still lands on the error page", () => {
    const ready: ViewState = { status: "ready", summary: summary(), refreshing: false, stale: false };
    const next = start(ready, { type: "load-failed", keepExisting: false, message: "hard" });
    expect(next).toEqual({ status: "error", message: "hard" });
  });

  it("retry (load-started) drops back to loading from the error page", () => {
    const next = start({ status: "error", message: "hard" }, { type: "load-started" });
    expect(next).toEqual({ status: "loading" });
  });

  it("a tray event applies a fresh snapshot and clears staleness", () => {
    const staleReady: ViewState = { status: "ready", summary: summary(), refreshing: false, stale: true };
    const fresh = summary({ collectedAt: "2026-08-24T13:00:00Z" });
    const next = start(staleReady, { type: "event-received", summary: fresh });
    expect(next).toEqual({ status: "ready", summary: fresh, refreshing: false, stale: false });
  });
});
