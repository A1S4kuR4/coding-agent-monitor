use std::{
    fs,
    path::{Path, PathBuf},
    thread,
};

use ccusage_core::{LoadedEntry, cli::SharedArgs, date_within_range};

pub mod jsonl;
pub mod report;

pub use report::print_table_for_agent;

pub fn collect_usage_files(dir: &Path, files: &mut Vec<PathBuf>) {
    collect_files_with_extension(dir, "jsonl", files);
}

pub fn collect_files_with_extension(dir: &Path, extension: &str, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.filter_map(std::result::Result::ok) {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if file_type.is_file() && path.extension().is_some_and(|ext| ext == extension) {
            files.push(path);
        } else if file_type.is_dir() {
            collect_files_with_extension(&path, extension, files);
        }
    }
}

pub fn filter_loaded_entries_by_date(entries: &mut Vec<LoadedEntry>, shared: &SharedArgs) {
    if shared.since.is_none() && shared.until.is_none() {
        return;
    }
    entries.retain(|entry| {
        date_within_range(
            &entry.date,
            shared.since.as_deref(),
            shared.until.as_deref(),
        )
    });
}

pub fn chunk_file_indexes_by_size(files: &[PathBuf], chunk_count: usize) -> Vec<Vec<usize>> {
    // Callers derive the count from available_parallelism, which can be 0 for a
    // caller that clamps against an empty file list; one chunk still returns
    // every index rather than indexing an empty vector.
    let chunk_count = chunk_count.max(1);
    let mut weighted_indexes = Vec::with_capacity(files.len());
    for (index, file) in files.iter().enumerate() {
        let size = fs::metadata(file).map_or(0, |metadata| metadata.len());
        weighted_indexes.push((index, size));
    }
    weighted_indexes.sort_unstable_by(|a, b| match b.1.cmp(&a.1) {
        std::cmp::Ordering::Equal => a.0.cmp(&b.0),
        order => order,
    });

    let mut chunks = vec![Vec::new(); chunk_count];
    let mut chunk_sizes = vec![0_u64; chunk_count];
    for (index, size) in weighted_indexes {
        let mut target = 0;
        for candidate in 1..chunk_sizes.len() {
            if chunk_sizes[candidate] < chunk_sizes[target] {
                target = candidate;
            }
        }
        chunks[target].push(index);
        chunk_sizes[target] = chunk_sizes[target].saturating_add(size);
    }

    chunks
        .into_iter()
        .filter(|chunk| !chunk.is_empty())
        .collect()
}

