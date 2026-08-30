//! Adapter from the vendored ccusage unified report to [`Collector`] types.
//!
//! This is the only module that converts vendor JSON into CAM-owned types.
//! The vendor seam used here is `ccusage_adapter_all::daily_report_json_by_agent`
//! (Gate 0 PoC entry point): it returns the JSON shape of
//! `ccusage daily --json --by-agent` for *all* registered agents. Each
//! [`AgentCollector`] extracts the breakdown rows for its own agent.
//!
//! Known vendor seam limitations (documented, Phase 2 prerequisites):
//! - One pass scans every agent's data roots, so unrelated agents'
//!   misconfiguration can fail a single-agent request.
//! - The vendor error type is an opaque string; classification relies on the
//!   documented "No valid … data directories" sentinel, pinned by contract
//!   tests.

use chrono::NaiveDate;
use serde_json::Value;

use super::{
    AgentKind, CollectRequest, CollectResult, Collector, CollectorError, CostNanoUsd,
    ModelBreakdown, ModelName, UsageRecord,
};

const VENDOR_ID: &str = "ccusage v20.0.20";

/// Sentinel the vendored claude adapter emits when no valid data directory is
/// configured. Pinned by `collector_contract.rs` — if upstream rewords it,
/// the contract test must be updated together with the mapping below.
const NO_VALID_DATA_DIR_SENTINEL: &str = "No valid ";

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
        match request.source {
            super::DataSource::Environment => {} // Exhaustive match: a future DataSource variant must be handled
                                                 // here explicitly, not silently ignored.
        }

        let report = ccusage_adapter_all::daily_report_json_by_agent(&vendor_shared_args(request))
            .map_err(|error| classify_vendor_error(request.agent, error.to_string()))?;
        from_vendor_report(&report, self.agent)
    }
}

/// Builds the vendor call arguments. `single_thread` is forced for
/// deterministic Phase 1 results; `offline` is mandatory (no network pricing).
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

fn classify_vendor_error(agent: AgentKind, message: String) -> CollectorError {
    let lowered = message.to_ascii_lowercase();
    if message.contains(NO_VALID_DATA_DIR_SENTINEL) && lowered.contains("data director") {
        CollectorError::SourceUnavailable {
            agent,
            details: message,
        }
    } else if lowered.contains("sqlite") || lowered.contains("database") {
        CollectorError::DatabaseQuery {
            agent,
            details: message,
        }
    } else {
        CollectorError::VendorAdapter {
            vendor: VENDOR_ID,
            details: message,
        }
    }
}

/// Converts the unified daily-by-agent report into per-agent typed records.
fn from_vendor_report(report: &Value, agent: AgentKind) -> Result<CollectResult, CollectorError> {
    let daily = report
        .get("daily")
        .and_then(Value::as_array)
        .ok_or_else(|| CollectorError::VendorAdapter {
            vendor: VENDOR_ID,
            details: "report is missing the `daily` array".to_string(),
        })?;

    let mut records = Vec::new();
    for row in daily {
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
                vendor: VENDOR_ID,
                details: "daily row is missing `period`".to_string(),
            }
        })?;
        let date = NaiveDate::parse_from_str(period, "%Y-%m-%d").map_err(|error| {
            CollectorError::VendorAdapter {
                vendor: VENDOR_ID,
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

        records.push(UsageRecord {
            date,
            agent,
            input_tokens: row_u64(row, "inputTokens")?,
            output_tokens: row_u64(row, "outputTokens")?,
            cache_creation_tokens: row_u64(row, "cacheCreationTokens")?,
            cache_read_tokens: row_u64(row, "cacheReadTokens")?,
            total_tokens: row_u64(row, "totalTokens")?,
            cost,
            models_used,
            model_breakdowns: breakdowns,
            models_missing_pricing,
        });
    }

    // Deterministic output regardless of vendor ordering.
    records.sort_by_key(|record| record.date);
    Ok(CollectResult { agent, records })
}

fn row_u64(row: &Value, key: &str) -> Result<u64, CollectorError> {
    row.get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| CollectorError::VendorAdapter {
            vendor: VENDOR_ID,
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
            vendor: VENDOR_ID,
            details: "expected an array of model names".to_string(),
        }),
    }
}

fn model_breakdowns(agent_row: &Value) -> Result<Vec<ModelBreakdown>, CollectorError> {
    let entries = agent_row
        .get("modelBreakdowns")
        .and_then(Value::as_array)
        .ok_or_else(|| CollectorError::VendorAdapter {
            vendor: VENDOR_ID,
            details: "agent breakdown is missing `modelBreakdowns`".to_string(),
        })?;

    let mut breakdowns = Vec::with_capacity(entries.len());
    for entry in entries {
        let model = entry
            .get("modelName")
            .and_then(Value::as_str)
            .ok_or_else(|| CollectorError::VendorAdapter {
                vendor: VENDOR_ID,
                details: "model breakdown is missing `modelName`".to_string(),
            })?;
        // The vendored core serializes `missingPricing` on model breakdowns
        // (0002 patch extension). It is authoritative: a missing-pricing
        // model's `cost` is a zero placeholder, never a priced zero.
        let missing_pricing = entry
            .get("missingPricing")
            .and_then(Value::as_bool)
            .ok_or_else(|| CollectorError::VendorAdapter {
                vendor: VENDOR_ID,
                details: "model breakdown is missing `missingPricing`".to_string(),
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
            vendor: VENDOR_ID,
            details: format!("model breakdown field {key:?} is missing or not an unsigned integer"),
        })
}
