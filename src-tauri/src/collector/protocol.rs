//! Versioned, pure-data transport protocol for collector requests/responses
//! (`CollectorRequestV1` / `CollectorResponseV1`).
//!
//! These types exist for the Phase 2 worker boundary. They are pure data with
//! `serde` derives — no vendor types, no I/O, no process spawning. This phase
//! only implements and tests the protocol; nothing reads stdin/stdout yet.
//!
//! # Wire format rules
//!
//! - **Version gate.** `version` must equal [`PROTOCOL_VERSION`]; a reader
//!   that sees another version must reject the payload rather than guess.
//! - **Precision.** Token counts (`u64`) and costs (`CostNanoUsd`, i128
//!   nano-USD) are encoded as **decimal strings**, so every value round-trips
//!   losslessly even through consumers that would otherwise narrow JSON
//!   numbers to f64 (JavaScript `Number` loses integers above 2⁵³).
//! - **Unknown fields.** Within a protocol version, unknown fields are
//!   *ignored* on read (forward-compatible minor additions) and never
//!   produced by this reader version. Anything that changes or removes a
//!   field bumps the protocol version instead.
//! - **Errors.** Error responses carry a structured [`ErrorCodeV1`] plus a
//!   human-readable message (same content rules as
//!   [`crate::collector::CollectorError`]: no debug dumps, no stack traces,
//!   no secrets).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::{
    AgentKind, CollectRequest, CollectResult, CollectorError, CostNanoUsd, DataSource,
    ModelBreakdown, ModelName, UsageRecord,
};

/// Current protocol version. Bump on any breaking field change.
pub const PROTOCOL_VERSION: u32 = 1;

/// A versioned collection request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollectorRequestV1 {
    pub version: u32,
    /// Opaque correlation id echoed back on the response.
    pub request_id: String,
    /// Vendor agent id (see [`AgentKind::id`]).
    pub agent: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window: Option<DateWindowV1>,
    /// IANA time-zone name for daily bucketing.
    pub timezone: String,
    pub source: DataSourceV1,
}

/// Inclusive date window, both bounds `YYYY-MM-DD`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DateWindowV1 {
    pub start_inclusive: String,
    pub end_inclusive: String,
}

/// Data-source selection on the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DataSourceV1 {
    /// Vendor resolves the agent's roots from the process environment.
    Environment,
    /// Explicit data roots for the requested agent.
    Paths { roots: Vec<String> },
}

/// A versioned collection response: either a report or a structured error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollectorResponseV1 {
    pub version: u32,
    pub request_id: String,
    #[serde(flatten)]
    pub outcome: OutcomeV1,
}

/// Success/failure envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum OutcomeV1 {
    Ok { report: ReportV1 },
    Error { error: ErrorV1 },
}

/// Successful report payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportV1 {
    pub records: Vec<UsageRecordV1>,
    pub diagnostics: Vec<DiagnosticV1>,
}

/// Error payload. `message` is display-quality text; never a Rust `Debug`
/// string or backtrace. `agent` keeps the error's agent attribution (agent
/// id, when the classification carries one); `vendor` is present only for
/// vendor-classified errors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorV1 {
    pub code: ErrorCodeV1,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vendor: Option<String>,
}

/// Structured error codes, mirroring [`CollectorError`] classifications.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCodeV1 {
    InvalidRequest,
    SourceUnavailable,
    CorruptData,
    DatabaseQuery,
    VendorAdapter,
    PricingUnavailable,
    Timeout,
    Cancelled,
    Internal,
    Protocol,
}

/// Wire form of a daily aggregate record. Token counts and costs are decimal
/// strings for lossless transport.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageRecordV1 {
    pub date: String,
    pub agent: String,
    pub input_tokens: String,
    pub output_tokens: String,
    pub cache_creation_tokens: String,
    pub cache_read_tokens: String,
    pub reasoning_tokens: String,
    pub unclassified_tokens: String,
    pub total_tokens: String,
    pub cost_nano_usd: Option<String>,
    pub models_used: Vec<String>,
    pub model_breakdowns: Vec<ModelBreakdownV1>,
    pub models_missing_pricing: Vec<String>,
}

