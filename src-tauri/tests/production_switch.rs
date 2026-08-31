//! Phase 4B production-path verification: the public `get_usage_summary`
//! chain (`worker_runner::collect_usage`) after the sidecar → batch-worker
//! switch.
//!
//! Coverage (see docs/V0.3_PHASE4B_PRODUCTION_SWITCH.md §3 for the policy
//! table these tests pin):
//! - uninstalled/absent agents are empty successes, never errors;
//! - mixed installed/absent registries produce exactly the installed agents;
//! - approved v0.3 value semantics on the public contract (reasoning vs
//!   unclassified, unpriced-only and mixed-pricing day cost null);
//! - worker panics fail the WHOLE refresh, the failure is cached like v0.2,
//!   and the next refresh recovers;
//! - 20 concurrent public refreshes share ONE worker;
//! - the production chain never consults the sidecar: a poisoned
//!   (spawn-failing) sidecar at the runner's packed path is never executed,
//!   and no `ccusage*` process ever appears.
//!
//! The shutdown-race scenario lives in `production_shutdown.rs`: the shared
//! shutdown flag is process-global and sticky, so it must not share a test
//! process with the rest of this suite.

mod common;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use coding_agent_monitor_lib::collector::worker_runner::{
    collect_usage, production_snapshot_request,
};
use coding_agent_monitor_lib::collector::{supervisor, AgentKind};

fn lock_tests() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let guard = LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    // Scenario isolation: the snapshot result cache is process-global in the
    // real app; a test must never observe a previous scenario's cached
    // success/failure.
    coding_agent_monitor_lib::collector::worker_runner::clear_snapshot_result_cache_for_tests();
    guard
}

fn worker_override() {
    // The override seam is debug/test-only by design; release builds of this
    // suite (perf benchmark) exercise the production `current_exe()` path.
    #[cfg(debug_assertions)]
    supervisor::set_worker_exe_override(std::path::PathBuf::from(env!(
        "CARGO_BIN_EXE_coding-agent-monitor"
    )));
}

fn unique_dir(name: &str) -> PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("cam-prod4b-{name}-{unique}"));
    std::fs::create_dir_all(&root).expect("mkdir fixture root");
    root
}

/// Records every env var's previous value and restores it on drop.
struct EnvGuard {
    saved: Vec<(String, Option<std::ffi::OsString>)>,
}

