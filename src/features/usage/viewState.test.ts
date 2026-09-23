import { describe, expect, it } from "vitest";

import type { UsageCollectionState, UsageSummary } from "../../types/usage";
import {
  initialViewState,
  needsRefresh,
  viewReducer,
  type ViewState,
} from "./viewState";

const summary: UsageSummary = {
  collectedAt: "2026-09-06T04:00:00Z",
  today: {
    date: "2026-09-06",
    totalTokens: 10,
    tokenBreakdown: {
      inputTokens: 10,
      outputTokens: 0,
      cacheReadTokens: 0,
      cacheCreationTokens: 0,
      reasoningTokens: 0,
      unclassifiedTokens: 0,
    },
    estimatedCostUsd: null,
    costUnknownReason: null,
    cacheReadShare: null,
    agents: [],
  },
  last7Days: [],
  coverage: { status: "complete", diagnostics: [] },
};

function state(
  revision: number,
  options: {
    snapshot?: UsageSummary | null;
    refreshing?: boolean;
    outcome?: "inProgress" | "succeeded" | "failed";
    freshness?: "fresh" | "stale" | "unknown";
    checkedAt?: string;
  } = {},
): UsageCollectionState {
  const scope = {
    startDate: "2026-08-31",
    endDate: "2026-09-06",
    timeZone: "Asia/Shanghai",
  };
  const snapshot = options.snapshot === undefined ? summary : options.snapshot;
  const outcome = options.outcome ?? "succeeded";
  return {
    revision,
    refreshing: options.refreshing ?? false,
    snapshot: snapshot ? { scope, summary: snapshot } : null,
    lastAttempt: {
      id: 1,
      trigger: "manual",
      scope,
      startedAt: "2026-09-06T03:59:59Z",
      finishedAt: outcome === "inProgress" ? null : "2026-09-06T04:00:00Z",
      outcome,
      failure: outcome === "failed" ? "failed" : null,
    },
    freshness: {
      status: options.freshness ?? (snapshot ? "fresh" : "unknown"),
      reason: snapshot ? "current" : "noSnapshot",
      checkedAt: options.checkedAt ?? "2026-09-06T04:00:00Z",
      currentDate: "2026-09-06",
      currentTimeZone: "Asia/Shanghai",
      staleAfterSeconds: 600,
    },
  };
}

const reduce = (view: ViewState, incoming: UsageCollectionState) =>
  viewReducer(view, { type: "state-received", state: incoming });

describe("viewReducer", () => {
  it("projects first success and first failure without inventing data", () => {
    expect(reduce(initialViewState, state(2))).toMatchObject({ status: "ready" });
    expect(
      reduce(
        initialViewState,
        state(2, { snapshot: null, outcome: "failed", freshness: "unknown" }),
      ),
    ).toMatchObject({ status: "error", reason: "failed" });
  });

  it("maps failure categories to stable reasons without raw details", () => {
    const timedOut = state(2, {
      snapshot: null,
      outcome: "failed",
      freshness: "unknown",
    });
    timedOut.lastAttempt = { ...timedOut.lastAttempt!, failure: "timedOut" };
    expect(reduce(initialViewState, timedOut)).toMatchObject({
      status: "error",
      reason: "timedOut",
    });
    const cancelled = state(2, {
      snapshot: null,
      outcome: "failed",
      freshness: "unknown",
    });
    cancelled.lastAttempt = { ...cancelled.lastAttempt!, failure: "cancelled" };
    expect(reduce(initialViewState, cancelled)).toMatchObject({
      status: "error",
      reason: "cancelled",
    });
  });

  it("keeps a snapshot while refresh is running or failed", () => {
    const ready = reduce(initialViewState, state(2));
    const running = reduce(
      ready,
      state(3, { refreshing: true, outcome: "inProgress" }),
    );
    expect(running).toMatchObject({
      status: "ready",
      collection: { refreshing: true, snapshot: { summary } },
    });
    const failed = reduce(running, state(4, { outcome: "failed" }));
    expect(failed).toMatchObject({
      status: "ready",
      collection: { refreshing: false, snapshot: { summary } },
    });
  });

  it("rejects late reads, events and responses by revision", () => {
    const newestSummary = {
      ...summary,
      today: { ...summary.today, totalTokens: 99 },
    };
    const newest = reduce(
      initialViewState,
      state(6, { snapshot: newestSummary, outcome: "succeeded" }),
    );
    const lateFailure = reduce(
      newest,
      state(4, { outcome: "failed", freshness: "stale" }),
    );
    expect(lateFailure).toBe(newest);
    const lateSuccess = reduce(newest, state(5));
    expect(lateSuccess).toBe(newest);
  });

  it("accepts a newer freshness evaluation at the same revision", () => {
    const ready = reduce(initialViewState, state(2));
    const stale = state(2, {
      freshness: "stale",
      checkedAt: "2026-09-06T04:11:00Z",
    });
    expect(reduce(ready, stale)).toMatchObject({
      status: "ready",
      collection: { freshness: { status: "stale" } },
    });
  });

  it("refreshes only absent or non-fresh idle state", () => {
    expect(needsRefresh(state(0, { snapshot: null, outcome: "failed" }))).toBe(true);
    expect(needsRefresh(state(2, { freshness: "stale" }))).toBe(true);
    expect(needsRefresh(state(2))).toBe(false);
    expect(
      needsRefresh(state(3, { refreshing: true, outcome: "inProgress" })),
    ).toBe(false);
  });
});
