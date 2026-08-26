use std::{fs, path::Path};

use rusqlite::Connection;
use tauri::{AppHandle, Manager};

use crate::error::AppError;

const DATABASE_FILE: &str = "usage-cache.sqlite3";

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
    let connection = Connection::open(app_data_dir.join(DATABASE_FILE))?;
    connection.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")?;
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

        let connection = Connection::open(&database).expect("reopen test database");
        let journal_mode: String = connection
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .expect("read journal_mode pragma");
        assert_eq!(journal_mode.to_ascii_lowercase(), "wal");
        drop(connection);

        fs::remove_file(database).expect("remove test database");
        fs::remove_dir(directory).expect("remove non-ASCII test directory");
    }
}
