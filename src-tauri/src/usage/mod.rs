use serde::{Deserialize, Serialize};

pub(crate) mod snapshot_store;
pub(crate) mod state;

pub use snapshot_store::{FileSnapshotStore, NoopSnapshotStore, SnapshotStore};
pub use state::{
    CollectionCoordinator, FreshnessInfo, FreshnessReason, FreshnessStatus, RefreshAttempt,
    RefreshFailureKind, RefreshOutcome, RefreshTrigger, UsageCollectionState, UsageScope,
    UsageSnapshot, STALE_AFTER,
};

/// One distinct model that contributed to an agent's day. ccusage does not emit
/// a summed `totalTokens` per model, so the adapter computes it from the token
/// components. `modelDisplayName` is a normalized, safe label (the raw
/// `modelName` is kept as the stable identity/key). The UI renders this list
/// when an agent is expanded for its "model detail" view.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModelUsage {
    pub model_name: String,
    pub model_display_name: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    /// Sum of the four component totals (= input + output + cacheRead +
    /// cacheCreation). Distinct from an agent's unattributed residual.
    pub total_tokens: u64,
}

/// One agent on a single day. `id` is the open-string agent id from ccusage
/// (e.g. `claude`, `codex`, or any future agent), so new agents appear without
/// a contract or CLI change. `displayName` is produced by the Rust adapter and
/// is always safe to render. `tokens` is the agent's authoritative total
/// straight from ccusage; `models` is an informational breakdown for the
/// expandable view and may not reproduce `tokens` exactly (residual
/// unattributed tokens), so the agent figure wins for every total.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentUsage {
    pub id: String,
    pub display_name: String,
    pub tokens: u64,
    /// Reasoning/thinking tokens that the source reports outside the standard
    /// input/output/cache components. This is an agent-level aggregate; it is
    /// not assigned to a model unless the source provides that attribution.
    pub reasoning_tokens: u64,
    /// Remaining agent total that cannot be classified by token type after all
    /// known components, including reasoning, have been accounted for.
    pub unclassified_tokens: u64,
    pub models: Vec<ModelUsage>,
}

/// A day's token composition by type. Known source-specific thinking/reasoning
/// is separated from genuinely unknown residue. The six counts together equal
/// `DailyUsage.total_tokens` for a well-formed source report.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TokenBreakdown {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    pub reasoning_tokens: u64,
    pub unclassified_tokens: u64,
}

/// Why a day's `estimated_cost_usd` is `None` even though the day has usage.
/// A no-usage day needs no reason (it is already visible through
/// `total_tokens == 0`); this only explains "usage exists but no complete
/// estimate could be computed".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CostUnknownReason {
    /// At least one contributing model has no price in the vendored offline
    /// price table, so no complete estimate exists and a partial sum (or a
    /// faked `$0.00`) must never be shown instead.
    MissingModelPricing,
}

/// Stable, sanitized category of a recoverable source problem inside a
/// successful agent result. This is the public-safe projection of the
/// collector's internal diagnostics: it carries counts and agent display
/// names only — never file paths, raw log lines, SQL, or vendor types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SourceDiagnosticKind {
    CorruptFile,
    CorruptRecord,
    DatabaseError,
    SourceUnreadable,
    SourceChanged,
    InvariantViolation,
}

/// One aggregated diagnostic: how many recoverable problems of one kind were
/// skipped for one agent while its data was still read successfully. The
/// affected data stays in the totals; `CoverageInfo` marks the coverage risk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceDiagnostic {
    pub kind: SourceDiagnosticKind,
    pub agent_id: String,
    pub agent_display_name: String,
    pub count: u32,
}

/// Whether a successful snapshot's numbers cover all the source records or
/// some were skipped. `possiblyIncomplete` means accepted data is shown but
/// coverage may be under-counted; it never means the refresh failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CoverageStatus {
    Complete,
    PossiblyIncomplete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverageInfo {
    pub status: CoverageStatus,
    pub diagnostics: Vec<SourceDiagnostic>,
}

impl CoverageInfo {
    /// A snapshot with no observed skip diagnostics covers its sources fully.
    pub fn complete() -> Self {
        Self {
            status: CoverageStatus::Complete,
            diagnostics: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DailyUsage {
    pub date: String,
    pub total_tokens: u64,
    pub token_breakdown: TokenBreakdown,
    /// Estimated USD cost for the day, or `None` when the day has no active
    /// agents or any contributing agent's price is unknown (a partial sum would
    /// mislead). Missing is never faked as `0`.
    pub estimated_cost_usd: Option<f64>,
    /// Why the cost is unknown despite usage being present; `None` when the
    /// cost is known or the day has no usage to price.
    pub cost_unknown_reason: Option<CostUnknownReason>,
    /// Cache-input share `cacheRead / (input + cacheRead + cacheCreation)` as a
    /// ratio in `0..=1`, or `None` when the denominator is zero. Never named a
    /// "saving ratio" because it has no counterfactual.
    pub cache_read_share: Option<f64>,
    pub agents: Vec<AgentUsage>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageSummary {
    /// UTC RFC 3339 timestamp of the most recent successful collection.
    pub collected_at: String,
    pub today: DailyUsage,
    pub last7_days: Vec<DailyUsage>,
    /// Whether the successful collection covered every source record or some
    /// were skipped. Success with skips is not a failure: totals stay and the
    /// diagnostics remain available for troubleshooting without interrupting
    /// the normal dashboard.
    pub coverage: CoverageInfo,
}

/// On-demand history, separate from the tray and persisted seven-day summary.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HistoryUsage {
    pub scope: UsageScope,
    pub collected_at: String,
    pub days: Vec<DailyUsage>,
    pub estimated_cost_usd: Option<f64>,
    pub coverage: CoverageInfo,
}
