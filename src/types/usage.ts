/** One distinct model that contributed to an agent's day. `totalTokens` is
 * computed by the Rust adapter (== input + output + cacheRead + cacheCreation);
 * `modelDisplayName` is a normalized, safe-to-render label; `modelName` is the
 * raw ccusage id kept as a stable key. */
export interface ModelUsage {
  modelName: string;
  modelDisplayName: string;
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheCreationTokens: number;
  totalTokens: number;
}

/** One agent on a single day. `id` mirrors the open-string ccusage agent id
 * (e.g. `claude`, `codex`, or any future agent); `displayName` is produced by
 * the Rust adapter and is always safe to render. `tokens` is the agent's
 * authoritative total (it may exceed the sum of `models`, which is an
 * informational breakdown for the expandable view). */
export interface AgentUsage {
  id: string;
  displayName: string;
  tokens: number;
  models: ModelUsage[];
}

/** A day's token composition by type. `otherTokens` absorbs the residual
 * between the authoritative day total and the summed component types, so
 * `input + output + cacheRead + cacheCreation + other === totalTokens`. */
export interface TokenBreakdown {
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheCreationTokens: number;
  otherTokens: number;
}

export interface DailyUsage {
  date: string;
  totalTokens: number;
  tokenBreakdown: TokenBreakdown;
  /** Estimated USD cost, or `null` when unknown (never faked as `$0.00`). */
  estimatedCostUsd: number | null;
  /** Cache-input share ratio in `0..=1`, or `null` when the denominator is 0. */
  cacheReadShare: number | null;
  agents: AgentUsage[];
}

export interface UsageSummary {
  /** UTC RFC 3339 timestamp of the most recent successful collection. */
  collectedAt: string;
  today: DailyUsage;
  last7Days: DailyUsage[];
}