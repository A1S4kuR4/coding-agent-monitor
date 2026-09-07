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
  /** Source-confirmed additive reasoning/thinking tokens reported at agent scope. */
  reasoningTokens: number;
  /** Agent tokens whose type remains unknown after known components are counted. */
  unclassifiedTokens: number;
  models: ModelUsage[];
}

/** A day's mutually additive token composition. Known provider reasoning is
 * separated from genuinely unknown residue. */
export interface TokenBreakdown {
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheCreationTokens: number;
  reasoningTokens: number;
  unclassifiedTokens: number;
}

export interface DailyUsage {
  date: string;
  totalTokens: number;
  tokenBreakdown: TokenBreakdown;
  /** Estimated USD cost, or `null` when unknown (never faked as `$0.00`). */
  estimatedCostUsd: number | null;
  /** Why the cost is unknown despite usage being present; `null` when the
   * cost is known or the day has no usage to price. */
  costUnknownReason: CostUnknownReason | null;
  /** Cache-input share ratio in `0..=1`, or `null` when the denominator is 0. */
  cacheReadShare: number | null;
  agents: AgentUsage[];
}

/** Why a day's estimated cost is unknown even though the day has usage. A
 * no-usage day needs no reason — `totalTokens === 0` already says so. */
export type CostUnknownReason = "missingModelPricing";

/** Stable, sanitized category of a recoverable source problem inside a
 * successful agent result. No file paths, raw log lines, or vendor types. */
export type SourceDiagnosticKind =
  | "corruptFile"
  | "corruptRecord"
  | "databaseError"
  | "sourceUnreadable"
  | "sourceChanged"
  | "invariantViolation";

/** One aggregated diagnostic: how many recoverable problems of one kind were
 * skipped for one agent while its data still read successfully. */
export interface SourceDiagnostic {
  kind: SourceDiagnosticKind;
  agentId: string;
  agentDisplayName: string;
  count: number;
}

/** Whether a successful snapshot covered every source record or some were
 * skipped. `possiblyIncomplete` keeps the accepted totals but requires the UI
 * to label coverage as a risk; it never means the refresh failed. */
export interface CoverageInfo {
  status: "complete" | "possiblyIncomplete";
  diagnostics: SourceDiagnostic[];
}

export interface UsageSummary {
  /** UTC RFC 3339 timestamp of the most recent successful collection. */
  collectedAt: string;
  today: DailyUsage;
  last7Days: DailyUsage[];
  /** Whether the collection covered every source record or skipped some. */
  coverage: CoverageInfo;
}

/** The local date range and IANA/system time-zone captured before a worker
 * starts. It remains attached to the successful snapshot across failures. */
export interface UsageScope {
  startDate: string;
  endDate: string;
  timeZone: string;
}

export type RefreshTrigger =
  | "startup"
  | "manual"
  | "focus"
  | "tray"
  | "periodic"
  | "recovery";

export type RefreshOutcome = "inProgress" | "succeeded" | "failed";
export type RefreshFailureKind = "timedOut" | "cancelled" | "failed";

export interface RefreshAttempt {
  /** Process-local monotonic identity; Rust uses a JS-safe u32. */
  id: number;
  trigger: RefreshTrigger;
  scope: UsageScope;
  startedAt: string;
  finishedAt: string | null;
  outcome: RefreshOutcome;
  /** Stable, sanitized category. Raw collector errors never cross this boundary. */
  failure: RefreshFailureKind | null;
}

export interface UsageSnapshot {
  scope: UsageScope;
  summary: UsageSummary;
}

export type FreshnessStatus = "fresh" | "stale" | "unknown";
export type FreshnessReason =
  | "current"
  | "noSnapshot"
  | "expired"
  | "dateChanged"
  | "timeZoneChanged"
  | "clockSkew"
  | "invalidTimestamp";

export interface FreshnessInfo {
  status: FreshnessStatus;
  reason: FreshnessReason;
  checkedAt: string;
  currentDate: string;
  currentTimeZone: string;
  staleAfterSeconds: number;
}

/** Unified process-local state returned by reads/refreshes and emitted after
 * every transition. `revision` prevents late IPC responses from winning. */
export interface UsageCollectionState {
  revision: number;
  refreshing: boolean;
  snapshot: UsageSnapshot | null;
  lastAttempt: RefreshAttempt | null;
  freshness: FreshnessInfo;
}
