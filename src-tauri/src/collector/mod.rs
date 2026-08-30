//! Typed in-process collection boundary over the vendored ccusage sources.
//!
//! # Responsibilities
//!
//! This module is the *only* place where CAM code may touch the vendored
//! ccusage crates. It converts vendor output into CAM-owned types so that the
//! rest of the application (and future worker/adapter layers) never depends on
//! vendor crates, vendor JSON shapes, CLI arguments, or stdout parsing.
//!
//! # Boundaries (Phase 1)
//!
//! - **Not the production path.** Production collection still runs the ccusage
//!   sidecar executable (`src-tauri/src/sidecar`). This API is exercised by
//!   contract tests only until Phase 2 switches the call chain.
//! - **Record granularity.** One [`UsageRecord`] per *daily aggregate per
//!   agent* — the granularity the vendored unified report provides reliably.
//!   Session-level records are a later extension.
//! - **Data sources.** [`DataSource::Environment`] resolves each agent's data
//!   root from the process environment (`CLAUDE_CONFIG_DIR`,
//!   `ANTIGRAVITY_DATA_DIR`, …), exactly like the vendored CLI. The vendor
//!   seam scans *all* registered agents in one pass, so a misconfigured
//!   directory of an *unrelated* agent fails every collector request; per-agent
//!   isolation requires a vendor-side per-agent load API (Phase 2
//!   prerequisite, see `docs/V0.3_PHASE1_COLLECTOR_DESIGN.md`).
//! - **No vendor types leak.** Public items here reference nothing from
//!   `ccusage_*` crates. `serde_json::Value` appears only inside
//!   [`collector::ccusage`], never in signatures.
//! - **Money precision.** Costs cross the boundary once, as
//!   [`CostNanoUsd`] (fixed-point i128 nano-USD), never as bare `f64`.
//!   Tokens are `u64` throughout.
//! - **Time semantics.** Windows are date-granular (agent-local calendar days)
//!   with **inclusive** bounds on both ends; the timezone is explicit on every
//!   request and defaults to UTC.
//! - **Determinism.** Records are sorted by date ascending; model lists are
//!   sorted and deduplicated. `single_thread` is forced in the vendor call so
//!   Phase 1 results do not depend on task scheduling.
//! - **Null-vs-zero contract.** `cost: None` means "pricing unavailable or not
//!   computed" — never a computed zero. `Some(CostNanoUsd(0))` is a genuine
//!   zero from a priced model. Missing models stay in the record with their
//!   name and `None` cost; nothing is fabricated.

use std::fmt;

use chrono::NaiveDate;

pub mod ccusage;

mod error;

pub use error::CollectorError;

/// Which agent's usage a collector reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AgentKind {
    Claude,
    Antigravity,
}

impl AgentKind {
    /// The agent identifier used by the vendored unified report.
    pub fn id(self) -> &'static str {
        match self {
            AgentKind::Claude => "claude",
            AgentKind::Antigravity => "antigravity",
        }
    }

    /// Human-facing display name (matches the vendored agent labels).
    pub fn label(self) -> &'static str {
        match self {
            AgentKind::Claude => "Claude",
            AgentKind::Antigravity => "Antigravity",
        }
    }
}

/// Alias for [`AgentKind`] at collector-registration sites: which collector
/// implementation is behind a given contract object.
pub type CollectorKind = AgentKind;

/// Inclusive date window for a collection, in the request's timezone.
///
/// Both bounds are inclusive: usage on `start_inclusive` and on
/// `end_inclusive` is part of the result. Construct with [`CollectWindow::new`],
/// which rejects inverted ranges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CollectWindow {
    pub start_inclusive: NaiveDate,
    pub end_inclusive: NaiveDate,
}

impl CollectWindow {
    pub fn new(
        start_inclusive: NaiveDate,
        end_inclusive: NaiveDate,
    ) -> Result<Self, CollectorError> {
        if start_inclusive > end_inclusive {
            return Err(CollectorError::InvalidRequest {
                details: format!("window start {start_inclusive} is after end {end_inclusive}"),
            });
        }
        Ok(Self {
            start_inclusive,
            end_inclusive,
        })
    }
}

