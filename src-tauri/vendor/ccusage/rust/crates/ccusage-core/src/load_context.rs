//! Load-scoped context for in-process collectors: structured diagnostics,
//! structured load failures, and per-load data-root overrides.
//!
//! The three stores here are **scoped to a single load call**: the in-process
//! collector entry point (in `ccusage-adapter-all`) clears them when a load
//! begins and drains them when it ends, so nothing leaks between loads. They
//! are process-global rather than thread-local because adapter loaders run on
//! worker threads while the entry point consumes the results. Loads that use
//! this module must therefore run one at a time and single-threaded — the
//! collector entry point enforces `single_thread` and documents the contract.
//! This is deliberately still *not* environment state: nothing here outlives
//! a load or is visible to code outside the load path.
//!
//! Diagnostics carry short, self-contained details only — no log dumps, no
//! file contents, no secrets. File paths are limited to the skipped source
//! file's path, which the local user already owns.

use std::{
    path::PathBuf,
    sync::Mutex,
};

/// Category of a recoverable problem recorded while loading agent data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadDiagKind {
    /// A source file existed but could not be parsed; it was skipped.
    CorruptFile,
    /// A record inside a readable file was skipped (malformed line/entry).
    CorruptRecord,
    /// A SQLite source could not be opened or queried; it was skipped.
    DatabaseError,
    /// A data source existed but could not be read (permissions, I/O error).
    SourceUnreadable,
}

/// One recoverable problem observed while loading a single agent's data.
#[derive(Debug, Clone)]
pub struct LoadDiag {
    pub agent: &'static str,
    pub kind: LoadDiagKind,
    /// Path of the skipped source, when the problem is file-scoped.
    pub file: Option<String>,
    pub details: String,
}

/// Structured kind of a fatal load failure. The in-process collector API
/// surfaces these directly so callers never classify by error *text*.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadFailureKind {
    /// The agent's data root is missing or not a valid agent data directory.
    SourceUnavailable,
    /// A configured source is malformed (bad config value, invalid path shape).
    InvalidConfig,
    /// A SQLite data source failed in a way that is not skippable.
    Database,
    /// Anything else: internal invariant violation or unexpected engine state.
    Internal,
}

/// A fatal load failure with a machine-readable kind.
#[derive(Debug, Clone)]
pub struct LoadFailure {
    pub kind: LoadFailureKind,
    pub details: String,
}

static DIAGNOSTICS: Mutex<Vec<LoadDiag>> = Mutex::new(Vec::new());
static FAILURE: Mutex<Option<LoadFailure>> = Mutex::new(None);
static ROOT_OVERRIDE: Mutex<Option<(&'static str, Vec<PathBuf>)>> = Mutex::new(None);

fn lock<T>(slot: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Records a recoverable problem for the current load.
pub fn record(diag: LoadDiag) {
    lock(&DIAGNOSTICS).push(diag);
}

/// Takes every diagnostic recorded during the current load.
pub fn drain_diags() -> Vec<LoadDiag> {
    std::mem::take(&mut lock(&DIAGNOSTICS))
}

/// Raises a structured failure for the current load. Callers that need to
/// keep the existing `Result<_, CliError>` flow short-circuiting should
/// return their normal error too; the collector entry point consults this
/// slot first when mapping failures.
pub fn raise_failure(failure: LoadFailure) {
    *lock(&FAILURE) = Some(failure);
}

/// Takes the structured failure recorded for the current load, if any.
pub fn take_failure() -> Option<LoadFailure> {
    lock(&FAILURE).take()
}

/// Installs explicit data roots for one agent for the duration of the load.
/// The adapter path resolvers consult this before environment/default
/// resolution, so an explicit override makes the load independent of the
/// process environment.
pub fn set_root_override(agent: &'static str, roots: Vec<PathBuf>) {
    *lock(&ROOT_OVERRIDE) = Some((agent, roots));
}

/// Clears any installed root override.
pub fn clear_root_override() {
    *lock(&ROOT_OVERRIDE) = None;
}

/// The override installed for `agent`, if any.
pub fn root_override(agent: &str) -> Option<Vec<PathBuf>> {
    lock(&ROOT_OVERRIDE)
        .as_ref()
        .filter(|(name, _)| *name == agent)
        .map(|(_, roots)| roots.clone())
}
