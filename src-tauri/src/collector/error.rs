//! Error taxonomy for the collector boundary.
//!
//! Every variant carries short, self-contained details: no secrets, no raw log
//! dumps, no user file contents. Details may name data *directories* (they are
//! local paths the user already owns); they never leave the machine except via
//! user-initiated bug reports.
//!
//! Mapping contract (see `docs/V0.3_PHASE1_COLLECTOR_DESIGN.md`): vendor
//! failures are classified structurally via `load_context::LoadFailureKind`
//! (raised by the vendored adapters at the failure site and consumed by the
//! per-agent collector entry point) — never by matching error message text.
//! The remaining string-based vendor surface is mapped to
//! [`CollectorError::VendorAdapter`].

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
    /// The vendored engine failed for a reason outside CAM's control (loader
    /// internal error, unexpected output shape).
    VendorAdapter { vendor: String, details: String },
    /// Pricing information itself could not be loaded. (Per-model missing
    /// pricing is *not* an error — it surfaces as `cost: None` on records.)
    PricingUnavailable { details: String },
    /// The collection did not finish within its budget.
    Timeout { details: String },
    /// The collection was cancelled before completion.
    Cancelled,
    /// Unexpected internal failure (invariant violation, bad adapter state).
    Internal { details: String },
    /// The V1 transport payload itself is malformed (wrong version, bad
    /// envelope, unparseable values). This is a protocol-level failure, not a
    /// collector business condition — it must never be presented as if the
    /// agent data itself were at fault.
    Protocol { details: String },
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
                write!(f, "vendor adapter failed ({}): {details}", vendor)
            }
            CollectorError::PricingUnavailable { details } => {
                write!(f, "pricing information unavailable: {details}")
            }
            CollectorError::Timeout { details } => write!(f, "collection timed out: {details}"),
            CollectorError::Cancelled => write!(f, "collection cancelled"),
            CollectorError::Internal { details } => {
                write!(f, "internal collector error: {details}")
            }
            CollectorError::Protocol { details } => {
                write!(f, "collector protocol violation: {details}")
            }
        }
    }
}

impl std::error::Error for CollectorError {}
