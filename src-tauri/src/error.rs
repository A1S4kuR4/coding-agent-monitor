use std::fmt::{Display, Formatter};

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppError {
    pub code: String,
    pub message: String,
}

impl AppError {
    pub fn invalid_ccusage(agent: &str, details: String) -> Self {
        Self {
            code: "invalid_ccusage_output".into(),
            message: format!("{agent} returned invalid JSON: {details}"),
        }
    }

    pub fn invalid_date(details: String) -> Self {
        Self {
            code: "invalid_date".into(),
            message: format!("Unable to build the seven-day range: {details}"),
        }
    }

    pub fn filesystem(details: String) -> Self {
        Self {
            code: "filesystem_error".into(),
            message: details,
        }
    }

    /// The ccusage executable is absent from both the packaged directory and the
    /// staged `src-tauri/binaries/` tree. This is not a data condition; the
    /// report cannot start at all, so the whole summary fails.
    pub fn sidecar_missing() -> Self {
        Self {
            code: "sidecar_missing".into(),
            message: "Unable to locate the ccusage sidecar (ccusage.exe). \
                      Run `pnpm fetch:sidecar` to stage it, then rebuild."
                .into(),
        }
    }

    /// The sidecar ran but failed for one agent (spawn failure, non-zero exit,
    /// or a read error). `details` carries diagnostics only, never usage records.
    pub fn sidecar_failed(agent: &str, details: String) -> Self {
        Self {
            code: "sidecar_failed".into(),
            message: format!("ccusage failed for {agent}: {details}"),
        }
    }

    /// The sidecar exceeded the bound and was killed; no child is left behind.
    pub fn sidecar_timeout(agent: &str) -> Self {
        Self {
            code: "sidecar_timeout".into(),
            message: format!("ccusage did not finish for {agent} within the timeout."),
        }
    }
}

impl Display for AppError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for AppError {}

impl From<std::io::Error> for AppError {
    fn from(error: std::io::Error) -> Self {
        Self {
            code: "filesystem_error".into(),
            message: error.to_string(),
        }
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(error: rusqlite::Error) -> Self {
        Self {
            code: "database_error".into(),
            message: error.to_string(),
        }
    }
}
