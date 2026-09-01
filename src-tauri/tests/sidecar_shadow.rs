//! Phase 4A shadow harness: runs the v0.2 sidecar path and the v0.3 batch
//! worker path against the SAME fixtures/environment and compares the
//! resulting `UsageSummary` values field by field.
//!
//! - The sidecar executable comes from `CAM_SHADOW_SIDECAR_EXE` (externally
//!   pinned ccusage v20.0.20); the worker is the vendored v20.0.20 + #1487
//!   port inside the product EXE. The sidecar supply chain was removed in
//!   Phase 5, so this is an opt-in upgrade audit tool.
//! - Both paths see identical env vars, roots, timezone and `--since` window.
//! - Dev/test only: this binary is never packaged into the release installer.
//!
//! Without the env var the shadow tests skip rather than fabricate a result.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use coding_agent_monitor_lib::collector::snapshot_protocol::{
    AgentSpecV1, CollectorSnapshotRequestV1, CollectorSnapshotResponseV1,
};
use coding_agent_monitor_lib::collector::{supervisor, AgentKind};
use coding_agent_monitor_lib::sidecar::adapter;
use coding_agent_monitor_lib::usage::UsageSummary;

/// Phase 5: sidecar binaries are no longer staged/bundled. This harness is an
/// explicit opt-in upgrade audit tool — point `CAM_SHADOW_SIDECAR_EXE` at an
/// externally pinned ccusage v20.0.20 build; without it the sidecar
/// comparison tests skip (the default build/test never needs a sidecar).
fn sidecar_exe() -> Option<PathBuf> {
    std::env::var_os("CAM_SHADOW_SIDECAR_EXE").map(PathBuf::from)
}
const WORKER_EXE: &str = env!("CARGO_BIN_EXE_coding-agent-monitor");

const SINCE: &str = "20260101";

/// Serializes shadow tests: they share process-global env vars (injection
/// markers) and the supervisor's exe override, so concurrent tests would
/// interfere. The 20-concurrent test uses explicit threads for concurrency.
static SHADOW_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock_shadow_tests() -> std::sync::MutexGuard<'static, ()> {
    SHADOW_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Env vars that must point at the fixture (or empty scratch dirs) for both
/// paths. Every agent the sidecar scans gets an explicit value so neither path
/// accidentally reads real user data.
const ENV_KEYS: [&str; 17] = [
    "CLAUDE_CONFIG_DIR",
    "CODEX_HOME",
    "OPENCODE_DATA_DIR",
    "AMP_DATA_DIR",
    "DROID_SESSIONS_DIR",
    "CODEBUFF_DATA_DIR",
    "HERMES_HOME",
    "PI_AGENT_DIR",
    "GOOSE_PATH_ROOT",
    "OPENCLAW_DIR",
    "KILO_DATA_DIR",
    "COPILOT_OTEL_FILE_EXPORTER_PATH",
    "GEMINI_DATA_DIR",
    "KIMI_DATA_DIR",
    "QWEN_DATA_DIR",
    "GROK_HOME",
    "ANTIGRAVITY_DATA_DIR",
];

fn fixture_root(name: &str) -> PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("cam-shadow-{name}-{unique}"));
    std::fs::create_dir_all(&root).expect("create shadow fixture root");
    root
}

/// Builds the shared fixture environment: the claude and codex fixture dirs
/// get data; every other agent root is an empty directory so both paths agree
/// on "no data".
fn build_shadow_fixture(name: &str) -> PathBuf {
    let root = fixture_root(name);
    for key in ENV_KEYS {
        let dir = root.join(key.to_ascii_lowercase());
        std::fs::create_dir_all(&dir).expect("mkdir agent root");
    }
    // claude fixture: two days, two models.
    let claude_projects = root.join("claude_config_dir/projects/cam");
    std::fs::create_dir_all(&claude_projects).expect("mkdir claude projects");
    std::fs::write(
        claude_projects.join("session-a.jsonl"),
        concat!(
            r#"{"timestamp":"2026-01-02T00:00:00.000Z","sessionId":"s","requestId":"r1","costUSD":0.01,"message":{"usage":{"input_tokens":100,"output_tokens":50,"cache_creation_input_tokens":0,"cache_read_input_tokens":10},"model":"claude-sonnet-4-20250514","id":"m1"}}"#,
            "\n",
            r#"{"timestamp":"2026-01-03T08:30:00.000Z","sessionId":"s","requestId":"r2","costUSD":0.125,"message":{"usage":{"input_tokens":300,"output_tokens":30,"cache_creation_input_tokens":5,"cache_read_input_tokens":3},"model":"claude-sonnet-4-20250514","id":"m2"}}"#,
        ),
    )
    .expect("write claude fixture");
    // codex fixture: one day with reasoning.
    let codex_sessions = root.join("codex_home/sessions");
    std::fs::create_dir_all(&codex_sessions).expect("mkdir codex sessions");
    std::fs::write(
        codex_sessions.join("session-a.jsonl"),
        r#"{"timestamp":"2026-01-02T08:01:00.000Z","type":"event_msg","payload":{"type":"token_count","info":{"model":"gpt-5.2","last_token_usage":{"input_tokens":1000,"cached_input_tokens":100,"output_tokens":200,"reasoning_output_tokens":20,"total_tokens":1320}}}}"#,
    )
    .expect("write codex fixture");
    root
}

