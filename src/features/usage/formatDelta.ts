/**
 * Day-over-day token delta formatting for the header and chart tooltips.
 * Percentages keep 1 decimal. The label always carries an arrow, a sign, and the
 * number so the direction is conveyed redundantly (never by colour alone).
 *
 * Boundary rules (from the acceptance plan):
 *   - delta >= +1%            -> "▲ +66.2% vs 昨日"  (up, --delta-up)
 *   - delta <= -1%            -> "▼ -12.4% vs 昨日"  (down, --delta-down)
 *   - |delta| < 1%            -> "• +0.6% vs 昨日"   (flat, --delta-flat)
 *   - yesterday=0, today>0    -> "— 昨日无使用"
 *   - today=0, yesterday=0    -> hidden (null label)
 *   - today=0, yesterday>0    -> "▼ -100.0% vs 昨日"
 *   - missing / non-finite    -> null (never NaN/Infinity)
 *   - `yesterday === undefined` (no prior point) -> "— 无前一日数据"
 */
export type DeltaKind =
  | "up"
  | "down"
  | "flat"
  | "no-yesterday"
  | "no-usage-yesterday"
  | "none";

export interface DeltaResult {
  kind: DeltaKind;
  /** Full display label, or `null` when the delta should be hidden. */
  label: string | null;
  /** Sign-oriented percent string like "+66.2%" (without the arrow). */
  percent: string | null;
}

const VS = " vs 昨日";
const NO_YESTERDAY = "— 无前一日数据";
const NO_USAGE_YESTERDAY = "— 昨日无使用";
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
): DeltaResult {
  if (today == null || !Number.isFinite(today)) {
    return { kind: "none", label: null, percent: null };
  }
  if (yesterday === undefined) {
    return { kind: "no-yesterday", label: NO_YESTERDAY, percent: null };
  }
  if (yesterday == null || !Number.isFinite(yesterday)) {
    return { kind: "none", label: null, percent: null };
  }

  if (yesterday === 0) {
    if (today === 0) return { kind: "none", label: null, percent: null };
    return { kind: "no-usage-yesterday", label: NO_USAGE_YESTERDAY, percent: null };
  }
  if (today === 0) {
    const pct = "-100.0%";
    return { kind: "down", label: `▼ ${pct}${VS}`, percent: pct };
  }

  const delta = (today - yesterday) / yesterday;
  const pct = percentLabel(delta);
  if (delta >= THRESHOLD) return { kind: "up", label: `▲ ${pct}${VS}`, percent: pct };
  if (delta <= -THRESHOLD) return { kind: "down", label: `▼ ${pct}${VS}`, percent: pct };
  return { kind: "flat", label: `• ${pct}${VS}`, percent: pct };
}