//! Error taxonomy for the collector boundary.
//!
//! Every variant carries short, self-contained details: no secrets, no raw log
//! dumps, no user file contents. Details may name data *directories* (they are
//! local paths the user already owns); they never leave the machine except via
//! user-initiated bug reports.
//!
//! Mapping contract (see `docs/V0.3_PHASE1_COLLECTOR_DESIGN.md`): vendor
//! failures arrive as an opaque string (`CliError`). The adapter maps the one
//! documented sentinel (missing data directories) to
//! [`CollectorError::SourceUnavailable`] and everything else to
//! [`CollectorError::VendorAdapter`], pinned by contract tests so an upstream
//! wording change fails loudly instead of misclassifying silently.

use std::fmt;

use super::AgentKind;

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CollectorError {
    /// The request itself is invalid (inverted window, agent/collector
    /// mismatch, unsupported data source).
    InvalidRequest { details: String },
    /// A data source root does not exist or is not a valid agent data
    /// directory.
    SourceUnavailable { agent: AgentKind, details: String },
    /// Agent data exists but could not be parsed (malformed records,
    /// undecodable files).
    CorruptData { agent: AgentKind, details: String },
    /// A SQLite data source could not be opened or queried.
    DatabaseQuery { agent: AgentKind, details: String },
    /// The vendored engine failed for a reason outside CAM's control (CLI
    /// argument rejection, internal loader error, unexpected output shape).
    VendorAdapter {
        vendor: &'static str,
        details: String,
    },
    /// Pricing information itself could not be loaded. (Per-model missing
    /// pricing is *not* an error — it surfaces as `cost: None` on records.)
    PricingUnavailable { details: String },
    /// The collection did not finish within its budget.
    Timeout { details: String },
    /// The collection was cancelled before completion.
    Cancelled,
    /// Unexpected internal failure (invariant violation, bad adapter state).
    Internal { details: String },
}

impl fmt::Display for CollectorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CollectorError::InvalidRequest { details } => {
                write!(f, "invalid collect request: {details}")
            }
            CollectorError::SourceUnavailable { agent, details } => {
                write!(
                    f,
                    "data source unavailable for {}: {details}",
                    agent.label()
                )
            }
            CollectorError::CorruptData { agent, details } => {
                write!(f, "corrupt agent data for {}: {details}", agent.label())
            }
            CollectorError::DatabaseQuery { agent, details } => {
                write!(f, "database query failed for {}: {details}", agent.label())
            }
            CollectorError::VendorAdapter { vendor, details } => {
                write!(f, "vendor adapter failed ({vendor}): {details}")
            }
            CollectorError::PricingUnavailable { details } => {
                write!(f, "pricing information unavailable: {details}")
            }
            CollectorError::Timeout { details } => write!(f, "collection timed out: {details}"),
            CollectorError::Cancelled => write!(f, "collection cancelled"),
            CollectorError::Internal { details } => {
                write!(f, "internal collector error: {details}")
            }
        }
    }
}

impl std::error::Error for CollectorError {}
