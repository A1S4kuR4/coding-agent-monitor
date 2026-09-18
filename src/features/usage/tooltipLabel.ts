import type { DailyUsage } from "../../types/usage";
import type { Language } from "./i18n";
import { dictFor } from "./i18n";
import { sortAgents } from "./agents";
import { formatDelta, type DeltaBasis } from "./formatDelta";
import { formatTokens } from "./formatTokens";

const localeTag = (lang: Language) => (lang === "zh-CN" ? "zh-CN" : "en");

/** Deterministic full-date label (e.g. "August 24, 2026" / "2026年8月24日")
 * so tooltips read the complete date, not the short MM/DD axis tick. Falls
 * back to the raw ISO date for malformed input — never a fabricated date. */
export function fullDate(lang: Language, isoDate: string): string {
  const [y, m, d] = isoDate.split("-").map(Number);
  if (!y || !m || !d) return isoDate;
  return new Intl.DateTimeFormat(localeTag(lang), {
    year: "numeric",
    month: "long",
    day: "numeric",
  }).format(new Date(y, m - 1, d));
}

/** Accessible (aria-label) description of a day in All mode: full date, the
 * day total, then each agent's value and share, then the day-over-day delta.
 * `dateLabel` overrides the full date for aggregated buckets (a week range
 * instead of a single day). */
export function allDayAriaLabel(
  day: DailyUsage,
  prevTotal: number | undefined,
  lang: Language,
  basis: DeltaBasis,
  dateLabel?: string,
): string {
  const d = dictFor(lang);
  const delta = formatDelta(day.totalTokens, prevTotal, lang, basis);
  const total = formatTokens(day.totalTokens);
  const parts = [`${dateLabel ?? fullDate(lang, day.date)}, ${d.ariaTotal(total)}`];
  for (const agent of sortAgents(day.agents)) {
    const share = day.totalTokens > 0 ? (agent.tokens / day.totalTokens) * 100 : 0;
    parts.push(
      d.ariaAgentShare(
        agent.displayName,
        formatTokens(agent.tokens),
        share.toFixed(1),
      ),
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
  lang: Language,
  basis: DeltaBasis,
  dateLabel?: string,
): string {
  const d = dictFor(lang);
  const agent = day.agents.find((a) => a.id === agentId);
  const value = agent?.tokens ?? 0;
  const share = day.totalTokens > 0 ? (value / day.totalTokens) * 100 : 0;
  const name = agent?.displayName ?? agentId;
  const delta = formatDelta(value, prevAgentValue, lang, basis);
  const parts = [
    `${dateLabel ?? fullDate(lang, day.date)}, ${d.ariaAgentOfDay(name, formatTokens(value), share.toFixed(1))}`,
  ];
  if (delta.label) parts.push(delta.label);
  return parts.join(" ");
}
