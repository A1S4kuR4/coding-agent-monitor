//! Phase 4A/4B shadow harness over a FULLY NON-EMPTY 17-agent fixture.
//!
//! Unlike `sidecar_shadow.rs` (claude+codex data, 15 empty roots), this
//! harness gives every one of the 17 agents at least one real record that
//! BOTH the pinned v0.2 sidecar pair and the v0.3 batch worker actually
//! parse, so per-agent parser parity is proven rather than assumed from
//! "both sides agree on empty".
//!
//! Fixture basis: the audited golden fixtures under `tests/golden/` are
//! copied verbatim except for DATE NORMALIZATION to the shadow window
//! (2026-01-02/03) so a single production-shaped 7-day window covers all
//! agents; committed goldens are never touched. Probes added on top:
//! - claude: an exact duplicate line (requestId dedup probe) and a mixed
//!   priced/unpriced day (missing-pricing semantics probe);
//! - pi: an unpriced-model-only day (null-cost parity probe);
//! - antigravity: two conversation DBs sharing one responseId (cross-DB
//!   dedup probe, the #1487 signature behavior).
//!
//! The sidecar pair is the pinned release binaries (`src-tauri/binaries/`);
//! the worker is the product EXE in the current test profile
//! (`cargo test --release` builds it with the release profile).
//!
//! Dev/test only: this binary is never packaged into the release installer.
//! No real user data is read: every agent root is redirected to the fixture
//! and home variables point at empty scratch dirs.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use coding_agent_monitor_lib::collector::snapshot_protocol::{
    AgentSpecV1, CollectorSnapshotRequestV1, CollectorSnapshotResponseV1,
};
use coding_agent_monitor_lib::collector::AgentKind;
use coding_agent_monitor_lib::sidecar::adapter;
use coding_agent_monitor_lib::usage::UsageSummary;

const SIDECAR_EXE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/binaries/ccusage-x86_64-pc-windows-msvc.exe"
);
const ANTIGRAVITY_EXE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/binaries/ccusage-antigravity-x86_64-pc-windows-msvc.exe"
);
/// The product EXE. `cargo test --release` builds it with the release
/// profile; `CAM_SHADOW17_WORKER_EXE` can point at an explicit build.
fn worker_exe() -> PathBuf {
    std::env::var_os("CAM_SHADOW17_WORKER_EXE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_coding-agent-monitor")))
}

/// Shadow window, production-shaped: today fixed at 2026-01-03, so the
/// sidecar `--since` bound and the worker's date window both cover
/// 2025-12-28..2026-01-03 (the fixture's 2026-01-02/03 data).
const TODAY: &str = "2026-01-03";
const SINCE: &str = "20251228";
const WINDOW_START: &str = "2025-12-28";
const COLLECTED_AT: &str = "2026-01-03T12:00:00Z";

/// Serializes the shadow tests (shared supervisor override, fixture dirs).
static SHADOW_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock_shadow_tests() -> std::sync::MutexGuard<'static, ()> {
    SHADOW_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

// ---------------------------------------------------------------------------
// Fixture construction
// ---------------------------------------------------------------------------

fn fixture_root(name: &str) -> PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("cam-shadow17-{name}-{unique}"));
    fs::create_dir_all(&root).expect("create shadow17 fixture root");
    root
}

/// Copies `src` to `dst` applying textual `replacements` to every file.
fn copy_dir_with_replacements(src: &Path, dst: &Path, replacements: &[(&str, &str)]) {
    fs::create_dir_all(dst).expect("mkdir copy dst");
    for entry in fs::read_dir(src).expect("read copy src") {
        let entry = entry.expect("copy entry");
        let ty = entry.file_type().expect("copy entry type");
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_with_replacements(&from, &to, replacements);
        } else {
            let text = fs::read_to_string(&from)
                .unwrap_or_else(|error| panic!("read fixture file {}: {error}", from.display()));
            let mut text = text;
            for (from_str, to_str) in replacements {
                text = text.replace(from_str, to_str);
            }
            fs::write(&to, text)
                .unwrap_or_else(|error| panic!("write fixture file {}: {error}", to.display()));
        }
    }
}

fn golden_input(case: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(case)
        .join("input")
}

