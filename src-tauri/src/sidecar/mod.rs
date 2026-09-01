//! The usage-data adapter layer.
//!
//! Phase 5 removed the external ccusage sidecar supply chain: the module name
//! is historical (the adapter was built to normalize the v0.2 sidecar JSON).
//! Two entry points remain:
//!
//! - [`adapter::normalize_snapshot`] — the PRODUCTION path: folds the v0.3
//!   batch worker's typed snapshot into the public `UsageSummary`.
//! - [`adapter::normalize_reports`] — the v0.2 sidecar-JSON decoder, kept ONLY
//!   as part of the opt-in dev shadow harness (`tests/sidecar_shadow.rs`,
//!   `tests/shadow17.rs`) that compares a pinned external sidecar against the
//!   worker for upgrade audits. It is not reachable from production code; the
//!   sidecar binaries themselves are no longer staged, bundled, or downloaded.

pub mod adapter;

pub use adapter::{normalize_reports, normalize_snapshot};
