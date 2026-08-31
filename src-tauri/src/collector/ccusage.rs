//! Adapter from the vendored ccusage per-agent loader to [`Collector`] types.
//!
//! This is the only module that converts vendor output into CAM-owned types.
//! The vendor seam is `ccusage_adapter_all::daily_report_for_agent` (Phase 1
//! per-agent entry point): it runs exactly one agent's loader, returns a
//! structurally classified outcome (no error-text matching), and surfaces
//! recoverable problems as diagnostics. Explicit [`DataSource::Paths`] roots
//! are passed straight through to the vendor load — the collector never
//! mutates the process environment.
//!
//! Known vendor seam limitations (documented, Phase 2 prerequisites):
//! - Record granularity is the daily aggregate; session-level records come
//!   later.
//! - Loads serialize behind a mutex inside the vendor entry point (required
//!   by its load-scoped context stores).

use std::path::PathBuf;

use chrono::NaiveDate;
use serde_json::Value;

use super::{
    AgentKind, CollectRequest, CollectResult, CollectionDiagnostic, Collector, CollectorError,
    CostNanoUsd, DiagnosticKind, ModelBreakdown, ModelName, UsageRecord,
};

const VENDOR_ID: &str = "ccusage v20.0.20";

/// Collector implementation backed by the vendored ccusage sources.
///
/// One instance per agent; all instances share the same vendor seam.
#[derive(Debug, Clone, Copy)]
pub struct AgentCollector {
    agent: AgentKind,
}

impl AgentCollector {
    pub const fn new(agent: AgentKind) -> Self {
        Self { agent }
    }
}

impl Collector for AgentCollector {
    fn agent(&self) -> AgentKind {
        self.agent
    }

    fn collect(&self, request: &CollectRequest) -> Result<CollectResult, CollectorError> {
        if request.agent != self.agent {
            return Err(CollectorError::InvalidRequest {
                details: format!(
                    "request targets {} but this collector reads {}",
                    request.agent.label(),
                    self.agent.label()
                ),
            });
        }
        request.validate()?;
        let root_override: Option<Vec<PathBuf>> = match &request.source {
            super::DataSource::Environment => None,
            super::DataSource::Paths(paths) => Some(paths.clone()),
            // Exhaustive match: a future DataSource variant must be handled
            // here explicitly, not silently ignored.
        };

        let shared = vendor_shared_args(request);
        let outcome = ccusage_adapter_all::daily_report_for_agent(
            self.agent.id(),
            root_override.as_deref(),
            &shared,
        );

        match outcome {
            ccusage_adapter_all::AgentLoadOutcome::Report {
                report,
                diagnostics,
            } => {
                let (records, invariant_diagnostics) = from_vendor_report(&report, self.agent)?;
                let mut diagnostics: Vec<CollectionDiagnostic> = diagnostics
                    .into_iter()
                    .map(|diag| CollectionDiagnostic {
                        kind: match diag.kind {
                            ccusage_core::load_context::LoadDiagKind::CorruptFile => {
                                DiagnosticKind::CorruptFile
                            }
                            ccusage_core::load_context::LoadDiagKind::CorruptRecord => {
                                DiagnosticKind::CorruptRecord
                            }
                            ccusage_core::load_context::LoadDiagKind::DatabaseError => {
                                DiagnosticKind::DatabaseError
                            }
                            ccusage_core::load_context::LoadDiagKind::SourceUnreadable => {
                                DiagnosticKind::SourceUnreadable
                            }
                            ccusage_core::load_context::LoadDiagKind::SourceChanged => {
                                DiagnosticKind::SourceChanged
                            }
                        },
                        file: diag.file,
                        details: diag.details,
                    })
                    .collect();
                diagnostics.extend(invariant_diagnostics);
                Ok(CollectResult {
                    agent: self.agent,
                    records,
                    diagnostics,
                })
            }
            ccusage_adapter_all::AgentLoadOutcome::SourceUnavailable { details, .. } => {
                Err(CollectorError::SourceUnavailable {
                    agent: self.agent,
                    details,
                })
            }
            ccusage_adapter_all::AgentLoadOutcome::Failed { kind, details } => Err(match kind {
                ccusage_core::load_context::LoadFailureKind::SourceUnavailable => {
                    CollectorError::SourceUnavailable {
                        agent: self.agent,
                        details,
                    }
                }
                ccusage_core::load_context::LoadFailureKind::InvalidConfig => {
                    CollectorError::InvalidRequest { details }
                }
                ccusage_core::load_context::LoadFailureKind::Database => {
                    CollectorError::DatabaseQuery {
                        agent: self.agent,
                        details,
                    }
                }
                ccusage_core::load_context::LoadFailureKind::Internal => {
                    CollectorError::VendorAdapter {
                        vendor: VENDOR_ID.to_string(),
                        details,
                    }
                }
            }),
        }
    }
}

