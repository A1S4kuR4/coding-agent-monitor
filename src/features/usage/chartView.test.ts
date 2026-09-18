import { describe, expect, it } from "vitest";
import {
  allChartGrouping,
  buildAgentChart,
  buildAllChart,
  buildWeekBuckets,
  dayValue,
} from "./chartView";
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
    reasoningTokens: 0,
    unclassifiedTokens: 0,
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
      reasoningTokens: dtotal(0),
      unclassifiedTokens: dtotal(0),
    },
    estimatedCostUsd: null,
    costUnknownReason: null,
    cacheReadShare: null,
    agents,
  };
}

/** Assert all heights are finite and within [0, 100]. */
function expectFinite(days: ReturnType<typeof buildAllChart>["days"]) {
  for (const d of days) {
    for (const s of d.segments) {
      expect(Number.isFinite(s.height)).toBe(true);
      expect(s.height).toBeGreaterThanOrEqual(0);
      expect(s.height).toBeLessThanOrEqual(100);
    }
  }
}

describe("allChartGrouping", () => {
  it("keeps every agent individual when at most four carry tokens", () => {
    const days = [
      day("2026-08-24", [
        ["claude", "Claude Code", 20_000_000],
        ["codex", "Codex", 10_000_000],
        ["antigravity", "Antigravity", 2_000_000],
        ["opencode", "OpenCode", 1_000_000],
      ]),
    ];
    expect(allChartGrouping(days)).toEqual({
      memberIds: ["claude", "codex", "antigravity", "opencode"],
      hasOthers: false,
      othersCount: 0,
    });
  });

  it("groups the window's top three agents by total and folds the rest into others", () => {
    // Window totals: claude 60M, codex 30M, opencode 3M, others 2M+1M.
    const days = [
      day("2026-08-24", [
        ["claude", "Claude Code", 20_000_000],
        ["codex", "Codex", 10_000_000],
        ["tiny-a", "Tiny Agent A", 2_000_000],
      ]),
      day("2026-08-25", [
        ["claude", "Claude Code", 40_000_000],
        ["codex", "Codex", 20_000_000],
        ["opencode", "OpenCode", 3_000_000],
        ["tiny-b", "Tiny Agent B", 1_000_000],
      ]),
    ];
    expect(allChartGrouping(days)).toEqual({
      memberIds: ["claude", "codex", "opencode"],
      hasOthers: true,
      othersCount: 2,
    });
  });

  it("breaks total ties deterministically on the canonical order", () => {
    const days = [
      day("2026-08-25", [
        ["codex", "Codex", 5_000_000],
        ["claude", "Claude Code", 5_000_000],
        ["antigravity", "Antigravity", 5_000_000],
        ["opencode", "OpenCode", 5_000_000],
        ["mystery", "Mystery", 5_000_000],
      ]),
    ];
    // Equal totals: canonical order decides the three individual members.
    expect(allChartGrouping(days).memberIds).toEqual([
      "claude",
      "codex",
      "antigravity",
    ]);
  });

  it("is stable across repeated calls (pure, no mutation)", () => {
    const days = [
      day("2026-08-24", [
        ["claude", "Claude Code", 1_000_000],
        ["mystery", "Mystery", 2_000_000],
      ]),
    ];
    const first = allChartGrouping(days);
    const second = allChartGrouping(days);
    expect(first).toEqual(second);
    expect(days[0].agents).toHaveLength(2);
  });
});

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
    const { days: chartDays } = buildAllChart(weeks);
    // 08/24 total 32M of a 60M window => the bar is 32/60 = 53.33%.
    const sum = chartDays[0].segments.reduce((s, x) => s + x.height, 0);
    expect(sum).toBeCloseTo((32_000_000 / maxDaily) * 100, 5);
    // 08/25 is the max day => its bar fills 100%.
    const sum25 = chartDays[1].segments.reduce((s, x) => s + x.height, 0);
    expect(sum25).toBeCloseTo(100, 5);
  });

  it("orders segments bottom-up: Claude, Codex, then Antigravity", () => {
    const { days: chartDays } = buildAllChart(weeks);
    const ids = chartDays[0].segments.map((s) => s.agentId);
    expect(ids).toEqual(["claude", "codex", "antigravity"]);
  });

  it("gives the topmost non-zero segment the isTop flag only", () => {
    const { days: chartDays } = buildAllChart(weeks);
    const tops = chartDays[0].segments.filter((s) => s.isTop);
    expect(tops).toHaveLength(1);
    // In canonical order claude is last (top), and its stack height is non-zero.
    const segments = chartDays[0].segments;
    expect(segments[segments.length - 1].isTop).toBe(true);
  });

  it("produces no NaN/Infinity even with zero totals", () => {
    const days = [
      day("2026-08-19", []),
      day("2026-08-20", [["claude", "Claude Code", 5_000_000]]),
    ];
    const vm = buildAllChart(days);
    expectFinite(vm.days);
    expect(vm.days[0].segments).toHaveLength(0);
    expect(vm.days[1].segments[0].height).toBeCloseTo(100, 5);
  });

  it("preserves unknown agents after the known four when they stay individual", () => {
    const days = [
      day("2026-08-24", [
        ["mystery", "Some New Agent", 1_000_000],
        ["claude", "Claude Code", 2_000_000],
      ]),
    ];
    const { days: chartDays } = buildAllChart(days);
    expect(chartDays[0].segments.map((s) => s.agentId)).toEqual(["claude", "mystery"]);
    expect(chartDays[0].segments[1].colorVar).toBe("--agent-unknown");
  });

  it("within-bar ratios are within ±0.5% of the true share", () => {
    const { days: chartDays } = buildAllChart(weeks);
    const d = chartDays[0];
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

  it("folds non-member agents into a neutral top-stacked others segment with the sum preserved", () => {
    // Window totals: claude 60M, codex 30M, mystery 3M are the top three;
    // antigravity 2M + opencode 1M fold into a 3M others segment.
    const days = [
      day("2026-08-24", [
        ["codex", "Codex", 30_000_000],
        ["mystery", "Mystery", 3_000_000],
        ["claude", "Claude Code", 60_000_000],
        ["antigravity", "Antigravity", 2_000_000],
        ["opencode", "OpenCode", 1_000_000],
      ]),
      day("2026-08-25", [["claude", "Claude Code", 10_000_000]]),
    ];
    const { days: chartDays, grouping } = buildAllChart(days);
    expect(grouping.memberIds).toEqual(["claude", "codex", "mystery"]);
    expect(grouping.hasOthers).toBe(true);
    expect(grouping.othersCount).toBe(2);

    const segments = chartDays[0].segments;
    expect(segments.map((s) => s.agentId)).toEqual([
      "claude",
      "codex",
      "mystery",
      "others",
    ]);
    const others = segments[3];
    expect(others.isOthers).toBe(true);
    expect(others.tokens).toBe(3_000_000);
    expect(others.colorVar).toBe("--agent-others");
    expect(others.isTop).toBe(true);

    // Sum invariant: segment tokens == day total, and heights == day/max.
    const tokenSum = segments.reduce((s, x) => s + x.tokens, 0);
    expect(tokenSum).toBe(96_000_000);
    const heightSum = segments.reduce((s, x) => s + x.height, 0);
    expect(heightSum).toBeCloseTo((96_000_000 / 96_000_000) * 100, 5);

    // A day where only a member has tokens renders no others segment.
    expect(chartDays[1].segments.map((s) => s.agentId)).toEqual(["claude"]);
  });

  it("keeps grouping membership stable across every day of the window", () => {
    const days = [
      day("2026-08-19", [
        ["mystery", "Mystery", 500],
        ["claude", "Claude Code", 1_000],
      ]),
      day("2026-08-20", []),
      day("2026-08-21", [
        ["mystery", "Mystery", 2_000],
        ["codex", "Codex", 1_000],
        ["tiny", "Tiny", 700],
        ["tiny-two", "Tiny Two", 100],
      ]),
    ];
    const { days: chartDays, grouping } = buildAllChart(days);
    // Window totals: mystery 2_500, claude 1_000, codex 1_000 (canonical tie-break
    // ahead of tiny 700 and tiny-two 100) — the same three ids are individual on
    // every day, and the rest fold into the labelled others segment.
    expect(grouping.memberIds).toEqual(["mystery", "claude", "codex"]);
    expect(grouping.hasOthers).toBe(true);
    for (const cd of chartDays) {
      const individual = cd.segments.filter((s) => !s.isOthers).map((s) => s.agentId);
      expect(individual.every((id) => grouping.memberIds.includes(id))).toBe(true);
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

describe("buildWeekBuckets (review B2)", () => {
  function pricedDay(
    date: string,
    rows: [string, string, number][],
    overrides: Partial<DailyUsage> = {},
  ): DailyUsage {
    return {
      ...day(date, rows),
      estimatedCostUsd: null,
      costUnknownReason: null,
      tokenBreakdown: {
        inputTokens: 100,
        outputTokens: 50,
        cacheReadTokens: 300,
        cacheCreationTokens: 0,
        reasoningTokens: 0,
        unclassifiedTokens: 0,
      },
      ...overrides,
    };
  }

  it("buckets 30 days into four full weeks plus a short current bucket", () => {
    const days = Array.from({ length: 30 }, (_, i) =>
      pricedDay(`2026-08-${String(i + 1).padStart(2, "0")}`, [
        ["claude", "Claude Code", 1000],
      ]),
    );
    const buckets = buildWeekBuckets(days);
    expect(buckets).toHaveLength(5);
    expect(buckets.slice(0, 4).map((b) => b.totalTokens)).toEqual([
      7000, 7000, 7000, 7000,
    ]);
    // The final bucket carries the two remaining days.
    expect(buckets[4].totalTokens).toBe(2000);
    expect(buckets[0].date).toBe("2026-08-01");
    expect(buckets[0].dateEnd).toBe("2026-08-07");
    expect(buckets[4].date).toBe("2026-08-29");
    expect(buckets[4].dateEnd).toBe("2026-08-30");
  });

  it("merges agents and models by id/modelName with all counts summed", () => {
    const days = [
      pricedDay("2026-08-24", [["claude", "Claude Code", 10]], {
        agents: [
          {
            id: "claude",
            displayName: "Claude Code",
            tokens: 10,
            reasoningTokens: 1,
            unclassifiedTokens: 2,
            models: [
              {
                modelName: "m1",
                modelDisplayName: "M1",
                inputTokens: 3,
                outputTokens: 3,
                cacheReadTokens: 3,
                cacheCreationTokens: 1,
                totalTokens: 10,
              },
            ],
          },
        ],
      }),
      pricedDay("2026-08-25", [["claude", "Claude Code", 15]], {
        agents: [
          {
            id: "claude",
            displayName: "Claude Code",
            tokens: 15,
            reasoningTokens: 4,
            unclassifiedTokens: 0,
            models: [
              {
                modelName: "m1",
                modelDisplayName: "M1",
                inputTokens: 5,
                outputTokens: 5,
                cacheReadTokens: 5,
                cacheCreationTokens: 0,
                totalTokens: 15,
              },
              {
                modelName: "m2",
                modelDisplayName: "M2",
                inputTokens: 1,
                outputTokens: 1,
                cacheReadTokens: 1,
                cacheCreationTokens: 0,
                totalTokens: 3,
              },
            ],
          },
        ],
      }),
    ];
    const [bucket] = buildWeekBuckets(days);
    expect(bucket.totalTokens).toBe(25);
    expect(bucket.agents).toHaveLength(1);
    const agent = bucket.agents[0];
    expect(agent.tokens).toBe(25);
    expect(agent.reasoningTokens).toBe(5);
    expect(agent.unclassifiedTokens).toBe(2);
    expect(agent.models).toHaveLength(2);
    expect(agent.models[0].totalTokens).toBe(25);
    expect(agent.models[0].inputTokens).toBe(8);
    expect(agent.models[1].totalTokens).toBe(3);
  });

  it("recomputes the cached-input share from the merged denominator", () => {
    const days = [
      pricedDay("2026-08-24", [["claude", "Claude Code", 450]], {
        tokenBreakdown: {
          inputTokens: 100,
          outputTokens: 50,
          cacheReadTokens: 300,
          cacheCreationTokens: 50,
          reasoningTokens: 0,
          unclassifiedTokens: 0,
        },
      }),
      pricedDay("2026-08-25", [["claude", "Claude Code", 450]], {
        tokenBreakdown: {
          inputTokens: 100,
          outputTokens: 50,
          cacheReadTokens: 300,
          cacheCreationTokens: 50,
          reasoningTokens: 0,
          unclassifiedTokens: 0,
        },
      }),
    ];
    const [bucket] = buildWeekBuckets(days);
    // cacheRead ÷ (input + cacheRead + cacheCreation) = 600/900.
    expect(bucket.cacheReadShare).toBeCloseTo(2 / 3, 6);
  });

  it("never fakes a bucket cost: one unpriced day with usage poisons the sum", () => {
    const days = [
      pricedDay("2026-08-24", [["claude", "Claude Code", 100]], {
        estimatedCostUsd: 1.5,
      }),
      pricedDay("2026-08-25", [["claude", "Claude Code", 100]], {
        estimatedCostUsd: null,
        costUnknownReason: "missingModelPricing",
      }),
    ];
    const [bucket] = buildWeekBuckets(days);
    expect(bucket.estimatedCostUsd).toBeNull();
    expect(bucket.costUnknownReason).toBe("missingModelPricing");
  });

  it("sums the bucket cost when every day with usage is priced", () => {
    const days = [
      pricedDay("2026-08-24", [["claude", "Claude Code", 100]], {
        estimatedCostUsd: 1.5,
      }),
      pricedDay("2026-08-25", [], { estimatedCostUsd: null }), // no usage day
      pricedDay("2026-08-26", [["claude", "Claude Code", 100]], {
        estimatedCostUsd: 0,
      }),
    ];
    const [bucket] = buildWeekBuckets(days);
    expect(bucket.estimatedCostUsd).toBe(1.5);
    expect(bucket.costUnknownReason).toBeNull();
  });
});
