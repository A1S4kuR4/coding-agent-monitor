import type { AgentUsage, CostUnknownReason, DailyUsage } from "../../types/usage";
import { agentMeta, compareByMeta, sortAgents } from "./agents";
import { cacheInputShare } from "./cacheInputShare";

/**
 * Bar-chart view-model. It is a pure transform from `DailyUsage[]` to the exact
 * per-segment fractional heights (0–100) a bar should render, so the stacking
 * ratios are unit-tested without a DOM.
 *
 * All mode — one stacked bar per day. The whole-bar height expresses the day's
 * absolute total relative to the window max, and each segment expresses that
 * agent's own share, so `segmentHeight = agentValue / maxDailyTotal`. A day with
 * total 0 renders no data segments (just the faint track + baseline).
 *
 * When more than `ALL_CHART_TOP_N + 1` agents carry tokens across the window,
 * only the top `ALL_CHART_TOP_N` (by window total, canonical order as the
 * deterministic tie-break) render as individual segments; every remaining agent
 * is folded into ONE clearly-labelled "others" presentation segment. Membership
 * is computed once for the whole visible range, so the same agents stay
 * individual on every day, and per-day segment token sums always equal the day
 * total. The "others" group is display-only: it is never written back into the
 * public data and never poses as a real agent or model — tooltips and the
 * selected-day detail always show the full composition.
 *
 * Single-agent mode — one monochrome bar per day, `value / maxSelected`.
 */

/** How many agents stay individually stacked in All mode before the rest are
 * grouped into the labelled "others" segment. */
export const ALL_CHART_TOP_N = 3;

export interface ChartSegment {
  agentId: string;
  /** View-model group key only. For the "others" segment this is the sentinel
   * "others"; it is never rendered as an agent or model name. */
  displayName: string;
  tokens: number;
  /** Percent of the bar-track height (0–100). */
  height: number;
  /** Readable CSS custom-property name, e.g. "--agent-claude". */
  colorVar: string;
  /** True only for the topmost non-zero segment in this bar. */
  isTop: boolean;
  /** True for the display-only aggregated "others" segment. */
  isOthers?: boolean;
}

export interface ChartDay {
  date: string;
  totalTokens: number;
  /** Non-zero segments in bottom-up stacking order. */
  segments: ChartSegment[];
}

/** Which agents render as individual segments in All mode. Members are chosen
 * deterministically from the whole visible range so bars never reshuffle their
 * membership from day to day. */
export interface AllChartGrouping {
  /** Agent ids rendered as individual segments. */
  memberIds: string[];
  /** True when remaining agents are folded into the "others" segment. */
  hasOthers: boolean;
  /** Number of agents folded into the "others" group (0 when none). */
  othersCount: number;
}

/** Compute the stable All-mode grouping from window totals: non-zero agents
 * ranked by total tokens descending, with the canonical presentation order
 * (`compareByMeta`) as the tie-break. When at most TOP_N+1 agents have tokens
 * everyone stays individual — four distinct agent colours never get grouped. */
export function allChartGrouping(days: DailyUsage[]): AllChartGrouping {
  const totals = new Map<string, number>();
  for (const day of days) {
    for (const agent of day.agents) {
      if (agent.tokens <= 0) continue;
      totals.set(agent.id, (totals.get(agent.id) ?? 0) + agent.tokens);
    }
  }
  const ids = [...totals.keys()].sort((a, b) => {
    const ta = totals.get(a) ?? 0;
    const tb = totals.get(b) ?? 0;
    if (ta !== tb) return tb - ta;
    return compareByMeta(
      { id: a, displayName: agentMeta(a).displayName },
      { id: b, displayName: agentMeta(b).displayName },
    );
  });
  if (ids.length <= ALL_CHART_TOP_N + 1) {
    return { memberIds: ids, hasOthers: false, othersCount: 0 };
  }
  return {
    memberIds: ids.slice(0, ALL_CHART_TOP_N),
    hasOthers: true,
    othersCount: ids.length - ALL_CHART_TOP_N,
  };
}

