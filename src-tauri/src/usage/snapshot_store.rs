//! Minimal cross-restart last-success snapshot cache (T06).
//!
//! One CAM-owned, schema-versioned JSON file in the app data directory holds
//! the last SUCCESSFUL aggregated `UsageSnapshot` so a restart can show the
//! previous data immediately (with its original dates and freshness) while a
//! background refresh collects the current numbers. Storage comparison record:
//!
//! - **SQLite (chosen against):** the existing `usage-cache.sqlite3` is
//!   deliberately table-free, and this cache is a single opaque blob that is
//!   always read whole and written whole — a table would add connection,
//!   schema and borrow complexity for zero query benefit. AGENTS.md forbids
//!   business tables without measured need.
//! - **Atomic file (chosen):** write a sibling temp file, then rename over the
//!   target (`MoveFileExW …REPLACE_EXISTING` on Windows), which gives the same
//!   single-record crash safety as a transaction commit: a process killed
//!   mid-write leaves the previous valid file untouched and a partial temp
//!   file that is never read.
//!
//! Safety rules (v0.4 plan §6.6 / acceptance matrix):
//! - the stored payload is only the CAM-normalized aggregate (scope, token
//!   totals, cost, coverage) — never raw logs, messages, file paths or
//!   credentials, and never vendor types;
//! - a missing, unreadable, oversized, corrupt or unknown-version file is
//!   ignored (falls back to "no cache") and never blocks a new collection;
//! - only a successful refresh persists, and only when it actually replaces
//!   the in-memory snapshot, so a failed collection can never overwrite the
//!   last success;
//! - retention is exactly one file: every save overwrites it, and a file
//!   larger than [`MAX_FILE_BYTES`] is treated as corrupt (no history, no
//!   periodic cleanup);
//! - the stored `collected_at`, data dates and time zone are the original
//!   ones from the moment of collection. Restoring rewrites none of them;
//!   freshness (`crate::usage::state::freshness`) recomputes against the
//!   current clock, so a yesterday or old-time-zone cache is shown as stale
//!   under its own date and replaced after the next successful refresh —
//!   different day boundaries are never mixed.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use super::UsageSnapshot;

/// Current snapshot-cache schema version. A file written for another version
/// is discarded in favour of "no cache" rather than guessed at.
pub const SNAPSHOT_SCHEMA_VERSION: u32 = 1;
const SNAPSHOT_FILE: &str = "last-snapshot.json";
/// A restored snapshot larger than this is treated as corrupt rather than
/// parsed: the real payload is a few kilobytes, so a huge file means the
/// cache concept is being misused (or the file is damaged).
const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredSnapshot {
    version: u32,
    snapshot: UsageSnapshot,
}

/// Reads and writes the one persisted last-success snapshot.
pub trait SnapshotStore: Send + Sync {
    /// Returns the last persisted success, or `None` when there is no usable
    /// cache (missing, corrupt, unknown version, oversized, invalid dates).
    fn load(&self) -> Option<UsageSnapshot>;
    /// Persists a success atomically. A storage failure is reported to the
    /// caller and must never affect the in-memory state.
    fn save(&self, snapshot: &UsageSnapshot) -> std::io::Result<()>;
}

/// No-op store for constructions that must not touch the filesystem.
pub struct NoopSnapshotStore;

impl SnapshotStore for NoopSnapshotStore {
    fn load(&self) -> Option<UsageSnapshot> {
        None
    }

    fn save(&self, _snapshot: &UsageSnapshot) -> std::io::Result<()> {
        Ok(())
    }
}

/// The production store: one JSON file in the app data directory.
pub struct FileSnapshotStore {
    path: PathBuf,
}

impl FileSnapshotStore {
    pub fn new(app_data_dir: &Path) -> Self {
        Self {
            path: app_data_dir.join(SNAPSHOT_FILE),
        }
    }
}

impl SnapshotStore for FileSnapshotStore {
    fn load(&self) -> Option<UsageSnapshot> {
        // Size gate before reading: an oversized "cache" is never parsed.
        let size = fs::metadata(&self.path).ok()?.len();
        if size > MAX_FILE_BYTES {
            return None;
        }
        let text = fs::read_to_string(&self.path).ok()?;
        let stored: StoredSnapshot = serde_json::from_str(&text).ok()?;
        if stored.version != SNAPSHOT_SCHEMA_VERSION {
            return None;
        }
        // The scope dates are rendered verbatim by the UI when the snapshot
        // is from another day, so they must be real ISO calendar dates.
        NaiveDate::parse_from_str(&stored.snapshot.scope.start_date, "%Y-%m-%d").ok()?;
        NaiveDate::parse_from_str(&stored.snapshot.scope.end_date, "%Y-%m-%d").ok()?;
        Some(stored.snapshot)
    }

