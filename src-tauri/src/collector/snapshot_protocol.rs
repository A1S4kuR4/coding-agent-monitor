//! Batch snapshot protocol (Phase 4A): one product refresh = one worker = one
//! full-agent snapshot. The single-agent request remains the internal
//! primitive; the product refresh path uses this batch form so a refresh never
//! spawns one worker per agent.

use serde::{Deserialize, Serialize};

use super::protocol::{
    parse_date, record_from_v1, CollectorResponseV1, DataSourceV1, ErrorV1, OutcomeV1, ReportV1,
};
use super::{AgentKind, CollectRequest, CollectResult, CollectorError, DataSource};

/// Current batch snapshot protocol version (independent gate from the single
/// protocol so the two can evolve without touching each other).
pub const SNAPSHOT_PROTOCOL_VERSION: u32 = 1;

/// Hard cap on agents per snapshot request: the registry has 17 agents; the
/// cap leaves headroom while bounding a malicious/buggy request's work.
pub const MAX_SNAPSHOT_AGENTS: usize = 32;

/// One target agent of a snapshot request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSpecV1 {
    /// Vendor agent id (see [`AgentKind::id`]).
    pub agent: String,
    /// Per-agent data source: each agent may read from its own explicit roots
    /// while siblings use the environment.
    pub source: DataSourceV1,
}

/// A versioned batch snapshot request: collect every listed agent once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollectorSnapshotRequestV1 {
    pub version: u32,
    /// Opaque correlation id echoed back on the response.
    pub request_id: String,
    /// Explicit, deduplicated, capped agent list (max
    /// [`MAX_SNAPSHOT_AGENTS`]). Order in the request is not significant; the
    /// response is ordered by the vendor registry.
    pub agents: Vec<AgentSpecV1>,
    /// Common date window (inclusive on both ends), or all history.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window: Option<super::protocol::DateWindowV1>,
    /// IANA time-zone name for daily bucketing.
    pub timezone: String,
}

/// One agent's outcome inside a snapshot response: an independent success
/// (records + diagnostics) or an independent structured error. One agent's
/// failure never discards its siblings' results.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum AgentSnapshotOutcomeV1 {
    Ok { report: ReportV1 },
    Error { error: ErrorV1 },
}

/// Wire form of one agent's snapshot result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSnapshotV1 {
    pub agent: String,
    #[serde(flatten)]
    pub outcome: AgentSnapshotOutcomeV1,
}

/// A versioned batch snapshot response.
///
/// Two forms:
/// - **Complete**: `fatal_error` is `None` and `agents` carries per-agent
///   outcomes (some may independently be `Error`).
/// - **Fatal**: `fatal_error` is `Some` and `agents` is empty — the entire
///   batch aborted (e.g. worker panic) and no partial results exist. The
///   supervisor maps this to `Internal`, never `Protocol`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollectorSnapshotResponseV1 {
    pub version: u32,
    pub request_id: String,
    /// Present only when the whole batch aborted (e.g. panic in the shared
    /// engine). The supervisor maps this to `Internal`, never `Protocol`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fatal_error: Option<ErrorV1>,
    /// Per-agent outcomes. Empty when `fatal_error` is present.
    #[serde(default)]
    pub agents: Vec<AgentSnapshotV1>,
}

impl CollectorSnapshotResponseV1 {
    /// Builds a whole-batch fatal error response (no partial agent results).
    pub fn fatal(
        request_id: impl Into<String>,
        code: crate::collector::protocol::ErrorCodeV1,
        message: impl Into<String>,
    ) -> Self {
        Self {
            version: SNAPSHOT_PROTOCOL_VERSION,
            request_id: request_id.into(),
            fatal_error: Some(ErrorV1 {
                code,
                message: message.into(),
                agent: None,
                vendor: Some("ccusage v20.0.20".to_string()),
            }),
            agents: Vec::new(),
        }
    }

    /// Returns the fatal error if this response represents a whole-batch
    /// abort.
    pub fn fatal_error(&self) -> Option<&ErrorV1> {
        self.fatal_error.as_ref()
    }
}

