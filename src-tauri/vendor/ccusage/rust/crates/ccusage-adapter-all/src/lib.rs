mod loader;
mod report;
mod types;

use std::path::PathBuf;

use ccusage_adapter_codex::CodexGroup;
#[cfg(test)]
use ccusage_adapter_codex::CodexModelUsage;
use ccusage_adapter_common::filter_loaded_entries_by_date;
use ccusage_core::*;

mod adapter {
    pub use ccusage_adapter_amp as amp;
    pub use ccusage_adapter_antigravity as antigravity;
    pub use ccusage_adapter_claude as claude;
    pub use ccusage_adapter_codebuff as codebuff;
    pub use ccusage_adapter_codex as codex;
    pub use ccusage_adapter_copilot as copilot;
    pub use ccusage_adapter_droid as droid;
    pub use ccusage_adapter_gemini as gemini;
    pub use ccusage_adapter_goose as goose;
    pub use ccusage_adapter_grok as grok;
    pub use ccusage_adapter_hermes as hermes;
    pub use ccusage_adapter_kilo as kilo;
    pub use ccusage_adapter_kimi as kimi;
    pub use ccusage_adapter_openclaw as openclaw;
    pub use ccusage_adapter_opencode as opencode;
    pub use ccusage_adapter_pi as pi;
    pub use ccusage_adapter_qwen as qwen;
}

use crate::{
    Result,
    cli::{AgentCommandArgs, AgentReportKind, SharedArgs},
    print_json_or_jq, wants_json,
};

pub fn run(args: AgentCommandArgs) -> Result<()> {
    let kind = args.kind;
    let shared = args.shared;
    let include_agents = args.by_agent;
    if let Some(sections) = args.sections {
        let sections = requested_sections(kind, sections);
        let result = loader::load_sections(&sections, &shared)?;
        if wants_json(&shared) {
            return report::print_sections_report_json(
                &result.sections,
                kind,
                include_agents,
                shared.jq.as_deref(),
                shared.no_cost,
            );
        }
        for (section_kind, rows) in &result.sections {
            report::print_table(
                rows,
                *section_kind,
                &shared,
                result.detected_agents_for(*section_kind),
            )?;
        }
        return Ok(());
    }
    let result = loader::load_rows(kind, &shared)?;
    if wants_json(&shared) {
        let output = report::report_json_with_agents(&result.rows, kind, include_agents);
        return print_json_or_jq(output, shared.jq.as_deref(), shared.no_cost);
    }
    report::print_table(&result.rows, kind, &shared, &result.detected_agents)
}

/// Downstream (Coding Agent Monitor) collector entry point: loads the unified
/// daily report rows in-process and returns the same JSON report shape that
/// `ccusage daily --json --by-agent` prints - without going through the CLI
/// parser, terminal rendering, or stdout. Callers are expected to set
/// `json: true` and `offline: true` on `shared`.
pub fn daily_report_json_by_agent(shared: &SharedArgs) -> Result<serde_json::Value> {
    let result = loader::load_rows(AgentReportKind::Daily, shared)?;
    Ok(report::report_json_with_agents(
        &result.rows,
        AgentReportKind::Daily,
        true,
    ))
}

/// Downstream (Coding Agent Monitor) 0002 patch: structured single-agent load
/// outcome. Failures carry a machine-readable kind, so callers never classify
/// by error text.
#[derive(Debug, Clone)]
pub enum AgentLoadOutcome {
    /// The agent's daily report rows (same shape as
    /// `daily_report_json_by_agent`, restricted to this agent) plus the
    /// recoverable problems observed while loading.
    Report {
        report: serde_json::Value,
        diagnostics: Vec<ccusage_core::load_context::LoadDiag>,
    },
    /// The agent's data root is missing or not a valid agent data directory.
    SourceUnavailable {
        agent: String,
        details: String,
    },
    /// Any other fatal failure, classified structurally.
    Failed {
        kind: ccusage_core::load_context::LoadFailureKind,
        details: String,
    },
}

