/**
 * Cache-input share ratio for a set of token counts, using the same口径 as the
 * global today figure: `cacheRead / (input + cacheRead + cacheCreation)`.
 *
 * Deliberately NOT `cacheRead / totalTokens` — totalTokens may include output and
 * Other tokens that are outside the cached-input denominator.
 *
 * Returns `null` when the denominator is 0 or non-finite, so callers can hide the
 * percentage rather than render a misleading `0%` for unavailable data. The ratio
 * is left unrounded so callers may reuse `formatPercent` for the integer readout.
 */
export function cacheInputShare(
  input: number,
  cacheRead: number,
  cacheCreation: number,
): number | null {
  const denominator = input + cacheRead + cacheCreation;
  if (denominator <= 0 || !Number.isFinite(denominator)) return null;
  return cacheRead / denominator;
}