/// Builds the vendor call arguments. `single_thread` is forced for
/// deterministic results; `offline` is mandatory (no network pricing).
fn vendor_shared_args(request: &CollectRequest) -> ccusage_cli::SharedArgs {
    ccusage_cli::SharedArgs {
        json: true,
        offline: true,
        single_thread: true,
        timezone: Some(request.timezone.0.clone()),
        since: request
            .window
            .map(|window| window.start_inclusive.format("%Y%m%d").to_string()),
        until: request
            .window
            .map(|window| window.end_inclusive.format("%Y%m%d").to_string()),
        ..ccusage_cli::SharedArgs::default()
    }
}

/// Converts the per-agent daily report into typed records plus any token
/// invariant diagnostics produced along the way.
fn from_vendor_report(
    report: &Value,
    agent: AgentKind,
) -> Result<(Vec<UsageRecord>, Vec<CollectionDiagnostic>), CollectorError> {
    let daily = report
        .get("daily")
        .and_then(Value::as_array)
        .ok_or_else(|| CollectorError::VendorAdapter {
            vendor: VENDOR_ID.to_string(),
            details: "report is missing the `daily` array".to_string(),
        })?;

    let mut records = Vec::new();
    let mut diagnostics = Vec::new();
    for row in daily {
        // The per-agent report still emits the unified row shape (agent
        // "all" plus an `agents` breakdown array); find this agent's entry.
        let Some(breakdown) = row
            .get("agents")
            .and_then(Value::as_array)
            .and_then(|agents| {
                agents
                    .iter()
                    .find(|entry| entry.get("agent").and_then(Value::as_str) == Some(agent.id()))
            })
        else {
            // This date has no usage for the requested agent.
            continue;
        };

        let period = row.get("period").and_then(Value::as_str).ok_or_else(|| {
            CollectorError::VendorAdapter {
                vendor: VENDOR_ID.to_string(),
                details: "daily row is missing `period`".to_string(),
            }
        })?;
        let date = NaiveDate::parse_from_str(period, "%Y-%m-%d").map_err(|error| {
            CollectorError::VendorAdapter {
                vendor: VENDOR_ID.to_string(),
                details: format!("unparseable daily period {period:?}: {error}"),
            }
        })?;

        let models_used = string_array(row.get("modelsUsed"))?;
        let breakdowns = model_breakdowns(breakdown)?;
        let models_missing_pricing: Vec<ModelName> = breakdowns
            .iter()
            .filter(|entry| entry.missing_pricing)
            .map(|entry| entry.model.clone())
            .collect();
        // The vendor emits `totalCost` as a plain f64 that counts unpriced
        // models as zero; the record-level cost is only meaningful when at
        // least one priced model contributed.
        let any_priced = breakdowns.iter().any(|entry| !entry.missing_pricing);
        let cost = if any_priced {
            row.get("totalCost")
                .and_then(Value::as_f64)
                .and_then(CostNanoUsd::try_from_usd_f64)
        } else {
            None
        };

        // Reasoning/unclassified: the vendored rows carry the reasoning-like
        // extra tokens (codex reasoning, antigravity thinking per #1487,
        // opencode fallbacks) as `extraTotalTokens`. `unclassified` is the
        // saturating remainder against the agent-reported total; a violation
        // of the bucket invariant is flagged, never silently rewritten.
        let input_tokens = row_u64(row, "inputTokens")?;
        let output_tokens = row_u64(row, "outputTokens")?;
        let cache_creation_tokens = row_u64(row, "cacheCreationTokens")?;
        let cache_read_tokens = row_u64(row, "cacheReadTokens")?;
        let reasoning_tokens = row
            .get("extraTotalTokens")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let total_tokens = row_u64(row, "totalTokens")?;
        let four_buckets = input_tokens
            .checked_add(output_tokens)
            .and_then(|sum| sum.checked_add(cache_creation_tokens))
            .and_then(|sum| sum.checked_add(cache_read_tokens));
        let (unclassified_tokens, invariant_diag) = match four_buckets
            .and_then(|sum| sum.checked_add(reasoning_tokens))
            .and_then(|sum| total_tokens.checked_sub(sum))
        {
            Some(unclassified) => (unclassified, None),
            None => (
                0,
                Some(CollectionDiagnostic {
                    kind: DiagnosticKind::InvariantViolation,
                    file: None,
                    details: format!(
                        "token buckets plus reasoning exceed the reported total for {period}"
                    ),
                }),
            ),
        };

        records.push(UsageRecord {
            date,
            agent,
            input_tokens,
            output_tokens,
            cache_creation_tokens,
            cache_read_tokens,
            reasoning_tokens,
            unclassified_tokens,
            total_tokens,
            cost,
            models_used,
            model_breakdowns: breakdowns,
            models_missing_pricing,
        });
        diagnostics.extend(invariant_diag);
    }

    // Deterministic output regardless of vendor ordering.
    records.sort_by_key(|record| record.date);
    Ok((records, diagnostics))
}

