import { describe, expect, it } from "vitest";
import {
  effectiveAgentFilter,
  visibleFilterAgents,
} from "./trendFilter";
import type { DailyUsage } from "../../types/usage";

function day(date: string, agentIds: string[]): DailyUsage {
  const agents = agentIds.map((id) => ({
    id,
    displayName: id,
    tokens: 1_000,
    reasoningTokens: 0,
    unclassifiedTokens: 0,
    models: [],
  }));
  return {
    date,
    totalTokens: agents.length * 1_000,
    tokenBreakdown: {
      inputTokens: 0,
      outputTokens: 0,
      cacheReadTokens: 0,
      cacheCreationTokens: 0,
      reasoningTokens: 0,
      unclassifiedTokens: 0,
    },
    estimatedCostUsd: null,
    costUnknownReason: null,
    cacheReadShare: null,
    agents,
  };
}

describe("effectiveAgentFilter", () => {
  it("passes through a filter the trend window contains", () => {
    const days = [day("2026-08-24", ["claude"]), day("2026-08-25", ["claude", "codex"])];
    expect(effectiveAgentFilter("codex", days)).toBe("codex");
  });

  it("falls back to All (null) when the selected agent has vanished from the range", () => {
    const days = [day("2026-08-24", ["claude"]), day("2026-08-25", ["codex"])];
    expect(effectiveAgentFilter("antigravity", days)).toBeNull();
    expect(effectiveAgentFilter("mystery", [])).toBeNull();
  });

  it("keeps All as All", () => {
    expect(effectiveAgentFilter(null, [day("2026-08-25", ["claude"])])).toBeNull();
  });
});

const recognized = [
  { id: "claude", displayName: "Claude Code" },
  { id: "codex", displayName: "Codex" },
  { id: "antigravity", displayName: "Antigravity" },
  { id: "opencode", displayName: "OpenCode" },
  { id: "a5", displayName: "Agent Five" },
  { id: "a6", displayName: "Agent Six" },
  { id: "a7", displayName: "Agent Seven" },
];

describe("visibleFilterAgents", () => {
  it("shows every chip without a disclosure when within capacity", () => {
    const { visible, hidden } = visibleFilterAgents(recognized.slice(0, 4), null);
    expect(visible).toHaveLength(4);
    expect(hidden).toEqual([]);
  });

  it("keeps the first four inline and moves the rest into More agents", () => {
    const { visible, hidden } = visibleFilterAgents(recognized, null);
    expect(visible.map((a) => a.id)).toEqual([
      "claude",
      "codex",
      "antigravity",
      "opencode",
    ]);
    expect(hidden.map((a) => a.id)).toEqual(["a5", "a6", "a7"]);
  });

  it("swaps a selected overflow agent into the visible set without reordering", () => {
    const { visible, hidden } = visibleFilterAgents(recognized, "a6");
    expect(visible.map((a) => a.id)).toEqual([
      "claude",
      "codex",
      "antigravity",
      "a6",
    ]);
    expect(hidden.map((a) => a.id)).toEqual(["opencode", "a5", "a7"]);
  });

  it("keeps a selected inline agent in place", () => {
    const { visible, hidden } = visibleFilterAgents(recognized, "codex");
    expect(visible.map((a) => a.id)).toEqual([
      "claude",
      "codex",
      "antigravity",
      "opencode",
    ]);
    expect(hidden.map((a) => a.id)).toEqual(["a5", "a6", "a7"]);
  });

  it("is deterministic and does not mutate the input", () => {
    const once = visibleFilterAgents(recognized, "a7");
    const twice = visibleFilterAgents(recognized, "a7");
    expect(once).toEqual(twice);
    expect(recognized).toHaveLength(7);
  });
});