impl EnvGuard {
    fn acquire() -> Self {
        Self { saved: Vec::new() }
    }
    fn set(&mut self, key: &str, value: &Path) {
        self.saved.push((key.to_string(), std::env::var_os(key)));
        // SAFETY: tests holding the shared lock are the only env writers.
        unsafe { std::env::set_var(key, value) };
    }
    fn set_text(&mut self, key: &str, value: &str) {
        self.saved.push((key.to_string(), std::env::var_os(key)));
        // SAFETY: see set.
        unsafe { std::env::set_var(key, value) };
    }
    fn remove(&mut self, key: &str) {
        self.saved.push((key.to_string(), std::env::var_os(key)));
        // SAFETY: see set.
        unsafe { std::env::remove_var(key) };
    }
    fn unset(&mut self, key: &str) {
        self.saved.retain(|(saved_key, _)| saved_key != key);
        // SAFETY: see set.
        unsafe { std::env::remove_var(key) };
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (key, previous) in self.saved.iter().rev() {
            // SAFETY: see set.
            unsafe {
                match previous {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
    }
}

/// Removes every agent-root env var and points the home variables at an empty
/// scratch dir: the "nothing is installed" production shape.
fn scrub_to_uninstalled(root: &Path) -> EnvGuard {
    let mut env = EnvGuard::acquire();
    for key in common::AGENT_ENV_KEYS {
        env.remove(key);
    }
    for key in ["HOME", "USERPROFILE", "XDG_CONFIG_HOME"] {
        let empty = root.join("empty-home");
        std::fs::create_dir_all(&empty).expect("mkdir empty home");
        env.set(key, &empty);
    }
    env
}

/// A fixture timestamp at LOCAL noon `offset_days` days ago (so the record
/// always lands on that local day, whatever the machine's zone is), plus that
/// local day as `YYYY-MM-DD` — the day bucket the production worker must
/// report it in.
fn local_noon(offset_days: i64) -> (String, String) {
    let local_date = chrono::Local::now().date_naive() - chrono::Duration::days(offset_days);
    let noon = local_date
        .and_hms_opt(12, 0, 0)
        .expect("noon")
        .and_local_timezone(chrono::Local)
        .earliest()
        .expect("noon local time");
    (
        local_date.format("%Y-%m-%d").to_string(),
        noon.with_timezone(&chrono::Utc)
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string(),
    )
}

/// Writes a codex fixture with one day of data (including reasoning tokens).
fn write_codex_fixture(root: &Path, timestamp: &str) {
    let codex_sessions = root.join("codex-home/sessions");
    std::fs::create_dir_all(&codex_sessions).expect("mkdir codex fixture");
    std::fs::write(
        codex_sessions.join("session-a.jsonl"),
        format!(
            r#"{{"timestamp":"{timestamp}","type":"event_msg","payload":{{"type":"token_count","info":{{"model":"gpt-5.2","last_token_usage":{{"input_tokens":1000,"cached_input_tokens":100,"output_tokens":200,"reasoning_output_tokens":20,"total_tokens":1320}}}}}}}}"#
        ),
    )
    .expect("write codex fixture");
}

// --- Request shape -----------------------------------------------------------

#[test]
fn production_request_covers_full_registry_with_environment_sources() {
    let request = production_snapshot_request();
    assert_eq!(request.version, 1);
    assert!(request.request_id.starts_with("prod-"));
    assert_eq!(request.agents.len(), AgentKind::ALL.len());
    for (spec, agent) in request.agents.iter().zip(AgentKind::ALL.iter()) {
        assert_eq!(spec.agent, agent.id());
        assert_eq!(
            spec.source,
            coding_agent_monitor_lib::collector::protocol::DataSourceV1::Environment
        );
    }
    let window = request.window.expect("production window");
    let today = chrono::Local::now().date_naive();
    assert_eq!(window.end_inclusive, today.format("%Y-%m-%d").to_string());
    assert_eq!(
        window.start_inclusive,
        (today - chrono::Duration::days(6))
            .format("%Y-%m-%d")
            .to_string()
    );
    assert!(
        !request.timezone.is_empty() && request.timezone.len() <= 64,
        "timezone {:?} out of bounds",
        request.timezone
    );
}

// --- Uninstalled / absent agents ----------------------------------------------

#[test]
fn production_refresh_with_no_agents_installed_is_an_empty_success() {
    let _lock = lock_tests();
    worker_override();
    let root = unique_dir("uninstalled");
    let _env = scrub_to_uninstalled(&root);

    let summary = collect_usage().expect("all-agents-absent must be an empty success");
    assert!(
        summary.last7_days.iter().all(|day| day.agents.is_empty()),
        "no agent should be invented for an uninstalled environment"
    );
    assert_eq!(summary.today.total_tokens, 0);
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn production_refresh_with_single_agent_installed_reports_only_that_agent() {
    let _lock = lock_tests();
    worker_override();
    let root = unique_dir("claude-only");
    let mut env = scrub_to_uninstalled(&root);

    let (day, timestamp) = local_noon(1);
    let projects = root.join("claude-config/projects/cam");
    std::fs::create_dir_all(&projects).expect("mkdir claude fixture");
    std::fs::write(
        projects.join("session-a.jsonl"),
        format!(
            r#"{{"timestamp":"{timestamp}","sessionId":"s","requestId":"r1","costUSD":0.01,"message":{{"usage":{{"input_tokens":100,"output_tokens":50,"cache_creation_input_tokens":0,"cache_read_input_tokens":10}},"model":"claude-sonnet-4-20250514","id":"m1"}}}}"#
        ),
    )
    .expect("write claude fixture");
    env.set("CLAUDE_CONFIG_DIR", &root.join("claude-config"));

    let summary = collect_usage().expect("single-agent refresh must succeed");
    let agent_ids: HashSet<&str> = summary
        .last7_days
        .iter()
        .flat_map(|d| d.agents.iter().map(|a| a.id.as_str()))
        .collect();
    assert_eq!(agent_ids, HashSet::from(["claude"]));
    assert!(
        summary
            .last7_days
            .iter()
            .any(|d| d.date == day && d.total_tokens == 160),
        "claude's record must land in its local-day bucket: {summary:?}"
    );
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn production_refresh_with_mixed_installed_and_absent_agents() {
    let _lock = lock_tests();
    worker_override();
    let root = unique_dir("mixed");
    let mut env = scrub_to_uninstalled(&root);

    // codex has data; claude and antigravity are explicitly installed but
    // empty; the other 14 agents have no env vars at all (uninstalled).
    let (_, timestamp) = local_noon(0);
    write_codex_fixture(&root, timestamp.as_str());
    env.set("CODEX_HOME", &root.join("codex-home"));
    let empty_claude = root.join("claude-empty/projects");
    std::fs::create_dir_all(&empty_claude).expect("mkdir empty claude");
    env.set("CLAUDE_CONFIG_DIR", &root.join("claude-empty"));
    let empty_antigravity = root.join("antigravity-empty/conversations");
    std::fs::create_dir_all(&empty_antigravity).expect("mkdir empty antigravity");
    env.set("ANTIGRAVITY_DATA_DIR", &root.join("antigravity-empty"));

    let summary = collect_usage().expect("mixed refresh must succeed");
    let agent_ids: HashSet<&str> = summary
        .last7_days
        .iter()
        .flat_map(|d| d.agents.iter().map(|a| a.id.as_str()))
        .collect();
    assert_eq!(
        agent_ids,
        HashSet::from(["codex"]),
        "installed-but-empty and uninstalled agents must not appear"
    );
    std::fs::remove_dir_all(&root).ok();
}

// --- Approved v0.3 value semantics on the public contract ----------------------

#[test]
fn production_refresh_reasoning_semantics_on_public_contract() {
    let _lock = lock_tests();
    worker_override();
    let root = unique_dir("reasoning");
    let mut env = scrub_to_uninstalled(&root);

    let (_, timestamp) = local_noon(0);
    write_codex_fixture(&root, timestamp.as_str());
    env.set("CODEX_HOME", &root.join("codex-home"));

    // Approved change #1: source-reported reasoning lands in `reasoningTokens`
    // (not `unclassifiedTokens`); the total is unchanged.
    let summary = collect_usage().expect("refresh must succeed");
    let codex = summary
        .today
        .agents
        .iter()
        .find(|a| a.id == "codex")
        .expect("codex present");
    assert_eq!(codex.tokens, 1320);
    // v0.3 (approved): reasoning=20 identified out of the residue; v0.2
    // reported reasoning=0 / unclassified=120 for the same record.
    assert_eq!(codex.reasoning_tokens, 20);
    assert_eq!(codex.unclassified_tokens, 100);
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn production_refresh_unpriced_only_agent_has_null_day_cost() {
    let _lock = lock_tests();
    worker_override();
    let root = unique_dir("unpriced");
    let mut env = scrub_to_uninstalled(&root);

    // A day whose ONLY record uses a model missing from the pricing snapshot.
    let (day, timestamp) = local_noon(0);
    let codex_sessions = root.join("codex-home/sessions");
    std::fs::create_dir_all(&codex_sessions).expect("mkdir codex fixture");
    std::fs::write(
        codex_sessions.join("session-a.jsonl"),
        format!(
            r#"{{"timestamp":"{timestamp}","type":"event_msg","payload":{{"type":"token_count","info":{{"model":"cam-unpriced-probe-model","last_token_usage":{{"input_tokens":10,"cached_input_tokens":0,"output_tokens":5,"reasoning_output_tokens":0,"total_tokens":15}}}}}}}}"#
        ),
    )
    .expect("write codex fixture");
    env.set("CODEX_HOME", &root.join("codex-home"));

    // Approved change #2: an unpriced-only agent nulls the day cost — never a
    // faked $0.00 (the v0.2 sidecar faked 0.0 in this situation).
    let summary = collect_usage().expect("refresh must succeed");
    let today = summary
        .last7_days
        .iter()
        .find(|d| d.date == day)
        .expect("day present");
    assert_eq!(today.total_tokens, 15);
    assert_eq!(today.estimated_cost_usd, None);
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn production_refresh_mixed_priced_and_unpriced_day_cost_is_null() {
    let _lock = lock_tests();
    worker_override();
    let root = unique_dir("mixed-pricing");
    let mut env = scrub_to_uninstalled(&root);

    let (day, timestamp) = local_noon(0);
    let claude_projects = root.join("claude-config/projects/cam");
    std::fs::create_dir_all(&claude_projects).expect("mkdir claude fixture");
    std::fs::write(
        claude_projects.join("session-a.jsonl"),
        format!(
            // One priced line (explicit costUSD) + one unpriced-model line.
            r#"{{"timestamp":"{timestamp}","sessionId":"s","requestId":"r1","costUSD":0.125,"message":{{"usage":{{"input_tokens":300,"output_tokens":30,"cache_creation_input_tokens":0,"cache_read_input_tokens":3}},"model":"claude-sonnet-4-20250514","id":"m1"}}}}
{{"timestamp":"{timestamp}","sessionId":"s","requestId":"r2","message":{{"usage":{{"input_tokens":10,"output_tokens":5,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}},"model":"cam-unpriced-probe-model","id":"m2"}}}}"#
        ),
    )
    .expect("write claude fixture");
    env.set("CLAUDE_CONFIG_DIR", &root.join("claude-config"));

    let summary = collect_usage().expect("refresh must succeed");
    let day_usage = summary
        .last7_days
        .iter()
        .find(|d| d.date == day)
        .expect("day present");
    assert_eq!(day_usage.total_tokens, 348);
    assert_eq!(
        day_usage.estimated_cost_usd, None,
        "a day with any unpriced contribution must not show a partial cost"
    );
    std::fs::remove_dir_all(&root).ok();
}

// --- Failure semantics ---------------------------------------------------------

/// Cache window of the production runner (v0.2 parity). Copied as a constant
/// because the runner's constant is crate-private.
const CACHE_FRESH_FOR: Duration = Duration::from_secs(2);

#[test]
fn production_worker_panic_fails_whole_refresh_then_recovers() {
    let _lock = lock_tests();
    worker_override();
    let root = unique_dir("panic");
    let mut env = scrub_to_uninstalled(&root);

    let (_, timestamp) = local_noon(0);
    write_codex_fixture(&root, timestamp.as_str());
    env.set("CODEX_HOME", &root.join("codex-home"));

    // First refresh: the worker panics before any response exists.
    env.set_text("CAM_TEST_WORKER_PANIC", "1");
    let error = collect_usage().expect_err("panicked worker must fail the whole refresh");
    assert!(
        error.message.contains("exited with"),
        "a worker panic must fail the refresh at the process boundary: {error}"
    );

    // Immediately cached failure (v0.2 parity): same result, still no data.
    let error2 = collect_usage().expect_err("cached failure");
    assert_eq!(error.message, error2.message);

    // After the cache window, with the fault removed, the refresh recovers.
    std::thread::sleep(CACHE_FRESH_FOR + Duration::from_millis(300));
    env.unset("CAM_TEST_WORKER_PANIC");
    let summary = collect_usage().expect("refresh must recover after a panic");
    let ids: HashSet<&str> = summary
        .last7_days
        .iter()
        .flat_map(|d| d.agents.iter().map(|a| a.id.as_str()))
        .collect();
    assert_eq!(ids, HashSet::from(["codex"]));
    std::fs::remove_dir_all(&root).ok();
}

// --- Single-flight on the public entry -----------------------------------------

#[test]
fn production_twenty_concurrent_public_refreshes_share_one_worker() {
    let _lock = lock_tests();
    worker_override();
    let root = unique_dir("single-flight");
    let mut env = scrub_to_uninstalled(&root);

    let marker = root.join("spawn-marker.txt");
    env.set_text("CAM_TEST_WORKER_SPAWN_MARKER", &marker.to_string_lossy());
    env.set_text("CAM_TEST_WORKER_SLEEP_MS", "2000");

    let handles: Vec<_> = (0..20)
        .map(|i| {
            std::thread::spawn(move || {
                if i > 0 {
                    std::thread::sleep(Duration::from_millis(50 * i as u64));
                }
                collect_usage()
            })
        })
        .collect();
    let mut results = Vec::new();
    for handle in handles {
        results.push(handle.join().expect("caller thread"));
    }
    let first = &results[0];
    for result in &results[1..] {
        match (first, result) {
            (Ok(a), Ok(b)) => assert_eq!(a.today.total_tokens, b.today.total_tokens),
            (Err(a), Err(b)) => assert_eq!(a.message, b.message),
            (a, b) => panic!("divergent single-flight results: {a:?} vs {b:?}"),
        }
    }
    let spawns = std::fs::read_to_string(&marker).unwrap_or_default();
    assert_eq!(
        spawns.lines().count(),
        1,
        "20 concurrent PUBLIC refreshes must share ONE worker"
    );
    std::fs::remove_dir_all(&root).ok();
}

// --- The sidecar is unreachable from the production chain ----------------------

#[test]
fn production_chain_never_executes_a_sidecar_poison_marker() {
    let _lock = lock_tests();
    worker_override();
    let root = unique_dir("sidecar-poison");
    let _env = scrub_to_uninstalled(&root);

    // Poison the sidecar runner's PACKED lookup path (next to the current
    // executable): a file that fails to spawn if any code path ever tries to
    // execute it. A production fallback to the sidecar would consult this
    // path first and fail the refresh; the worker path ignores it entirely.
    let exe_dir = std::path::PathBuf::from(env!("CARGO_BIN_EXE_coding-agent-monitor"))
        .parent()
        .expect("exe dir")
        .to_path_buf();
    let poisoned_unified = exe_dir.join("ccusage.exe");
    let poisoned_antigravity = exe_dir.join("ccusage-antigravity.exe");
    std::fs::write(&poisoned_unified, b"not an executable - poison marker").expect("write poison");
    std::fs::write(&poisoned_antigravity, b"not an executable - poison marker")
        .expect("write poison");
    struct Cleanup {
        files: Vec<std::path::PathBuf>,
    }
    impl Drop for Cleanup {
        fn drop(&mut self) {
            for file in &self.files {
                let _ = std::fs::remove_file(file);
            }
        }
    }
    let _cleanup = Cleanup {
        files: vec![poisoned_unified.clone(), poisoned_antigravity.clone()],
    };

    // Production refresh succeeds even though a (broken) sidecar sits exactly
    // where the v0.2 runner would look first.
    let summary = collect_usage().expect("production refresh must ignore the sidecar entirely");
    assert!(summary.last7_days.iter().all(|d| d.agents.is_empty()));
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn no_ccusage_process_appears_during_production_refresh() {
    let _lock = lock_tests();
    worker_override();
    let root = unique_dir("process-observe");
    let mut env = scrub_to_uninstalled(&root);

    let (_, timestamp) = local_noon(0);
    write_codex_fixture(&root, timestamp.as_str());
    env.set("CODEX_HOME", &root.join("codex-home"));

    let observed = std::sync::Arc::new(AtomicBool::new(false));
    let stop = std::sync::Arc::new(AtomicBool::new(false));
    let observer_stop = std::sync::Arc::clone(&stop);
    let observer_observed = std::sync::Arc::clone(&observed);
    let observer = std::thread::spawn(move || {
        while !observer_stop.load(Ordering::Relaxed) {
            if running_process_names()
                .into_iter()
                .any(|name| name.to_ascii_lowercase().starts_with("ccusage"))
            {
                observer_observed.store(true, Ordering::Relaxed);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    });

    let summary = collect_usage().expect("refresh must succeed");
    stop.store(true, Ordering::Relaxed);
    observer.join().expect("observer thread");

    assert!(
        !observed.load(Ordering::Relaxed),
        "no ccusage* process may appear"
    );
    let ids: HashSet<&str> = summary
        .last7_days
        .iter()
        .flat_map(|d| d.agents.iter().map(|a| a.id.as_str()))
        .collect();
    assert_eq!(ids, HashSet::from(["codex"]));
    std::fs::remove_dir_all(&root).ok();
}

// --- Static audit: production chain source never references the runner --------

#[test]
fn production_chain_source_has_no_sidecar_runner_references() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let production_chain = [
        // The public command and the tray entry (the only two callers).
        "src/commands/usage.rs",
        "src/tray/mod.rs",
        // The runner and supervisor they reach.
        "src/collector/worker_runner.rs",
        "src/collector/supervisor.rs",
        "src/collector/worker.rs",
        "src/collector/ccusage.rs",
        "src/shutdown.rs",
    ];
    let forbidden = [
        "sidecar::collect_usage",
        "sidecar::run_sidecar",
        "sidecar_paths",
        "spawn ccusage",
        "ccusage.exe",
        "ccusage-antigravity.exe",
        "UNIFIED_ARGS",
        "ANTIGRAVITY_ARGS",
        "fetch-ccusage",
    ];
    for file in production_chain {
        let text = std::fs::read_to_string(manifest.join(file))
            .unwrap_or_else(|error| panic!("read {file}: {error}"));
        for symbol in forbidden {
            assert!(
                !text.contains(symbol),
                "{file} must not reference the sidecar runner (found {symbol:?})"
            );
        }
    }
}

// --- Production-path release performance (opt-in: CAM_PROD4B_PERF=1) -----------

/// Release-to-release performance of the PRODUCTION request shape (full
/// registry, Environment sources, today window, system time zone). Each run
/// spawns a fresh release EXE in worker mode — the exact launch the Tauri
/// command performs (the runner's single-flight/cache only coalesce; a real
/// refresh every 2 minutes never hits the cache). Gated so normal suites skip
/// it; needs `CAM_PROD4B_RELEASE_EXE` pointing at the release EXE because the
/// supervisor's override seam is unavailable in release test builds.
#[cfg(windows)]
#[test]
fn production_release_perf_30_runs() {
    if std::env::var("CAM_PROD4B_PERF").as_deref() != Ok("1") {
        eprintln!("PRODPERF SKIP: set CAM_PROD4B_PERF=1 to run the release benchmark");
        return;
    }
    let release_exe = std::path::PathBuf::from(
        std::env::var("CAM_PROD4B_RELEASE_EXE").expect("CAM_PROD4B_RELEASE_EXE"),
    );
    let _lock = lock_tests();
    let _env = scrub_to_uninstalled(&unique_dir("perf4b"));
    let request = production_snapshot_request();
    let request_bytes = serde_json::to_vec(&request).expect("serialize production request");

    let runs = 30;
    let mut walls: Vec<f64> = Vec::new();
    for index in 1..=runs {
        let started = std::time::Instant::now();
        let mut command = std::process::Command::new(&release_exe);
        command
            .arg(coding_agent_monitor_lib::collector::worker::INTERNAL_FLAG)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());
        let mut child = command.spawn().expect("spawn production worker");
        {
            use std::io::Write;
            child
                .stdin
                .as_mut()
                .expect("stdin")
                .write_all(&request_bytes)
                .expect("write request");
        }
        child.stdin.take();
        let mut response = Vec::new();
        if let Some(stdout) = child.stdout.as_mut() {
            use std::io::Read;
            let _ = stdout.read_to_end(&mut response);
        }
        let status = child.wait().expect("wait worker");
        assert!(status.success(), "production perf run {index} failed");
        let _: coding_agent_monitor_lib::collector::snapshot_protocol::CollectorSnapshotResponseV1 =
            serde_json::from_slice(&response).expect("parse response");
        let wall_ms = started.elapsed().as_secs_f64() * 1000.0;
        eprintln!("PRODPERF RUN,{index},{wall_ms:.3}");
        walls.push(wall_ms);
    }
    let mut sorted = walls.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = sorted[sorted.len() / 2];
    let p95 = sorted[((sorted.len() as f64 * 0.95) as usize).min(sorted.len() - 1)];
    eprintln!(
        "PRODPERF median={median:.3}ms p95={p95:.3}ms min={:.3} max={:.3}",
        sorted[0],
        sorted[sorted.len() - 1]
    );
    assert!(p95 < 1000.0, "production p95 must stay under 1s");
}

// --- Process helpers -----------------------------------------------------------

#[cfg(windows)]
fn running_process_names() -> HashSet<String> {
    #[repr(C)]
    struct ProcessEntry32W {
        size: u32,
        usage_count: u32,
        process_id: u32,
        default_heap: isize,
        module_id: u32,
        threads: u32,
        parent_process_id: u32,
        pc_pri_class_base: i32,
        flags: u32,
        exe_file: [u16; 260],
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> isize;
        fn Process32FirstW(snapshot: isize, entry: *mut ProcessEntry32W) -> i32;
        fn Process32NextW(snapshot: isize, entry: *mut ProcessEntry32W) -> i32;
        fn CloseHandle(handle: isize) -> i32;
    }
    const TH32CS_SNAPPROCESS: u32 = 0x2;
    let mut names = HashSet::new();
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == -1 {
        return names;
    }
    let mut entry = ProcessEntry32W {
        size: std::mem::size_of::<ProcessEntry32W>() as u32,
        usage_count: 0,
        process_id: 0,
        default_heap: 0,
        module_id: 0,
        threads: 0,
        parent_process_id: 0,
        pc_pri_class_base: 0,
        flags: 0,
        exe_file: [0; 260],
    };
    if unsafe { Process32FirstW(snapshot, &mut entry) } != 0 {
        loop {
            let len = entry
                .exe_file
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(entry.exe_file.len());
            names.insert(String::from_utf16_lossy(&entry.exe_file[..len]));
            if unsafe { Process32NextW(snapshot, &mut entry) } == 0 {
                break;
            }
        }
    }
    unsafe { CloseHandle(snapshot) };
    names
}

#[cfg(not(windows))]
fn running_process_names() -> HashSet<String> {
    HashSet::new()
}

// --- Worker safety: a timeout never touches unrelated CAM processes ------------

#[test]
fn supervisor_timeout_kills_only_the_supervised_worker() {
    let _lock = lock_tests();
    worker_override();
    let root = unique_dir("timeout-decoy");
    let mut env = scrub_to_uninstalled(&root);

    // A decoy "another CAM instance" — a worker-mode process this supervisor
    // did NOT create (e.g. a second install or a dev run). It must survive a
    // supervisor timeout untouched.
    let mut decoy = std::process::Command::new(env!("CARGO_BIN_EXE_coding-agent-monitor"))
        .arg(coding_agent_monitor_lib::collector::worker::INTERNAL_FLAG)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .env("CAM_TEST_WORKER_SLEEP_MS", "60000")
        .spawn()
        .expect("spawn decoy CAM worker");
    let decoy_pid = decoy.id();
    let decoy_created = std::time::SystemTime::now();

    // A supervised worker that hangs: the supervisor times out quickly and
    // kills ONLY the child it created and holds a handle to.
    env.set_text("CAM_TEST_WORKER_SLEEP_MS", "60000");
    let started = std::time::Instant::now();
    let request = production_snapshot_request();
    let error = supervisor::collect_snapshot_with_options(
        &request,
        &AtomicBool::new(false),
        Duration::from_secs(2),
    )
    .expect_err("supervised hung worker must time out");
    assert!(
        matches!(
            error,
            coding_agent_monitor_lib::collector::CollectorError::Timeout { .. }
        ),
        "expected timeout: {error}"
    );
    assert!(started.elapsed() < Duration::from_secs(10));

    // The decoy (recorded pid + creation time) is still alive and untouched.
    let _ = (decoy_pid, decoy_created);
    assert!(
        decoy.try_wait().expect("decoy status").is_none(),
        "the unrelated CAM process must survive the supervisor timeout"
    );
    // Reap the decoy (the test owns it).
    let _ = decoy.kill();
    let _ = decoy.wait();
    std::fs::remove_dir_all(&root).ok();
}