/// The env-var map both paths use: agent vars point at the fixture layout,
/// home vars at empty scratch dirs.
fn shadow_env(root: &Path) -> Vec<(&'static str, PathBuf)> {
    let mut env: Vec<(&'static str, PathBuf)> = ENV_KEYS
        .iter()
        .map(|key| {
            let dir = match *key {
                "CLAUDE_CONFIG_DIR" => root.join("claude_config_dir"),
                "CODEX_HOME" => root.join("codex_home"),
                other => root.join(other.to_ascii_lowercase()),
            };
            (*key, dir)
        })
        .collect();
    for (key, name) in [
        ("HOME", "shadow-home"),
        ("USERPROFILE", "shadow-home"),
        ("XDG_CONFIG_HOME", "shadow-home"),
    ] {
        env.push((key, root.join(name)));
    }
    env
}

/// Runs the v20.0.20 unified sidecar CLI and returns the unified JSON string.
fn run_sidecar_unified(env: &[(&'static str, PathBuf)]) -> Result<String, String> {
    let sidecar = sidecar_exe().expect("caller verifies CAM_SHADOW_SIDECAR_EXE");
    let mut command = Command::new(sidecar);
    command
        .args([
            "daily",
            "--json",
            "--offline",
            "--by-agent",
            "--since",
            SINCE,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, path) in env {
        command.env(key, path);
    }
    let output = command.output().map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "sidecar exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
                .chars()
                .take(200)
                .collect::<String>()
        ));
    }
    String::from_utf8(output.stdout).map_err(|error| error.to_string())
}

/// Runs the worker snapshot via the supervisor with the same env vars.
fn run_worker_snapshot(
    env: &[(&'static str, PathBuf)],
) -> Result<CollectorSnapshotResponseV1, String> {
    // The supervisor test seam: point at the real product binary.
    supervisor::set_worker_exe_override(std::path::PathBuf::from(WORKER_EXE));

    let agents = AgentKind::ALL;
    let request = CollectorSnapshotRequestV1::new("shadow", &agents);
    // Rewrite each agent's source to the fixture Paths (same dirs the sidecar
    // saw via env), so both paths read identical data.
    let mut request = request;
    request.agents = agents
        .iter()
        .map(|agent| {
            let key = env
                .iter()
                .find(|(name, _)| env_key_for(*agent) == Some(*name))
                .map(|(_, path)| path.clone());
            let roots = match key {
                Some(path) => vec![path.to_string_lossy().into_owned()],
                None => vec![],
            };
            AgentSpecV1 {
                agent: agent.id().to_string(),
                source: coding_agent_monitor_lib::collector::protocol::DataSourceV1::Paths {
                    roots,
                },
            }
        })
        .collect();

    let cancel = AtomicBool::new(false);
    supervisor::collect_snapshot_with_options(&request, &cancel, Duration::from_secs(120))
        .map_err(|error| format!("worker snapshot failed: {error}"))
}

fn env_key_for(agent: AgentKind) -> Option<&'static str> {
    Some(match agent {
        AgentKind::Claude => "CLAUDE_CONFIG_DIR",
        AgentKind::Codex => "CODEX_HOME",
        AgentKind::OpenCode => "OPENCODE_DATA_DIR",
        AgentKind::Amp => "AMP_DATA_DIR",
        AgentKind::Droid => "DROID_SESSIONS_DIR",
        AgentKind::Codebuff => "CODEBUFF_DATA_DIR",
        AgentKind::Hermes => "HERMES_HOME",
        AgentKind::Pi => "PI_AGENT_DIR",
        AgentKind::Goose => "GOOSE_PATH_ROOT",
        AgentKind::OpenClaw => "OPENCLAW_DIR",
        AgentKind::Kilo => "KILO_DATA_DIR",
        AgentKind::Copilot => "COPILOT_OTEL_FILE_EXPORTER_PATH",
        AgentKind::Gemini => "GEMINI_DATA_DIR",
        AgentKind::Kimi => "KIMI_DATA_DIR",
        AgentKind::Qwen => "QWEN_DATA_DIR",
        AgentKind::Grok => "GROK_HOME",
        AgentKind::Antigravity => "ANTIGRAVITY_DATA_DIR",
    })
}

/// Converts the snapshot response into a UsageSummary through the CAM adapter.
fn snapshot_to_summary(snapshot: &CollectorSnapshotResponseV1) -> Result<UsageSummary, String> {
    adapter::normalize_snapshot(snapshot, "2026-01-03", "2026-01-03T12:00:00Z")
        .map_err(|error| format!("snapshot normalize failed: {error}"))
}

/// Field-by-field comparison of two UsageSummary values. Returns a list of
/// human-readable differences (empty = parity).
fn diff_summaries(sidecar: &UsageSummary, worker: &UsageSummary) -> Vec<String> {
    let mut diffs = Vec::new();
    if sidecar.collected_at != worker.collected_at {
        // Both are set by the caller to the same value; a mismatch is a bug.
        diffs.push(format!(
            "collected_at: sidecar={:?} worker={:?}",
            sidecar.collected_at, worker.collected_at
        ));
    }
    let pairs = sidecar.last7_days.iter().zip(worker.last7_days.iter());
    for (index, (s, w)) in pairs.enumerate() {
        if s.date != w.date {
            diffs.push(format!("day[{index}].date: {} vs {}", s.date, w.date));
        }
        if s.total_tokens != w.total_tokens {
            diffs.push(format!(
                "day[{}].totalTokens: {} vs {}",
                s.date, s.total_tokens, w.total_tokens
            ));
        }
        let sb = &s.token_breakdown;
        let wb = &w.token_breakdown;
        // Category-4 difference (documented): the v0.2 sidecar JSON cannot
        // express reasoning for codex (no extraTotalTokens field), so the
        // v0.2 adapter classifies the entire residue as unclassified. The
        // worker splits it correctly into reasoning + unclassified. The total
        // and all other buckets must still match exactly.
        let _codex_reasoning_reclassification = sb.unclassified_tokens >= wb.unclassified_tokens
            && wb.reasoning_tokens >= sb.reasoning_tokens
            && sb.reasoning_tokens + sb.unclassified_tokens
                == wb.reasoning_tokens + wb.unclassified_tokens;
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
                diffs.push(format!(
                    "day[{}].tokenBreakdown.{}: {} vs {}",
                    s.date, name, sv, wv
                ));
            }
        }
        match (s.estimated_cost_usd, w.estimated_cost_usd) {
            (None, None) => {}
            (Some(sc), Some(wc)) => {
                // Compare in nano-USD to avoid float noise.
                let s_nano = (sc * 1e9).round() as i128;
                let w_nano = (wc * 1e9).round() as i128;
                if s_nano != w_nano {
                    diffs.push(format!(
                        "day[{}].estimatedCostUsd: {} vs {} (nano: {} vs {})",
                        s.date, sc, wc, s_nano, w_nano
                    ));
                }
            }
            (sc, wc) => diffs.push(format!(
                "day[{}].estimatedCostUsd: sidecar={sc:?} worker={wc:?} (null-vs-value mismatch)",
                s.date
            )),
        }
        if s.agents.len() != w.agents.len() {
            diffs.push(format!(
                "day[{}].agents count: {} vs {}",
                s.date,
                s.agents.len(),
                w.agents.len()
            ));
        }
        for (sa, wa) in s.agents.iter().zip(w.agents.iter()) {
            if sa.id != wa.id {
                diffs.push(format!("day[{}].agent.id: {} vs {}", s.date, sa.id, wa.id));
            }
            if sa.display_name != wa.display_name {
                diffs.push(format!(
                    "day[{}].agent.{} displayName: {:?} vs {:?}",
                    s.date, sa.id, sa.display_name, wa.display_name
                ));
            }
            if sa.tokens != wa.tokens {
                diffs.push(format!(
                    "day[{}].agent.{}.tokens: {} vs {}",
                    s.date, sa.id, sa.tokens, wa.tokens
                ));
            }
            // Category-4 documented difference: for codex the sidecar cannot
            // express reasoning (no extraTotalTokens in its JSON), so the
            // reasoning/unclassified classification differs while the sum
            // and total match. Only flag when the combined residue differs.
            let residue_matches = sa.reasoning_tokens + sa.unclassified_tokens
                == wa.reasoning_tokens + wa.unclassified_tokens;
            let codex_exception = sa.id == "codex" && residue_matches;
            if !codex_exception {
                if sa.reasoning_tokens != wa.reasoning_tokens {
                    diffs.push(format!(
                        "day[{}].agent.{}.reasoningTokens: {} vs {}",
                        s.date, sa.id, sa.reasoning_tokens, wa.reasoning_tokens
                    ));
                }
                if sa.unclassified_tokens != wa.unclassified_tokens {
                    diffs.push(format!(
                        "day[{}].agent.{}.unclassifiedTokens: {} vs {}",
                        s.date, sa.id, sa.unclassified_tokens, wa.unclassified_tokens
                    ));
                }
            }
            if sa.models.len() != wa.models.len() {
                diffs.push(format!(
                    "day[{}].agent.{}.models count: {} vs {}",
                    s.date,
                    sa.id,
                    sa.models.len(),
                    wa.models.len()
                ));
            }
            for (sm, wm) in sa.models.iter().zip(wa.models.iter()) {
                if sm.model_name != wm.model_name {
                    diffs.push(format!(
                        "day[{}].agent.{}.model: {} vs {}",
                        s.date, sa.id, sm.model_name, wm.model_name
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
                            "day[{}].agent.{}.model.{}.{}: {} vs {}",
                            s.date, sa.id, sm.model_name, name, sv, wv
                        ));
                    }
                }
            }
        }
    }
    diffs
}