/// Where a collector reads agent data from.
///
/// Phase 1 supports only process-environment resolution (the vendored CLI
/// contract). Path overrides will be added when the vendor seam accepts them
/// without process-global env mutation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DataSource {
    /// Resolve each agent's data root from the process environment
    /// (`CLAUDE_CONFIG_DIR`, `ANTIGRAVITY_DATA_DIR`, …). Missing roots are
    /// reported per the vendor's own semantics; an agent with no data yields
    /// an empty successful result, not an error.
    Environment,
}

/// IANA time-zone name used for daily bucketing, e.g. `"UTC"`.
///
/// Validated by the vendored engine; an invalid name surfaces as
/// [`CollectorError::VendorAdapter`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeZoneSpec(pub String);

impl TimeZoneSpec {
    pub fn utc() -> Self {
        Self("UTC".to_string())
    }
}

impl fmt::Display for TimeZoneSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A collection request against one agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectRequest {
    /// The agent this request targets. Collectors reject requests naming a
    /// different agent.
    pub agent: AgentKind,
    /// Date window, inclusive on both ends. `None` collects all recorded
    /// history.
    pub window: Option<CollectWindow>,
    /// Timezone for daily bucketing. Defaults to UTC.
    pub timezone: TimeZoneSpec,
    /// Data source resolution strategy.
    pub source: DataSource,
}

impl CollectRequest {
    pub fn new(agent: AgentKind) -> Self {
        Self {
            agent,
            window: None,
            timezone: TimeZoneSpec::utc(),
            source: DataSource::Environment,
        }
    }

    pub fn with_window(mut self, window: CollectWindow) -> Self {
        self.window = Some(window);
        self
    }

    pub fn with_timezone(mut self, timezone: TimeZoneSpec) -> Self {
        self.timezone = timezone;
        self
    }
}

/// Stable model identifier as reported by the agent's own logs.
///
/// Unknown models pass through unchanged — the API never rewrites or drops
/// model names it does not recognise.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ModelName(pub String);

impl fmt::Display for ModelName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A cost in USD as fixed-point nano-dollars (10⁻⁹ USD), i128.
///
/// Created once at the vendor adapter boundary from the vendor's IEEE-754
/// value (rounded half-away-from-zero); all downstream code works on the
/// integer only. `i128` overflows are unreachable for realistic totals
/// (≈10¹¹ years of $1M/day), and float→int casts saturate rather than wrap.
/// `CostNanoUsd(0)` is a genuine priced zero; `None` on a record field means
/// pricing was unavailable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CostNanoUsd(i128);

impl CostNanoUsd {
    /// Converts a vendor-computed USD value. Returns `None` for non-finite
    /// input (treated as "unavailable", matching the null-cost contract).
    pub fn try_from_usd_f64(value: f64) -> Option<Self> {
        if !value.is_finite() {
            return None;
        }
        // f64::round is half-away-from-zero; the `as` cast saturates.
        Some(Self((value * 1e9).round() as i128))
    }

    pub const ZERO: Self = Self(0);

    pub fn as_nano_usd(self) -> i128 {
        self.0
    }
}

impl fmt::Display for CostNanoUsd {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Exact fixed-point rendering: sign, integer part, 9 fractional digits
        // (trailing zeros trimmed) — no floating point involved.
        let sign = if self.0 < 0 { "-" } else { "" };
        let magnitude = self.0.unsigned_abs();
        let whole = magnitude / 1_000_000_000;
        let nano = magnitude % 1_000_000_000;
        if nano == 0 {
            write!(f, "{sign}{whole}")
        } else {
            let fraction = format!("{nano:09}");
            write!(f, "{sign}{whole}.{}", fraction.trim_end_matches('0'))
        }
    }
}

