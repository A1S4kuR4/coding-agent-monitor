export function formatTokens(tokens: number): string {
  if (tokens >= 1_000_000_000) return compact(tokens, 1_000_000_000, "B");
  if (tokens >= 1_000_000) return compact(tokens, 1_000_000, "M");
  if (tokens >= 1_000) return compact(tokens, 1_000, "K");
  return Math.max(0, tokens).toLocaleString("en-US");
}

function compact(tokens: number, divisor: number, suffix: string): string {
  return `${(tokens / divisor).toFixed(2).replace(/\.00$/, "").replace(/(\.\d)0$/, "$1")}${suffix}`;
}