/// Wire form of a per-model breakdown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelBreakdownV1 {
    pub model: String,
    pub input_tokens: String,
    pub output_tokens: String,
    pub cache_creation_tokens: String,
    pub cache_read_tokens: String,
    pub reasoning_tokens: String,
    pub missing_pricing: bool,
    pub cost_nano_usd: Option<String>,
}

/// Wire form of a collection diagnostic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticV1 {
    pub kind: String,
    pub file: Option<String>,
    pub details: String,
}

impl CollectorRequestV1 {
    /// A V1 request for `agent` with UTC default timezone.
    pub fn new(request_id: impl Into<String>, agent: AgentKind) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            request_id: request_id.into(),
            agent: agent.id().to_string(),
            window: None,
            timezone: "UTC".to_string(),
            source: DataSourceV1::Environment,
        }
    }

    /// Converts the wire request into a domain request. Rejects wrong
    /// protocol versions, unknown agent ids, and malformed dates/paths.
    pub fn into_domain(self) -> Result<CollectRequest, CollectorError> {
        if self.version != PROTOCOL_VERSION {
            return Err(CollectorError::InvalidRequest {
                details: format!(
                    "unsupported protocol version {} (expected {PROTOCOL_VERSION})",
                    self.version
                ),
            });
        }
        let agent =
            AgentKind::from_id(&self.agent).ok_or_else(|| CollectorError::InvalidRequest {
                details: format!("unknown agent id {:?}", self.agent),
            })?;
        let window = self
            .window
            .map(|window| {
                let start = parse_date(&window.start_inclusive)?;
                let end = parse_date(&window.end_inclusive)?;
                super::CollectWindow::new(start, end)
            })
            .transpose()?;
        let source = match self.source {
            DataSourceV1::Environment => DataSource::Environment,
            DataSourceV1::Paths { roots } => {
                DataSource::Paths(roots.into_iter().map(PathBuf::from).collect())
            }
        };
        let domain = CollectRequest {
            agent,
            window,
            timezone: super::TimeZoneSpec(self.timezone),
            source,
        };
        domain.validate()?;
        Ok(domain)
    }
}

fn parse_date(text: &str) -> Result<chrono::NaiveDate, CollectorError> {
    chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").map_err(|error| {
        CollectorError::InvalidRequest {
            details: format!("invalid date {text:?}: {error}"),
        }
    })
}

