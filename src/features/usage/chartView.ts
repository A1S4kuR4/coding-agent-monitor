import type { DailyUsage } from "../../types/usage";
import { agentMeta, sortAgents } from "./agents";

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
 * Single-agent mode — one monochrome bar per day, `value / maxSelected`.
 */

export interface ChartSegment {
  agentId: string;
  displayName: string;
  tokens: number;
  /** Percent of the bar-track height (0–100). */
  height: number;
  /** Readable CSS custom-property name, e.g. "--agent-claude". */
  colorVar: string;
  /** True only for the topmost non-zero segment in this bar. */
  isTop: boolean;
}

export interface ChartDay {
  date: string;
  totalTokens: number;
  /** Non-zero segments in bottom-up stacking order. */
  segments: ChartSegment[];
}

/** Build the stacked (All) chart. */
export function buildAllChart(days: DailyUsage[]): ChartDay[] {
  const maxDailyTotal = Math.max(0, ...days.map((d) => d.totalTokens));
  return days.map((day) => {
    const segments: ChartSegment[] = [];
    if (maxDailyTotal > 0) {
      const order = sortAgents(day.agents);
      for (const agent of order) {
        if (agent.tokens <= 0) continue;
        segments.push({
          agentId: agent.id,
          displayName: agent.displayName,
          tokens: agent.tokens,
          height: (agent.tokens / maxDailyTotal) * 100,
          colorVar: agentMeta(agent.id).colorVar,
          isTop: false,
        });
      }
      if (segments.length > 0) segments[segments.length - 1].isTop = true;
    }
    return { date: day.date, totalTokens: day.totalTokens, segments };
  });
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