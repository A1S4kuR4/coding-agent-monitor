use std::collections::BTreeMap;

use chrono::{Duration, NaiveDate};
use serde::Deserialize;

use crate::{
    error::AppError,
    usage::{AgentUsage, DailyUsage, ModelUsage, TokenBreakdown, UsageSummary},
};

/// `Number.MAX_SAFE_INTEGER` (2^53 - 1). ccusage reports tokens as `u64`, but
/// Tauri serializes them into a JavaScript `number`, which cannot represent
/// integers above this bound losslessly. Any day that would exceed it is
/// rejected with a stable error rather than silently rounding — see
/// `emit_day_total`.
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

impl TokenBreakdown {
    /// All-zero breakdown for a day with no recorded agents (and a zero total).
    fn zero() -> Self {
        Self {
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            reasoning_tokens: 0,
            unclassified_tokens: 0,
        }
    }
}

/// The unified report emitted by `ccusage daily --json --offline --by-agent`.
/// Only `daily` is consumed; the top-level `totals` is ignored in favor of
/// re-aggregating the per-day agent rows. Unknown ccusage fields are ignored.
#[derive(Debug, Deserialize)]
struct UnifiedReport {
    #[serde(default)]
    daily: Vec<UnifiedDailyRow>,
}

/// Focused report emitted by the pinned Antigravity compatibility sidecar.
/// The upstream PR predates unified Antigravity support in a released ccusage,
/// so it still uses the focused `daily[].date` shape.
#[derive(Debug, Deserialize)]
struct AntigravityReport {
    #[serde(default)]
    daily: Vec<AntigravityDailyRow>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AntigravityDailyRow {
    date: String,
    total_tokens: u64,
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    cache_read_tokens: u64,
    #[serde(default)]
    cache_creation_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
    #[serde(default)]
    total_cost: Option<f64>,
    #[serde(default)]
    model_breakdowns: Vec<UnifiedModel>,
}

/// A single day in the unified report. `period` is the `YYYY-MM-DD` date (the
/// v0.1 focused reports used `date`; the unified report uses `period`). The
/// day-level `totalTokens` is optional because some consumer reports omit it;
/// when present it must equal the sum of the day's agent rows (enforced below)
/// so a report can never silently show zero while ccusage counted usage.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UnifiedDailyRow {
    period: String,
    #[serde(default)]
    total_tokens: Option<u64>,
    #[serde(default)]
    agents: Vec<UnifiedAgent>,
}

/// A single model within a unified agent row. ccusage does not emit a summed
/// `totalTokens` for each model, so it is derived from the token components
/// (`input + output + cacheRead + cacheCreation`) at parse time.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UnifiedModel {
    model_name: String,
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    cache_read_tokens: u64,
    #[serde(default)]
    cache_creation_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UnifiedAgent {
    agent: String,
    total_tokens: u64,
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    cache_read_tokens: u64,
    #[serde(default)]
    cache_creation_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
    /// Present when ccusage knows a price for this agent's usage; absent/null
    /// means the price is unknown and must never be faked as `0`.
    #[serde(default)]
    total_cost: Option<f64>,
    #[serde(default)]
    model_breakdowns: Vec<UnifiedModel>,
    /// Additive reasoning/thinking tokens known to sit outside the four common
    /// components. This is populated explicitly by source adapters rather than
    /// deserialized generically because some providers report reasoning as a
    /// subset of `outputTokens` instead.
    #[serde(skip)]
    supplemental_reasoning_tokens: u64,
}

/// Running per-model aggregates (one day) keyed by raw model name, accumulated
/// with saturating arithmetic so a model split across multiple rows is merged,
/// never dropped or double counted.
#[derive(Clone, Default)]
struct ModelTotals {
    input: u64,
    output: u64,
    cache_read: u64,
    cache_creation: u64,
    total: u64,
}

/// Running per-agent aggregates for one day, accumulated across duplicate
/// (period, agent) rows with saturating arithmetic. Models aggregate separately
/// keyed by model name, so a model split across multiple rows is merged, never
/// dropped or double counted.
#[derive(Clone)]
struct AgentTotals {
    tokens: u64,
    input: u64,
    cache_read: u64,
    cache_creation: u64,
    output: u64,
    /// Source-confirmed additive reasoning/thinking outside the common fields.
    reasoning: u64,
    cost_sum: f64,
    /// Flips to false the moment any contributing row has an unknown or
    /// non-finite cost, so a partial daily cost can never masquerade as a
    /// complete total. Starts true because an empty aggregate has no unknown
    /// price yet.
    cost_known: bool,
    /// raw model name -> per-model aggregates across every row for this agent.
    models: BTreeMap<String, ModelTotals>,
}

impl Default for AgentTotals {
    fn default() -> Self {
        Self {
            tokens: 0,
            input: 0,
            cache_read: 0,
            cache_creation: 0,
            output: 0,
            reasoning: 0,
            cost_sum: 0.0,
            cost_known: true,
            models: BTreeMap::new(),
        }
    }
}

impl AgentTotals {
    fn add(&mut self, agent: UnifiedAgent) {
        self.tokens = self.tokens.saturating_add(agent.total_tokens);
        self.input = self.input.saturating_add(agent.input_tokens);
        self.cache_read = self.cache_read.saturating_add(agent.cache_read_tokens);
        self.cache_creation = self
            .cache_creation
            .saturating_add(agent.cache_creation_tokens);
        self.output = self.output.saturating_add(agent.output_tokens);
        self.reasoning = self
            .reasoning
            .saturating_add(agent.supplemental_reasoning_tokens);
        match agent.total_cost {
            Some(cost) if cost.is_finite() => self.cost_sum += cost,
            _ => self.cost_known = false,
        }
        for model in agent.model_breakdowns {
            // Copy the (Copy) component fields before moving `model_name` into
            // the key, and derive the model total from them (ccusage gives no
            // summed total per model).
            let input = model.input_tokens;
            let output = model.output_tokens;
            let cache_read = model.cache_read_tokens;
            let cache_creation = model.cache_creation_tokens;
            let total = input
                .saturating_add(output)
                .saturating_add(cache_read)
                .saturating_add(cache_creation);
            let entry = self.models.entry(model.model_name).or_default();
            entry.input = entry.input.saturating_add(input);
            entry.output = entry.output.saturating_add(output);
            entry.cache_read = entry.cache_read.saturating_add(cache_read);
            entry.cache_creation = entry.cache_creation.saturating_add(cache_creation);
            entry.total = entry.total.saturating_add(total);
        }
    }
}

/// Display names for agents ccusage reports today. Any id not listed falls back
/// to a safe title-cased form of the id, so an unfamiliar agent is shown rather
/// than failing or hiding the report.
fn display_name(id: &str) -> String {
    match id {
        "claude" => "Claude Code".to_string(),
        "codex" => "Codex".to_string(),
        other => other
            .split(['-', '_'])
            .filter(|word| !word.is_empty())
            .map(|word| {
                let mut chars = word.chars();
                let mut out = chars
                    .next()
                    .map(|first| first.to_uppercase().collect::<String>())
                    .unwrap_or_default();
                out.push_str(chars.as_str());
                out
            })
            .collect::<Vec<_>>()
            .join(" "),
    }
}