impl CollectorResponseV1 {
    /// Builds a success response from a domain result.
    pub fn ok(request_id: impl Into<String>, result: &CollectResult) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            request_id: request_id.into(),
            outcome: OutcomeV1::Ok {
                report: ReportV1 {
                    records: result.records.iter().map(record_to_v1).collect(),
                    diagnostics: result
                        .diagnostics
                        .iter()
                        .map(|diag| DiagnosticV1 {
                            kind: diag_kind_str(diag.kind).to_string(),
                            file: diag.file.clone(),
                            details: diag.details.clone(),
                        })
                        .collect(),
                },
            },
        }
    }

    /// Builds an error response from a domain error. The message is the
    /// error's `Display` output — clean text, no debug dumps or backtraces.
    /// Agent attribution and vendor labels travel in structured fields so the
    /// receiver can rebuild the real classification.
    pub fn error(request_id: impl Into<String>, error: &CollectorError) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            request_id: request_id.into(),
            outcome: OutcomeV1::Error {
                error: ErrorV1 {
                    code: error_code(error),
                    message: error.to_string(),
                    agent: error_agent(error).map(|agent| agent.id().to_string()),
                    vendor: error_vendor(error),
                },
            },
        }
    }

    /// Parses, version-gates, and validates a wire response. Malformed
    /// payloads map to [`CollectorError::Protocol`] — never to a collector
    /// business error.
    pub fn from_wire(json: &str) -> Result<Self, CollectorError> {
        let parsed: Self =
            serde_json::from_str(json).map_err(|error| CollectorError::Protocol {
                details: format!("malformed collector response: {error}"),
            })?;
        parsed.validate()?;
        Ok(parsed)
    }

    /// Validates an already-deserialized response: version gate first, then
    /// request id, agent ids, dates, integer/cost encodings, diagnostic kinds
    /// and the record token invariant.
    pub fn validate(&self) -> Result<(), CollectorError> {
        if self.version != PROTOCOL_VERSION {
            return Err(CollectorError::Protocol {
                details: format!(
                    "unsupported protocol version {} (expected {PROTOCOL_VERSION})",
                    self.version
                ),
            });
        }
        if self.request_id.is_empty() || self.request_id.len() > super::MAX_REQUEST_ID_LEN {
            return Err(CollectorError::Protocol {
                details: format!(
                    "request id must be 1..={}_LEN bytes",
                    super::MAX_REQUEST_ID_LEN
                ),
            });
        }
        let protocol_error = |details: String| CollectorError::Protocol { details };
        match &self.outcome {
            OutcomeV1::Ok { report } => {
                for record in &report.records {
                    let rebuilt = record_from_v1(record)
                        .map_err(|error| protocol_error(error.to_string()))?;
                    if !super::token_bucket_invariant_holds(&rebuilt)
                        && !report
                            .diagnostics
                            .iter()
                            .any(|diag| diag.kind == "invariant_violation")
                    {
                        return Err(protocol_error(format!(
                            "record for {} {} violates the token bucket invariant",
                            record.date, record.agent
                        )));
                    }
                }
                for diag in &report.diagnostics {
                    match diag_kind_from_str(&diag.kind) {
                        Some(_) => {}
                        None => {
                            return Err(protocol_error(format!(
                                "unknown diagnostic kind {:?}",
                                diag.kind
                            )));
                        }
                    }
                }
                Ok(())
            }
            OutcomeV1::Error { error } => {
                if error.message.is_empty() {
                    return Err(protocol_error(
                        "error response carries an empty message".to_string(),
                    ));
                }
                if let Some(agent) = &error.agent {
                    if super::AgentKind::from_id(agent).is_none() {
                        return Err(protocol_error(format!("unknown agent id {agent:?}")));
                    }
                }
                Ok(())
            }
        }
    }

    /// The domain error behind an error outcome, or `None` for success.
    ///
    /// Agent attribution comes from the payload's structured `agent` field;
    /// agent-classified errors without a known agent degrade to
    /// [`CollectorError::Protocol`] rather than being misattributed.
    pub fn as_error(&self) -> Option<CollectorError> {
        match &self.outcome {
            OutcomeV1::Ok { .. } => None,
            OutcomeV1::Error { error, .. } => {
                let agent = error.agent.as_deref().and_then(super::AgentKind::from_id);
                let message = error.message.clone();
                let attribution_lost = || {
                    Some(CollectorError::Protocol {
                        details: format!("error response lost its agent attribution: {message}"),
                    })
                };
                Some(match error.code {
                    ErrorCodeV1::InvalidRequest => {
                        CollectorError::InvalidRequest { details: message }
                    }
                    ErrorCodeV1::SourceUnavailable => match agent {
                        Some(agent) => CollectorError::SourceUnavailable {
                            agent,
                            details: message,
                        },
                        None => return attribution_lost(),
                    },
                    ErrorCodeV1::CorruptData => match agent {
                        Some(agent) => CollectorError::CorruptData {
                            agent,
                            details: message,
                        },
                        None => return attribution_lost(),
                    },
                    ErrorCodeV1::DatabaseQuery => match agent {
                        Some(agent) => CollectorError::DatabaseQuery {
                            agent,
                            details: message,
                        },
                        None => return attribution_lost(),
                    },
                    ErrorCodeV1::VendorAdapter => CollectorError::VendorAdapter {
                        vendor: error.vendor.clone().unwrap_or_else(|| "vendor".to_string()),
                        details: message,
                    },
                    ErrorCodeV1::PricingUnavailable => {
                        CollectorError::PricingUnavailable { details: message }
                    }
                    ErrorCodeV1::Timeout => CollectorError::Timeout { details: message },
                    ErrorCodeV1::Cancelled => CollectorError::Cancelled,
                    ErrorCodeV1::Internal => CollectorError::Internal { details: message },
                    ErrorCodeV1::Protocol => CollectorError::Protocol { details: message },
                })
            }
        }
    }
}