export interface AllChartResult {
  days: ChartDay[];
  grouping: AllChartGrouping;
}

/** Build the stacked (All) chart with its stable top-agents/others grouping. */
export function buildAllChart(days: DailyUsage[]): AllChartResult {
  const grouping = allChartGrouping(days);
  const memberIds = new Set(grouping.memberIds);
  const maxDailyTotal = Math.max(0, ...days.map((d) => d.totalTokens));
  const chartDays = days.map((day) => {
    const segments: ChartSegment[] = [];
    if (maxDailyTotal > 0) {
      // Individual members first, in the canonical bottom-up order.
      for (const agent of sortAgents(day.agents)) {
        if (agent.tokens <= 0 || !memberIds.has(agent.id)) continue;
        segments.push({
          agentId: agent.id,
          displayName: agent.displayName,
          tokens: agent.tokens,
          height: (agent.tokens / maxDailyTotal) * 100,
          colorVar: agentMeta(agent.id).colorVar,
          isTop: false,
        });
      }
      // Then the display-only "others" aggregate, always stacked on top.
      if (grouping.hasOthers) {
        const othersTokens = day.agents.reduce(
          (sum, a) => (a.tokens > 0 && !memberIds.has(a.id) ? sum + a.tokens : sum),
          0,
        );
        if (othersTokens > 0) {
          segments.push({
            agentId: "others",
            displayName: "",
            tokens: othersTokens,
            height: (othersTokens / maxDailyTotal) * 100,
            colorVar: "--agent-others",
            isTop: false,
            isOthers: true,
          });
        }
      }
      if (segments.length > 0) segments[segments.length - 1].isTop = true;
    }
    return { date: day.date, totalTokens: day.totalTokens, segments };
  });
  return { days: chartDays, grouping };
}

/** Build the single-agent chart for `agentId`. Agent values are per-day tokens
 * (0 when absent), scaled by the max over the window. */
export function buildAgentChart(days: DailyUsage[], agentId: string): ChartDay[] {
  const values = days.map(
    (d) => d.agents.find((a) => a.id === agentId)?.tokens ?? 0,
  );
  const maxSelected = Math.max(0, ...values);
  const meta = agentMeta(agentId);
  return days.map((day, index) => {
    const value = values[index];
    const segments: ChartSegment[] = [];
    if (value > 0 && maxSelected > 0) {
      segments.push({
        agentId,
        displayName: day.agents.find((a) => a.id === agentId)?.displayName ?? agentId,
        tokens: value,
        height: (value / maxSelected) * 100,
        colorVar: meta.colorVar,
        isTop: true,
      });
    }
    return { date: day.date, totalTokens: day.totalTokens, segments };
  });
}

/** Convenience: per-day label value honoured by the axis above each bar. */
export function dayValue(day: DailyUsage, agentId: string | null): number {
  if (agentId === null) return day.totalTokens;
  return day.agents.find((a) => a.id === agentId)?.tokens ?? 0;
}

/* ------------------------------------------------------------------ *
 * Week aggregation (review B2): a 30-day window defaults to five
 * 7-day buckets (the last one usually partial), so the chart is
 * structurally identical to the 7-day view — no horizontal scroll.
 * Each bucket is a merged DailyUsage, so every downstream consumer
 * (stacking, tooltip, pinned panel, filter) works unchanged; the
 * extra `dateEnd` marks the bucket as a range.
 * ------------------------------------------------------------------ */

/** A week bucket: a real merged sum of DailyUsage rows (never fabricated),
 * spanning `date`..`dateEnd`. */
export interface WeekUsage extends DailyUsage {
  /** Inclusive end of the bucket (ISO date). */
  dateEnd: string;
}

/** Days per bucket. 30 / 7 → four full weeks plus a short current bucket. */
export const WEEK_BUCKET_DAYS = 7;

type ModelUsageList = AgentUsage["models"];

