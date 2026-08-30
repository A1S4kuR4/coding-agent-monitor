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

/// Downstream (Coding Agent Monitor) 0002 patch: central sanitization applied
/// to every diagnostic/failure before it is stored. Masks absolute path
/// occurrences (Windows drive-letter and UNC, common POSIX roots) so details
/// never carry full user paths, log contents, or other sensitive material.
/// Vendor `debug_log` output is untouched — it stays a local-only channel.
fn sanitize_details(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < bytes.len() {
        let rest = &text[i..];
        let masked_prefix: Option<usize> = if rest.starts_with("\\\\?\\") || rest.starts_with("\\\\")
        {
            Some(2)
        } else if bytes.len() > i + 2
            && bytes[i].is_ascii_alphabetic()
            && bytes[i + 1] == b':'
            && (bytes[i + 2] == b'\\' || bytes[i + 2] == b'/')
        {
            Some(3)
        } else {
            ["/home/", "/Users/", "/root/", "/tmp/", "/var/"]
                .iter()
                .find(|prefix| rest.starts_with(**prefix))
                .map(|prefix| prefix.len())
        };
        match masked_prefix {
            Some(prefix_len) => {
                // Consume the rest of the path run (up to whitespace, quote,
                // or bracket) and replace the whole occurrence with "[path]".
                let mut end = prefix_len;
                for ch in rest[prefix_len..].chars() {
                    if ch.is_whitespace() || ch == '"' || ch == '\'' || ch == ')' || ch == ']' {
                        break;
                    }
                    end += ch.len_utf8();
                }
                out.push_str("[path]");
                i += end;
            }
            None => {
                let ch = rest.chars().next().expect("non-empty remainder");
                out.push(ch);
                i += ch.len_utf8();
            }
        }
    }
    out
}

/// Records a recoverable problem for the current load. The diagnostic is
/// sanitized centrally before storage — recording sites cannot forget.
pub fn record(diag: LoadDiag) {
    let mut diag = diag;
    diag.details = sanitize_details(&diag.details);
    if let Some(file) = &diag.file {
        // Structured file field: basename only, defence in depth against
        // recording sites passing a full path.
        diag.file = std::path::Path::new(file)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned());
    }
    lock(&DIAGNOSTICS).push(diag);
}

/// Takes every diagnostic recorded during the current load.
pub fn drain_diags() -> Vec<LoadDiag> {
    std::mem::take(&mut lock(&DIAGNOSTICS))
}

/// Raises a structured failure for the current load. The details are
/// sanitized centrally before storage. Callers that need to keep the existing
/// `Result<_, CliError>` flow short-circuiting should return their normal
/// error too; the collector entry point consults this slot first when mapping
/// failures.
pub fn raise_failure(failure: LoadFailure) {
    *lock(&FAILURE) = Some(LoadFailure {
        kind: failure.kind,
        details: sanitize_details(&failure.details),
    });
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
