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
/// string or backtrace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorV1 {
    pub code: ErrorCodeV1,
    pub message: String,
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
        Ok(CollectRequest {
            agent,
            window,
            timezone: super::TimeZoneSpec(self.timezone),
            source,
        })
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
    pub fn error(request_id: impl Into<String>, error: &CollectorError) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            request_id: request_id.into(),
            outcome: OutcomeV1::Error {
                error: ErrorV1 {
                    code: error_code(error),
                    message: error.to_string(),
                },
            },
        }
    }

    /// The domain error behind an error outcome, or `None` for success.
    pub fn as_error(&self) -> Option<CollectorError> {
        match &self.outcome {
            OutcomeV1::Ok { .. } => None,
            OutcomeV1::Error { error, .. } => Some(match error.code {
                ErrorCodeV1::InvalidRequest => CollectorError::InvalidRequest {
                    details: error.message.clone(),
                },
                ErrorCodeV1::SourceUnavailable => CollectorError::SourceUnavailable {
                    agent: AgentKind::Claude,
                    details: error.message.clone(),
                },
                ErrorCodeV1::CorruptData => CollectorError::CorruptData {
                    agent: AgentKind::Claude,
                    details: error.message.clone(),
                },
                ErrorCodeV1::DatabaseQuery => CollectorError::DatabaseQuery {
                    agent: AgentKind::Claude,
                    details: error.message.clone(),
                },
                ErrorCodeV1::VendorAdapter => CollectorError::VendorAdapter {
                    vendor: "vendor",
                    details: error.message.clone(),
                },
                ErrorCodeV1::PricingUnavailable => CollectorError::PricingUnavailable {
                    details: error.message.clone(),
                },
                ErrorCodeV1::Timeout => CollectorError::Timeout {
                    details: error.message.clone(),
                },
                ErrorCodeV1::Cancelled => CollectorError::Cancelled,
                ErrorCodeV1::Internal => CollectorError::Internal {
                    details: error.message.clone(),
                },
            }),
        }
    }
}

fn diag_kind_str(kind: crate::collector::DiagnosticKind) -> &'static str {
    match kind {
        crate::collector::DiagnosticKind::CorruptFile => "corrupt_file",
        crate::collector::DiagnosticKind::CorruptRecord => "corrupt_record",
        crate::collector::DiagnosticKind::DatabaseError => "database_error",
        crate::collector::DiagnosticKind::SourceUnreadable => "source_unreadable",
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
    }
}

fn u64_str(value: u64) -> String {
    value.to_string()
}

fn parse_u64(text: &str) -> Result<u64, CollectorError> {
    text.parse().map_err(|error| CollectorError::Internal {
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
    let agent = AgentKind::from_id(&record.agent).ok_or_else(|| CollectorError::Internal {
        details: format!("unknown agent id {:?}", record.agent),
    })?;
    let date = parse_date(&record.date)?;
    let cost = record
        .cost_nano_usd
        .as_deref()
        .map(|text| {
            text.parse::<i128>()
                .map(CostNanoUsd)
                .map_err(|error| CollectorError::Internal {
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
                            CollectorError::Internal {
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