/// Downstream (Coding Agent Monitor) 0002 patch: loads the daily report for
/// exactly ONE agent. Other agents' loaders are never constructed or run, so
/// their data roots are not scanned, validated, or able to influence the
/// result. Failures are returned structurally (`AgentLoadOutcome`), and
/// recoverable problems (skipped corrupt files, unreadable sources) come back
/// as diagnostics alongside the report.
///
/// `root_override` installs explicit data roots for this one load; when
/// `Some`, the agent's path resolver uses them instead of reading the process
/// environment or default home directories. When `None`, the vendor resolves
/// roots from the environment as the CLI does. The override is thread-local
/// and cleared before this function returns.
///
/// `shared` should set `json: true`, `offline: true`, and (for deterministic
/// diagnostics) `single_thread: true`.
pub fn daily_report_for_agent(
    agent: &str,
    root_override: Option<&[PathBuf]>,
    shared: &SharedArgs,
) -> AgentLoadOutcome {
    use ccusage_core::load_context::{
        clear_root_override, drain_diags, set_root_override, take_failure, LoadFailure,
        LoadFailureKind,
    };
    use std::sync::Mutex;

    // The load context stores are process-global and load-scoped, so loads
    // must serialize: one at a time, single-threaded internals.
    static LOAD_LOCK: Mutex<()> = Mutex::new(());
    let _load_serial = LOAD_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

    let Some(agent_name) = BUILT_IN_AGENT_NAMES.iter().copied().find(|name| *name == agent) else {
        return AgentLoadOutcome::Failed {
            kind: LoadFailureKind::InvalidConfig,
            details: format!("unknown agent '{agent}'"),
        };
    };

    // Scope the override and stores to this load: clear whatever a previous
    // load left behind, and drain again on the way out.
    struct LoadScopeGuard;
    impl Drop for LoadScopeGuard {
        fn drop(&mut self) {
            clear_root_override();
            drain_diags();
            take_failure();
        }
    }
    let _guard = LoadScopeGuard;
    clear_root_override();
    drain_diags();
    take_failure();
    if let Some(roots) = root_override {
        set_root_override(agent_name, roots.to_vec());
    }

    match loader::load_rows_filtered(AgentReportKind::Daily, shared, Some(agent)) {
        Ok(result) => AgentLoadOutcome::Report {
            report: report::report_json_with_agents(
                &result.rows,
                AgentReportKind::Daily,
                true,
            ),
            diagnostics: drain_diags(),
        },
        Err(error) => match take_failure() {
            Some(LoadFailure {
                kind: LoadFailureKind::SourceUnavailable,
                details,
            }) => AgentLoadOutcome::SourceUnavailable {
                agent: agent.to_string(),
                details,
            },
            Some(LoadFailure { kind, details }) => AgentLoadOutcome::Failed { kind, details },
            None => AgentLoadOutcome::Failed {
                kind: LoadFailureKind::Internal,
                details: error.to_string(),
            },
        },
    }
}

fn requested_sections(
    command_kind: AgentReportKind,
    sections: Vec<AgentReportKind>,
) -> Vec<AgentReportKind> {
    let mut requested = vec![command_kind];
    for section in [
        AgentReportKind::Daily,
        AgentReportKind::Weekly,
        AgentReportKind::Monthly,
        AgentReportKind::Session,
    ] {
        if section != command_kind && sections.contains(&section) {
            requested.push(section);
        }
    }
    requested
}

#[cfg(test)]
use loader::{aggregate_rows, codex_group_row, load_agent_rows_parallel, load_rows, load_sections};
#[cfg(test)]
use report::{
    all_report_title, all_table_columns, all_table_row, report_json, report_json_with_agents,
    sections_report_json,
};
#[cfg(test)]
use types::{AgentLoadSpec, AgentRows, AllRow};

#[cfg(test)]
mod tests;