/// Normalizes a model id into a stable, understandable display label. ccusage
/// model ids frequently end in a `-ga-<digits>` release marker (e.g.
/// `deepseek-v4-flash-ga-260731`); stripping that leaves the family name. A
/// missing or blank id falls back to a stable placeholder so the UI never shows
/// an empty label; any other id is kept as-is (trimmed). Unknown ids are thus
/// shown readably rather than hidden or mangled.
fn model_display_name(name: &str) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return "Unknown model".to_string();
    }
    if let Some(index) = trimmed.find("-ga-") {
        let family = &trimmed[..index];
        let family = family.trim();
        if !family.is_empty() {
            return family.to_string();
        }
    }
    trimmed.to_string()
}

/// Test-only convenience for exercising a released unified report without a
/// compatibility contribution.
#[cfg(test)]
pub fn normalize_unified(
    json: &str,
    today: &str,
    collected_at: &str,
) -> Result<UsageSummary, AppError> {
    normalize_reports(json, r#"{"daily":[]}"#, today, collected_at)
}

/// Combines the released ccusage unified snapshot with the focused report from
/// the locally pinned Antigravity PR. If a future released unified sidecar
/// already reports Antigravity for a date, that authoritative row wins and the
/// compatibility row is skipped, preventing double counting during upgrades.
pub fn normalize_reports(
    unified_json: &str,
    antigravity_json: &str,
    today: &str,
    collected_at: &str,
) -> Result<UsageSummary, AppError> {
    let today = NaiveDate::parse_from_str(today, "%Y-%m-%d")
        .map_err(|error| AppError::invalid_date(error.to_string()))?;
    let report = parse_unified(unified_json)?;
    let antigravity = parse_antigravity(antigravity_json)?;

    // date -> (agent id -> aggregates), merged across duplicate rows. Each row's
    // day-level `totalTokens` (when present) is validated against that same row's
    // agent sum before any cross-row aggregation (`validate_row_total`), so a
    // duplicate-date row can never be compared against the merged set or smuggle a
    // contradicting declaration past the check.
    let mut by_date = BTreeMap::<String, BTreeMap<String, AgentTotals>>::new();
    for row in report.daily {
        validate_row_total(&row)?;
        let day = by_date.entry(row.period).or_default();
        for agent in row.agents {
            day.entry(agent.agent.clone()).or_default().add(agent);
        }
    }
    for row in antigravity.daily {
        let day = by_date.entry(row.date).or_default();
        if day.contains_key("antigravity") {
            continue;
        }
        let total_cost = match row.total_cost {
            Some(cost) if cost.is_finite() && (cost > 0.0 || row.total_tokens == 0) => Some(cost),
            _ => None,
        };
        // The pinned Antigravity adapter stores Gemini thinking tokens in its
        // internal `extra_total_tokens`. Its focused JSON includes them in
        // `totalTokens` but omits a named field, so the exact additive residue
        // after the four exported components is the authoritative reasoning
        // count for this source. Do not apply this inference generically: Codex
        // and Claude report reasoning/thinking as a subset of output instead.
        let exported_components = row
            .input_tokens
            .saturating_add(row.output_tokens)
            .saturating_add(row.cache_read_tokens)
            .saturating_add(row.cache_creation_tokens);
        let supplemental_reasoning_tokens = row.total_tokens.saturating_sub(exported_components);
        day.entry("antigravity".to_string())
            .or_default()
            .add(UnifiedAgent {
                agent: "antigravity".to_string(),
                total_tokens: row.total_tokens,
                input_tokens: row.input_tokens,
                cache_read_tokens: row.cache_read_tokens,
                cache_creation_tokens: row.cache_creation_tokens,
                output_tokens: row.output_tokens,
                total_cost,
                // The pinned focused report uses the same model-breakdown shape
                // as unified agent rows. Preserve it so Antigravity can use the
                // same expandable model detail as every other agent.
                model_breakdowns: row.model_breakdowns,
                supplemental_reasoning_tokens,
            });
    }

    let last7_days = summarize_days(&by_date, today)?;
    let today_summary = last7_days
        .last()
        .cloned()
        .expect("a seven-day range always contains today");

    Ok(UsageSummary {
        collected_at: collected_at.to_string(),
        today: today_summary,
        last7_days,
    })
}

/// Phase 4A: converts a worker batch snapshot into the same `UsageSummary`
/// the v0.2 sidecar path produces. Successful agents contribute their daily
/// records; agents with structured errors are **rejected** — the whole
/// refresh fails with the first agent error, matching the v0.2 behavior where
/// a sidecar failure fails the refresh and the previous cache remains.
/// (Per-record diagnostics inside successful agents — skipped corrupt files,
/// SQLite skips — keep the v0.2 semantics: data survives, the diagnostic
/// surfaces through the collector contract.)
///
/// Reasoning/unclassified use the Phase 1 six-bucket rule: source-reported
/// additive reasoning rides the reasoning bucket; the residual
/// (`total - input - output - cacheRead - cacheCreation - reasoning`) is
/// unclassified. The six counts equal the total by construction.
///
/// JS-safe bound: per-agent and per-day totals above 2^53-1 are rejected
/// exactly like the sidecar path (`emit_day_total`/`validate_row_total`
/// equivalents), so no lossy number ever reaches the frontend.
pub fn normalize_snapshot(
    snapshot: &crate::collector::snapshot_protocol::CollectorSnapshotResponseV1,
    today: &str,
    collected_at: &str,
) -> Result<UsageSummary, AppError> {
    let today = NaiveDate::parse_from_str(today, "%Y-%m-%d")
        .map_err(|error| AppError::invalid_date(error.to_string()))?;

    let mut by_date = BTreeMap::<String, BTreeMap<String, AgentTotals>>::new();
    for agent_snapshot in &snapshot.agents {
        let report = match &agent_snapshot.outcome {
            crate::collector::snapshot_protocol::AgentSnapshotOutcomeV1::Ok { report } => report,
            crate::collector::snapshot_protocol::AgentSnapshotOutcomeV1::Error { error } => {
                // Partial-failure policy: a structured agent error is fatal for
                // the whole refresh (v0.2-consistent). The caller keeps the
                // previous cache; no partial UsageSummary is emitted.
                return Err(AppError::invalid_ccusage(
                    agent_snapshot.agent.as_str(),
                    error.message.clone(),
                ));
            }
        };
        for wire_record in &report.records {
            // Wire → domain conversion re-applies the six-bucket invariant and
            // JS-safe parsing before any aggregation.
            let record =
                crate::collector::protocol::record_from_v1(wire_record).map_err(|error| {
                    AppError::invalid_ccusage(&agent_snapshot.agent, error.to_string())
                })?;
            let date_key = record.date.format("%Y-%m-%d").to_string();
            let total_cost = record.cost.map(|cost| {
                // Nano-USD → f64 exactly as the v0.2 sidecar path reports
                // costs (f64 USD). One rounding at this boundary; all parity
                // comparisons use nano-USD before this point.
                cost.as_nano_usd() as f64 / 1_000_000_000.0
            });
            let day = by_date.entry(date_key).or_default();
            let agent_totals = day.entry(record.agent.id().to_string()).or_default();
            agent_totals.tokens = agent_totals.tokens.saturating_add(record.total_tokens);
            agent_totals.input = agent_totals.input.saturating_add(record.input_tokens);
            agent_totals.output = agent_totals.output.saturating_add(record.output_tokens);
            agent_totals.cache_read = agent_totals
                .cache_read
                .saturating_add(record.cache_read_tokens);
            agent_totals.cache_creation = agent_totals
                .cache_creation
                .saturating_add(record.cache_creation_tokens);
            agent_totals.reasoning = agent_totals
                .reasoning
                .saturating_add(record.reasoning_tokens);
            match total_cost {
                Some(cost) if cost.is_finite() => agent_totals.cost_sum += cost,
                _ => agent_totals.cost_known = false,
            }
            for breakdown in &record.model_breakdowns {
                let entry = agent_totals
                    .models
                    .entry(breakdown.model.0.clone())
                    .or_default();
                entry.input = entry.input.saturating_add(breakdown.input_tokens);
                entry.output = entry.output.saturating_add(breakdown.output_tokens);
                entry.cache_read = entry.cache_read.saturating_add(breakdown.cache_read_tokens);
                entry.cache_creation = entry
                    .cache_creation
                    .saturating_add(breakdown.cache_creation_tokens);
                entry.total = entry
                    .total
                    .saturating_add(breakdown.input_tokens)
                    .saturating_add(breakdown.output_tokens)
                    .saturating_add(breakdown.cache_read_tokens)
                    .saturating_add(breakdown.cache_creation_tokens);
            }
        }
    }

    let last7_days = summarize_days(&by_date, today)?;
    let today_summary = last7_days
        .last()
        .cloned()
        .expect("a seven-day range always contains today");

    Ok(UsageSummary {
        collected_at: collected_at.to_string(),
        today: today_summary,
        last7_days,
    })
}

/// Shared seven-day summarization over per-day agent aggregates. Both the
/// sidecar path and the worker snapshot path converge here so the
/// UsageSummary semantics (day gap zero-fill, ordering, JS-safe totals,
/// six-bucket breakdown, cache share) live in exactly one place.
fn summarize_days(
    by_date: &BTreeMap<String, BTreeMap<String, AgentTotals>>,
    today: chrono::NaiveDate,
) -> Result<Vec<DailyUsage>, AppError> {
    let mut last7_days = Vec::with_capacity(7);
    for days_ago in (0..7).rev() {
        let date = today - Duration::days(days_ago);
        let date_key = date.format("%Y-%m-%d").to_string();
        let (agents, estimated_cost_usd, cache_read_share) = match by_date.get(&date_key) {
            Some(day) => {
                let agents = build_agents(day);
                let (cost, share) = day_metrics(day);
                (agents, cost, share)
            }
            None => (Vec::new(), None, None),
        };
        let total_tokens = emit_day_total(&agents)?;
        // Component totals come from the aggregated day rows; `other` absorbs the
        // residual so the five counts always sum to `total_tokens`.
        let token_breakdown = match by_date.get(&date_key) {
            Some(day) => token_breakdown(day, total_tokens),
            None => TokenBreakdown::zero(),
        };
        last7_days.push(DailyUsage {
            date: date_key,
            total_tokens,
            token_breakdown,
            estimated_cost_usd,
            cache_read_share,
            agents,
        });
    }
    Ok(last7_days)
}

/// Computes the day's estimated cost and cache-input share from its active
/// (tokens > 0) agents. Cost is only a complete total when every active agent
/// has a known cost; the cache share is `cacheRead / (input + cacheRead +
/// cacheCreation)` and is unavailable (`None`) when the denominator is zero.
fn day_metrics(day: &BTreeMap<String, AgentTotals>) -> (Option<f64>, Option<f64>) {
    let mut cost_sum = 0.0f64;
    let mut cost_known = true;
    let mut any_active = false;
    let mut input = 0u64;
    let mut cache_read = 0u64;
    let mut cache_creation = 0u64;

    for totals in day.values().filter(|totals| totals.tokens > 0) {
        any_active = true;
        if totals.cost_known {
            cost_sum += totals.cost_sum;
        } else {
            cost_known = false;
        }
        input = input.saturating_add(totals.input);
        cache_read = cache_read.saturating_add(totals.cache_read);
        cache_creation = cache_creation.saturating_add(totals.cache_creation);
    }

    let cost = if any_active && cost_known && cost_sum.is_finite() && cost_sum >= 0.0 {
        Some(cost_sum)
    } else {
        None
    };

    let denominator = input
        .saturating_add(cache_read)
        .saturating_add(cache_creation);
    let share = if denominator > 0 {
        Some(cache_read as f64 / denominator as f64)
    } else {
        None
    };

    (cost, share)
}

/// Sums a day's common components and source-confirmed additive reasoning.
/// Anything still left after those known types is explicitly `unclassified`
/// rather than the ambiguous `other`.
fn token_breakdown(day: &BTreeMap<String, AgentTotals>, total_tokens: u64) -> TokenBreakdown {
    let mut input = 0u64;
    let mut output = 0u64;
    let mut cache_read = 0u64;
    let mut cache_creation = 0u64;
    let mut reasoning = 0u64;
    for totals in day.values() {
        input = input.saturating_add(totals.input);
        output = output.saturating_add(totals.output);
        cache_read = cache_read.saturating_add(totals.cache_read);
        cache_creation = cache_creation.saturating_add(totals.cache_creation);
        reasoning = reasoning.saturating_add(totals.reasoning);
    }
    let components = input
        .saturating_add(output)
        .saturating_add(cache_read)
        .saturating_add(cache_creation)
        .saturating_add(reasoning);
    TokenBreakdown {
        input_tokens: input,
        output_tokens: output,
        cache_read_tokens: cache_read,
        cache_creation_tokens: cache_creation,
        reasoning_tokens: reasoning,
        unclassified_tokens: total_tokens.saturating_sub(components),
    }
}

/// Validates a single unified row *before* it is merged. When the row carries a
/// day-level `totalTokens` it must equal the sum of that same row's agent rows.
/// Doing this per-row (rather than keeping a first-wins declaration per date)
/// means a duplicate-date row is checked against its own agent set, never the
/// merged one, and a later contradicting declaration is caught rather than
/// ignored. The individual agents are still re-checked against the safe JS
/// integer bound in `emit_day_total` after aggregation.
fn validate_row_total(row: &UnifiedDailyRow) -> Result<(), AppError> {
    let Some(declared) = row.total_tokens else {
        return Ok(());
    };
    let mut sum: u64 = 0;
    for agent in &row.agents {
        if agent.total_tokens > MAX_SAFE_INTEGER {
            return Err(AppError::invalid_ccusage(
                "ccusage",
                format!(
                    "agent `{}` total {} exceeds the safe integer bound {}",
                    agent.agent, agent.total_tokens, MAX_SAFE_INTEGER
                ),
            ));
        }
        sum = sum.checked_add(agent.total_tokens).ok_or_else(|| {
            AppError::invalid_ccusage("ccusage", "agent token total overflow".into())
        })?;
    }
    if sum > MAX_SAFE_INTEGER {
        return Err(AppError::invalid_ccusage(
            "ccusage",
            format!("daily token total {sum} exceeds the safe integer bound {MAX_SAFE_INTEGER}"),
        ));
    }
    if declared != sum {
        return Err(AppError::invalid_ccusage(
            "ccusage",
            format!("unified daily totalTokens ({declared}) disagrees with the agent sum ({sum})"),
        ));
    }
    Ok(())
}

/// Agrees a day's emitted token total with the safe JavaScript integer range.
///
/// A total above `Number.MAX_SAFE_INTEGER` (2^53-1) cannot be represented
/// losslessly once Tauri serializes the `u64` into a JS `number`, so it is
/// rejected with a stable error rather than silently rounded. (Row-level
/// `totalTokens` agreement is enforced earlier in `validate_row_total`.)
fn emit_day_total(agents: &[AgentUsage]) -> Result<u64, AppError> {
    let mut sum: u64 = 0;
    for agent in agents {
        if agent.tokens > MAX_SAFE_INTEGER {
            return Err(AppError::invalid_ccusage(
                "ccusage",
                format!(
                    "agent `{}` total {} exceeds the safe integer bound {}",
                    agent.id, agent.tokens, MAX_SAFE_INTEGER
                ),
            ));
        }
        sum = sum.checked_add(agent.tokens).ok_or_else(|| {
            AppError::invalid_ccusage("ccusage", "agent token total overflow".into())
        })?;
    }
    if sum > MAX_SAFE_INTEGER {
        return Err(AppError::invalid_ccusage(
            "ccusage",
            format!("daily token total {sum} exceeds the safe integer bound {MAX_SAFE_INTEGER}"),
        ));
    }
    Ok(sum)
}

/// Renders one day's active agents (tokens > 0) sorted by tokens descending,
/// then display name ascending for a stable, deterministic list. Each agent's
/// per-model breakdown rides along so the UI can expand any agent into its
/// models' token detail.
fn build_agents(totals: &BTreeMap<String, AgentTotals>) -> Vec<AgentUsage> {
    let mut agents: Vec<AgentUsage> = totals
        .iter()
        .filter(|(_, totals)| totals.tokens > 0)
        .map(|(id, totals)| AgentUsage {
            id: id.clone(),
            display_name: display_name(id),
            tokens: totals.tokens,
            reasoning_tokens: totals.reasoning,
            unclassified_tokens: totals.tokens.saturating_sub(
                totals
                    .input
                    .saturating_add(totals.output)
                    .saturating_add(totals.cache_read)
                    .saturating_add(totals.cache_creation)
                    .saturating_add(totals.reasoning),
            ),
            models: build_models(&totals.models),
        })
        .collect();
    agents.sort_by(|a, b| {
        b.tokens
            .cmp(&a.tokens)
            .then(a.display_name.cmp(&b.display_name))
    });
    agents
}

/// Renders an agent's distinct models sorted by total tokens descending, then
/// model name ascending for a stable list. Each model carries its own token
/// composition (`input + output + cacheRead + cacheCreation == totalTokens`).
/// The sum of these model totals may not equal the agent total (ccusage keeps
/// residual unattributed tokens separate), so the models are an informational
/// breakdown, never the authoritative figure.
fn build_models(models: &BTreeMap<String, ModelTotals>) -> Vec<ModelUsage> {
    let mut list: Vec<ModelUsage> = models
        .iter()
        .map(|(model_name, totals)| ModelUsage {
            model_name: model_name.clone(),
            model_display_name: model_display_name(model_name),
            input_tokens: totals.input,
            output_tokens: totals.output,
            cache_read_tokens: totals.cache_read,
            cache_creation_tokens: totals.cache_creation,
            total_tokens: totals.total,
        })
        .collect();
    list.sort_by(|a, b| {
        b.total_tokens
            .cmp(&a.total_tokens)
            .then(a.model_name.cmp(&b.model_name))
    });
    list
}

fn parse_unified(json: &str) -> Result<UnifiedReport, AppError> {
    let report: UnifiedReport = serde_json::from_str(json)
        .map_err(|error| AppError::invalid_ccusage("ccusage", error.to_string()))?;

    for row in &report.daily {
        let parsed = NaiveDate::parse_from_str(&row.period, "%Y-%m-%d")
            .map_err(|error| AppError::invalid_ccusage("ccusage", error.to_string()))?;
        if parsed.format("%Y-%m-%d").to_string() != row.period {
            return Err(AppError::invalid_ccusage(
                "ccusage",
                format!("invalid daily period: {}", row.period),
            ));
        }
    }

    Ok(report)
}

fn parse_antigravity(json: &str) -> Result<AntigravityReport, AppError> {
    let report: AntigravityReport = serde_json::from_str(json)
        .map_err(|error| AppError::invalid_ccusage("Antigravity", error.to_string()))?;

    for row in &report.daily {
        let parsed = NaiveDate::parse_from_str(&row.date, "%Y-%m-%d")
            .map_err(|error| AppError::invalid_ccusage("Antigravity", error.to_string()))?;
        if parsed.format("%Y-%m-%d").to_string() != row.date {
            return Err(AppError::invalid_ccusage(
                "Antigravity",
                format!("invalid daily date: {}", row.date),
            ));
        }
    }

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sanitized unified report encoding exactly the v0.1 focused fixture data
    /// (claude-daily.json + codex-daily.json combined), extended with the real
    /// ccusage token composition and per-agent cost so the adapter's daily cost
    /// path is exercised. The `totals` key mirrors the real ccusage shape and is
    /// intentionally ignored by the adapter.
    const UNIFIED: &str = include_str!("../../tests/fixtures/ccusage/unified-daily.json");
    const ANTIGRAVITY: &str = include_str!("../../tests/fixtures/ccusage/antigravity-daily.json");
    const COLLECTED_AT: &str = "2026-08-24T12:00:00Z";

    fn summarize(json: &str) -> UsageSummary {
        normalize_unified(json, "2026-08-24", COLLECTED_AT).unwrap()
    }

    #[test]
    fn unifies_daily_agents_and_fills_missing_days() {
        let summary = summarize(UNIFIED);

        assert_eq!(summary.last7_days.len(), 7);
        assert_eq!(summary.today.date, "2026-08-24");
        // Parity with the v0.1 focused fixtures:
        //   today.total = 8.42M (Claude) + 5.17M (Codex)
        //   seven-day   = 62.29M
        assert_eq!(summary.today.total_tokens, 13_590_000);
        assert_eq!(summary.last7_days[1].agents.len(), 1); // only Claude active 08-19
        assert_eq!(summary.last7_days[1].agents[0].id, "claude");
        assert_eq!(summary.last7_days[1].total_tokens, 5_000_000);
        // Dynamic agent list, sorted by tokens descending (Claude > Codex today).
        assert_eq!(summary.today.agents.len(), 2);
        assert_eq!(summary.today.agents[0].id, "claude");
        assert_eq!(summary.today.agents[0].display_name, "Claude Code");
        assert_eq!(summary.today.agents[0].tokens, 8_420_000);
        assert_eq!(summary.today.agents[1].id, "codex");
        assert_eq!(summary.today.agents[1].display_name, "Codex");
        assert_eq!(summary.today.agents[1].tokens, 5_170_000);
        assert_eq!(
            summary
                .last7_days
                .iter()
                .map(|day| day.total_tokens)
                .sum::<u64>(),
            62_290_000
        );
        // carried-through collection time
        assert_eq!(summary.collected_at, COLLECTED_AT);
    }

    #[test]
    fn merges_the_pinned_antigravity_report_without_faking_unknown_cost() {
        let summary = normalize_reports(UNIFIED, ANTIGRAVITY, "2026-08-24", COLLECTED_AT).unwrap();

        assert_eq!(summary.today.total_tokens, 14_590_000);
        assert_eq!(summary.today.agents.len(), 3);
        assert_eq!(summary.today.agents[2].id, "antigravity");
        assert_eq!(summary.today.agents[2].display_name, "Antigravity");
        assert_eq!(summary.today.agents[2].tokens, 1_000_000);
        assert_eq!(summary.today.agents[2].reasoning_tokens, 5_000);
        assert_eq!(summary.today.agents[2].unclassified_tokens, 0);
        assert_eq!(summary.today.agents[2].models.len(), 1);
        assert_eq!(
            summary.today.agents[2].models[0].model_name,
            "gemini-3.7-flash-safety-le"
        );
        assert_eq!(
            summary.today.agents[2].models[0].model_display_name,
            "gemini-3.7-flash-safety-le"
        );
        assert_eq!(summary.today.agents[2].models[0].total_tokens, 995_000);
        assert_eq!(summary.today.token_breakdown.reasoning_tokens, 5_000);
        assert_eq!(summary.today.token_breakdown.unclassified_tokens, 0);
        // The focused PR reports zero when a model has no embedded price. A
        // positive-token zero therefore becomes unknown, not a fake $0.00.
        assert!(summary.today.estimated_cost_usd.is_none());
        let expected_share = (9_444_242.0 + 700_000.0) / (13_338_850.0 + 900_000.0);
        assert_eq!(summary.today.cache_read_share, Some(expected_share));
    }

    #[test]
    fn released_unified_antigravity_rows_win_over_the_compatibility_report() {
        let unified = r#"{
            "daily": [{
                "period": "2026-08-24",
                "totalTokens": 42,
                "agents": [{ "agent": "antigravity", "totalTokens": 42 }]
            }]
        }"#;
        let summary = normalize_reports(unified, ANTIGRAVITY, "2026-08-24", COLLECTED_AT).unwrap();

        assert_eq!(summary.today.total_tokens, 42);
        assert_eq!(summary.today.agents.len(), 1);
        assert_eq!(summary.today.agents[0].tokens, 42);
    }

    #[test]
    fn rejects_malformed_antigravity_json_with_an_actionable_code() {
        let error = normalize_reports(UNIFIED, "not-json", "2026-08-24", COLLECTED_AT).unwrap_err();
        assert_eq!(error.code, "invalid_ccusage_output");
        assert!(error.message.contains("Antigravity"));
    }

    #[test]
    fn folds_today_cost_and_cache_share_from_agent_rows() {
        let summary = summarize(UNIFIED);
        // Today's fixture agents carry known costs: claude 7.43 + codex 4.57 = 12.
        let cost = summary
            .today
            .estimated_cost_usd
            .expect("today has a known cost");
        assert!((cost - 12.0).abs() < 1e-9, "expected ~12.0, got {cost}");
        // Cache share cacheRead / (input + cacheRead + cacheCreation) on today's
        // fixture summed composition: numerators 6144242+3300000, denominator
        // 2174608+1720000 + 9444242 + 0.
        let share = summary
            .today
            .cache_read_share
            .expect("today has a cache share");
        let expected = 9_444_242.0 / 13_338_850.0;
        assert!(
            (share - expected).abs() < 1e-12,
            "expected {expected}, got {share}"
        );
    }

    #[test]
    fn returns_zeroes_when_the_unified_report_is_empty() {
        let summary = summarize(r#"{"daily":[]}"#);
        assert!(summary.last7_days.iter().all(|day| day.total_tokens == 0));
        assert_eq!(summary.last7_days.len(), 7);
        assert!(summary.today.agents.is_empty());
        assert!(summary.today.estimated_cost_usd.is_none());
        assert!(summary.today.cache_read_share.is_none());
    }

    #[test]
    fn shows_unknown_agents_with_a_title_cased_fallback_name() {
        // Any agent id, present or future, is now included rather than dropped.
        let json = r#"{
            "daily": [
                {
                    "period": "2026-08-24",
                    "agents": [
                        { "agent": "claude",      "totalTokens": 1000 },
                        { "agent": "codex",       "totalTokens": 500 },
                        { "agent": "opencode",    "totalTokens": 987654 },
                        { "agent": "future-agent","totalTokens": 200 }
                    ]
                }
            ]
        }"#;
        let summary = summarize(json);
        // Sorted by tokens desc: opencode, claude, codex, future-agent.
        assert_eq!(summary.today.total_tokens, 989_354);
        assert_eq!(
            summary
                .today
                .agents
                .iter()
                .map(|a| a.id.as_str())
                .collect::<Vec<_>>(),
            vec!["opencode", "claude", "codex", "future-agent"]
        );
        assert_eq!(summary.today.agents[0].display_name, "Opencode");
        assert_eq!(summary.today.agents[3].display_name, "Future Agent");
        assert_eq!(summary.today.agents[3].tokens, 200);
    }

    #[test]
    fn excludes_today_zero_agents_but_keeps_their_history() {
        // `claude` has history (yesterday) but zero today; it must not occupy the
        // today list, while yesterday's total still counts it.
        let json = r#"{
            "daily": [
                { "period": "2026-08-23", "agents": [ { "agent": "claude", "totalTokens": 4000 } ] },
                { "period": "2026-08-24", "agents": [ { "agent": "claude", "totalTokens": 0 },
                                                      { "agent": "codex",  "totalTokens": 900 } ] }
            ]
        }"#;
        let summary = summarize(json);
        assert_eq!(summary.today.agents.len(), 1);
        assert_eq!(summary.today.agents[0].id, "codex");
        assert_eq!(summary.today.total_tokens, 900);
        // yesterday still counts claude's 4000
        assert_eq!(summary.last7_days[5].total_tokens, 4000);
        assert_eq!(summary.last7_days[5].agents[0].id, "claude");
    }

    #[test]
    fn handles_zero_one_two_and_six_active_agents() {
        let day = |agents: &str| format!(r#"{{ "period": "2026-08-24", "agents": [{}] }}"#, agents);

        // Zero active agents (all zero) -> empty today list and no cost/share.
        let zero = summarize(&format!(
            r#"{{"daily":[{}]}}"#,
            day(r#"{ "agent":"claude", "totalTokens":0 }"#)
        ));
        assert_eq!(zero.today.agents.len(), 0);
        assert_eq!(zero.today.total_tokens, 0);
        assert!(zero.today.estimated_cost_usd.is_none());
        assert!(zero.today.cache_read_share.is_none());

        // One active agent.
        let one = summarize(&format!(
            r#"{{"daily":[{}]}}"#,
            day(r#"{ "agent":"codex", "totalTokens":5 } "#)
        ));
        assert_eq!(one.today.agents.len(), 1);
        assert_eq!(one.today.agents[0].id, "codex");
        assert_eq!(one.today.total_tokens, 5);

        // Two active agents, sorted by tokens desc.
        let two = summarize(&format!(
            r#"{{"daily":[{}]}}"#,
            day(r#"{ "agent":"claude", "totalTokens":3 }, { "agent":"codex","totalTokens":7 }"#)
        ));
        assert_eq!(two.today.total_tokens, 10);
        assert_eq!(
            two.today
                .agents
                .iter()
                .map(|a| a.id.as_str())
                .collect::<Vec<_>>(),
            vec!["codex", "claude"]
        );

        // Six active agents render in full with stable sorted order.
        let six = summarize(&format!(
            r#"{{"daily":[{}]}}"#,
            day(r#"{ "agent":"claude", "totalTokens":1 },
                        { "agent":"codex", "totalTokens":2 },
                        { "agent":"opencode", "totalTokens":3 },
                        { "agent":"future-agent", "totalTokens":4 },
                        { "agent":"copilot", "totalTokens":5 },
                        { "agent":"cursor", "totalTokens":6 }"#)
        ));
        assert_eq!(six.today.agents.len(), 6);
        assert_eq!(
            six.today
                .agents
                .iter()
                .map(|a| a.id.as_str())
                .collect::<Vec<_>>(),
            vec![
                "cursor",
                "copilot",
                "future-agent",
                "opencode",
                "codex",
                "claude"
            ]
        );
        assert_eq!(six.today.agents[5].display_name, "Claude Code");
        assert_eq!(six.today.agents[2].display_name, "Future Agent");
        assert_eq!(six.today.total_tokens, 21);
    }

    #[test]
    fn sorts_equal_token_agents_by_display_name() {
        let json = r#"{
            "daily": [
                {
                    "period": "2026-08-24",
                    "agents": [
                        { "agent": "codex",  "totalTokens": 500 },
                        { "agent": "claude", "totalTokens": 500 }
                    ]
                }
            ]
        }"#;
        // Equal tokens -> display name ascending: "Claude Code" before "Codex".
        let summary = summarize(json);
        assert_eq!(summary.today.agents[0].id, "claude");
        assert_eq!(summary.today.agents[1].id, "codex");
    }

    #[test]
    fn builds_a_day_token_breakdown_that_sums_to_the_day_total() {
        // Components across the day's agents, with a residual the agent rows do
        // not attribute (`codex` total 1000 but components sum to 400) that must
        // land in `unclassified_tokens` so the known types plus the explicit
        // fallback equal the day total exactly.
        let json = r#"{
            "daily": [{
                "period": "2026-08-24",
                "agents": [
                    { "agent": "claude", "totalTokens": 200,
                      "inputTokens": 50, "outputTokens": 50, "cacheReadTokens": 100, "cacheCreationTokens": 0 },
                    { "agent": "codex",  "totalTokens": 1000,
                      "inputTokens": 100, "outputTokens": 100, "cacheReadTokens": 200, "cacheCreationTokens": 0 }
                ]
            }]
        }"#;
        let summary = summarize(json);
        let day = &summary.today;
        assert_eq!(day.total_tokens, 1200);
        let b = &day.token_breakdown;
        assert_eq!(b.input_tokens, 150);
        assert_eq!(b.output_tokens, 150);
        assert_eq!(b.cache_read_tokens, 300);
        assert_eq!(b.cache_creation_tokens, 0);
        // residual: 1200 - (150+150+300+0) = 600 (all of codex's unattributed share).
        assert_eq!(b.reasoning_tokens, 0);
        assert_eq!(b.unclassified_tokens, 600);
        assert_eq!(
            b.input_tokens
                + b.output_tokens
                + b.cache_read_tokens
                + b.cache_creation_tokens
                + b.reasoning_tokens
                + b.unclassified_tokens,
            day.total_tokens
        );
    }

    #[test]
    fn token_breakdown_defaults_missing_component_fields_to_zero() {
        // Empty days and zero-token days stay all-zero and never emit a bogus
        // breakdown; a day with only cacheRead tokens reports the rest as
        // unclassified rather than guessing a token category.
        let empty = summarize(r#"{"daily":[]}"#);
        let b = &empty.today.token_breakdown;
        assert_eq!(b.input_tokens, 0);
        assert_eq!(b.output_tokens, 0);
        assert_eq!(b.cache_read_tokens, 0);
        assert_eq!(b.cache_creation_tokens, 0);
        assert_eq!(b.reasoning_tokens, 0);
        assert_eq!(b.unclassified_tokens, 0);

        let json = r#"{
            "daily": [{
                "period": "2026-08-24",
                "agents": [ { "agent": "claude", "totalTokens": 700,
                              "cacheReadTokens": 500 } ]
            }]
        }"#;
        let summary = summarize(json);
        let b = &summary.today.token_breakdown;
        assert_eq!(b.cache_read_tokens, 500);
        assert_eq!(b.input_tokens, 0);
        assert_eq!(b.output_tokens, 0);
        assert_eq!(b.cache_creation_tokens, 0);
        assert_eq!(b.reasoning_tokens, 0);
        assert_eq!(b.unclassified_tokens, 200); // 700 - 500
    }

    #[test]
    fn aggregates_per_model_composition_without_double_counting() {
        // The same agent+model appears across two rows for the day. Each model's
        // composition must be the merge of both rows, and the model display name
        // is normalized from the raw id.
        let json = r#"{
            "daily": [
                { "period": "2026-08-24",
                  "agents": [
                    { "agent": "codex", "totalTokens": 300,
                      "modelBreakdowns": [
                        { "modelName": "gpt-5.6-sol-ga-1", "inputTokens": 150, "outputTokens": 50, "cacheReadTokens": 100, "cacheCreationTokens": 0 }
                      ] },
                    { "agent": "codex", "totalTokens": 700,
                      "modelBreakdowns": [
                        { "modelName": "gpt-5.6-sol-ga-1", "inputTokens": 300, "outputTokens": 100, "cacheReadTokens": 300, "cacheCreationTokens": 0 }
                      ] }
                  ] }
            ]
        }"#;
        let summary = summarize(json);
        let codex = &summary.today.agents[0];
        assert_eq!(codex.id, "codex");
        assert_eq!(codex.models.len(), 1, "same model across rows must merge");
        let model = &codex.models[0];
        assert_eq!(model.model_name, "gpt-5.6-sol-ga-1");
        assert_eq!(model.model_display_name, "gpt-5.6-sol");
        assert_eq!(model.input_tokens, 450);
        assert_eq!(model.output_tokens, 150);
        assert_eq!(model.cache_read_tokens, 400);
        assert_eq!(model.cache_creation_tokens, 0);
        assert_eq!(model.total_tokens, 1000);
    }

    #[test]
    fn model_display_name_normalizes_and_falls_back() {
        // `-ga-<digits>` release markers are stripped; empty/missing ids get a
        // stable, understandable placeholder; other ids are kept trimmed.
        assert_eq!(
            model_display_name("deepseek-v4-flash-ga-260731"),
            "deepseek-v4-flash"
        );
        assert_eq!(model_display_name("gpt-5.6-sol"), "gpt-5.6-sol");
        assert_eq!(model_display_name("  "), "Unknown model");
        assert_eq!(model_display_name(""), "Unknown model");
        assert_eq!(
            model_display_name("gemini-3.7-flash-safety-le"),
            "gemini-3.7-flash-safety-le"
        );
        // A bare `-ga-` (empty family) falls through to the full name.
        assert_eq!(model_display_name("-ga-260731"), "-ga-260731");
    }

    #[test]
    fn aggregates_multi_model_agent_into_sorted_model_detail() {
        // One agent split across two models. The agent total stays authoritative;
        // each model's total is computed from its components and both are shown
        // sorted by total tokens descending.
        let json = r#"{
            "daily": [{
                "period": "2026-08-24",
                "agents": [{
                    "agent": "codex", "totalTokens": 1000,
                    "modelBreakdowns": [
                        { "modelName": "gpt-5.6-sol",  "inputTokens": 100, "cacheReadTokens": 800, "cacheCreationTokens": 0, "outputTokens": 50 },
                        { "modelName": "gpt-5.6-luna", "inputTokens": 20,  "cacheReadTokens": 20,  "cacheCreationTokens": 10, "outputTokens": 0 }
                    ]
                }]
            }]
        }"#;
        let summary = summarize(json);
        assert_eq!(summary.today.agents.len(), 1);
        let codex = &summary.today.agents[0];
        assert_eq!(codex.id, "codex");
        assert_eq!(codex.tokens, 1000);
        assert_eq!(codex.models.len(), 2);
        // sol = 100+800+50 = 950; luna = 20+20+10 = 50; sorted desc by tokens.
        assert_eq!(codex.models[0].model_name, "gpt-5.6-sol");
        assert_eq!(codex.models[0].total_tokens, 950);
        assert_eq!(codex.models[1].model_name, "gpt-5.6-luna");
        assert_eq!(codex.models[1].total_tokens, 50);
        assert_eq!(summary.today.total_tokens, 1000);
    }

    #[test]
    fn merges_a_model_repeated_across_agent_rows_without_double_counting() {
        // The same agent+model appears in two rows for the same day (ccusage can
        // emit split rows). Its model total must be the sum, never double counted,
        // and the agent token total matches ccusage's aggregate.
        let json = r#"{
            "daily": [
                { "period": "2026-08-24",
                  "agents": [
                    { "agent": "codex", "totalTokens": 300,
                      "modelBreakdowns": [ { "modelName": "gpt-5.6-sol", "inputTokens": 150, "outputTokens": 50, "cacheReadTokens": 100, "cacheCreationTokens": 0 } ] },
                    { "agent": "codex", "totalTokens": 700,
                      "modelBreakdowns": [ { "modelName": "gpt-5.6-sol", "inputTokens": 300, "outputTokens": 100, "cacheReadTokens": 300, "cacheCreationTokens": 0 } ] }
                  ] }
            ]
        }"#;
        let summary = summarize(json);
        assert_eq!(summary.today.total_tokens, 1000);
        let codex = &summary.today.agents[0];
        assert_eq!(codex.id, "codex");
        assert_eq!(codex.tokens, 1000);
        assert_eq!(codex.models.len(), 1, "same model across rows must merge");
        assert_eq!(codex.models[0].model_name, "gpt-5.6-sol");
        // (150+50+100) + (300+100+300) = 300 + 700 = 1000.
        assert_eq!(codex.models[0].total_tokens, 1000);
    }

    #[test]
    fn agents_without_a_model_breakdown_have_no_models() {
        // A model-less agent (e.g. the pinned Antigravity compatibility path)
        // or a future agent with no per-model data renders as itself with an
        // empty model list — the UI just does not offer an expansion.
        let json = r#"{
            "daily": [{
                "period": "2026-08-24",
                "agents": [ { "agent": "claude", "totalTokens": 50 } ]
            }]
        }"#;
        let summary = summarize(json);
        assert_eq!(summary.today.agents.len(), 1);
        assert!(summary.today.agents[0].models.is_empty());
        assert_eq!(summary.today.agents[0].tokens, 50);
    }

    #[test]
    fn sorts_equal_token_models_by_name() {
        let json = r#"{
            "daily": [{
                "period": "2026-08-24",
                "agents": [{
                    "agent": "codex", "totalTokens": 600,
                    "modelBreakdowns": [
                        { "modelName": "b", "inputTokens": 300, "outputTokens": 0, "cacheReadTokens": 0, "cacheCreationTokens": 0 },
                        { "modelName": "a", "inputTokens": 300, "outputTokens": 0, "cacheReadTokens": 0, "cacheCreationTokens": 0 }
                    ]
                }]
            }]
        }"#;
        let summary = summarize(json);
        let codex = &summary.today.agents[0];
        assert_eq!(codex.models[0].model_name, "a");
        assert_eq!(codex.models[1].model_name, "b");
    }

    #[test]
    fn aggregates_duplicate_agent_rows_within_the_safe_integer_range() {
        let json = r#"{
            "daily": [
                {
                    "period": "2026-08-24",
                    "agents": [
                        { "agent": "claude", "totalTokens": 1000 },
                        { "agent": "claude", "totalTokens": 2000 },
                        { "agent": "codex",  "totalTokens": 400 }
                    ]
                }
            ]
        }"#;
        let summary = summarize(json);
        assert_eq!(summary.today.agents[0].id, "claude"); // 3000 > 400
        assert_eq!(summary.today.agents[0].tokens, 3_000);
        assert_eq!(summary.today.agents[1].id, "codex");
        assert_eq!(summary.today.agents[1].tokens, 400);
        assert_eq!(summary.today.total_tokens, 3_400);
    }

    #[test]
    fn rejects_token_totals_beyond_the_safe_js_integer_bound() {
        // A single agent reporting more than Number.MAX_SAFE_INTEGER would be
        // silently rounded once serialized to a JS number — reject it instead.
        let json = r#"{
            "daily": [{
                "period": "2026-08-24",
                "agents": [ { "agent": "codex", "totalTokens": 18446744073709551615 } ]
            }]
        }"#;
        let error = normalize_unified(json, "2026-08-24", COLLECTED_AT).unwrap_err();
        assert_eq!(error.code, "invalid_ccusage_output");
        assert!(error.message.contains("safe integer"));
    }

    #[test]
    fn accepts_day_total_equal_to_the_agent_sum() {
        // The unified report's day-level totalTokens (fixture) must agree with
        // the agent rows; when it does, the total is emitted as-is.
        let json = r#"{
            "daily": [{
                "period": "2026-08-24", "totalTokens": 9000,
                "agents": [
                    { "agent": "claude", "totalTokens": 7000 },
                    { "agent": "codex",  "totalTokens": 2000 }
                ]
            }]
        }"#;
        let summary = summarize(json);
        assert_eq!(summary.today.total_tokens, 9000);
    }

    #[test]
    fn rejects_when_day_total_disagrees_with_the_agent_sum() {
        // A non-zero ccusage day whose rows parse to a different total would
        // otherwise silently show the wrong (or zero) number.
        let json = r#"{
            "daily": [{
                "period": "2026-08-24", "totalTokens": 99999,
                "agents": [ { "agent": "claude", "totalTokens": 7000 } ]
            }]
        }"#;
        let error = normalize_unified(json, "2026-08-24", COLLECTED_AT).unwrap_err();
        assert_eq!(error.code, "invalid_ccusage_output");
        assert!(error.message.contains("disagrees"));
    }

    #[test]
    fn aggregates_duplicate_date_rows_after_validating_each_row_total() {
        // The same period appears twice; each row's day-level totalTokens must
        // equal that row's own agent sum before the rows aggregate. Nothing
        // contradicts here, and both rows merge into today's total.
        let json = r#"{
            "daily": [
                { "period": "2026-08-24", "totalTokens": 3000,
                  "agents": [ { "agent": "claude", "totalTokens": 3000 } ] },
                { "period": "2026-08-24", "totalTokens": 500,
                  "agents": [ { "agent": "codex", "totalTokens": 500 } ] }
            ]
        }"#;
        let summary = summarize(json);
        assert_eq!(summary.today.total_tokens, 3500);
        assert_eq!(summary.today.agents.len(), 2);
    }

    #[test]
    fn rejects_a_duplicate_date_row_whose_total_disagrees_with_its_own_sum() {
        // Row two's declared totalTokens contradicts row two's own agent sum. A
        // per-date first-wins declaration would ignore this; per-row validation
        // must reject it before aggregation.
        let json = r#"{
            "daily": [
                { "period": "2026-08-24", "totalTokens": 3000,
                  "agents": [ { "agent": "claude", "totalTokens": 3000 } ] },
                { "period": "2026-08-24", "totalTokens": 999,
                  "agents": [ { "agent": "codex", "totalTokens": 500 } ] }
            ]
        }"#;
        let error = normalize_unified(json, "2026-08-24", COLLECTED_AT).unwrap_err();
        assert_eq!(error.code, "invalid_ccusage_output");
        assert!(error.message.contains("disagrees"));
    }

    #[test]
    fn fills_missing_and_unordered_dates_in_ascending_windows() {
        // Two unordered, non-contiguous days; everything else must be zero-filled
        // in ascending date order over today-6..today.
        let json = r#"{
            "daily": [
                { "period": "2026-08-24", "agents": [ { "agent": "codex", "totalTokens": 700 } ] },
                { "period": "2026-08-21", "agents": [ { "agent": "claude", "totalTokens": 300 } ] }
            ]
        }"#;
        let summary = summarize(json);
        assert_eq!(summary.last7_days.len(), 7);
        assert_eq!(summary.last7_days[0].date, "2026-08-18");
        assert_eq!(summary.last7_days[6].date, "2026-08-24");
        assert_eq!(summary.last7_days[3].total_tokens, 300); // 08-21
        assert_eq!(summary.last7_days[6].total_tokens, 700); // 08-24
        assert_eq!(summary.last7_days[1].total_tokens, 0); // 08-19 gap
        assert_eq!(summary.last7_days[5].total_tokens, 0); // 08-23 gap
    }

    #[test]
    fn cost_is_absent_when_any_active_agent_price_is_missing() {
        // Both agents active, claude priced but codex unpriced: the day cost must
        // be None (a partial sum would mislead), never faked as 0. The cache
        // share is independent of price and stays computable.
        let json = r#"{
            "daily": [
                {
                    "period": "2026-08-24",
                    "agents": [
                        { "agent": "claude", "totalTokens": 500,
                          "inputTokens": 100, "cacheReadTokens": 100, "cacheCreationTokens": 0,
                          "totalCost": 0.5 },
                        { "agent": "codex",  "totalTokens": 700, "totalCost": null }
                    ]
                }
            ]
        }"#;
        let summary = summarize(json);
        assert!(summary.today.estimated_cost_usd.is_none());
        assert_eq!(summary.today.cache_read_share, Some(0.5));
    }

    #[test]
    fn cache_share_is_absent_when_the_denominator_is_zero() {
        // Active agent with usage but no input/cacheRead/cacheCreation tokens:
        // denominator is zero, so the share must be None, never NaN/Infinity.
        let json = r#"{
            "daily": [{
                "period": "2026-08-24",
                "agents": [
                    { "agent": "claude", "totalTokens": 700, "totalCost": 1.0 }
                ]
            }]
        }"#;
        let summary = summarize(json);
        assert!(summary.today.cache_read_share.is_none());
        assert_eq!(summary.today.estimated_cost_usd, Some(1.0));
    }

    #[test]
    fn a_zero_denominator_or_missing_price_never_produces_nan_or_infinity() {
        // Only cacheCreation tokens, no input or cacheRead -> denominator > 0 but
        // numerator 0 -> share 0.0 (not NaN). No cost -> None.
        let json = r#"{
            "daily": [{
                "period": "2026-08-24",
                "agents": [
                    { "agent": "claude", "totalTokens": 50,
                      "inputTokens": 0, "cacheReadTokens": 0, "cacheCreationTokens": 50,
                      "totalCost": 0.25 }
                ]
            }]
        }"#;
        let summary = summarize(json);
        assert_eq!(summary.today.cache_read_share, Some(0.0));
        assert_eq!(summary.today.estimated_cost_usd, Some(0.25));
    }

    #[test]
    fn a_non_finite_cost_is_rejected_at_parse() {
        // JSON has no bare NaN literal, and a cost of `1e999` (Infinity) is
        // rejected by serde_json as out of range — so a non-finite cost can
        // never enter the public contract at all.
        let json = r#"{
            "daily": [{
                "period": "2026-08-24",
                "agents": [
                    { "agent": "claude", "totalTokens": 50, "totalCost": 1.0 },
                    { "agent": "codex",  "totalTokens": 50, "totalCost": 1e999 }
                ]
            }]
        }"#;
        let error = normalize_unified(json, "2026-08-24", COLLECTED_AT).unwrap_err();
        assert_eq!(error.code, "invalid_ccusage_output");
    }

    #[test]
    fn rejects_malformed_unified_json_with_an_actionable_code() {
        let error = normalize_unified("not-json", "2026-08-24", COLLECTED_AT).unwrap_err();
        assert_eq!(error.code, "invalid_ccusage_output");
        assert!(error.message.contains("ccusage"));
    }

    #[test]
    fn rejects_an_invalid_unified_period() {
        let json = r#"{"daily":[{"period":"not-a-date","agents":[]}]}"#;
        let error = normalize_unified(json, "2026-08-24", COLLECTED_AT).unwrap_err();
        assert_eq!(error.code, "invalid_ccusage_output");
    }

    #[test]
    fn rejects_a_wrong_field_type() {
        // totalTokens as a string is a type error, not silently coerced.
        let json = r#"{"daily":[{"period":"2026-08-24","agents":[{"agent":"claude","totalTokens":"1000"}]}]}"#;
        let error = normalize_unified(json, "2026-08-24", COLLECTED_AT).unwrap_err();
        assert_eq!(error.code, "invalid_ccusage_output");
    }
}
