/**
 * Formats a cache-input share ratio (in `0..=1`) as a percentage string, or
 * `null` for an unavailable (zero-denominator) share so it is never misread.
 */
export function formatPercent(ratio: number | null): string | null {
  if (ratio === null || !Number.isFinite(ratio)) return null;
  return `${Math.round(ratio * 100)}%`;
}