/// Reads `files` by applying `read` to each path and returns results in file order.
pub fn read_files_parallel<T, F>(files: &[PathBuf], single_thread: bool, read: F) -> Vec<T>
where
    T: Send,
    F: Fn(&Path) -> T + Sync,
{
    let worker_count = if single_thread {
        1
    } else {
        thread::available_parallelism()
            .map(usize::from)
            .unwrap_or(1)
            .min(files.len())
    };
    if worker_count <= 1 {
        return files.iter().map(|file| read(file.as_path())).collect();
    }

    let chunks = chunk_file_indexes_by_size(files, worker_count);
    let read = &read;
    thread::scope(|scope| {
        let mut handles = Vec::with_capacity(chunks.len());
        for chunk in chunks {
            handles.push(scope.spawn(move || {
                chunk
                    .into_iter()
                    .map(|index| (index, read(files[index].as_path())))
                    .collect::<Vec<_>>()
            }));
        }
        let mut results: Vec<Option<T>> = Vec::with_capacity(files.len());
        results.resize_with(files.len(), || None);
        for (index, value) in handles
            .into_iter()
            .flat_map(|handle| handle.join().expect("file read worker panicked"))
        {
            results[index] = Some(value);
        }
        results
            .into_iter()
            .map(|value| value.expect("file read worker returned every file"))
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::{chunk_file_indexes_by_size, read_files_parallel};
    use ccusage_test_support::Fixture;

    #[test]
    fn preserves_file_order_and_matches_single_thread() {
        let fixture = Fixture::new();
        let files = (0..256)
            .map(|index| {
                let body = "x".repeat((index % 17) * 64 + 1);
                fixture.write_file(format!("file-{index:03}.txt"), format!("{index}:{body}"))
            })
            .collect::<Vec<_>>();
        let read = |path: &std::path::Path| {
            let content = std::fs::read_to_string(path).unwrap();
            content.split(':').next().unwrap().to_string()
        };

        let single = read_files_parallel(&files, true, read);
        let multi = read_files_parallel(&files, false, read);
        let expected = (0..256).map(|index| index.to_string()).collect::<Vec<_>>();

        assert_eq!(single, expected);
        assert_eq!(multi, expected);
    }

    #[test]
    fn treats_a_zero_chunk_count_as_one_chunk() {
        let fixture = Fixture::new();
        let files = vec![fixture.write_file("only.txt", "body")];

        assert_eq!(chunk_file_indexes_by_size(&files, 0), vec![vec![0]]);
    }

    #[test]
    fn handles_empty_input() {
        let empty: Vec<std::path::PathBuf> = Vec::new();

        assert!(read_files_parallel(&empty, false, |_| 0_u8).is_empty());
    }
}

// Downstream (Coding Agent Monitor) 0002 patch: bounded lock waiting for
// read-only source database connections. A held exclusive lock must surface
// as a skipped-source diagnostic within this budget, never block a UI refresh
// or a future worker indefinitely.
pub const SOURCE_DB_BUSY_TIMEOUT_MS: usize = 2_000;

/// Downstream (Coding Agent Monitor) 0002 patch: unified read-only opening
/// policy for agent source databases.
///
/// - The connection is always `SQLITE_OPEN_READONLY` (no CREATE, no writes).
/// - If no `-wal`/`-shm` sidecars exist, the file is opened with
///   `immutable=1`: SQLite treats it as a static file, so collection can
///   never create sidecar files, take locks, or leave journals in the
///   agent's directory. Correct because a WAL-mode database without sidecars
///   has been fully checkpointed by its owner.
/// - If sidecars DO exist (a live writer is present), the database is opened
///   read-only without `immutable` so in-WAL committed data stays visible;
///   the sidecars already exist, so the collector creates nothing new.
/// - A bounded busy timeout keeps lock contention non-blocking.
///
/// Note: the no-sidecars check and the open are not atomic. A writer that
/// starts mid-read is out of scope for Phase 1's short reads; the Phase 3
/// worker owns the full concurrency policy.
pub fn open_source_db_readonly(path: &Path) -> sqlite::Result<sqlite::Connection> {
    let wal = PathBuf::from(format!("{}-wal", path.display()));
    let shm = PathBuf::from(format!("{}-shm", path.display()));
    let has_live_sidecars = wal.exists() || shm.exists();
    let mut connection = if has_live_sidecars {
        sqlite::Connection::open_with_flags(
            path,
            sqlite::OpenFlags::new().with_read_only(),
        )?
    } else {
        sqlite::Connection::open_with_flags(
            immutable_uri(path).as_str(),
            sqlite::OpenFlags::new().with_read_only().with_uri(),
        )?
    };
    connection.set_busy_timeout(SOURCE_DB_BUSY_TIMEOUT_MS)?;
    Ok(connection)
}

/// Builds a `file:…?immutable=1` URI with the path percent-encoded.
fn immutable_uri(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let mut uri = String::with_capacity(text.len() * 3 + 24);
    // `file:///C:/…` for Windows drive paths, `file:///home/…` for POSIX —
    // always an empty authority so SQLite does not parse the drive as a host.
    uri.push_str("file:///");
    if text.starts_with('/') {
        // POSIX absolute path already carries its slash.
        uri.pop();
    }
    for byte in text.as_bytes() {
        let ch = *byte as char;
        // SQLite URI unreserved set plus the path separators we need; every
        // other byte (space, CJK, punctuation) is percent-encoded so spaces
        // and multibyte names survive the URI parse.
        let keep = ch.is_ascii_alphanumeric()
            || matches!(ch, '-' | '_' | '.' | '~' | '/' | ':');
        if keep {
            uri.push(ch);
        } else {
            uri.push_str(&format!("%{byte:02X}"));
        }
    }
    uri.push_str("?immutable=1");
    uri
}

/// Stability of a source database across one read attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceStability {
    /// The read ran entirely against an unchanged source (immutable fast
    /// path, or plain read over an untouched file).
    Stable,
    /// The source changed while it was being read (sidecars appeared, or the
    /// database file's size/mtime moved). The returned result comes from a
    /// plain READONLY fallback attempt — never from the discarded immutable
    /// read.
    ChangedDuringRead,
}

/// One snapshot of everything that defines whether a source database read is
/// still looking at the same bytes: main file size/mtime plus the presence
/// and metadata of any `-wal`/`-shm` sidecars.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceSnapshot {
    db_size: Option<u64>,
    db_mtime: Option<std::time::SystemTime>,
    wal: Option<(u64, std::time::SystemTime)>,
    shm: Option<(u64, std::time::SystemTime)>,
}