function mergeModels(target: ModelUsageList, models: ModelUsageList): void {
  for (const model of models) {
    const existing = target.find((m) => m.modelName === model.modelName);
    if (!existing) {
      target.push({ ...model });
      continue;
    }
    existing.inputTokens += model.inputTokens;
    existing.outputTokens += model.outputTokens;
    existing.cacheReadTokens += model.cacheReadTokens;
    existing.cacheCreationTokens += model.cacheCreationTokens;
    existing.totalTokens += model.totalTokens;
  }
}

/** A day with usage but no price poisons the whole bucket's estimate: the sum
 * would be a partial cost masquerading as a total, so the bucket reports the
 * same honest three-state semantics as a single day. */
function mergeCost(
  days: DailyUsage[],
): { usd: number | null; reason: CostUnknownReason | null } {
  let sum = 0;
  for (const day of days) {
    if (day.estimatedCostUsd !== null) {
      sum += day.estimatedCostUsd;
      continue;
    }
    if (day.totalTokens > 0) return { usd: null, reason: "missingModelPricing" };
  }
  return { usd: sum, reason: null };
}

function mergeBucket(days: DailyUsage[]): WeekUsage {
  const agents = new Map<string, AgentUsage>();
  const breakdown = {
    inputTokens: 0,
    outputTokens: 0,
    cacheReadTokens: 0,
    cacheCreationTokens: 0,
    reasoningTokens: 0,
    unclassifiedTokens: 0,
  };
  let totalTokens = 0;
  for (const day of days) {
    totalTokens += day.totalTokens;
    breakdown.inputTokens += day.tokenBreakdown.inputTokens;
    breakdown.outputTokens += day.tokenBreakdown.outputTokens;
    breakdown.cacheReadTokens += day.tokenBreakdown.cacheReadTokens;
    breakdown.cacheCreationTokens += day.tokenBreakdown.cacheCreationTokens;
    breakdown.reasoningTokens += day.tokenBreakdown.reasoningTokens;
    breakdown.unclassifiedTokens += day.tokenBreakdown.unclassifiedTokens;
    for (const agent of day.agents) {
      const existing = agents.get(agent.id);
      if (!existing) {
        agents.set(agent.id, {
          id: agent.id,
          displayName: agent.displayName,
          tokens: agent.tokens,
          reasoningTokens: agent.reasoningTokens,
          unclassifiedTokens: agent.unclassifiedTokens,
          models: agent.models.map((m) => ({ ...m })),
        });
        continue;
      }
      existing.tokens += agent.tokens;
      existing.reasoningTokens += agent.reasoningTokens;
      existing.unclassifiedTokens += agent.unclassifiedTokens;
      mergeModels(existing.models, agent.models);
    }
  }
  const cost = mergeCost(days);
  return {
    date: days[0].date,
    dateEnd: days[days.length - 1].date,
    totalTokens,
    tokenBreakdown: breakdown,
    estimatedCostUsd: cost.usd,
    costUnknownReason: cost.reason,
    // Same denominator口径 as everywhere else (cacheRead ÷ input+cacheRead+
    // cacheCreation), recomputed from the merged counts.
    cacheReadShare: cacheInputShare(
      breakdown.inputTokens,
      breakdown.cacheReadTokens,
      breakdown.cacheCreationTokens,
    ),
    agents: [...agents.values()],
  };
}

/** Aggregate a run of DailyUsage rows into 7-day buckets, first day first.
 * The final bucket carries whatever days remain (1–7). Deterministic for a
 * given window: the bucket's ISO start date identifies it. */
export function buildWeekBuckets(days: DailyUsage[]): WeekUsage[] {
  const buckets: WeekUsage[] = [];
  for (let start = 0; start < days.length; start += WEEK_BUCKET_DAYS) {
    const slice = days.slice(start, start + WEEK_BUCKET_DAYS);
    if (slice.length === 0) break;
    buckets.push(mergeBucket(slice));
  }
  return buckets;
}