fn diag_kind_str(kind: crate::collector::DiagnosticKind) -> &'static str {
    match kind {
        crate::collector::DiagnosticKind::CorruptFile => "corrupt_file",
        crate::collector::DiagnosticKind::CorruptRecord => "corrupt_record",
        crate::collector::DiagnosticKind::DatabaseError => "database_error",
        crate::collector::DiagnosticKind::SourceUnreadable => "source_unreadable",
        crate::collector::DiagnosticKind::SourceChanged => "source_changed",
        crate::collector::DiagnosticKind::InvariantViolation => "invariant_violation",
    }
}

fn diag_kind_from_str(kind: &str) -> Option<crate::collector::DiagnosticKind> {
    match kind {
        "corrupt_file" => Some(crate::collector::DiagnosticKind::CorruptFile),
        "corrupt_record" => Some(crate::collector::DiagnosticKind::CorruptRecord),
        "database_error" => Some(crate::collector::DiagnosticKind::DatabaseError),
        "source_unreadable" => Some(crate::collector::DiagnosticKind::SourceUnreadable),
        "source_changed" => Some(crate::collector::DiagnosticKind::SourceChanged),
        "invariant_violation" => Some(crate::collector::DiagnosticKind::InvariantViolation),
        _ => None,
    }
}

fn error_agent(error: &CollectorError) -> Option<AgentKind> {
    match error {
        CollectorError::SourceUnavailable { agent, .. }
        | CollectorError::CorruptData { agent, .. }
        | CollectorError::DatabaseQuery { agent, .. } => Some(*agent),
        _ => None,
    }
}

fn error_vendor(error: &CollectorError) -> Option<String> {
    match error {
        CollectorError::VendorAdapter { vendor, .. } => Some(vendor.clone()),
        _ => None,
    }
}

fn error_code(error: &CollectorError) -> ErrorCodeV1 {
    match error {
        CollectorError::InvalidRequest { .. } => ErrorCodeV1::InvalidRequest,
        CollectorError::SourceUnavailable { .. } => ErrorCodeV1::SourceUnavailable,
        CollectorError::CorruptData { .. } => ErrorCodeV1::CorruptData,
        CollectorError::DatabaseQuery { .. } => ErrorCodeV1::DatabaseQuery,
        CollectorError::VendorAdapter { .. } => ErrorCodeV1::VendorAdapter,
        CollectorError::PricingUnavailable { .. } => ErrorCodeV1::PricingUnavailable,
        CollectorError::Timeout { .. } => ErrorCodeV1::Timeout,
        CollectorError::Cancelled => ErrorCodeV1::Cancelled,
        CollectorError::Internal { .. } => ErrorCodeV1::Internal,
        CollectorError::Protocol { .. } => ErrorCodeV1::Protocol,
    }
}

fn u64_str(value: u64) -> String {
    value.to_string()
}

fn parse_u64(text: &str) -> Result<u64, CollectorError> {
    text.parse().map_err(|error| CollectorError::Protocol {
        details: format!("malformed token count {text:?}: {error}"),
    })
}

fn i128_str(value: i128) -> String {
    value.to_string()
}