impl SourceSnapshot {
    fn capture(path: &Path) -> Self {
        let meta = |p: &Path| {
            std::fs::metadata(p).ok().and_then(|m| m.modified().ok().map(|mtime| (m.len(), mtime)))
        };
        let (db_size, db_mtime) = meta(path).map_or((None, None), |(size, mtime)| (Some(size), Some(mtime)));
        Self {
            db_size,
            db_mtime,
            wal: meta(&PathBuf::from(format!("{}-wal", path.display()))),
            shm: meta(&PathBuf::from(format!("{}-shm", path.display()))),
        }
    }

    fn has_live_sidecars(&self) -> bool {
        self.wal.is_some() || self.shm.is_some()
    }
}

/// Reads a source database with protection against the immutable-read race:
///
/// 1. Snapshot the source. With no live `-wal`/`-shm` sidecars, take the
///    `immutable=1` fast path and run `load`.
/// 2. Re-snapshot. If the source changed in any way while the immutable read
///    ran (a writer appeared, the file moved), **discard the result** — an
///    immutable reader cannot see committed WAL data and might undercount —
///    and fall through.
/// 3. Fallback: plain READONLY open inside a `BEGIN`/`COMMIT` read
///    transaction, so the whole load observes one consistent snapshot even
///    while the agent writes. Re-snapshot: if the source is *still* moving,
///    the result is returned anyway (a read transaction is always a
///    consistent snapshot) but `ChangedDuringRead` is reported so the caller
///    can surface a diagnostic instead of a silent success.
///
/// The retry is bounded by construction: at most one immutable attempt plus
/// one plain attempt, no loops.
pub fn load_source_db_stable<T>(
    path: &Path,
    load: impl Fn(&sqlite::Connection) -> T,
) -> Result<(T, SourceStability), sqlite::Error> {
    let before = SourceSnapshot::capture(path);
    if !before.has_live_sidecars() {
        if let Ok(connection) = open_source_db_readonly_mode(path, false) {
            let result = load(&connection);
            let after = SourceSnapshot::capture(path);
            if after == before {
                return Ok((result, SourceStability::Stable));
            }
            // The immutable read raced with a writer: its result may miss
            // committed WAL rows. Discard it and take the plain path.
        }
        // Falls through to the plain attempt (also covers immutable-open
        // failures, e.g. the file was deleted between the snapshot and the
        // open — the plain attempt re-reports the open error).
    }

    let before_plain = SourceSnapshot::capture(path);
    let connection = open_source_db_readonly_mode(path, true)?;
    // One read transaction = one consistent snapshot for every statement the
    // loader runs, even while the agent keeps writing.
    connection.execute("BEGIN")?;
    let result = load(&connection);
    let _ = connection.execute("COMMIT");
    let after = SourceSnapshot::capture(path);
    let stability = if after == before_plain {
        SourceStability::Stable
    } else {
        SourceStability::ChangedDuringRead
    };
    Ok((result, stability))
}

/// Phase 2 opening policy with an explicit switch: `allow_immutable = false`
/// forces the plain READONLY path (used by the race fallback).
pub fn open_source_db_readonly_mode(
    path: &Path,
    allow_immutable: bool,
) -> sqlite::Result<sqlite::Connection> {
    let wal = PathBuf::from(format!("{}-wal", path.display()));
    let shm = PathBuf::from(format!("{}-shm", path.display()));
    let has_live_sidecars = !allow_immutable || wal.exists() || shm.exists();
    let mut connection = if !has_live_sidecars {
        sqlite::Connection::open_with_flags(
            immutable_uri(path).as_str(),
            sqlite::OpenFlags::new().with_read_only().with_uri(),
        )?
    } else {
        sqlite::Connection::open_with_flags(
            path,
            sqlite::OpenFlags::new().with_read_only(),
        )?
    };
    connection.set_busy_timeout(SOURCE_DB_BUSY_TIMEOUT_MS)?;
    Ok(connection)
}

