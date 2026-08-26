import { describe, expect, it } from "vitest";
import { buildAllChart, buildAgentChart, dayValue } from "./chartView";
import type { DailyUsage } from "../../types/usage";

function dtotal(totalTokens: number): number {
  return totalTokens;
}

/** Build a DailyUsage whose agents are the given (id, displayName, tokens) rows
 * and whose totalTokens is the sum of its agents (matching the Rust invariant). */
function day(
  date: string,
  rows: [string, string, number][],
): DailyUsage {
  const agents = rows.map(([id, displayName, tokens]) => ({
    id,
    displayName,
    tokens,
    models: [],
  }));
  return {
    date,
    totalTokens: agents.reduce((s, a) => s + a.tokens, 0),
    tokenBreakdown: {
      inputTokens: 0,
      outputTokens: 0,
      cacheReadTokens: 0,
      cacheCreationTokens: 0,
      otherTokens: dtotal(0),
    },
    estimatedCostUsd: null,
    cacheReadShare: null,
    agents,
  };
}

/** Assert all heights are finite and within [0, 100]. */
function expectFinite(days: ReturnType<typeof buildAllChart>) {
  for (const d of days) {
    for (const s of d.segments) {
      expect(Number.isFinite(s.height)).toBe(true);
      expect(s.height).toBeGreaterThanOrEqual(0);
      expect(s.height).toBeLessThanOrEqual(100);
    }
  }
}

describe("buildAllChart", () => {
  const weeks: DailyUsage[] = [
    day("2026-08-24", [
      ["claude", "Claude Code", 20_000_000],
      ["codex", "Codex", 10_000_000],
      ["antigravity", "Antigravity", 2_000_000],
    ]),
    day("2026-08-25", [
      ["claude", "Claude Code", 40_000_000],
      ["codex", "Codex", 20_000_000],
    ]),
  ];
  const maxDaily = 60_000_000;

  it("scales all segments against the window max so bars express absolute totals", () => {
    const [d24] = buildAllChart(weeks);
    // 08/24 total 32M of a 60M window => the bar is 32/60 = 53.33%.
    const sum = d24.segments.reduce((s, x) => s + x.height, 0);
    expect(sum).toBeCloseTo((32_000_000 / maxDaily) * 100, 5);
    // 08/25 is the max day => its bar fills 100%.
    const [, d25] = buildAllChart(weeks);
    const sum25 = d25.segments.reduce((s, x) => s + x.height, 0);
    expect(sum25).toBeCloseTo(100, 5);
  });

  it("orders segments bottom-up: Claude, Codex, then Antigravity", () => {
    const [d] = buildAllChart(weeks);
    const ids = d.segments.map((s) => s.agentId);
    expect(ids).toEqual(["claude", "codex", "antigravity"]);
  });

  it("gives the topmost non-zero segment the isTop flag only", () => {
    const [d] = buildAllChart(weeks);
    const tops = d.segments.filter((s) => s.isTop);
    expect(tops).toHaveLength(1);
    // In canonical order claude is last (top), and its stack height is non-zero.
    expect(d.segments[d.segments.length - 1].isTop).toBe(true);
  });

  it("produces no NaN/Infinity even with zero totals", () => {
    const days = [
      day("2026-08-19", []),
      day("2026-08-20", [["claude", "Claude Code", 5_000_000]]),
    ];
    const vm = buildAllChart(days);
    expectFinite(vm);
    expect(vm[0].segments).toHaveLength(0);
    expect(vm[1].segments[0].height).toBeCloseTo(100, 5);
  });

  it("preserves unknown agents after the known four", () => {
    const days = [
      day("2026-08-24", [
        ["mystery", "Some New Agent", 1_000_000],
        ["claude", "Claude Code", 2_000_000],
      ]),
    ];
    const [d] = buildAllChart(days);
    expect(d.segments.map((s) => s.agentId)).toEqual(["claude", "mystery"]);
    expect(d.segments[1].colorVar).toBe("--agent-unknown");
  });

  it("within-bar ratios are within ±0.5% of the true share", () => {
    const [d] = buildAllChart(weeks);
    const barSum = d.segments.reduce((s, x) => s + x.height, 0);
    const dayTotal = d.segments.reduce((s, x) => s + x.tokens, 0);
    for (const seg of d.segments) {
      // Each segment's fraction of the rendered bar must equal its share of the
      // day total (barSum == dayTotal):(segment/barSum == tokens/dayTotal).
      const trueShare = (seg.tokens / dayTotal) * 100;
      const renderedBarShare = (seg.height / barSum) * 100;
      expect(Math.abs(renderedBarShare - trueShare)).toBeLessThanOrEqual(0.5);
    }
  });
});

describe("buildAgentChart", () => {
  const weeks: DailyUsage[] = [
    day("2026-08-24", [
      ["claude", "Claude Code", 0],
      ["codex", "Codex", 10_000_000],
    ]),
    day("2026-08-25", [
      ["claude", "Claude Code", 30_000_000],
      ["codex", "Codex", 20_000_000],
    ]),
  ];

  it("renders a single monochrome bar scaled by the agent's own max", () => {
    const vm = buildAgentChart(weeks, "codex");
    const maxCodex = 20_000_000;
    expect(vm[0].segments).toHaveLength(1);
    expect(vm[0].segments[0].height).toBeCloseTo((10_000_000 / maxCodex) * 100, 5);
    expect(vm[1].segments[0].height).toBeCloseTo(100, 5);
    expect(vm[0].segments[0].colorVar).toBe("--agent-codex");
    expect(vm[0].segments[0].isTop).toBe(true);
  });

  it("leaves days where the agent had 0 tokens with no data segment", () => {
    const vm = buildAgentChart(
      [
        day("2026-08-24", [["codex", "Codex", 0]]),
        day("2026-08-25", [["codex", "Codex", 8_000_000]]),
      ],
      "codex",
    );
    expect(vm[0].segments).toHaveLength(0);
    expect(vm[0].totalTokens).toBe(0);
  });

  it("keeps all tracks when the selected agent is zero all week", () => {
    const vm = buildAgentChart(weeks, "antigravity");
    expect(vm).toHaveLength(2);
    for (const d of vm) expect(d.segments).toHaveLength(0);
    expectFinite(vm);
  });
});

describe("dayValue", () => {
  it("returns the day total for All mode and the agent value otherwise", () => {
    const d = day("2026-08-25", [
      ["claude", "Claude Code", 30_000_000],
      ["codex", "Codex", 20_000_000],
    ]);
    expect(dayValue(d, null)).toBe(50_000_000);
    expect(dayValue(d, "claude")).toBe(30_000_000);
    expect(dayValue(d, "antigravity")).toBe(0);
  });
});