impl CollectorSnapshotRequestV1 {
    /// A V1 snapshot request over `agents` (deduplicated, registry-ordered).
    pub fn new(request_id: impl Into<String>, agents: &[AgentKind]) -> Self {
        let mut seen = std::collections::BTreeSet::new();
        let agents = agents
            .iter()
            .filter(|agent| seen.insert(agent.id()))
            .map(|agent| AgentSpecV1 {
                agent: agent.id().to_string(),
                source: DataSourceV1::Environment,
            })
            .collect();
        Self {
            version: SNAPSHOT_PROTOCOL_VERSION,
            request_id: request_id.into(),
            agents,
            window: None,
            timezone: "UTC".to_string(),
        }
    }

    /// Converts the wire request into validated per-agent domain requests.
    /// Rejects wrong versions, empty/oversized lists, unknown ids, malformed
    /// dates, and out-of-bounds sources; duplicate ids are normalized (first
    /// source wins) with explicit semantics.
    pub fn into_domain(self) -> Result<Vec<CollectRequest>, CollectorError> {
        if self.version != SNAPSHOT_PROTOCOL_VERSION {
            return Err(CollectorError::InvalidRequest {
                details: format!(
                    "unsupported snapshot protocol version {} (expected {SNAPSHOT_PROTOCOL_VERSION})",
                    self.version
                ),
            });
        }
        if self.agents.is_empty() {
            return Err(CollectorError::InvalidRequest {
                details: "snapshot request must list at least one agent".to_string(),
            });
        }
        if self.agents.len() > MAX_SNAPSHOT_AGENTS {
            return Err(CollectorError::InvalidRequest {
                details: format!(
                    "too many snapshot agents: {} (max {MAX_SNAPSHOT_AGENTS})",
                    self.agents.len()
                ),
            });
        }
        if self.timezone.is_empty() || self.timezone.len() > super::MAX_TIMEZONE_LEN {
            return Err(CollectorError::InvalidRequest {
                details: format!(
                    "timezone must be 1..={} bytes, got {}",
                    super::MAX_TIMEZONE_LEN,
                    self.timezone.len()
                ),
            });
        }
        let window = self
            .window
            .map(|window| {
                let start = parse_date(&window.start_inclusive)?;
                let end = parse_date(&window.end_inclusive)?;
                super::CollectWindow::new(start, end)
            })
            .transpose()?;
        let mut seen = std::collections::BTreeSet::new();
        let mut requests = Vec::with_capacity(self.agents.len());
        for spec in self.agents {
            let agent =
                AgentKind::from_id(&spec.agent).ok_or_else(|| CollectorError::InvalidRequest {
                    details: format!("unknown agent id {:?}", spec.agent),
                })?;
            if !seen.insert(agent) {
                continue;
            }
            let source = match spec.source {
                DataSourceV1::Environment => DataSource::Environment,
                DataSourceV1::Paths { roots } => {
                    DataSource::Paths(roots.into_iter().map(std::path::PathBuf::from).collect())
                }
            };
            let domain = CollectRequest {
                agent,
                window,
                timezone: super::TimeZoneSpec(self.timezone.clone()),
                source,
            };
            domain.validate()?;
            requests.push(domain);
        }
        if requests.is_empty() {
            return Err(CollectorError::InvalidRequest {
                details: "snapshot request has no agents after deduplication".to_string(),
            });
        }
        // Deterministic order: vendor registry order.
        requests.sort_by_key(|request| {
            super::AgentKind::ALL
                .iter()
                .position(|kind| *kind == request.agent)
                .expect("validated agent")
        });
        Ok(requests)
    }
}

impl CollectorSnapshotResponseV1 {
    /// Builds a snapshot response from per-agent domain results (any order;
    /// the response is emitted in vendor registry order).
    pub fn ok(request_id: impl Into<String>, mut results: Vec<(AgentKind, CollectResult)>) -> Self {
        results.sort_by_key(|(agent, _)| {
            super::AgentKind::ALL
                .iter()
                .position(|kind| kind == agent)
                .expect("validated agent")
        });
        Self {
            version: SNAPSHOT_PROTOCOL_VERSION,
            request_id: request_id.into(),
            fatal_error: None,
            agents: results
                .into_iter()
                .map(|(agent, result)| {
                    let response = CollectorResponseV1::ok("internal", &result);
                    let outcome = match response.outcome {
                        OutcomeV1::Ok { report } => AgentSnapshotOutcomeV1::Ok { report },
                        OutcomeV1::Error { error } => AgentSnapshotOutcomeV1::Error { error },
                    };
                    AgentSnapshotV1 {
                        agent: agent.id().to_string(),
                        outcome,
                    }
                })
                .collect(),
        }
    }