#[test]
fn shadow_claude_codex_sidecar_vs_worker() {
    let _shadow_lock = lock_shadow_tests();
    if sidecar_exe().is_none() {
        eprintln!("SHADOW SKIP: set CAM_SHADOW_SIDECAR_EXE to run the sidecar audit");
        return;
    }
    let root = build_shadow_fixture("claude-codex");
    let env = shadow_env(&root);

    let unified_json = run_sidecar_unified(&env).expect("sidecar unified run");
    let sidecar_summary = adapter::normalize_reports(
        &unified_json,
        r#"{"daily":[]}"#,
        "2026-01-03",
        "2026-01-03T12:00:00Z",
    )
    .expect("sidecar normalize");

    let snapshot = run_worker_snapshot(&env).expect("worker snapshot run");
    let worker_summary = snapshot_to_summary(&snapshot).expect("worker normalize");

    let diffs = diff_summaries(&sidecar_summary, &worker_summary);
    assert!(
        diffs.is_empty(),
        "shadow parity violations (sidecar v20.0.20 vs vendored worker):\n{}",
        diffs.join("\n")
    );
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn shadow_full_17_agent_matrix() {
    let _shadow_lock = lock_shadow_tests();
    if sidecar_exe().is_none() {
        eprintln!("SHADOW SKIP: set CAM_SHADOW_SIDECAR_EXE to run the sidecar audit");
        return;
    }
    let root = build_shadow_fixture("full-matrix");
    let env = shadow_env(&root);

    let unified_json = run_sidecar_unified(&env).expect("sidecar unified run");
    let sidecar_summary = adapter::normalize_reports(
        &unified_json,
        r#"{"daily":[]}"#,
        "2026-01-03",
        "2026-01-03T12:00:00Z",
    )
    .expect("sidecar normalize");

    let snapshot = run_worker_snapshot(&env).expect("worker snapshot run");
    let worker_summary = snapshot_to_summary(&snapshot).expect("worker normalize");

    // Per-agent parity accounting: for each of the 17 agents, record whether
    // both paths agree (PASS) or report the difference.
    let mut matrix = Vec::new();
    for agent in AgentKind::ALL {
        let mut agent_diffs = Vec::new();
        for (index, (s, w)) in sidecar_summary
            .last7_days
            .iter()
            .zip(worker_summary.last7_days.iter())
            .enumerate()
        {
            let sa = s.agents.iter().find(|a| a.id == agent.id());
            let wa = w.agents.iter().find(|a| a.id == agent.id());
            match (sa, wa) {
                (None, None) => {} // both absent = agree
                (Some(sa), Some(wa)) => {
                    if sa.tokens != wa.tokens {
                        agent_diffs.push(format!(
                            "day[{index}] tokens {} vs {}",
                            sa.tokens, wa.tokens
                        ));
                    }
                    // Category-4: codex reasoning is inexpressible in the
                    // sidecar JSON; the combined residue must still match.
                    let residue_sidecar = sa.reasoning_tokens + sa.unclassified_tokens;
                    let residue_worker = wa.reasoning_tokens + wa.unclassified_tokens;
                    if agent.id() == "codex" {
                        if residue_sidecar != residue_worker {
                            agent_diffs.push(format!(
                                "day[{index}] codex residue {} vs {}",
                                residue_sidecar, residue_worker
                            ));
                        }
                    } else {
                        if sa.reasoning_tokens != wa.reasoning_tokens {
                            agent_diffs.push(format!(
                                "day[{index}] reasoning {} vs {}",
                                sa.reasoning_tokens, wa.reasoning_tokens
                            ));
                        }
                        if sa.unclassified_tokens != wa.unclassified_tokens {
                            agent_diffs.push(format!(
                                "day[{index}] unclassified {} vs {}",
                                sa.unclassified_tokens, wa.unclassified_tokens
                            ));
                        }
                    }
                }
                (Some(sa), None) => {
                    agent_diffs.push(format!("day[{index}] sidecar has {sa:?}, worker absent"));
                }
                (None, Some(wa)) => {
                    agent_diffs.push(format!("day[{index}] worker has {wa:?}, sidecar absent"));
                }
            }
        }
        matrix.push((agent, agent_diffs.is_empty(), agent_diffs));
    }

    let failures: Vec<_> = matrix.iter().filter(|(_, ok, _)| !ok).collect();
    for (agent, ok, diffs) in &matrix {
        eprintln!(
            "SHADOW {}: {}{}",
            agent.id(),
            if *ok { "PASS" } else { "FAIL" },
            if diffs.is_empty() {
                String::new()
            } else {
                format!(" — {}", diffs.join("; "))
            }
        );
    }
    assert!(
        failures.is_empty(),
        "shadow parity failures: {}",
        failures
            .iter()
            .map(|(agent, _, diffs)| format!("{}: {}", agent.id(), diffs.join("; ")))
            .collect::<Vec<_>>()
            .join("\n")
    );
    std::fs::remove_dir_all(&root).ok();
}

// --- Batch single-flight: 20 concurrent full refreshes share ONE worker ------

#[test]
fn snapshot_twenty_concurrent_calls_share_one_worker() {
    let _shadow_lock = lock_shadow_tests();
    // The supervisor must spawn the real product EXE, not the test harness.
    supervisor::set_worker_exe_override(std::path::PathBuf::from(WORKER_EXE));
    let marker = std::env::temp_dir().join(format!(
        "cam-snapshot-spawns-{}.txt",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    std::env::set_var(
        "CAM_TEST_WORKER_SPAWN_MARKER",
        marker.to_string_lossy().as_ref(),
    );
    std::env::set_var("CAM_TEST_WORKER_SLEEP_MS", "2000");

    // Staggered start: the first thread claims the flight; the remaining 19
    // start after short delays so they join the still-active flight (the
    // 2000 ms worker sleep keeps it alive long enough for all callers).
    let handles: Vec<_> = (0..20)
        .map(|i| {
            std::thread::spawn(move || {
                if i > 0 {
                    std::thread::sleep(std::time::Duration::from_millis(50 * i as u64));
                }
                coding_agent_monitor_lib::collector::worker_runner::collect_snapshot(
                    &CollectorSnapshotRequestV1::new("sf-snapshot", &AgentKind::ALL),
                )
            })
        })
        .collect();
    let mut results = Vec::new();
    for handle in handles {
        results.push(handle.join().expect("caller thread"));
    }
    // All 20 callers share the same outcome (success or failure — both are
    // valid since the worker reads the real environment).
    let first = &results[0];
    for result in &results[1..] {
        match (first, result) {
            (Ok(a), Ok(b)) => assert_eq!(a.request_id, b.request_id),
            (Err(_), Err(_)) => {}
            (a, b) => panic!("divergent single-flight results: {a:?} vs {b:?}"),
        }
    }

    let spawns = std::fs::read_to_string(&marker).unwrap_or_default();
    let count = spawns.lines().count();
    assert_eq!(
        count, 1,
        "20 concurrent full refreshes must share ONE worker (saw {count})"
    );
    std::env::remove_var("CAM_TEST_WORKER_SPAWN_MARKER");
    std::env::remove_var("CAM_TEST_WORKER_SLEEP_MS");
    std::fs::remove_file(&marker).ok();
}

// --- Performance baseline (10 warm runs per path) ----------------------------

#[test]
fn perf_baseline_sidecar_vs_worker_10_warm_runs() {
    let _shadow_lock = lock_shadow_tests();
    if sidecar_exe().is_none() {
        eprintln!("PERF SKIP: set CAM_SHADOW_SIDECAR_EXE to run the sidecar audit");
        return;
    }
    supervisor::set_worker_exe_override(std::path::PathBuf::from(WORKER_EXE));
    let root = build_shadow_fixture("perf");
    let env = shadow_env(&root);

    // Warm-up (not recorded).
    let _ = run_sidecar_unified(&env);
    let snapshot_req = snapshot_request_for_fixture(&root);
    {
        let cancel = AtomicBool::new(false);
        let _ = supervisor::collect_snapshot_with_options(
            &snapshot_req,
            &cancel,
            Duration::from_secs(120),
        );
    }

    let mut sidecar_times = Vec::new();
    for _ in 0..10 {
        let started = Instant::now();
        let json = run_sidecar_unified(&env).expect("sidecar perf run");
        let _ = adapter::normalize_reports(
            &json,
            r#"{"daily":[]}"#,
            "2026-01-03",
            "2026-01-03T12:00:00Z",
        )
        .expect("normalize");
        sidecar_times.push(started.elapsed());
    }

    let mut worker_times = Vec::new();
    for _ in 0..10 {
        let started = Instant::now();
        let cancel = AtomicBool::new(false);
        let snapshot = supervisor::collect_snapshot_with_options(
            &snapshot_req,
            &cancel,
            Duration::from_secs(120),
        )
        .expect("worker perf run");
        let _ = snapshot_to_summary(&snapshot).expect("normalize");
        worker_times.push(started.elapsed());
    }

    let median = |mut times: Vec<Duration>| {
        times.sort();
        times[times.len() / 2]
    };
    let p95 = |mut times: Vec<Duration>| {
        times.sort();
        times[(times.len() as f64 * 0.95) as usize % times.len()]
    };
    let s_median = median(sidecar_times.clone());
    let w_median = median(worker_times.clone());
    eprintln!(
        "PERF sidecar: median={s_median:?} p95={:?} | worker: median={w_median:?} p95={:?}",
        p95(sidecar_times),
        p95(worker_times)
    );
    eprintln!(
        "PERF ratio: worker median / sidecar median = {:.2}",
        w_median.as_secs_f64() / s_median.as_secs_f64().max(0.001)
    );

    if w_median > s_median.mul_f64(1.25) {
        eprintln!(
            "PERF BLOCKER for Phase 4B: worker median ({w_median:?}) exceeds 125% of the sidecar median ({s_median:?})"
        );
        // Recorded as a Phase 4B blocker, not a test failure — Phase 4A only
        // establishes the baseline.
    }
    std::fs::remove_dir_all(&root).ok();
}

fn snapshot_request_for_fixture(root: &Path) -> CollectorSnapshotRequestV1 {
    let mut request = CollectorSnapshotRequestV1::new("perf", &AgentKind::ALL);
    request.agents = AgentKind::ALL
        .iter()
        .map(|agent| AgentSpecV1 {
            agent: agent.id().to_string(),
            source: coding_agent_monitor_lib::collector::protocol::DataSourceV1::Paths {
                roots: vec![root
                    .join(
                        env_key_for(*agent)
                            .unwrap_or("unknown")
                            .to_ascii_lowercase(),
                    )
                    .to_string_lossy()
                    .into_owned()],
            },
        })
        .collect();
    request
}