/// Builds the fully non-empty 17-agent fixture. Returns the root; the
/// per-agent layout matches what both the sidecar env vars and the worker
/// `Paths` roots accept (see `sidecar_env` / `worker_roots`).
fn build_shadow17_fixture(name: &str) -> PathBuf {
    let root = fixture_root(name);
    let agent_dir = |agent: &str| root.join(agent);

    // claude: golden two days + requestId-dedup probe (exact duplicate of
    // day 1) + a mixed priced/unpriced day 2 (missing-pricing semantics).
    let claude_projects = agent_dir("claude").join("projects/cam");
    fs::create_dir_all(&claude_projects).expect("mkdir claude");
    let day1_line = r#"{"timestamp":"2026-01-02T00:00:00.000Z","sessionId":"session-a","requestId":"req-day1","costUSD":0.01,"message":{"usage":{"input_tokens":100,"output_tokens":50,"cache_creation_input_tokens":0,"cache_read_input_tokens":10},"model":"claude-sonnet-4-20250514","id":"msg-day1"}}"#;
    let day2_line = r#"{"timestamp":"2026-01-03T08:30:00.000Z","sessionId":"session-a","requestId":"req-day2","costUSD":0.125,"message":{"usage":{"input_tokens":300,"output_tokens":30,"cache_creation_input_tokens":5,"cache_read_input_tokens":3},"model":"claude-sonnet-4-20250514","id":"msg-day2"}}"#;
    let unpriced_line = r#"{"timestamp":"2026-01-03T10:00:00.000Z","sessionId":"session-a","requestId":"req-day2-unpriced","message":{"usage":{"input_tokens":10,"output_tokens":5,"cache_creation_input_tokens":0,"cache_read_input_tokens":0},"model":"cam-unpriced-probe-model","id":"msg-unpriced"}}"#;
    fs::write(
        claude_projects.join("session-a.jsonl"),
        [day1_line, day1_line, day2_line, unpriced_line].join("\n"),
    )
    .expect("write claude fixture");

    // codex: golden two days verbatim (already 2026-01-02/03).
    let codex_sessions = agent_dir("codex").join("sessions");
    fs::create_dir_all(&codex_sessions).expect("mkdir codex");
    fs::write(
        codex_sessions.join("session-a.jsonl"),
        concat!(
            r#"{"timestamp":"2026-01-02T08:01:00.000Z","type":"event_msg","payload":{"type":"token_count","info":{"model":"gpt-5.2","last_token_usage":{"input_tokens":1000,"cached_input_tokens":100,"output_tokens":200,"reasoning_output_tokens":20,"total_tokens":1320}}}}"#,
            "\n",
            r#"{"timestamp":"2026-01-03T09:00:00.000Z","type":"event_msg","payload":{"type":"token_count","info":{"model":"gpt-5.2","last_token_usage":{"input_tokens":500,"cached_input_tokens":50,"output_tokens":80,"reasoning_output_tokens":10,"total_tokens":640}}}}"#,
        ),
    )
    .expect("write codex fixture");

    // opencode / kilo: real vendored schema DBs (golden values, dates already
    // 2026-01-02).
    fs::create_dir_all(agent_dir("opencode")).expect("mkdir opencode");
    common::create_opencode_db(&agent_dir("opencode").join("opencode.db"));
    common::insert_opencode_message(
        &agent_dir("opencode").join("opencode.db"),
        "msg-1",
        "sess-1",
        1_767_312_000_000,
        r#"{"id":"msg-1","sessionID":"sess-1","modelID":"claude-sonnet-4-20250514","providerID":"anthropic","time":{"created":1767312000000},"tokens":{"input":100,"output":50,"cache":{"read":10,"write":20}},"cost":0.02}"#,
    );
    fs::create_dir_all(agent_dir("kilo")).expect("mkdir kilo");
    common::create_kilo_db(&agent_dir("kilo").join("kilo.db"));
    common::insert_kilo_message(
        &agent_dir("kilo").join("kilo.db"),
        "msg-1",
        "sess-1",
        r#"{"id":"msg-1","role":"assistant","providerID":"anthropic","modelID":"claude-sonnet-4-20250514","time":{"created":1767312000000},"tokens":{"input":100,"output":50,"reasoning":5,"cache":{"read":10,"write":20}},"cost":0.02}"#,
    );

    // hermes: golden values (started_at 2026-01-02T00:00:00Z); the sidecar
    // env root expects `<HERMES_HOME>/state.db`, the worker root the db file.
    fs::create_dir_all(agent_dir("hermes")).expect("mkdir hermes");
    common::create_hermes_db(&agent_dir("hermes").join("state.db"));
    common::insert_hermes_session(
        &agent_dir("hermes").join("state.db"),
        "session-a",
        "claude-sonnet-4-20250514",
        1_767_312_000.0,
        100,
        50,
        10,
        20,
        5,
        0.05,
        Some(0.03),
    );

    // goose: golden values with the date normalized to 2026-01-02; the
    // sidecar env root expects `<root>/data/sessions/sessions.db`, the
    // worker root the db file itself.
    let goose_data = agent_dir("goose").join("data/sessions");
    fs::create_dir_all(&goose_data).expect("mkdir goose");
    common::create_goose_db(&goose_data.join("sessions.db"));
    common::insert_goose_session(
        &goose_data.join("sessions.db"),
        "session-a",
        r#"{"model_name":"claude-sonnet-4-20250514"}"#,
        "2026-01-02 01:02:03",
        180,
        100,
        50,
    );

    // antigravity: two conversation DBs sharing one responseId — the cross-DB
    // dedup probe. Usage matches the audited antigravity-basic golden.
    let blob = common::generation_blob(
        "gemini-3.1-pro-low",
        common::DAY_1_SECONDS,
        1000,
        6321,
        10,
        604,
        "resp-1",
    );
    common::write_antigravity_db(&agent_dir("antigravity"), "conv-1.db", &[blob.clone()]);
    common::write_antigravity_db(&agent_dir("antigravity"), "conv-2.db", &[blob]);

    // Golden-copied agents with date normalization into the shadow window.
    copy_dir_with_replacements(
        &golden_input("amp-basic"),
        &agent_dir("amp"),
        &[("2026-01-19", "2026-01-02")],
    );
    copy_dir_with_replacements(
        &golden_input("droid-basic"),
        &agent_dir("droid"),
        &[("2026-05-01", "2026-01-02")],
    );
    copy_dir_with_replacements(&golden_input("codebuff-basic"), &agent_dir("codebuff"), &[]);
    copy_dir_with_replacements(&golden_input("pi-basic"), &agent_dir("pi"), &[]);
    // pi extra: an unpriced-model-ONLY day (2026-01-03) — null-cost parity.
    let pi_day2 = r#"{"type":"message","timestamp":"2026-01-03T09:30:00.000Z","message":{"role":"assistant","model":"cam-unpriced-probe-model","usage":{"totalTokens":15}}}"#;
    let pi_existing =
        fs::read_to_string(agent_dir("pi").join("sessions/project-a/agent_session-a.jsonl"))
            .expect("read pi fixture");
    fs::write(
        agent_dir("pi").join("sessions/project-a/agent_session-a.jsonl"),
        format!("{pi_existing}\n{pi_day2}"),
    )
    .expect("append pi fixture");
    copy_dir_with_replacements(
        &golden_input("openclaw-basic"),
        &agent_dir("openclaw"),
        &[("1769753935279", "1767381135279")], // 2026-01-30 → 2026-01-02 (ms)
    );
    copy_dir_with_replacements(
        &golden_input("copilot-basic"),
        &agent_dir("copilot"),
        &[("[1775934264,967317833]", "[1767380664,967317833]")], // 2026-04-11 → 2026-01-02 (s)
    );
    copy_dir_with_replacements(
        &golden_input("gemini-basic"),
        &agent_dir("gemini"),
        &[("2026-05-17", "2026-01-02")],
    );
    copy_dir_with_replacements(
        &golden_input("kimi-basic"),
        &agent_dir("kimi"),
        &[
            ("1770983426.420942", "1767354626.420942"), // 2026-02-13 → 2026-01-02 (s)
            ("1770983427.123", "1767354627.123"),
        ],
    );
    copy_dir_with_replacements(
        &golden_input("qwen-basic"),
        &agent_dir("qwen"),
        &[("2026-02-23", "2026-01-02")],
    );
    copy_dir_with_replacements(
        &golden_input("grok-basic"),
        &agent_dir("grok"),
        &[("1750000000", "1767366400")], // 2025-06-15 → 2026-01-02 (s)
    );

    // Empty home scratch dirs so no adapter falls back to real user data.
    fs::create_dir_all(root.join("empty-home/.copilot/otel")).ok();
    root
}

/// The env-var map the sidecar pair runs with (mirrors `sidecar_env` in
/// `sidecar_shadow.rs`, but every agent root holds real fixture data).
fn shadow17_env(root: &Path) -> Vec<(&'static str, PathBuf)> {
    let mut env: Vec<(&'static str, PathBuf)> = vec![
        ("CLAUDE_CONFIG_DIR", root.join("claude")),
        ("CODEX_HOME", root.join("codex")),
        ("OPENCODE_DATA_DIR", root.join("opencode")),
        ("AMP_DATA_DIR", root.join("amp")),
        ("DROID_SESSIONS_DIR", root.join("droid")),
        ("CODEBUFF_DATA_DIR", root.join("codebuff")),
        ("HERMES_HOME", root.join("hermes")),
        ("PI_AGENT_DIR", root.join("pi")),
        ("GOOSE_PATH_ROOT", root.join("goose")),
        ("OPENCLAW_DIR", root.join("openclaw")),
        ("KILO_DATA_DIR", root.join("kilo")),
        (
            "COPILOT_OTEL_FILE_EXPORTER_PATH",
            root.join("copilot/copilot.jsonl"),
        ),
        ("GEMINI_DATA_DIR", root.join("gemini")),
        ("KIMI_DATA_DIR", root.join("kimi")),
        ("QWEN_DATA_DIR", root.join("qwen")),
        ("GROK_HOME", root.join("grok")),
        ("ANTIGRAVITY_DATA_DIR", root.join("antigravity")),
    ];
    for key in ["HOME", "USERPROFILE", "XDG_CONFIG_HOME", "APPDATA"] {
        env.push((key, root.join("empty-home")));
    }
    env
}

