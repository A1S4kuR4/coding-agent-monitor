import type { Language } from "./i18n";
import { dictFor } from "./i18n";

/**
 * Day-over-day token delta formatting for the header and chart tooltips.
 * Percentages keep 1 decimal. Direction is expressed NEUTRALLY: an arrow plus
 * a signed number, in ordinary ink colour — a rise or fall is never judged
 * good/bad by colour or wording (no budget rule has been agreed that would
 * justify that).
 *
 * Time basis: today's running total compared with yesterday's FULL day is
 * labelled explicitly (`yesterday-full-day`); any other day pair uses
 * `previous-day`. There is no hourly data, so no same-period comparison is
 * ever claimed.
 *
 * Boundary rules:
 *   - delta >= +1%            -> "▲ +66.2% vs yesterday (full day)"  (neutral ink)
 *   - delta <= -1%            -> "▼ -12.4% vs yesterday (full day)"
 *   - |delta| < 1%            -> "• +0.6% vs previous day"
 *   - yesterday=0, today>0    -> "— yesterday had no usage"
 *   - today=0, yesterday=0    -> hidden (null label)
 *   - today=0, yesterday>0    -> "▼ -100.0% vs previous day"
 *   - missing / non-finite    -> null (never NaN/Infinity)
 *   - `yesterday === undefined` (no prior point) -> "— no prior-day data"
 */
export type DeltaKind =
  | "up"
  | "down"
  | "flat"
  | "no-yesterday"
  | "no-usage-yesterday"
  | "none";

/** Which prior day the comparison is against, stated in the label. */
export type DeltaBasis = "previous-day" | "yesterday-full-day";

export interface DeltaResult {
  kind: DeltaKind;
  /** Full display label, or `null` when the delta should be hidden. */
  label: string | null;
  /** Sign-oriented percent string like "+66.2%" (without the arrow). */
  percent: string | null;
}

/** Switch threshold: |delta| >= 1% is a real up/down move. */
const THRESHOLD = 0.01;

function percentLabel(delta: number): string {
  const pct = delta * 100;
  const sign = pct >= 0 ? "+" : "-";
  return `${sign}${Math.abs(pct).toFixed(1)}%`;
}

export function formatDelta(
  today: number | null | undefined,
  yesterday: number | null | undefined,
  lang: Language,
  basis: DeltaBasis = "previous-day",
): DeltaResult {
  const d = dictFor(lang);
  const suffix = basis === "yesterday-full-day" ? d.deltaVsYesterdayFullDay : d.deltaVsPreviousDay;

  if (today == null || !Number.isFinite(today)) {
    return { kind: "none", label: null, percent: null };
  }
  if (yesterday === undefined) {
    return { kind: "no-yesterday", label: d.deltaNoYesterday, percent: null };
  }
  if (yesterday == null || !Number.isFinite(yesterday)) {
    return { kind: "none", label: null, percent: null };
  }

  if (yesterday === 0) {
    if (today === 0) return { kind: "none", label: null, percent: null };
    return { kind: "no-usage-yesterday", label: d.deltaNoUsageYesterday, percent: null };
  }
  if (today === 0) {
    const pct = "-100.0%";
    return { kind: "down", label: `▼ ${pct} ${suffix}`, percent: pct };
  }

  const delta = (today - yesterday) / yesterday;
  const pct = percentLabel(delta);
  if (delta >= THRESHOLD) return { kind: "up", label: `▲ ${pct} ${suffix}`, percent: pct };
  if (delta <= -THRESHOLD) return { kind: "down", label: `▼ ${pct} ${suffix}`, percent: pct };
  return { kind: "flat", label: `• ${pct} ${suffix}`, percent: pct };
}