/// Per-model token/cost detail for one daily record.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ModelBreakdown {
    pub model: ModelName,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_tokens: u64,
    pub cache_read_tokens: u64,
    /// The vendor has no pricing entry for this model; its raw cost is a zero
    /// placeholder and `cost` here is `None`.
    pub missing_pricing: bool,
    /// `None` = pricing unavailable for this model. Never a fabricated zero.
    pub cost: Option<CostNanoUsd>,
}

/// One daily aggregate row for one agent.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct UsageRecord {
    /// Calendar day (in the request's timezone) this aggregate covers.
    pub date: NaiveDate,
    pub agent: AgentKind,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_tokens: u64,
    pub cache_read_tokens: u64,
    pub total_tokens: u64,
    /// `None` = pricing unavailable (no priced model contributed). See the
    /// null-vs-zero contract in the module docs.
    pub cost: Option<CostNanoUsd>,
    /// Every model contributing to this record, sorted, deduplicated.
    pub models_used: Vec<ModelName>,
    /// Per-model detail, sorted by model name.
    pub model_breakdowns: Vec<ModelBreakdown>,
    /// Models that contributed tokens but have no pricing data, sorted.
    pub models_missing_pricing: Vec<ModelName>,
}

/// Successful collection output for one agent.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct CollectResult {
    pub agent: AgentKind,
    /// Daily records, sorted by date ascending. Empty when the agent has no
    /// data in the window (a successful empty result, never an error).
    pub records: Vec<UsageRecord>,
}

impl CollectResult {
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

/// The collection contract every agent collector implements.
pub trait Collector {
    /// The agent this collector reads.
    fn agent(&self) -> AgentKind;

    /// Runs one collection. Must be deterministic for identical requests and
    /// must not mutate global state beyond reading the process environment
    /// (see [`DataSource::Environment`]).
    fn collect(&self, request: &CollectRequest) -> Result<CollectResult, CollectorError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cost_nano_usd_converts_with_documented_rounding() {
        assert_eq!(
            CostNanoUsd::try_from_usd_f64(0.021892),
            Some(CostNanoUsd(21_892_000))
        );
        assert_eq!(CostNanoUsd::try_from_usd_f64(0.0), Some(CostNanoUsd::ZERO));
        assert_eq!(CostNanoUsd::try_from_usd_f64(0.5e-9), Some(CostNanoUsd(1)));
        assert_eq!(
            CostNanoUsd::try_from_usd_f64(-0.5e-9),
            Some(CostNanoUsd(-1))
        );
        // Half-away-from-zero on the nano boundary.
        assert_eq!(CostNanoUsd::try_from_usd_f64(1.5e-9), Some(CostNanoUsd(2)));
        assert_eq!(
            CostNanoUsd::try_from_usd_f64(-1.5e-9),
            Some(CostNanoUsd(-2))
        );
        // Non-finite input is "unavailable", never zero.
        assert_eq!(CostNanoUsd::try_from_usd_f64(f64::NAN), None);
        assert_eq!(CostNanoUsd::try_from_usd_f64(f64::INFINITY), None);
    }

    #[test]
    fn cost_display_renders_exact_fixed_point() {
        assert_eq!(CostNanoUsd(21_892_000).to_string(), "0.021892");
        assert_eq!(CostNanoUsd(0).to_string(), "0");
        assert_eq!(CostNanoUsd(1).to_string(), "0.000000001");
        assert_eq!(CostNanoUsd(-12_500_000_000).to_string(), "-12.5");
        assert_eq!(
            CostNanoUsd(1_234_567_891_234_567_891).to_string(),
            "1234567891.234567891"
        );
    }

    #[test]
    fn collect_window_rejects_inverted_ranges() {
        let start = NaiveDate::from_ymd_opt(2026, 1, 3).unwrap();
        let end = NaiveDate::from_ymd_opt(2026, 1, 2).unwrap();
        assert!(matches!(
            CollectWindow::new(start, end),
            Err(CollectorError::InvalidRequest { .. })
        ));
        assert!(
            CollectWindow::new(end, end).is_ok(),
            "single-day window is valid"
        );
    }
}