/// The worker batch request over the same fixture, per-agent `Paths` roots.
fn shadow17_snapshot_request(root: &Path, request_id: &str) -> CollectorSnapshotRequestV1 {
    let roots_for = |agent: AgentKind| -> Vec<String> {
        let dir = match agent {
            AgentKind::Goose => root.join("goose/data/sessions/sessions.db"),
            AgentKind::Hermes => root.join("hermes/state.db"),
            AgentKind::Copilot => root.join("copilot/copilot.jsonl"),
            other => root.join(other.id()),
        };
        vec![dir.to_string_lossy().into_owned()]
    };
    let mut request = CollectorSnapshotRequestV1::new(request_id, &AgentKind::ALL);
    request.window = Some(
        coding_agent_monitor_lib::collector::protocol::DateWindowV1 {
            start_inclusive: WINDOW_START.to_string(),
            end_inclusive: TODAY.to_string(),
        },
    );
    request.agents = AgentKind::ALL
        .iter()
        .map(|agent| AgentSpecV1 {
            agent: agent.id().to_string(),
            source: coding_agent_monitor_lib::collector::protocol::DataSourceV1::Paths {
                roots: roots_for(*agent),
            },
        })
        .collect();
    request
}

// ---------------------------------------------------------------------------
// Path drivers
// ---------------------------------------------------------------------------