fn record_to_v1(record: &UsageRecord) -> UsageRecordV1 {
    UsageRecordV1 {
        date: record.date.format("%Y-%m-%d").to_string(),
        agent: record.agent.id().to_string(),
        input_tokens: u64_str(record.input_tokens),
        output_tokens: u64_str(record.output_tokens),
        cache_creation_tokens: u64_str(record.cache_creation_tokens),
        cache_read_tokens: u64_str(record.cache_read_tokens),
        reasoning_tokens: u64_str(record.reasoning_tokens),
        unclassified_tokens: u64_str(record.unclassified_tokens),
        total_tokens: u64_str(record.total_tokens),
        cost_nano_usd: record.cost.map(|cost| i128_str(cost.as_nano_usd())),
        models_used: record.models_used.iter().map(|m| m.0.clone()).collect(),
        model_breakdowns: record
            .model_breakdowns
            .iter()
            .map(|breakdown| ModelBreakdownV1 {
                model: breakdown.model.0.clone(),
                input_tokens: u64_str(breakdown.input_tokens),
                output_tokens: u64_str(breakdown.output_tokens),
                cache_creation_tokens: u64_str(breakdown.cache_creation_tokens),
                cache_read_tokens: u64_str(breakdown.cache_read_tokens),
                reasoning_tokens: u64_str(breakdown.reasoning_tokens),
                missing_pricing: breakdown.missing_pricing,
                cost_nano_usd: breakdown.cost.map(|cost| i128_str(cost.as_nano_usd())),
            })
            .collect(),
        models_missing_pricing: record
            .models_missing_pricing
            .iter()
            .map(|m| m.0.clone())
            .collect(),
    }
}

/// Rebuilds a domain record from its wire form (used by round-trip tests and
/// by the Phase 2 worker when it must hand domain types to the UI layer).
pub fn record_from_v1(record: &UsageRecordV1) -> Result<UsageRecord, CollectorError> {
    let agent = AgentKind::from_id(&record.agent).ok_or_else(|| CollectorError::Protocol {
        details: format!("unknown agent id {:?}", record.agent),
    })?;
    let date = parse_date(&record.date)?;
    let cost = record
        .cost_nano_usd
        .as_deref()
        .map(|text| {
            text.parse::<i128>()
                .map(CostNanoUsd)
                .map_err(|error| CollectorError::Protocol {
                    details: format!("malformed cost {text:?}: {error}"),
                })
        })
        .transpose()?;
    Ok(UsageRecord {
        date,
        agent,
        input_tokens: parse_u64(&record.input_tokens)?,
        output_tokens: parse_u64(&record.output_tokens)?,
        cache_creation_tokens: parse_u64(&record.cache_creation_tokens)?,
        cache_read_tokens: parse_u64(&record.cache_read_tokens)?,
        reasoning_tokens: parse_u64(&record.reasoning_tokens)?,
        unclassified_tokens: parse_u64(&record.unclassified_tokens)?,
        total_tokens: parse_u64(&record.total_tokens)?,
        cost,
        models_used: record
            .models_used
            .iter()
            .map(|m| ModelName(m.clone()))
            .collect(),
        model_breakdowns: record
            .model_breakdowns
            .iter()
            .map(|breakdown| {
                let cost = breakdown
                    .cost_nano_usd
                    .as_deref()
                    .map(|text| {
                        text.parse::<i128>().map(CostNanoUsd).map_err(|error| {
                            CollectorError::Protocol {
                                details: format!("malformed cost {text:?}: {error}"),
                            }
                        })
                    })
                    .transpose()?;
                Ok(ModelBreakdown {
                    model: ModelName(breakdown.model.clone()),
                    input_tokens: parse_u64(&breakdown.input_tokens)?,
                    output_tokens: parse_u64(&breakdown.output_tokens)?,
                    cache_creation_tokens: parse_u64(&breakdown.cache_creation_tokens)?,
                    cache_read_tokens: parse_u64(&breakdown.cache_read_tokens)?,
                    reasoning_tokens: parse_u64(&breakdown.reasoning_tokens)?,
                    missing_pricing: breakdown.missing_pricing,
                    cost,
                })
            })
            .collect::<Result<Vec<_>, CollectorError>>()?,
        models_missing_pricing: record
            .models_missing_pricing
            .iter()
            .map(|m| ModelName(m.clone()))
            .collect(),
    })
}
