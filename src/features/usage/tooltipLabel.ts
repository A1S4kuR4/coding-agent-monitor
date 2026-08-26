import type { DailyUsage } from "../../types/usage";
import { sortAgents } from "./agents";
import { formatDelta } from "./formatDelta";
import { formatTokens } from "./formatTokens";

const MONTHS = [
  "January",
  "February",
  "March",
  "April",
  "May",
  "June",
  "July",
  "August",
  "September",
  "October",
  "November",
  "December",
];

/** Deterministic full-date label (e.g. "August 24, 2026") so tooltips read the
 * complete date, not the short MM/DD axis tick. */
export function fullDate(isoDate: string): string {
  const [y, m, d] = isoDate.split("-").map(Number);
  if (!y || !m || !d) return isoDate;
  return `${MONTHS[m - 1]} ${d}, ${y}`;
}

/** Accessible (aria-label) description of a day in All mode: full date, the
 * day total, then each agent's value and share, then the day-over-day delta. */
export function allDayAriaLabel(
  day: DailyUsage,
  prevTotal: number | undefined,
): string {
  const delta = formatDelta(day.totalTokens, prevTotal);
  const parts = [
    `${fullDate(day.date)}, ${formatTokens(day.totalTokens)} tokens total.`,
  ];
  for (const agent of sortAgents(day.agents)) {
    const share = day.totalTokens > 0 ? (agent.tokens / day.totalTokens) * 100 : 0;
    parts.push(
      `${agent.displayName}: ${formatTokens(agent.tokens)} tokens (${share.toFixed(1)}%).`,
    );
  }
  if (delta.label) parts.push(delta.label);
  return parts.join(" ");
}

/** Accessible description of a day in single-agent mode: full date, the agent
 * name and its value, its share of the day, and its own day-over-day delta. */
export function agentDayAriaLabel(
  day: DailyUsage,
  agentId: string,
  prevAgentValue: number | undefined,
): string {
  const agent = day.agents.find((a) => a.id === agentId);
  const value = agent?.tokens ?? 0;
  const share = day.totalTokens > 0 ? (value / day.totalTokens) * 100 : 0;
  const name = agent?.displayName ?? agentId;
  const delta = formatDelta(value, prevAgentValue);
  const parts = [
    `${fullDate(day.date)}, ${name} ${formatTokens(value)} tokens (${share.toFixed(1)}% of day).`,
  ];
  if (delta.label) parts.push(delta.label);
  return parts.join(" ");
}