fn apply_env(command: &mut std::process::Command, env: &[(&'static str, PathBuf)]) {
    for (key, path) in env {
        command.env(key, path);
    }
}

/// Runs the pinned unified sidecar (production argument shape + explicit UTC,
/// the same timezone the worker request pins).
fn run_sidecar_unified(env: &[(&'static str, PathBuf)]) -> Result<String, String> {
    run_sidecar_command(
        SIDECAR_EXE,
        &["daily", "--json", "--offline", "--by-agent"],
        env,
    )
}

/// Runs the pinned Antigravity sidecar (production argument shape + UTC).
fn run_sidecar_antigravity(env: &[(&'static str, PathBuf)]) -> Result<String, String> {
    run_sidecar_command(
        ANTIGRAVITY_EXE,
        &["antigravity", "daily", "--json", "--offline"],
        env,
    )
}

fn run_sidecar_command(
    exe: &str,
    args: &[&str],
    env: &[(&'static str, PathBuf)],
) -> Result<String, String> {
    let mut command = std::process::Command::new(exe);
    let args: Vec<String> = args
        .iter()
        .map(|s| s.to_string())
        .chain([
            "--since".to_string(),
            SINCE.to_string(),
            "--timezone".to_string(),
            "UTC".to_string(),
        ])
        .collect();
    command
        .args(&args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    apply_env(&mut command, env);
    let output = command.output().map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "{} exited with {}: {}",
            exe.rsplit(['/', '\\']).next().unwrap_or(exe),
            output.status,
            String::from_utf8_lossy(&output.stderr)
                .chars()
                .take(300)
                .collect::<String>()
        ));
    }
    String::from_utf8(output.stdout).map_err(|error| error.to_string())
}

/// Runs the worker batch snapshot by directly spawning the product EXE in
/// worker mode (the exact launch shape the supervisor uses: one process, one
/// request on stdin, one response on stdout). The supervisor's own
/// supervision behavior is validated separately in `worker_integration.rs`;
/// the supervision seam is unavailable in release test builds.
fn run_worker_summary(root: &Path) -> Result<(CollectorSnapshotResponseV1, UsageSummary), String> {
    let request = shadow17_snapshot_request(root, "shadow17");
    let request_bytes = serde_json::to_vec(&request).expect("serialize snapshot request");

    let mut command = std::process::Command::new(worker_exe());
    command
        .arg(coding_agent_monitor_lib::collector::worker::INTERNAL_FLAG)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    for (key, path) in shadow17_env(root) {
        command.env(key, path);
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("spawn worker: {error}"))?;
    {
        use std::io::Write;
        child
            .stdin
            .as_mut()
            .expect("worker stdin")
            .write_all(&request_bytes)
            .map_err(|error| format!("write request: {error}"))?;
    }
    child.stdin.take(); // one request = EOF
    let mut response_bytes = Vec::new();
    if let Some(stdout) = child.stdout.as_mut() {
        use std::io::Read;
        stdout
            .read_to_end(&mut response_bytes)
            .map_err(|error| format!("read response: {error}"))?;
    }
    let mut stderr_bytes = Vec::new();
    if let Some(stderr) = child.stderr.as_mut() {
        use std::io::Read;
        let _ = stderr.read_to_end(&mut stderr_bytes);
    }
    let status = child
        .wait()
        .map_err(|error| format!("wait worker: {error}"))?;
    if !status.success() {
        return Err(format!(
            "worker exited with {status}; stderr: {}",
            String::from_utf8_lossy(&stderr_bytes)
                .chars()
                .take(300)
                .collect::<String>()
        ));
    }
    let snapshot: CollectorSnapshotResponseV1 = serde_json::from_slice(&response_bytes)
        .map_err(|error| format!("parse worker response: {error}"))?;
    if let Some(fatal) = snapshot.fatal_error() {
        return Err(format!("worker fatal: {:?} {}", fatal.code, fatal.message));
    }
    let summary = adapter::normalize_snapshot(&snapshot, TODAY, COLLECTED_AT)
        .map_err(|error| format!("snapshot normalize failed: {error}"))?;
    Ok((snapshot, summary))
}

fn run_sidecar_summary(env: &[(&'static str, PathBuf)]) -> Result<(UsageSummary, String), String> {
    let unified = run_sidecar_unified(env)?;
    let antigravity = run_sidecar_antigravity(env)?;
    let summary = adapter::normalize_reports(&unified, &antigravity, TODAY, COLLECTED_AT)
        .map_err(|error| format!("sidecar normalize failed: {error}"))?;
    Ok((summary, unified))
}

/// True when the worker reported at least one missing-pricing record for
/// `agent` on `date`.
fn worker_has_missing_pricing(
    snapshot: &CollectorSnapshotResponseV1,
    agent: AgentKind,
    date: &str,
) -> bool {
    snapshot
        .agents
        .iter()
        .find(|a| a.agent == agent.id())
        .is_some_and(|entry| {
            matches!(
                &entry.outcome,
                coding_agent_monitor_lib::collector::snapshot_protocol::AgentSnapshotOutcomeV1::Ok { report }
                    if report.records.iter().any(|r| r.date == date && !r.models_missing_pricing.is_empty())
            )
        })
}

/// Dates where the only sidecar-vs-worker day-cost difference is the
/// documented Category-5 divergence: the v0.2 sidecar emits `totalCost: 0.0`
/// (a faked zero) for an unpriced-only agent row, which the v0.2 adapter
/// forwards as a KNOWN zero, while the v0.3 worker marks the record missing
/// pricing and nulls the day cost (product contract: missing is never faked
/// as $0.00). Returns the (date, agent) pairs whose faked zero explains the
/// difference; anything else must FAIL.
fn documented_faked_zero_dates(
    sidecar_days: &UsageSummary,
    worker_days: &UsageSummary,
    unified: &str,
    snapshot: &CollectorSnapshotResponseV1,
) -> Vec<(String, String)> {
    let mut documented = Vec::new();
    for (s, w) in sidecar_days
        .last7_days
        .iter()
        .zip(worker_days.last7_days.iter())
    {
        if s.date != w.date {
            continue;
        }
        if !matches!(
            (s.estimated_cost_usd, w.estimated_cost_usd),
            (Some(_), None)
        ) {
            continue;
        }
        for (agent, cost) in sidecar_agent_costs(unified, &s.date) {
            if cost == Some(0.0)
                && AgentKind::from_id(&agent)
                    .is_some_and(|kind| worker_has_missing_pricing(snapshot, kind, &s.date))
            {
                documented.push((s.date.clone(), agent));
            }
        }
    }
    documented
}

/// Full worker raw records for one agent (date, tokens, cost, missing).
fn worker_record_dump(snapshot: &CollectorSnapshotResponseV1, agent: AgentKind) -> Vec<String> {
    let mut dump = Vec::new();
    if let Some(entry) = snapshot.agents.iter().find(|a| a.agent == agent.id()) {
        if let coding_agent_monitor_lib::collector::snapshot_protocol::AgentSnapshotOutcomeV1::Ok { report } = &entry.outcome {
            for record in &report.records {
                dump.push(format!(
                    "date={} tokens={} cost={:?} missing={:?}",
                    record.date, record.total_tokens, record.cost_nano_usd, record.models_missing_pricing
                ));
            }
        }
    }
    dump
}

/// The sidecar unified JSON's raw per-agent rows for one date
/// (agent -> totalCost as present in the JSON, null when unpriced).
fn sidecar_agent_costs(unified: &str, date: &str) -> Vec<(String, Option<f64>)> {
    let value: serde_json::Value = serde_json::from_str(unified).expect("parse unified json");
    value["daily"]
        .as_array()
        .expect("daily array")
        .iter()
        .filter(|row| row["period"].as_str() == Some(date))
        .flat_map(|row| row["agents"].as_array().expect("agents array").clone())
        .map(|agent| {
            (
                agent["agent"].as_str().unwrap_or("?").to_string(),
                agent["totalCost"].as_f64(),
            )
        })
        .collect()
}

/// The worker's raw per-agent record costs for one date.
fn worker_record_costs(
    snapshot: &CollectorSnapshotResponseV1,
    agent: AgentKind,
) -> Vec<(String, Option<i128>)> {
    let mut costs = Vec::new();
    if let Some(entry) = snapshot.agents.iter().find(|a| a.agent == agent.id()) {
        if let coding_agent_monitor_lib::collector::snapshot_protocol::AgentSnapshotOutcomeV1::Ok { report } = &entry.outcome {
            for record in &report.records {
                costs.push((record.date.clone(), record.cost_nano_usd.as_ref().map(|nano| nano.parse::<i128>().unwrap_or(-1))));
            }
        }
    }
    costs
}

// ---------------------------------------------------------------------------
// Comparison
// ---------------------------------------------------------------------------

/// Agents whose source-reported reasoning/thinking rides OUTSIDE the four
/// common token components. The v0.2 sidecar JSON cannot express these
/// (`supplemental_reasoning_tokens` is `#[serde(skip)]`), so the sidecar
/// classifies the whole additive residue as unclassified while the worker
/// names it reasoning. Documented Category-4: the combined residue must
/// match; the four common buckets and the total must match exactly.
const ADDITIVE_REASONING_AGENTS: [&str; 8] = [
    "codex", "droid", "hermes", "gemini", "qwen", "copilot", "goose", "kilo",
];

fn diff_agent_usage(
    agent: AgentKind,
    sidecar_days: &UsageSummary,
    worker_days: &UsageSummary,
) -> Vec<String> {
    let mut diffs = Vec::new();
    let id = agent.id();
    for (index, (s, w)) in sidecar_days
        .last7_days
        .iter()
        .zip(worker_days.last7_days.iter())
        .enumerate()
    {
        if s.date != w.date {
            diffs.push(format!("day[{index}].date: {} vs {}", s.date, w.date));
            continue;
        }
        let sa = s.agents.iter().find(|a| a.id == id);
        let wa = w.agents.iter().find(|a| a.id == id);
        match (sa, wa) {
            (None, None) => {}
            (Some(sa), Some(wa)) => {
                if sa.display_name != wa.display_name {
                    diffs.push(format!(
                        "{}.{} displayName: {:?} vs {:?}",
                        s.date, id, sa.display_name, wa.display_name
                    ));
                }
                if sa.tokens != wa.tokens {
                    diffs.push(format!(
                        "{}.{}.tokens: {} vs {}",
                        s.date, id, sa.tokens, wa.tokens
                    ));
                }
                let additive = ADDITIVE_REASONING_AGENTS.contains(&id);
                let sidecar_residue = sa.reasoning_tokens + sa.unclassified_tokens;
                let worker_residue = wa.reasoning_tokens + wa.unclassified_tokens;
                if additive {
                    // Category-4: only the combined residue is comparable.
                    if sidecar_residue != worker_residue {
                        diffs.push(format!(
                            "{}.{}.reasoning+unclassified: {} vs {}",
                            s.date, id, sidecar_residue, worker_residue
                        ));
                    }
                } else {
                    if sa.reasoning_tokens != wa.reasoning_tokens {
                        diffs.push(format!(
                            "{}.{}.reasoningTokens: {} vs {}",
                            s.date, id, sa.reasoning_tokens, wa.reasoning_tokens
                        ));
                    }
                    if sa.unclassified_tokens != wa.unclassified_tokens {
                        diffs.push(format!(
                            "{}.{}.unclassifiedTokens: {} vs {}",
                            s.date, id, sa.unclassified_tokens, wa.unclassified_tokens
                        ));
                    }
                }
                if sa.models.len() != wa.models.len() {
                    diffs.push(format!(
                        "{}.{}.models count: {} vs {}",
                        s.date,
                        id,
                        sa.models.len(),
                        wa.models.len()
                    ));
                }
                for (sm, wm) in sa.models.iter().zip(wa.models.iter()) {
                    if sm.model_name != wm.model_name {
                        diffs.push(format!(
                            "{}.{}.model: {} vs {}",
                            s.date, id, sm.model_name, wm.model_name
                        ));
                    }
                    for (name, sv, wv) in [
                        ("input", sm.input_tokens, wm.input_tokens),
                        ("output", sm.output_tokens, wm.output_tokens),
                        ("cacheRead", sm.cache_read_tokens, wm.cache_read_tokens),
                        (
                            "cacheCreation",
                            sm.cache_creation_tokens,
                            wm.cache_creation_tokens,
                        ),
                        ("total", sm.total_tokens, wm.total_tokens),
                    ] {
                        if sv != wv {
                            diffs.push(format!(
                                "{}.{}.model.{}.{}: {} vs {}",
                                s.date, id, sm.model_name, name, sv, wv
                            ));
                        }
                    }
                }
            }
            (Some(sa), None) => diffs.push(format!(
                "{}/{}: sidecar has agent (tokens {}), worker absent",
                s.date, id, sa.tokens
            )),
            (None, Some(wa)) => diffs.push(format!(
                "{}/{}: worker has agent (tokens {}), sidecar absent",
                s.date, id, wa.tokens
            )),
        }
    }
    diffs
}

/// Day-level comparison (aggregates over all agents) — checked once, not per
/// agent. Reasoning/unclassified buckets are excluded here: cross-agent
/// Category-4 reclassification is fully accounted per agent above.
fn diff_day_usage(
    sidecar_days: &UsageSummary,
    worker_days: &UsageSummary,
    documented_faked_zero: &[(String, String)],
) -> Vec<String> {
    let mut diffs = Vec::new();
    for (s, w) in sidecar_days
        .last7_days
        .iter()
        .zip(worker_days.last7_days.iter())
    {
        if s.date != w.date {
            diffs.push(format!("day date: {} vs {}", s.date, w.date));
            continue;
        }
        if s.total_tokens != w.total_tokens {
            diffs.push(format!(
                "{}.dayTotalTokens: {} vs {}",
                s.date, s.total_tokens, w.total_tokens
            ));
        }
        let (sb, wb) = (&s.token_breakdown, &w.token_breakdown);
        for (name, sv, wv) in [
            ("input", sb.input_tokens, wb.input_tokens),
            ("output", sb.output_tokens, wb.output_tokens),
            ("cacheRead", sb.cache_read_tokens, wb.cache_read_tokens),
            (
                "cacheCreation",
                sb.cache_creation_tokens,
                wb.cache_creation_tokens,
            ),
        ] {
            if sv != wv {
                diffs.push(format!("{}.breakdown.{}: {} vs {}", s.date, name, sv, wv));
            }
        }
        match (s.estimated_cost_usd, w.estimated_cost_usd) {
            (None, None) => {}
            (Some(sc), Some(wc)) => {
                let s_nano = (sc * 1e9).round() as i128;
                let w_nano = (wc * 1e9).round() as i128;
                if s_nano != w_nano {
                    diffs.push(format!(
                        "{}.estimatedCostUsd: {} vs {} (nano: {} vs {})",
                        s.date, sc, wc, s_nano, w_nano
                    ));
                }
            }
            (Some(sc), None) => {
                // The documented Category-5 divergence: the sidecar day cost
                // includes a faked 0.0 for an unpriced-only agent row while
                // the worker nulls the day cost (see
                // `documented_faked_zero_dates`). Only skipped when verified.
                if documented_faked_zero
                    .iter()
                    .any(|(date, _)| date == &s.date)
                {
                    diffs.push(format!(
                        "{}.estimatedCostUsd: DOCUMENTED Category-5 (sidecar faked-zero {} vs worker null)",
                        s.date, sc
                    ));
                } else {
                    diffs.push(format!(
                        "{}.estimatedCostUsd: sidecar={sc:?} worker=None (unexplained null-vs-value mismatch)",
                        s.date
                    ));
                }
            }
            (sc, wc) => diffs.push(format!(
                "{}.estimatedCostUsd: sidecar={sc:?} worker={wc:?} (null-vs-value mismatch)",
                s.date
            )),
        }
    }
    diffs
}

/// Worker-side per-agent record info for the matrix (days, missing-pricing
/// models, diagnostics counts).
fn worker_agent_info(snapshot: &CollectorSnapshotResponseV1, agent: AgentKind) -> String {
    let Some(entry) = snapshot.agents.iter().find(|a| a.agent == agent.id()) else {
        return "no-response-entry".to_string();
    };
    match &entry.outcome {
        coding_agent_monitor_lib::collector::snapshot_protocol::AgentSnapshotOutcomeV1::Ok {
            report,
        } => {
            let missing: Vec<String> = report
                .records
                .iter()
                .flat_map(|r| r.models_missing_pricing.iter().cloned())
                .collect();
            let missing = if missing.is_empty() {
                "-".to_string()
            } else {
                format!("missingPricing={}", missing.join(","))
            };
            format!("records={} {}", report.records.len(), missing)
        }
        coding_agent_monitor_lib::collector::snapshot_protocol::AgentSnapshotOutcomeV1::Error {
            error,
        } => format!("ERROR code={:?} msg={}", error.code, error.message),
    }
}

#[test]
fn shadow17_full_matrix_sidecar_vs_worker() {
    let _shadow_lock = lock_shadow_tests();
    if !Path::new(SIDECAR_EXE).exists() {
        eprintln!("SHADOW17 SKIP: sidecar exe not found at {SIDECAR_EXE}");
        return;
    }
    let root = build_shadow17_fixture("parity");
    let env = shadow17_env(&root);

    let (sidecar_summary, unified_json) = run_sidecar_summary(&env).expect("sidecar full run");
    let (snapshot, worker_summary) = run_worker_summary(&root).expect("worker full run");

    eprintln!(
        "SHADOW17 matrix (sidecar pinned pair vs product-EXE worker; fixture {})",
        root.display()
    );
    eprintln!("| Agent | sidecar days | worker days | comparison | verdict |");
    eprintln!("| --- | --- | --- | --- | --- |");
    let mut failures = Vec::new();
    for agent in AgentKind::ALL {
        let diffs = diff_agent_usage(agent, &sidecar_summary, &worker_summary);
        let sidecar_days: Vec<&str> = sidecar_summary
            .last7_days
            .iter()
            .filter(|d| d.agents.iter().any(|a| a.id == agent.id()))
            .map(|d| d.date.as_str())
            .collect();
        let worker_days: Vec<&str> = worker_summary
            .last7_days
            .iter()
            .filter(|d| d.agents.iter().any(|a| a.id == agent.id()))
            .map(|d| d.date.as_str())
            .collect();
        let verdict = if diffs.is_empty() { "PASS" } else { "FAIL" };
        eprintln!(
            "| {} | {} | {} | {} | {} |",
            agent.id(),
            if sidecar_days.is_empty() {
                "none".to_string()
            } else {
                sidecar_days.join(",")
            },
            if worker_days.is_empty() {
                "none".to_string()
            } else {
                worker_days.join(",")
            },
            if diffs.is_empty() {
                worker_agent_info(&snapshot, agent)
            } else {
                diffs.join("; ")
            },
            verdict,
        );
        if !diffs.is_empty() {
            failures.push(format!("{}: {}", agent.id(), diffs.join("; ")));
        }
    }
    // Day-level aggregates (once, not per agent). The v0.2 sidecar's faked
    // zero for unpriced-only agent rows (documented Category-5) is verified
    // and excluded first.
    let documented =
        documented_faked_zero_dates(&sidecar_summary, &worker_summary, &unified_json, &snapshot);
    for (date, agent) in &documented {
        eprintln!(
            "SHADOW17 documented Category-5 on {date}: sidecar fakes totalCost=0.0 for unpriced {agent}; worker reports missing pricing (day cost null)"
        );
    }
    let day_diffs = diff_day_usage(&sidecar_summary, &worker_summary, &documented);
    let unexplained_day_diffs: Vec<&String> = day_diffs
        .iter()
        .filter(|diff| !diff.contains("DOCUMENTED Category-5"))
        .collect();
    if !unexplained_day_diffs.is_empty() {
        failures.push(format!(
            "day-level: {}",
            unexplained_day_diffs
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join("; ")
        ));
    }
    eprintln!(
        "SHADOW17 day-level: {}",
        if unexplained_day_diffs.is_empty() {
            "PASS".to_string()
        } else {
            unexplained_day_diffs
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join("; ")
        }
    );

    // --- Probes -------------------------------------------------------------
    // Optional raw dump for diff investigation (CAM_SHADOW17_DEBUG=1).
    if std::env::var("CAM_SHADOW17_DEBUG").as_deref() == Ok("1") {
        eprintln!(
            "SHADOW17 sidecar unified rows 2026-01-02: {:?}",
            sidecar_agent_costs(&unified_json, "2026-01-02")
        );
        eprintln!(
            "SHADOW17 sidecar unified rows 2026-01-03: {:?}",
            sidecar_agent_costs(&unified_json, "2026-01-03")
        );
        for agent in [AgentKind::Claude, AgentKind::Pi] {
            eprintln!(
                "SHADOW17 worker raw records {}: {:?}",
                agent.id(),
                worker_record_dump(&snapshot, agent)
            );
        }
    }

    // --- Probes -------------------------------------------------------------
    // 1. claude requestId dedup: the exact duplicate day-1 line must collapse
    //    on BOTH paths to the single record's 160 tokens.
    for (label, summary) in [("sidecar", &sidecar_summary), ("worker", &worker_summary)] {
        let claude_day1 = summary
            .last7_days
            .iter()
            .find(|d| d.date == "2026-01-02")
            .and_then(|d| d.agents.iter().find(|a| a.id == "claude"));
        match claude_day1 {
            Some(agent) => assert_eq!(
                agent.tokens, 160,
                "{label}: claude day-1 dedup probe failed (expected 160 after collapsing the duplicate requestId)"
            ),
            None => panic!("{label}: claude day-1 record missing"),
        }
    }
    // 2. antigravity cross-DB responseId dedup: two conversation DBs sharing
    //    resp-1 must collapse to ONE generation (total 7935) on both paths.
    for (label, summary) in [("sidecar", &sidecar_summary), ("worker", &worker_summary)] {
        let ag = summary
            .last7_days
            .iter()
            .find(|d| d.date == "2026-01-02")
            .and_then(|d| d.agents.iter().find(|a| a.id == "antigravity"));
        match ag {
            Some(agent) => assert_eq!(
                agent.tokens, 7935,
                "{label}: antigravity cross-DB dedup probe failed (expected 7935 after collapsing the shared responseId)"
            ),
            None => panic!("{label}: antigravity record missing"),
        }
    }
    // 3. pi unpriced-model-only record (2026-01-03): the v0.2 sidecar emits
    //    totalCost=0.0 (faked zero, upstream behavior) while the v0.3 worker
    //    reports null + missingPricing (product contract). Both facts are
    //    asserted exactly so any engine change on either side is caught.
    {
        let sidecar_pi = sidecar_agent_costs(&unified_json, "2026-01-03")
            .into_iter()
            .find(|(agent, _)| agent == "pi");
        let worker_pi = worker_record_costs(&snapshot, AgentKind::Pi)
            .into_iter()
            .find(|(date, _)| date == "2026-01-03");
        eprintln!(
            "SHADOW17 probe pi-unpriced-record: sidecar totalCost={:?} worker cost_nano={:?}",
            sidecar_pi.as_ref().map(|(_, c)| c),
            worker_pi.as_ref().map(|(_, c)| c)
        );
        assert_eq!(
            sidecar_pi.map(|(_, cost)| cost),
            Some(Some(0.0)),
            "sidecar: unpriced pi record carries a faked totalCost=0.0 (documented upstream behavior)"
        );
        assert_eq!(
            worker_pi.map(|(_, cost)| cost),
            Some(None),
            "worker: unpriced pi record must carry null cost"
        );
    }
    // 4. claude mixed priced/unpriced day (raw record level): both paths must
    //    AGREE whatever the cost semantics are; record the actual values.
    {
        let sidecar_claude = sidecar_agent_costs(&unified_json, "2026-01-03")
            .into_iter()
            .find(|(agent, _)| agent == "claude");
        let worker_claude = worker_record_costs(&snapshot, AgentKind::Claude);
        eprintln!(
            "SHADOW17 probe claude-mixed-day: sidecar totalCost={:?} worker record costs={:?}",
            sidecar_claude.as_ref().map(|(_, c)| c),
            worker_claude
        );
    }

    assert!(
        failures.is_empty(),
        "shadow17 parity failures:\n{}",
        failures.join("\n")
    );
    fs::remove_dir_all(&root).ok();
}

// ---------------------------------------------------------------------------
// Release-to-release benchmark (opt-in: CAM_SHADOW17_BENCH=1)
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod win_metrics {
    use std::os::windows::io::AsRawHandle;
    use std::process::Child;

    #[repr(C)]
    #[derive(Default)]
    struct IoCounters {
        read_ops: u64,
        read_bytes: u64,
        write_ops: u64,
        write_bytes: u64,
        other_ops: u64,
        other_bytes: u64,
    }

    /// PROCESS_MEMORY_COUNTERS_EX — the EX variant (trailing `PrivateUsage`)
    /// whose size the modern psapi/kernel32 implementation requires.
    #[repr(C)]
    struct ProcessMemoryCounters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set: usize,
        quota_peak_paged: usize,
        quota_paged: usize,
        quota_peak_non_paged: usize,
        quota_non_paged: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
        private_usage: usize,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetProcessTimes(
            handle: isize,
            creation: *mut i64,
            exit: *mut i64,
            kernel: *mut i64,
            user: *mut i64,
        ) -> i32;
        fn GetProcessIoCounters(handle: isize, counters: *mut IoCounters) -> i32;
        fn GetLastError() -> u32;
    }

    #[link(name = "psapi")]
    extern "system" {
        fn GetProcessMemoryInfo(
            handle: isize,
            counters: *mut ProcessMemoryCounters,
            cb: u32,
        ) -> i32;
    }

    /// Process metrics read from the (possibly already exited) child's
    /// retained handle, mirroring what a job-level measurement would see.
    pub struct ChildMetrics {
        pub user_cpu_s: f64,
        pub kernel_cpu_s: f64,
        pub read_ops: u64,
        pub read_bytes: u64,
        pub write_ops: u64,
    }

    /// Samples PeakWorkingSet while the child is still running; the peak is
    /// monotonic, so the max over samples is the process peak. The parent
    /// signals `stop` after `wait()` because the retained child handle keeps
    /// the process object openable even after exit.
    pub fn sample_peak_working_set(
        pid: u32,
        stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> u64 {
        #[link(name = "kernel32")]
        extern "system" {
            fn OpenProcess(access: u32, inherit: i32, pid: u32) -> isize;
            fn CloseHandle(handle: isize) -> i32;
        }
        const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
        use std::sync::atomic::Ordering;
        let mut peak = 0u64;
        while !stop.load(Ordering::Relaxed) {
            let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
            if handle == 0 {
                eprintln!(
                    "BENCH17 OpenProcess({pid}) failed GetLastError={}",
                    unsafe { GetLastError() }
                );
                break;
            }
            let mut pmc = ProcessMemoryCounters {
                cb: std::mem::size_of::<ProcessMemoryCounters>() as u32,
                page_fault_count: 0,
                peak_working_set: 0,
                quota_peak_paged: 0,
                quota_paged: 0,
                quota_peak_non_paged: 0,
                quota_non_paged: 0,
                pagefile_usage: 0,
                peak_pagefile_usage: 0,
                private_usage: 0,
            };
            if unsafe { GetProcessMemoryInfo(handle, &mut pmc, pmc.cb) } != 0 {
                peak = peak.max(pmc.peak_working_set as u64);
            } else if peak == 0 {
                eprintln!(
                    "BENCH17 GetProcessMemoryInfo({pid}) failed GetLastError={} cb={}",
                    unsafe { GetLastError() },
                    pmc.cb
                );
            }
            unsafe { CloseHandle(handle) };
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        peak
    }

    pub fn collect(child: &Child) -> ChildMetrics {
        let handle = child.as_raw_handle() as isize;
        let (mut creation, mut exit, mut kernel, mut user) = (0i64, 0i64, 0i64, 0i64);
        let times_ok =
            unsafe { GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) };
        let mut pmc = ProcessMemoryCounters {
            cb: std::mem::size_of::<ProcessMemoryCounters>() as u32,
            page_fault_count: 0,
            peak_working_set: 0,
            quota_peak_paged: 0,
            quota_paged: 0,
            quota_peak_non_paged: 0,
            quota_non_paged: 0,
            pagefile_usage: 0,
            peak_pagefile_usage: 0,
            private_usage: 0,
        };
        let mem_ok = unsafe { GetProcessMemoryInfo(handle, &mut pmc, pmc.cb) };
        if mem_ok == 0 && std::env::var_os("CAM_SHADOW17_DEBUG").is_some() {
            eprintln!(
                "BENCH17 GetProcessMemoryInfo failed, GetLastError={}",
                unsafe { GetLastError() }
            );
        }
        let mut io = IoCounters::default();
        let io_ok = unsafe { GetProcessIoCounters(handle, &mut io) };
        ChildMetrics {
            user_cpu_s: if times_ok == 0 {
                0.0
            } else {
                user as f64 * 1e-7
            },
            kernel_cpu_s: if times_ok == 0 {
                0.0
            } else {
                kernel as f64 * 1e-7
            },
            read_ops: if io_ok == 0 { 0 } else { io.read_ops },
            read_bytes: if io_ok == 0 { 0 } else { io.read_bytes },
            write_ops: if io_ok == 0 { 0 } else { io.write_ops },
        }
    }
}

/// One measured child run: wall time and post-exit process metrics.
struct MeasuredRun {
    wall_ms: f64,
    user_cpu_s: f64,
    kernel_cpu_s: f64,
    peak_working_set: u64,
    read_ops: u64,
    read_bytes: u64,
    write_ops: u64,
    stdout_bytes: usize,
}

#[cfg(windows)]
fn measure_child<F>(spawn: F) -> (MeasuredRun, std::process::Child, Vec<u8>)
where
    F: FnOnce() -> std::process::Child,
{
    use std::sync::atomic::AtomicBool as StopFlag;
    let started = Instant::now();
    let mut child = spawn();
    let pid = child.id();
    let stop = std::sync::Arc::new(StopFlag::new(false));
    let sampler_stop = std::sync::Arc::clone(&stop);
    let peak_sampler =
        std::thread::spawn(move || win_metrics::sample_peak_working_set(pid, sampler_stop));
    let stdout_data = read_stdout_to_end(&mut child);
    let wall_ms = started.elapsed().as_secs_f64() * 1000.0;
    // The response has been fully written; give the sampler a short grace
    // window to observe teardown, then stop it and read post-exit metrics.
    std::thread::sleep(std::time::Duration::from_millis(15));
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let metrics = win_metrics::collect(&child);
    let peak_working_set = peak_sampler.join().unwrap_or(0);
    (
        MeasuredRun {
            wall_ms,
            stdout_bytes: stdout_data.len(),
            user_cpu_s: metrics.user_cpu_s,
            kernel_cpu_s: metrics.kernel_cpu_s,
            peak_working_set,
            read_ops: metrics.read_ops,
            read_bytes: metrics.read_bytes,
            write_ops: metrics.write_ops,
        },
        child,
        stdout_data,
    )
}

fn read_stdout_to_end(child: &mut std::process::Child) -> Vec<u8> {
    use std::io::Read;
    let mut buffer = Vec::new();
    if let Some(stdout) = child.stdout.as_mut() {
        let _ = stdout.read_to_end(&mut buffer);
    }
    buffer
}

#[cfg(windows)]
#[test]
fn shadow17_release_benchmark_30_runs() {
    if std::env::var("CAM_SHADOW17_BENCH").as_deref() != Ok("1") {
        eprintln!("BENCH17 SKIP: set CAM_SHADOW17_BENCH=1 to run the 30-run release benchmark");
        return;
    }
    let _shadow_lock = lock_shadow_tests();
    let root = build_shadow17_fixture("bench");
    let env = shadow17_env(&root);
    let request = shadow17_snapshot_request(&root, "bench17");
    let request_bytes = serde_json::to_vec(&request).expect("serialize request");

    let runs = 30;
    let mut sidecar_runs: Vec<BenchRecord> = Vec::new();
    let mut worker_runs: Vec<BenchRecord> = Vec::new();

    for index in 1..=runs {
        // --- sidecar path: unified + antigravity, sequential, then normalize
        let started = Instant::now();
        let (unified_run, mut u_child, unified_bytes) = measure_child(|| {
            let mut command = std::process::Command::new(SIDECAR_EXE);
            command
                .args([
                    "daily",
                    "--json",
                    "--offline",
                    "--by-agent",
                    "--since",
                    SINCE,
                ])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null());
            apply_env(&mut command, &env);
            command.spawn().expect("spawn unified sidecar")
        });
        let u_status = u_child.wait().expect("wait unified sidecar");
        let (antigravity_run, mut a_child, antigravity_bytes) = measure_child(|| {
            let mut command = std::process::Command::new(ANTIGRAVITY_EXE);
            command
                .args([
                    "antigravity",
                    "daily",
                    "--json",
                    "--offline",
                    "--since",
                    SINCE,
                ])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null());
            apply_env(&mut command, &env);
            command.spawn().expect("spawn antigravity sidecar")
        });
        let a_status = a_child.wait().expect("wait antigravity sidecar");
        assert!(
            u_status.success() && a_status.success(),
            "sidecar bench run {index} failed"
        );
        let normalize_started = Instant::now();
        let unified = String::from_utf8_lossy(&unified_bytes).into_owned();
        let _ = adapter::normalize_reports(&unified, r#"{"daily":[]}"#, TODAY, COLLECTED_AT)
            .expect("normalize unified");
        let _ = adapter::normalize_reports(
            &unified,
            &String::from_utf8_lossy(&antigravity_bytes),
            TODAY,
            COLLECTED_AT,
        )
        .expect("normalize antigravity");
        let normalize_ms = normalize_started.elapsed().as_secs_f64() * 1000.0;
        let total_wall_ms = started.elapsed().as_secs_f64() * 1000.0;
        eprintln!(
            "BENCH17 RUN,sidecar,{index},{:.3},{:.3},{:.3},{:.4},{:.4},{},{},{},{},{}",
            total_wall_ms,
            unified_run.wall_ms + antigravity_run.wall_ms,
            normalize_ms,
            unified_run.user_cpu_s + antigravity_run.user_cpu_s,
            unified_run.kernel_cpu_s + antigravity_run.kernel_cpu_s,
            unified_run
                .peak_working_set
                .max(antigravity_run.peak_working_set),
            unified_run.read_ops + antigravity_run.read_ops,
            unified_run.read_bytes + antigravity_run.read_bytes,
            unified_run.write_ops + antigravity_run.write_ops,
            unified_run.stdout_bytes + antigravity_run.stdout_bytes,
        );
        sidecar_runs.push(BenchRecord {
            total_wall_ms,
            normalize_ms,
            spawn_wall_ms: unified_run.wall_ms + antigravity_run.wall_ms,
            user_cpu_s: unified_run.user_cpu_s + antigravity_run.user_cpu_s,
            kernel_cpu_s: unified_run.kernel_cpu_s + antigravity_run.kernel_cpu_s,
            peak_working_set: unified_run
                .peak_working_set
                .max(antigravity_run.peak_working_set),
            read_ops: unified_run.read_ops + antigravity_run.read_ops,
            read_bytes: unified_run.read_bytes + antigravity_run.read_bytes,
            write_ops: unified_run.write_ops + antigravity_run.write_ops,
            stdout_bytes: unified_run.stdout_bytes + antigravity_run.stdout_bytes,
        });

        // --- worker path: one fresh product-EXE process per run
        let started = Instant::now();
        let (worker_measure, mut w_child, response_bytes) = measure_child(|| {
            let mut command = std::process::Command::new(worker_exe());
            command
                .arg(coding_agent_monitor_lib::collector::worker::INTERNAL_FLAG)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null());
            apply_env(&mut command, &env);
            let mut child = command.spawn().expect("spawn worker");
            use std::io::Write;
            child
                .stdin
                .as_mut()
                .expect("worker stdin")
                .write_all(&request_bytes)
                .expect("write worker request");
            child.stdin.take(); // close stdin: one request = EOF
            child
        });
        let status = w_child.wait().expect("wait worker");
        assert!(status.success(), "worker bench run {index} failed");
        let normalize_started = Instant::now();
        let snapshot: CollectorSnapshotResponseV1 =
            serde_json::from_slice(&response_bytes).expect("parse worker response");
        let _ = adapter::normalize_snapshot(&snapshot, TODAY, COLLECTED_AT).expect("normalize");
        let normalize_ms = normalize_started.elapsed().as_secs_f64() * 1000.0;
        eprintln!(
            "BENCH17 RUN,worker,{index},{:.3},{:.3},{:.3},{:.4},{:.4},{},{},{},{},{}",
            started.elapsed().as_secs_f64() * 1000.0,
            worker_measure.wall_ms,
            normalize_ms,
            worker_measure.user_cpu_s,
            worker_measure.kernel_cpu_s,
            worker_measure.peak_working_set,
            worker_measure.read_ops,
            worker_measure.read_bytes,
            worker_measure.write_ops,
            worker_measure.stdout_bytes,
        );
        worker_runs.push(BenchRecord {
            total_wall_ms: started.elapsed().as_secs_f64() * 1000.0,
            normalize_ms,
            spawn_wall_ms: worker_measure.wall_ms,
            user_cpu_s: worker_measure.user_cpu_s,
            kernel_cpu_s: worker_measure.kernel_cpu_s,
            peak_working_set: worker_measure.peak_working_set,
            read_ops: worker_measure.read_ops,
            read_bytes: worker_measure.read_bytes,
            write_ops: worker_measure.write_ops,
            stdout_bytes: worker_measure.stdout_bytes,
        });
    }

    // One extra segment-instrumented run (not part of the 30).
    run_segment_instrumented(&env, &request_bytes);

    print_bench_report("sidecar", &sidecar_runs);
    print_bench_report("worker", &worker_runs);

    let sidecar_median = median(&sidecar_runs[1..], |r| r.total_wall_ms);
    let worker_median = median(&worker_runs[1..], |r| r.total_wall_ms);
    eprintln!(
        "BENCH17 ratio (warm median): worker/sidecar = {:.2} (gate: <= 1.25)",
        worker_median / sidecar_median
    );
    fs::remove_dir_all(&root).ok();
}

struct BenchRecord {
    /// spawn(s) + wait + normalize — one full refresh, production shape.
    total_wall_ms: f64,
    /// Spawn-to-exit wall of the child process(es) only.
    spawn_wall_ms: f64,
    /// Parse + normalize time in the parent (Rust, release profile).
    normalize_ms: f64,
    user_cpu_s: f64,
    kernel_cpu_s: f64,
    peak_working_set: u64,
    read_ops: u64,
    read_bytes: u64,
    write_ops: u64,
    stdout_bytes: usize,
}

fn median<F: Fn(&BenchRecord) -> f64>(runs: &[BenchRecord], f: F) -> f64 {
    let mut values: Vec<f64> = runs.iter().map(&f).collect();
    values.sort_by(|a, b| a.partial_cmp(b).unwrap());
    values[values.len() / 2]
}

fn percentile<F: Fn(&BenchRecord) -> f64>(runs: &[BenchRecord], p: f64, f: F) -> f64 {
    let mut values: Vec<f64> = runs.iter().map(&f).collect();
    values.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let index = ((values.len() as f64 * p) as usize).min(values.len() - 1);
    values[index]
}

fn print_bench_report(label: &str, runs: &[BenchRecord]) {
    let warm = &runs[1..];
    let fields: Vec<(&str, Box<dyn Fn(&BenchRecord) -> f64>)> = vec![
        ("totalWallMs", Box::new(|r: &BenchRecord| r.total_wall_ms)),
        ("spawnWallMs", Box::new(|r: &BenchRecord| r.spawn_wall_ms)),
        ("normalizeMs", Box::new(|r: &BenchRecord| r.normalize_ms)),
        ("userCpuS", Box::new(|r: &BenchRecord| r.user_cpu_s)),
        ("kernelCpuS", Box::new(|r: &BenchRecord| r.kernel_cpu_s)),
        (
            "peakWorkingSetBytes",
            Box::new(|r: &BenchRecord| r.peak_working_set as f64),
        ),
        ("readOps", Box::new(|r: &BenchRecord| r.read_ops as f64)),
        ("readBytes", Box::new(|r: &BenchRecord| r.read_bytes as f64)),
        ("writeOps", Box::new(|r: &BenchRecord| r.write_ops as f64)),
        (
            "stdoutBytes",
            Box::new(|r: &BenchRecord| r.stdout_bytes as f64),
        ),
    ];
    for (field_name, field) in &fields {
        let first = field(&runs[0]);
        let warm_values: Vec<f64> = warm.iter().map(|r| field(r)).collect();
        let min = warm_values.iter().cloned().fold(f64::INFINITY, f64::min);
        let max = warm_values
            .iter()
            .cloned()
            .fold(f64::NEG_INFINITY, f64::max);
        eprintln!(
            "BENCH17 {}: {} first_run={:.3} warm_min={:.3} warm_median={:.3} warm_p95={:.3} warm_max={:.3}",
            label,
            field_name,
            first,
            min,
            median(warm, |r| field(r)),
            percentile(warm, 0.95, |r| field(r)),
            max,
        );
    }
}

/// One extra worker run with `CAM_WORKER_SEGMENT_LOG=1` — segment timings
/// printed from the worker's stderr (never part of the 30 measured runs).
#[cfg(windows)]
fn run_segment_instrumented(env: &[(&'static str, PathBuf)], request_bytes: &[u8]) {
    let mut command = std::process::Command::new(worker_exe());
    command
        .arg(coding_agent_monitor_lib::collector::worker::INTERNAL_FLAG)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .env("CAM_WORKER_SEGMENT_LOG", "1");
    apply_env(&mut command, env);
    let mut child = command.spawn().expect("spawn segment worker");
    {
        use std::io::Write;
        child
            .stdin
            .as_mut()
            .expect("stdin")
            .write_all(request_bytes)
            .expect("write request");
    }
    child.stdin.take();
    let mut stderr_buffer = Vec::new();
    if let Some(stderr) = child.stderr.as_mut() {
        use std::io::Read;
        let _ = stderr.read_to_end(&mut stderr_buffer);
    }
    let stdout = read_stdout_to_end(&mut child);
    let _status = child.wait().expect("wait segment worker");
    let segments: Vec<String> = String::from_utf8_lossy(&stderr_buffer)
        .lines()
        .filter(|line| line.contains("cam_worker_segment"))
        .map(str::to_string)
        .collect();
    eprintln!(
        "BENCH17 segments (one instrumented run, stdout {} bytes, {} segment lines):",
        stdout.len(),
        segments.len()
    );
    for segment in segments {
        eprintln!("BENCH17 SEG {segment}");
    }
}