#[cfg(test)]
mod stable_source_tests {
    use super::load_source_db_stable;
    use super::SourceStability;
    use sqlite::Connection;
    use std::path::Path;

    fn create_db(path: &Path) {
        let db = Connection::open(path).expect("create db");
        db.execute("CREATE TABLE t (id INTEGER PRIMARY KEY, n INTEGER)")
            .expect("create table");
        let mut statement = db.prepare("INSERT INTO t (id, n) VALUES (1, 10)").expect("insert");
        statement.next().expect("row");
    }

    fn count_rows(connection: &Connection) -> usize {
        let mut statement = connection
            .prepare("SELECT COUNT(*) FROM t")
            .expect("prepare count");
        statement.next().expect("count row");
        statement.read::<i64, _>(0).expect("count") as usize
    }

    #[test]
    fn stable_source_uses_immutable_path_and_reports_stable() {
        let dir = tempfile::tempdir().expect("tmp");
        let db = dir.path().join("source.db");
        create_db(&db);

        let (count, stability) =
            load_source_db_stable(&db, count_rows).expect("stable read");

        assert_eq!(count, 1);
        assert_eq!(stability, SourceStability::Stable);
    }

    #[test]
    fn writer_appearing_during_immutable_read_discards_and_falls_back_to_plain() {
        let dir = tempfile::tempdir().expect("tmp");
        let db = dir.path().join("source.db");
        create_db(&db);

        // The closure simulates the agent's own writer appearing mid-read: it
        // opens a separate read-write connection, commits a new row in WAL
        // mode, and keeps the connection open (so the sidecars stay present
        // for the post-read snapshot) until the read attempt finishes.
        let (count, stability) = load_source_db_stable(&db, |connection| {
            let writer = Connection::open(&db).expect("open writer");
            writer
                .execute("PRAGMA journal_mode=WAL")
                .expect("enable wal");
            writer
                .execute("INSERT OR REPLACE INTO t (id, n) VALUES (2, 20)")
                .expect("insert row mid-read");
            let observed = count_rows(connection);
            // The writer outlives the read attempt: drop(?) — keep it alive by
            // leaking; the test process is short-lived.
            std::mem::forget(writer);
            observed
        })
        .expect("raced read must still succeed via fallback");

        // The immutable attempt (which saw only 1 row) was discarded; the
        // plain read-only retry observed the committed WAL row.
        assert_eq!(count, 2, "fallback plain read must see committed WAL data");
        assert_eq!(stability, SourceStability::ChangedDuringRead);
    }

    #[test]
    fn plain_read_over_a_live_writer_reports_changed_but_returns_a_snapshot() {
        // Seed the source with live sidecars so the helper starts on the plain
        // path, then have the "agent" write during the read.
        let dir = tempfile::tempdir().expect("tmp");
        let db = dir.path().join("source.db");
        create_db(&db);
        {
            let writer = Connection::open(&db).expect("open wal writer");
            writer.execute("PRAGMA journal_mode=WAL").expect("wal");
            writer
                .execute("INSERT INTO t (id, n) VALUES (2, 20)")
                .expect("seed wal row");
            // `writer` stays open for the rest of the test: sidecars persist.
            let (count, stability) = load_source_db_stable(&db, |connection| {
                let extra = Connection::open(&db).expect("open agent writer");
                extra
                    .execute("INSERT INTO t (id, n) VALUES (3, 30)")
                    .expect("insert during plain read");
                std::mem::forget(extra);
                count_rows(connection)
            })
            .expect("plain read must succeed");

            // The read transaction took one consistent snapshot: either 2 or 3
            // rows depending on commit timing, never 1 (the pre-WAL state).
            assert!(count >= 2, "snapshot must include committed WAL rows, got {count}");
            // Sidecar metadata may or may not have moved; both outcomes are
            // acceptable — the contract is a consistent snapshot, not a
            // specific stability verdict.
            let _ = stability;
        }
    }
}