    /// Appends one agent's structured error (used when an agent's collect
    /// failed independently of its siblings).
    pub fn with_agent_error(mut self, agent: AgentKind, error: &CollectorError) -> Self {
        let response = CollectorResponseV1::error("internal", error);
        let outcome = match response.outcome {
            OutcomeV1::Error { error } => AgentSnapshotOutcomeV1::Error { error },
            OutcomeV1::Ok { .. } => unreachable!("error() always yields an error outcome"),
        };
        self.agents.push(AgentSnapshotV1 {
            agent: agent.id().to_string(),
            outcome,
        });
        self
    }

    /// Validates an already-deserialized snapshot response: version gate,
    /// request id, agent ids and duplicates, per-agent reports (including the
    /// token bucket invariant), diagnostic kinds and error attribution.
    pub fn validate(&self) -> Result<(), CollectorError> {
        if self.version != SNAPSHOT_PROTOCOL_VERSION {
            return Err(CollectorError::Protocol {
                details: format!(
                    "unsupported snapshot protocol version {} (expected {SNAPSHOT_PROTOCOL_VERSION})",
                    self.version
                ),
            });
        }
        if self.request_id.is_empty() || self.request_id.len() > super::MAX_REQUEST_ID_LEN {
            return Err(CollectorError::Protocol {
                details: format!(
                    "snapshot request id must be 1..={} bytes, got {}",
                    super::MAX_REQUEST_ID_LEN,
                    self.request_id.len()
                ),
            });
        }
        // Whole-batch fatal error: no per-agent results exist, but the error
        // must be well-formed.
        if let Some(fatal) = &self.fatal_error {
            if fatal.message.is_empty() {
                return Err(CollectorError::Protocol {
                    details: "fatal error carries an empty message".to_string(),
                });
            }
            if !self.agents.is_empty() {
                return Err(CollectorError::Protocol {
                    details: "fatal error response must not carry agent results".to_string(),
                });
            }
            return Ok(());
        }
        let mut seen = std::collections::BTreeSet::new();
        for agent_snapshot in &self.agents {
            let agent = AgentKind::from_id(&agent_snapshot.agent).ok_or_else(|| {
                CollectorError::Protocol {
                    details: format!("unknown agent id {:?}", agent_snapshot.agent),
                }
            })?;
            if !seen.insert(agent) {
                return Err(CollectorError::Protocol {
                    details: format!(
                        "duplicate agent {:?} in snapshot response",
                        agent_snapshot.agent
                    ),
                });
            }
            match &agent_snapshot.outcome {
                AgentSnapshotOutcomeV1::Ok { report } => {
                    for record in &report.records {
                        let rebuilt =
                            record_from_v1(record).map_err(|error| CollectorError::Protocol {
                                details: error.to_string(),
                            })?;
                        if !super::token_bucket_invariant_holds(&rebuilt)
                            && !report
                                .diagnostics
                                .iter()
                                .any(|diag| diag.kind == "invariant_violation")
                        {
                            return Err(CollectorError::Protocol {
                                details: format!(
                                    "record for {} {} violates the token bucket invariant",
                                    record.date, record.agent
                                ),
                            });
                        }
                    }
                    for diag in &report.diagnostics {
                        match diag.kind.as_str() {
                            "corrupt_file"
                            | "corrupt_record"
                            | "database_error"
                            | "source_unreadable"
                            | "invariant_violation"
                            | "source_changed" => {}
                            other => {
                                return Err(CollectorError::Protocol {
                                    details: format!("unknown diagnostic kind {other:?}"),
                                })
                            }
                        }
                    }
                }
                AgentSnapshotOutcomeV1::Error { error } => {
                    if error.message.is_empty() {
                        return Err(CollectorError::Protocol {
                            details: "agent error carries an empty message".to_string(),
                        });
                    }
                    if let Some(agent_id) = &error.agent {
                        if AgentKind::from_id(agent_id).is_none() {
                            return Err(CollectorError::Protocol {
                                details: format!("unknown agent id {agent_id:?}"),
                            });
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Converts the snapshot response into per-agent domain results, preserving
    /// registry order and per-agent errors.
    pub fn into_domain_results(
        self,
    ) -> Result<Vec<(AgentKind, Result<CollectResult, CollectorError>)>, CollectorError> {
        // Whole-batch fatal error: no per-agent results exist.
        if let Some(fatal) = &self.fatal_error {
            return Err(CollectorError::Internal {
                details: fatal.message.clone(),
            });
        }
        let mut results = Vec::with_capacity(self.agents.len());
        for agent_snapshot in self.agents {
            let agent = AgentKind::from_id(&agent_snapshot.agent).ok_or_else(|| {
                CollectorError::Protocol {
                    details: format!("unknown agent id {:?}", agent_snapshot.agent),
                }
            })?;
            let outcome = match agent_snapshot.outcome {
                AgentSnapshotOutcomeV1::Ok { report } => {
                    let mut records = Vec::with_capacity(report.records.len());
                    for record in &report.records {
                        records.push(record_from_v1(record)?);
                    }
                    let mut diagnostics = Vec::with_capacity(report.diagnostics.len());
                    for diagnostic in &report.diagnostics {
                        let kind = match diagnostic.kind.as_str() {
                            "corrupt_file" => super::DiagnosticKind::CorruptFile,
                            "corrupt_record" => super::DiagnosticKind::CorruptRecord,
                            "database_error" => super::DiagnosticKind::DatabaseError,
                            "source_unreadable" => super::DiagnosticKind::SourceUnreadable,
                            "invariant_violation" => super::DiagnosticKind::InvariantViolation,
                            "source_changed" => super::DiagnosticKind::SourceChanged,
                            other => {
                                return Err(CollectorError::Protocol {
                                    details: format!("unknown diagnostic kind {other:?}"),
                                })
                            }
                        };
                        diagnostics.push(super::CollectionDiagnostic {
                            kind,
                            file: diagnostic.file.clone(),
                            details: diagnostic.details.clone(),
                        });
                    }
                    Ok(CollectResult::from_parts(agent, records, diagnostics))
                }
                AgentSnapshotOutcomeV1::Error { error } => match error.code {
                    crate::collector::protocol::ErrorCodeV1::InvalidRequest => {
                        Err(CollectorError::InvalidRequest {
                            details: error.message,
                        })
                    }
                    crate::collector::protocol::ErrorCodeV1::SourceUnavailable => {
                        match error.agent.as_deref().and_then(AgentKind::from_id) {
                            Some(agent) => Err(CollectorError::SourceUnavailable {
                                agent,
                                details: error.message,
                            }),
                            None => Err(CollectorError::Protocol {
                                details: format!("agent error lost attribution: {}", error.message),
                            }),
                        }
                    }
                    crate::collector::protocol::ErrorCodeV1::CorruptData => {
                        match error.agent.as_deref().and_then(AgentKind::from_id) {
                            Some(agent) => Err(CollectorError::CorruptData {
                                agent,
                                details: error.message,
                            }),
                            None => Err(CollectorError::Protocol {
                                details: format!("agent error lost attribution: {}", error.message),
                            }),
                        }
                    }
                    crate::collector::protocol::ErrorCodeV1::DatabaseQuery => {
                        match error.agent.as_deref().and_then(AgentKind::from_id) {
                            Some(agent) => Err(CollectorError::DatabaseQuery {
                                agent,
                                details: error.message,
                            }),
                            None => Err(CollectorError::Protocol {
                                details: format!("agent error lost attribution: {}", error.message),
                            }),
                        }
                    }
                    crate::collector::protocol::ErrorCodeV1::VendorAdapter => {
                        Err(CollectorError::VendorAdapter {
                            vendor: error.vendor.unwrap_or_else(|| "vendor".to_string()),
                            details: error.message,
                        })
                    }
                    crate::collector::protocol::ErrorCodeV1::PricingUnavailable => {
                        Err(CollectorError::PricingUnavailable {
                            details: error.message,
                        })
                    }
                    crate::collector::protocol::ErrorCodeV1::Timeout => {
                        Err(CollectorError::Timeout {
                            details: error.message,
                        })
                    }
                    crate::collector::protocol::ErrorCodeV1::Cancelled => {
                        Err(CollectorError::Cancelled)
                    }
                    crate::collector::protocol::ErrorCodeV1::Internal => {
                        Err(CollectorError::Internal {
                            details: error.message,
                        })
                    }
                    crate::collector::protocol::ErrorCodeV1::Protocol => {
                        Err(CollectorError::Protocol {
                            details: error.message,
                        })
                    }
                },
            };
            results.push((agent, outcome));
        }
        Ok(results)
    }
}