fn row_u64(row: &Value, key: &str) -> Result<u64, CollectorError> {
    row.get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| CollectorError::VendorAdapter {
            vendor: VENDOR_ID.to_string(),
            details: format!("daily row field {key:?} is missing or not an unsigned integer"),
        })
}

fn string_array(value: Option<&Value>) -> Result<Vec<ModelName>, CollectorError> {
    match value {
        None => Ok(Vec::new()),
        Some(Value::Array(entries)) => {
            let mut names: Vec<ModelName> = entries
                .iter()
                .filter_map(|entry| entry.as_str().map(|name| ModelName(name.to_string())))
                .collect();
            names.sort();
            names.dedup();
            Ok(names)
        }
        Some(_) => Err(CollectorError::VendorAdapter {
            vendor: VENDOR_ID.to_string(),
            details: "expected an array of model names".to_string(),
        }),
    }
}

fn model_breakdowns(agent_row: &Value) -> Result<Vec<ModelBreakdown>, CollectorError> {
    let entries = agent_row
        .get("modelBreakdowns")
        .and_then(Value::as_array)
        .ok_or_else(|| CollectorError::VendorAdapter {
            vendor: VENDOR_ID.to_string(),
            details: "agent breakdown is missing `modelBreakdowns`".to_string(),
        })?;

    let mut breakdowns = Vec::with_capacity(entries.len());
    for entry in entries {
        let model = entry
            .get("modelName")
            .and_then(Value::as_str)
            .ok_or_else(|| CollectorError::VendorAdapter {
                vendor: VENDOR_ID.to_string(),
                details: "model breakdown is missing `modelName`".to_string(),
            })?;
        // The vendored core serializes `missingPricing` on model breakdowns
        // (0002 patch extension). It is authoritative: a missing-pricing
        // model's `cost` is a zero placeholder, never a priced zero.
        let missing_pricing = entry
            .get("missingPricing")
            .and_then(Value::as_bool)
            .ok_or_else(|| CollectorError::VendorAdapter {
                vendor: VENDOR_ID.to_string(),
                details: "model breakdown is missing `missingPricing`".to_string(),
            })?;
        // Reasoning-like extra tokens for this model (0002 patch: the core
        // serializes `extra_total_tokens` as `reasoningTokens`).
        let reasoning_tokens = entry
            .get("reasoningTokens")
            .and_then(Value::as_u64)
            .ok_or_else(|| CollectorError::VendorAdapter {
                vendor: VENDOR_ID.to_string(),
                details: "model breakdown is missing `reasoningTokens`".to_string(),
            })?;
        let cost = if missing_pricing {
            None
        } else {
            entry
                .get("cost")
                .and_then(Value::as_f64)
                .and_then(CostNanoUsd::try_from_usd_f64)
        };
        breakdowns.push(ModelBreakdown {
            model: ModelName(model.to_string()),
            input_tokens: breakdown_u64(entry, "inputTokens")?,
            output_tokens: breakdown_u64(entry, "outputTokens")?,
            cache_creation_tokens: breakdown_u64(entry, "cacheCreationTokens")?,
            cache_read_tokens: breakdown_u64(entry, "cacheReadTokens")?,
            reasoning_tokens,
            missing_pricing,
            cost,
        });
    }
    breakdowns.sort_by(|a, b| a.model.cmp(&b.model));
    Ok(breakdowns)
}

fn breakdown_u64(entry: &Value, key: &str) -> Result<u64, CollectorError> {
    entry
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| CollectorError::VendorAdapter {
            vendor: VENDOR_ID.to_string(),
            details: format!("model breakdown field {key:?} is missing or not an unsigned integer"),
        })
}