    fn save(&self, snapshot: &UsageSnapshot) -> std::io::Result<()> {
        let stored = StoredSnapshot {
            version: SNAPSHOT_SCHEMA_VERSION,
            snapshot: snapshot.clone(),
        };
        let text = serde_json::to_string(&stored).map_err(std::io::Error::other)?;
        // Atomic save: the temp file carries the whole payload before the
        // rename, so an interrupted process can only leave the previous
        // valid cache (plus an orphaned temp file nothing reads).
        let tmp = self.path.with_extension("json.tmp");
        fs::write(&tmp, text)?;
        fs::rename(&tmp, &self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::{CoverageInfo, DailyUsage, TokenBreakdown};

    fn temp_dir(tag: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("cam-snapshot-store-{tag}-{unique}"));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn snapshot(date: &str, collected_at: &str) -> UsageSnapshot {
        let day = DailyUsage {
            date: date.to_string(),
            total_tokens: 42,
            token_breakdown: TokenBreakdown {
                input_tokens: 42,
                output_tokens: 0,
                cache_read_tokens: 0,
                cache_creation_tokens: 0,
                reasoning_tokens: 0,
                unclassified_tokens: 0,
            },
            estimated_cost_usd: None,
            cost_unknown_reason: None,
            cache_read_share: None,
            agents: vec![],
        };
        UsageSnapshot {
            scope: crate::usage::UsageScope {
                start_date: date.to_string(),
                end_date: date.to_string(),
                time_zone: "Asia/Shanghai".to_string(),
            },
            summary: crate::usage::UsageSummary {
                collected_at: collected_at.to_string(),
                today: day.clone(),
                last7_days: vec![day],
                coverage: CoverageInfo::complete(),
            },
        }
    }

    #[test]
    fn save_then_load_round_trips_the_whole_snapshot() {
        let dir = temp_dir("roundtrip");
        let store = FileSnapshotStore::new(&dir);
        let original = snapshot("2026-09-06", "2026-09-06T04:00:00Z");

        store.save(&original).expect("save snapshot");
        assert_eq!(store.load(), Some(original));

        // The atomic rename leaves nothing temporary behind.
        assert!(!dir.join("last-snapshot.json.tmp").exists());
        fs::remove_dir_all(&dir).expect("clean temp dir");
    }

    #[test]
    fn missing_cache_loads_as_none() {
        let dir = temp_dir("missing");
        assert_eq!(FileSnapshotStore::new(&dir).load(), None);
        fs::remove_dir_all(&dir).expect("clean temp dir");
    }

    #[test]
    fn corrupt_cache_loads_as_none() {
        let dir = temp_dir("corrupt");
        fs::write(dir.join("last-snapshot.json"), "{ not json ").expect("write corrupt file");
        assert_eq!(FileSnapshotStore::new(&dir).load(), None);
        fs::remove_dir_all(&dir).expect("clean temp dir");
    }

    #[test]
    fn unknown_version_is_discarded_not_guessed() {
        let dir = temp_dir("version");
        let store = FileSnapshotStore::new(&dir);
        store
            .save(&snapshot("2026-09-06", "2026-09-06T04:00:00Z"))
            .expect("save");
        // Rewrite the cache with a future schema version.
        let mut value: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(dir.join("last-snapshot.json")).unwrap())
                .unwrap();
        value["version"] = serde_json::json!(99);
        fs::write(
            dir.join("last-snapshot.json"),
            serde_json::to_string(&value).unwrap(),
        )
        .expect("write future version");
        assert_eq!(store.load(), None, "unknown versions must be discarded");
        fs::remove_dir_all(&dir).expect("clean temp dir");
    }

    #[test]
    fn oversized_file_is_not_parsed() {
        let dir = temp_dir("oversize");
        let blob = "x".repeat((MAX_FILE_BYTES + 1) as usize);
        fs::write(dir.join("last-snapshot.json"), blob).expect("write oversized file");
        assert_eq!(FileSnapshotStore::new(&dir).load(), None);
        fs::remove_dir_all(&dir).expect("clean temp dir");
    }

    #[test]
    fn invalid_scope_dates_are_rejected() {
        let dir = temp_dir("dates");
        let store = FileSnapshotStore::new(&dir);
        let mut invalid = snapshot("2026-09-06", "2026-09-06T04:00:00Z");
        invalid.scope.end_date = "not-a-date".to_string();
        store.save(&invalid).expect("save");
        assert_eq!(store.load(), None);
        fs::remove_dir_all(&dir).expect("clean temp dir");
    }

    #[test]
    fn save_failure_is_reported_and_leaves_the_previous_cache_intact() {
        let dir = temp_dir("writefail");
        let store = FileSnapshotStore::new(&dir);
        let first = snapshot("2026-09-06", "2026-09-06T04:00:00Z");
        store.save(&first).expect("save first success");

        // A directory at the temp-file path makes every write fail.
        fs::create_dir(dir.join("last-snapshot.json.tmp")).expect("block temp path");
        assert!(store
            .save(&snapshot("2026-09-07", "2026-09-07T04:00:00Z"))
            .is_err());

        // The previous success is still the only readable cache.
        assert_eq!(store.load(), Some(first));
        fs::remove_dir_all(&dir).expect("clean temp dir");
    }

    /// Interrupted-write semantics: a partial temp file (what a killed
    /// process leaves behind) is never read; the previous cache survives.
    #[test]
    fn orphaned_temp_file_is_ignored_by_load() {
        let dir = temp_dir("interrupted");
        let store = FileSnapshotStore::new(&dir);
        let first = snapshot("2026-09-06", "2026-09-06T04:00:00Z");
        store.save(&first).expect("save first success");
        fs::write(dir.join("last-snapshot.json.tmp"), "{ half-written").expect("orphan temp");

        assert_eq!(store.load(), Some(first));
        fs::remove_dir_all(&dir).expect("clean temp dir");
    }
}
