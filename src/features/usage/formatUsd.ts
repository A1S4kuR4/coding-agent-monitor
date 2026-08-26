/**
 * Formats an estimated USD cost for display. `null` (unknown price) stays
 * `null` so the caller can hide it — an unknown price must never render as
 * "$0.00". Non-finite values are treated as unknown too.
 */
export function formatUsd(costUsd: number | null): string | null {
  if (costUsd === null || !Number.isFinite(costUsd)) return null;
  return `$${costUsd.toFixed(2)}`;
}