//! Claude Desktop's local agent-mode session roots.
//!
//! Claude Desktop runs "local agent mode" sessions that are ordinary Claude
//! Code sessions: each one gets its own isolated Claude config directory and
//! writes the same JSONL transcripts (same `message.usage` buckets) that
//! `~/.claude/projects` holds. Those transcripts are invisible to the default
//! Claude Code root, so the product reads them through the vendored `claude`
//! loader pointed at these roots instead — see [`AgentKind::ClaudeDesktop`](super::AgentKind::ClaudeDesktop).
//!
//! Observed layout (Windows), under Claude Desktop's user-data directory:
//!
//! ```text
//! <user-data>/local-agent-mode-sessions/<account>/<workspace>/<session>/.claude/projects/…
//! ```
//!
//! Every `<session>/.claude` that actually contains a `projects/` child is a
//! data root of exactly the shape [`DataSource::Paths`](super::DataSource::Paths) documents. Desktop
//! keeps one such directory per session and never removes it, so the count
//! grows with use — see [`MAX_SOURCE_ROOTS`](super::MAX_SOURCE_ROOTS).
//!
//! Only the local agent-mode store is read: Desktop's `title-gen` transcripts
//! (its own chat-title generation) and its Electron chat stores are deliberately
//! excluded, so this counts the user's agent work rather than Desktop's internal
//! overhead.

use std::path::{Path, PathBuf};

/// Store holding one directory per local agent-mode session.
const SESSION_STORE: &str = "local-agent-mode-sessions";

/// Per-session Claude config directory, the parent of `projects/`.
const CLAUDE_DIR: &str = ".claude";

/// Data roots for Claude Desktop's local agent-mode sessions on this machine,
/// sorted and deduplicated.
///
/// Returns an empty vector when Claude Desktop is absent or has never run a
/// local agent session — a normal state, not a failure: the load then reports
/// an empty successful result for the agent.
pub fn session_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for user_data in user_data_dirs() {
        collect_session_roots(&user_data.join(SESSION_STORE), &mut roots);
    }
    roots.sort();
    roots.dedup();
    roots
}

/// Claude Desktop user-data directories on this machine.
///
/// Desktop's directory name varies by build (a plain `Claude`, and channel
/// builds such as `Claude-3p`), so every `Claude*` entry in the platform's
/// application-data locations is considered; only one that actually contains
/// the session store contributes roots.
///
/// Windows is the only platform wired up so far — the product builds and is
/// verified on Windows only. Other platforms report no roots, which degrades to
/// "Claude Desktop has no data here" rather than to a failure.
#[cfg(windows)]
fn user_data_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    for var in ["LOCALAPPDATA", "APPDATA"] {
        let Ok(base) = std::env::var(var) else {
            continue;
        };
        let Ok(entries) = std::fs::read_dir(base) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            // Case-sensitive on purpose: it must not match the Claude Code CLI
            // cache (`claude-cli-nodejs`), which shares the same parent.
            if path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("Claude"))
            {
                dirs.push(path);
            }
        }
    }
    // Also discover Windows Store / MSIX packaged Claude Desktop builds:
    // %LOCALAPPDATA%\Packages\Claude_*\LocalCache\Roaming\Claude
    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        let packages = Path::new(&local_app_data).join("Packages");
        if let Ok(entries) = std::fs::read_dir(packages) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir()
                    && path
                        .file_name()
                        .is_some_and(|name| name.to_string_lossy().starts_with("Claude"))
                {
                    let store_claude = path.join("LocalCache").join("Roaming").join("Claude");
                    if store_claude.is_dir() {
                        dirs.push(store_claude);
                    }
                }
            }
        }
    }
    dirs
}

#[cfg(not(windows))]
fn user_data_dirs() -> Vec<PathBuf> {
    Vec::new()
}

/// Walks `<store>/<account>/<workspace>/<session>/.claude`, collecting every
/// session config directory that holds transcripts.
///
/// The depth is fixed by Desktop's layout rather than searched, so an
/// unrelated `.claude` directory elsewhere in the tree is never picked up. A
/// layout change makes this find nothing (the agent simply reports no data)
/// instead of collecting the wrong files.
fn collect_session_roots(store: &Path, roots: &mut Vec<PathBuf>) {
    for account in child_dirs(store) {
        for workspace in child_dirs(&account) {
            for session in child_dirs(&workspace) {
                let config = session.join(CLAUDE_DIR);
                if config.join("projects").is_dir() {
                    roots.push(config);
                }
            }
        }
    }
}

/// Immediate subdirectories of `dir`, or nothing when it cannot be read.
fn child_dirs(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect()
}
