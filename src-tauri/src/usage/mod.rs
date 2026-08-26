use serde::{Deserialize, Serialize};

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
    pub models: Vec<ModelUsage>,
}

/// A day's token composition by type. `otherTokens` absorbs the residual between
/// the authoritative day total and the sum of the four component types
/// (unattributed/missing fields), so `input + output + cacheRead + cacheCreation
/// + other` always equals `DailyUsage.total_tokens`.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TokenBreakdown {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    pub other_tokens: u64,
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
}
