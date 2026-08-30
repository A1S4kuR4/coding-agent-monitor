use std::{fs, path::Path, time::Duration};

use tauri::{AppHandle, Manager};

use crate::error::AppError;

const DATABASE_FILE: &str = "usage-cache.sqlite3";

/// Bounded lock waiting for app-db initialization: a racing initialization
/// (or an external handle) resolves within this budget instead of failing
/// instantly with "database is locked" or hanging.
const BUSY_TIMEOUT_MS: usize = 2_000;

/// Creates the local SQLite file and applies connection-level safety settings.
/// MVP intentionally has no business tables until ccusage cache measurements justify them.
pub fn initialize(app: &AppHandle) -> Result<(), AppError> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| AppError::filesystem(error.to_string()))?;
    fs::create_dir_all(&app_data_dir)?;

    initialize_at(&app_data_dir)
}

fn initialize_at(app_data_dir: &Path) -> Result<(), AppError> {
    let mut connection = sqlite::open(app_data_dir.join(DATABASE_FILE))?;
    // Bound lock waits so a racing initialization (or any external handle)
    // resolves within the budget instead of failing instantly or hanging.
    connection
        .set_busy_timeout(BUSY_TIMEOUT_MS)
        .map_err(|error| AppError {
            code: "database_error".into(),
            message: error.to_string(),
        })?;
    // `journal_mode = WAL` briefly requires an exclusive lock and SQLite's
    // busy handler does not cover the mode change, so retry a bounded number
    // of times: concurrent initializations resolve within ~150ms worst case.
    let mut applied_wal = false;
    for attempt in 0..3 {
        match connection.execute("PRAGMA journal_mode = WAL") {
            Ok(()) => {
                applied_wal = true;
                break;
            }
            Err(error) if attempt < 2 => {
                std::thread::sleep(Duration::from_millis(50 * (attempt + 1)));
                let _ = error;
            }
            Err(error) => return Err(error.into()),
        }
    }
    let _ = applied_wal;
    connection.execute("PRAGMA foreign_keys = ON")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    #[test]
    fn initializes_sqlite_in_a_non_ascii_directory() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("coding-agent-monitor-用户-{unique}"));
        fs::create_dir(&directory).expect("create non-ASCII test directory");

        initialize_at(&directory).expect("initialize SQLite in non-ASCII directory");
        let database = directory.join(DATABASE_FILE);
        assert!(database.is_file());

        let connection = sqlite::open(&database).expect("reopen test database");
        let mut pragma = connection
            .prepare("PRAGMA journal_mode")
            .expect("prepare journal_mode pragma");
        assert!(matches!(pragma.next(), Ok(sqlite::State::Row)));
        let journal_mode: String = pragma.read(0).expect("read journal_mode pragma value");
        assert_eq!(journal_mode.to_ascii_lowercase(), "wal");
        drop(pragma);
        drop(connection);

        fs::remove_file(database).expect("remove test database");
        fs::remove_dir(directory).expect("remove non-ASCII test directory");
    }

    /// Phase 2 regression: re-running initialize on an existing database must
    /// be a no-op — no schema changes, no data loss, file still opens.
    #[test]
    fn reinitializing_existing_database_preserves_it() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("coding-agent-monitor-reinit-{unique}"));
        fs::create_dir_all(&directory).expect("create directory");

        // Simulate a v0.2-era database that already carries application data.
        let database = directory.join(DATABASE_FILE);
        {
            let connection = sqlite::open(&database).expect("create v0.2 db");
            connection
                .execute("CREATE TABLE legacy_marker (value TEXT)")
                .expect("create legacy table");
            connection
                .execute("INSERT INTO legacy_marker (value) VALUES ('v0.2-data')")
                .expect("insert legacy row");
        }

        initialize_at(&directory).expect("re-initialize over existing db");

        let connection = sqlite::open(&database).expect("reopen after re-init");
        let mut statement = connection
            .prepare("SELECT value FROM legacy_marker")
            .expect("prepare legacy read");
        assert!(matches!(statement.next(), Ok(sqlite::State::Row)));
        let value: String = statement.read(0).expect("read legacy value");
        assert_eq!(
            value, "v0.2-data",
            "v0.2 data must survive re-initialization"
        );
        drop(statement);
        drop(connection);

        fs::remove_file(database).expect("remove database");
        fs::remove_dir(directory).expect("remove directory");
    }

    /// Phase 2 regression: concurrent initialization (two tray refreshes
    /// racing at startup) stays controlled — one succeeds, the other sees an
    /// already-initialized database, and neither corrupts the file.
    #[test]
    fn concurrent_initialization_is_safe() {
        use std::sync::{Arc, Barrier};

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let directory = Arc::new(
            std::env::temp_dir().join(format!("coding-agent-monitor-concurrent-{unique}")),
        );
        fs::create_dir_all(&*directory).expect("create directory");
        let barrier = Arc::new(Barrier::new(2));
        let mut handles = Vec::new();
        for _ in 0..2 {
            let directory = Arc::clone(&directory);
            let barrier = Arc::clone(&barrier);
            handles.push(std::thread::spawn(move || {
                barrier.wait();
                initialize_at(&directory)
            }));
        }
        for handle in handles {
            handle.join().expect("init thread").expect("init ok");
        }

        // The database must still be healthy and in WAL mode.
        let connection = sqlite::open(directory.join(DATABASE_FILE)).expect("open after race");
        let mut pragma = connection
            .prepare("PRAGMA journal_mode")
            .expect("prepare pragma");
        assert!(matches!(pragma.next(), Ok(sqlite::State::Row)));
        let journal_mode: String = pragma.read(0).expect("read journal mode");
        assert_eq!(journal_mode.to_ascii_lowercase(), "wal");
        drop(pragma);
        drop(connection);

        // WAL sidecars may linger briefly after the last close; tolerate
        // their presence when cleaning the fixture.
        let _ = fs::remove_file(directory.join(DATABASE_FILE));
        for sidecar in ["-wal", "-shm"] {
            let _ = fs::remove_file(directory.join(format!("{DATABASE_FILE}{sidecar}")));
        }
        fs::remove_dir(&*directory).expect("remove directory");
    }
}
