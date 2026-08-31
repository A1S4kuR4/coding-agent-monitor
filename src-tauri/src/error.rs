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

impl From<sqlite::Error> for AppError {
    fn from(error: sqlite::Error) -> Self {
        Self {
            code: "database_error".into(),
            message: error.to_string(),
        }
    }
}

impl From<crate::collector::CollectorError> for AppError {
    /// Maps a worker-collection failure to the app error boundary. `message`
    /// is display-quality and sanitized (the collector never embeds user
    /// paths, session text, or raw records); the frontend shows the message
    /// and keeps its last-known data, exactly like the v0.2 sidecar errors.
    fn from(error: crate::collector::CollectorError) -> Self {
        use crate::collector::CollectorError;
        let (code, message) = match &error {
            CollectorError::Cancelled => (
                "collection_cancelled",
                "The usage refresh was cancelled (the application is exiting).".to_string(),
            ),
            CollectorError::Timeout { .. } => (
                "collection_timeout",
                "The usage refresh did not finish in time and was stopped.".to_string(),
            ),
            CollectorError::Protocol { details }
            | CollectorError::InvalidRequest { details }
            | CollectorError::Internal { details }
            | CollectorError::PricingUnavailable { details } => {
                ("collection_failed", details.clone())
            }
            CollectorError::SourceUnavailable { agent, details }
            | CollectorError::CorruptData { agent, details }
            | CollectorError::DatabaseQuery { agent, details } => (
                "collection_failed",
                format!("{}: {}", agent.label(), details),
            ),
            CollectorError::VendorAdapter { vendor, details } => {
                ("collection_failed", format!("{vendor}: {details}"))
            }
        };
        Self {
            code: code.into(),
            message,
        }
    }
}
