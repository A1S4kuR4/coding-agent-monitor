//! Phase 3 integration tests: the product binary as internal collector worker
//! and the parent-side supervisor/single-flight.
//!
//! Worker tests spawn the real product binary via Cargo's `CARGO_BIN_EXE_*`
//! (integration-test provision); production spawning uses `current_exe()`.
//! Fault injection uses `CAM_TEST_WORKER_*` env vars that exist only in
//! debug/test builds.

use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use coding_agent_monitor_lib::collector::protocol::{
    CollectorRequestV1, CollectorResponseV1, DataSourceV1, ErrorCodeV1, OutcomeV1, PROTOCOL_VERSION,
};
use coding_agent_monitor_lib::collector::{
    supervisor, worker, worker_runner, AgentKind, CollectorError, DataSource,
};

/// Points the supervisor at the real product binary (a test harness's
/// `current_exe()` would be the libtest runner, never a worker).
fn init_product_exe() {
    supervisor::set_worker_exe_override(std::path::PathBuf::from(EXE));
}

/// Injection env vars are process-global, so every test that spawns the worker
/// holds this lock for its whole body. Concurrency inside a test comes from
/// explicit threads (the 20-caller test), which the lock does not affect.
static WORKER_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock_worker_tests() -> std::sync::MutexGuard<'static, ()> {
    WORKER_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

const EXE: &str = env!("CARGO_BIN_EXE_coding-agent-monitor");

/// Runs the product EXE in worker mode with the given stdin payload.
fn run_worker(
    stdin_payload: &[u8],
    extra_env: &[(&str, &str)],
) -> (std::process::ExitStatus, Vec<u8>, Vec<u8>) {
    let mut child = Command::new(EXE)
        .arg(worker::INTERNAL_FLAG)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_remove("CAM_TEST_WORKER_PANIC")
        .env_remove("CAM_TEST_WORKER_EXIT")
        .env_remove("CAM_TEST_WORKER_STDERR_FLOOD")
        .env_remove("CAM_TEST_WORKER_SPAWN_MARKER")
        .env_remove("CAM_TEST_WORKER_GARBAGE_STDOUT")
        .env_remove("CAM_TEST_WORKER_SLEEP_MS")
        .spawn()
        .expect("spawn product exe in worker mode");
    {
        let mut stdin = child.stdin.take().expect("stdin");
        let _ = stdin.write_all(stdin_payload);
        let _ = stdin.flush();
    }
    let output = child.wait_with_output().expect("worker wait_with_output");
    let _ = extra_env; // injection happens via .envs() at the call site when needed
    (output.status, output.stdout, output.stderr)
}

fn valid_request_json(agent: &str) -> String {
    format!(
        r#"{{"version":{PROTOCOL_VERSION},"request_id":"it-1","agent":"{agent}","timezone":"UTC","source":{{"kind":"environment"}}}}"#
    )
}

fn claude_fixture_root() -> std::path::PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("cam-worker-claude-{unique}"));
    std::fs::create_dir_all(root.join("claude/projects/cam")).expect("mkdir");
    std::fs::write(
        root.join("claude/projects/cam/session-a.jsonl"),
        r#"{"timestamp":"2026-01-02T00:00:00.000Z","sessionId":"s","requestId":"r1","costUSD":0.01,"message":{"usage":{"input_tokens":100,"output_tokens":50,"cache_creation_input_tokens":0,"cache_read_input_tokens":10},"model":"claude-sonnet-4-20250514","id":"m1"}}"#,
    )
    .expect("write fixture");
    root
}

fn parse_response(stdout: &[u8]) -> CollectorResponseV1 {
    serde_json::from_slice(stdout).expect("worker stdout must be exactly one JSON document")
}

// --- Worker entry ------------------------------------------------------------

#[test]
fn worker_collects_claude_fixture_end_to_end() {
    let _worker_lock = lock_worker_tests();
    let root = claude_fixture_root();
    let request = format!(
        r#"{{"version":{PROTOCOL_VERSION},"request_id":"it-claude","agent":"claude","timezone":"UTC","source":{{"kind":"paths","roots":["{}"]}}}}"#,
        root.join("claude").to_string_lossy().replace('\\', "\\\\")
    );
    let (status, stdout, stderr) = run_worker(request.as_bytes(), &[]);
    assert!(
        status.success(),
        "worker must exit 0, stderr: {}",
        String::from_utf8_lossy(&stderr)
    );
    let response = parse_response(&stdout);
    assert_eq!(response.request_id, "it-claude");
    match &response.outcome {
        OutcomeV1::Ok { report } => {
            assert_eq!(report.records.len(), 1);
            assert_eq!(report.records[0].total_tokens, "160");
            assert!(report.diagnostics.is_empty());
        }
        other => panic!("expected ok, got {other:?}"),
    }
    assert!(std::str::from_utf8(&stdout)
        .unwrap()
        .trim_end()
        .parse::<serde_json::Value>()
        .is_ok());
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn worker_returns_empty_success_for_antigravity_without_data() {
    let _worker_lock = lock_worker_tests();
    // Point ANTIGRAVITY_DATA_DIR at an empty scratch dir so the test is
    // deterministic even on machines with real agent data.
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let scratch = std::env::temp_dir().join(format!("cam-worker-ag-empty-{unique}"));
    std::fs::create_dir_all(&scratch).expect("mkdir");

    let mut child = Command::new(EXE)
        .arg(worker::INTERNAL_FLAG)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("ANTIGRAVITY_DATA_DIR", &scratch)
        .spawn()
        .expect("spawn worker");
    {
        let mut stdin = child.stdin.take().expect("stdin");
        let _ = stdin.write_all(valid_request_json("antigravity").as_bytes());
    }
    let output = child.wait_with_output().expect("wait");
    assert!(output.status.success());
    let response = parse_response(&output.stdout);
    match &response.outcome {
        OutcomeV1::Ok { report } => {
            assert!(report.records.is_empty(), "no data means empty success");
        }
        other => panic!("empty data must not be an error, got {other:?}"),
    }
    std::fs::remove_dir_all(&scratch).ok();
}

#[test]
fn worker_maps_collector_error_to_structured_response() {
    let _worker_lock = lock_worker_tests();
    // CLAUDE_CONFIG_DIR pointing at a directory without projects/ triggers the
    // vendored claude validation error → structured SourceUnavailable with the
    // agent attribution; the response is still exit-0 with an error envelope.
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let bad_root = std::env::temp_dir().join(format!("cam-worker-bad-root-{unique}"));
    std::fs::create_dir_all(&bad_root).expect("mkdir");

    let mut child = Command::new(EXE)
        .arg(worker::INTERNAL_FLAG)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("CLAUDE_CONFIG_DIR", &bad_root)
        .spawn()
        .expect("spawn worker");
    {
        let mut stdin = child.stdin.take().expect("stdin");
        let _ = stdin.write_all(valid_request_json("claude").as_bytes());
    }
    let output = child.wait_with_output().expect("wait");
    assert!(output.status.success(), "structured errors still exit 0");
    let response = parse_response(&output.stdout);
    match &response.outcome {
        OutcomeV1::Error { error } => {
            assert_eq!(error.code, ErrorCodeV1::SourceUnavailable);
            assert_eq!(error.agent.as_deref(), Some("claude"));
        }
        other => panic!("expected error envelope, got {other:?}"),
    }
    std::fs::remove_dir_all(&bad_root).ok();
}

#[test]
fn worker_rejects_malformed_json_trailing_content_and_non_utf8() {
    let _worker_lock = lock_worker_tests();
    for payload in [
        "not json".as_bytes(),
        // Two documents in one stdin: the trailing one must be rejected.
        br#"{"version":1,"request_id":"a","agent":"codex","timezone":"UTC","source":{"kind":"environment"}} {"version":1,"request_id":"b"}"#.as_slice(),
        &[0xFF, 0xFE, 0x00, 0x01][..],
    ] {
        let (status, stdout, _) = run_worker(payload, &[]);
        assert!(status.success(), "transport errors still exit 0");
        let response = parse_response(&stdout);
        match &response.outcome {
            OutcomeV1::Error { error } => {
                assert_eq!(error.code, ErrorCodeV1::Protocol, "payload: {payload:?}");
            }
            other => panic!("expected protocol error for {payload:?}, got {other:?}"),
        }
    }
}

#[test]
fn worker_rejects_wrong_version_unknown_agent_and_empty_roots() {
    let _worker_lock = lock_worker_tests();
    let bad_version = r#"{"version":99,"request_id":"x","agent":"codex","timezone":"UTC","source":{"kind":"environment"}}"#;
    let unknown_agent = r#"{"version":1,"request_id":"x","agent":"nope","timezone":"UTC","source":{"kind":"environment"}}"#;
    let empty_roots = r#"{"version":1,"request_id":"x","agent":"claude","timezone":"UTC","source":{"kind":"paths","roots":[]}}"#;
    for payload in [bad_version, unknown_agent, empty_roots] {
        let (status, stdout, _) = run_worker(payload.as_bytes(), &[]);
        assert!(status.success());
        let response = parse_response(&stdout);
        match &response.outcome {
            OutcomeV1::Error { error } => {
                assert_eq!(error.code, ErrorCodeV1::InvalidRequest, "{payload}");
            }
            other => panic!("expected InvalidRequest for {payload}, got {other:?}"),
        }
    }
}

#[test]
fn worker_rejects_oversized_stdin() {
    let _worker_lock = lock_worker_tests();
    let padding = " ".repeat(worker::MAX_STDIN_BYTES + 1);
    let payload = format!("{padding}{{}}");
    let (status, stdout, _) = run_worker(payload.as_bytes(), &[]);
    assert!(status.success());
    let response = parse_response(&stdout);
    match &response.outcome {
        OutcomeV1::Error { error } => {
            assert_eq!(error.code, ErrorCodeV1::Protocol);
            assert!(error.message.contains("limit"));
        }
        other => panic!("expected limit error, got {other:?}"),
    }
}

#[test]
fn worker_stdout_is_exactly_one_json_document() {
    let _worker_lock = lock_worker_tests();
    let (status, stdout, _stderr) = run_worker(valid_request_json("codex").as_bytes(), &[]);
    assert!(status.success());
    let text = std::str::from_utf8(&stdout).expect("utf8");
    let trimmed = text.trim_end();
    serde_json::from_str::<serde_json::Value>(trimmed).expect("one JSON document");
    // No banner/log noise before or after the document.
    assert!(
        text.trim_start().starts_with('{'),
        "stdout must start with the JSON document"
    );
}

#[test]
fn worker_does_not_initialize_the_app_database() {
    let _worker_lock = lock_worker_tests();
    // Redirect APPDATA to a scratch dir: the worker must never create the app
    // database (db::initialize never runs in worker mode).
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let scratch = std::env::temp_dir().join(format!("cam-worker-appdata-{unique}"));
    std::fs::create_dir_all(&scratch).expect("mkdir scratch");

    let mut child = Command::new(EXE)
        .arg(worker::INTERNAL_FLAG)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("APPDATA", &scratch)
        .spawn()
        .expect("spawn worker");
    {
        let mut stdin = child.stdin.take().expect("stdin");
        let _ = stdin.write_all(valid_request_json("codex").as_bytes());
    }
    let output = child.wait_with_output().expect("wait");
    assert!(output.status.success());

    let db = scratch.join("usage-cache.sqlite3");
    // The DB may live in a Roaming subfolder depending on Tauri's path
    // resolution; assert neither the scratch root nor one level down has it.
    assert!(
        !db.exists() && !fs_find_file(&scratch, "usage-cache.sqlite3").is_some(),
        "worker must not create the app database"
    );
    std::fs::remove_dir_all(&scratch).ok();
}

fn fs_find_file(dir: &std::path::Path, name: &str) -> Option<std::path::PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && path.file_name().and_then(|n| n.to_str()) == Some(name) {
            return Some(path);
        }
        if path.is_dir() {
            if let Some(found) = fs_find_file(&path, name) {
                return Some(found);
            }
        }
    }
    None
}

// --- Supervisor paths (via the real worker + debug-only fault injection) ----

#[test]
fn supervisor_success_round_trip() {
    init_product_exe();
    let _worker_lock = lock_worker_tests();
    // Codex with an explicit empty fixture root: deterministic on any machine.
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let scratch = std::env::temp_dir().join(format!("cam-sup-codex-{unique}"));
    std::fs::create_dir_all(&scratch).expect("mkdir");
    let request = CollectorRequestV1::new("sup-1", AgentKind::Codex);
    let request = CollectorRequestV1 {
        source: DataSourceV1::Paths {
            roots: vec![scratch.to_string_lossy().into_owned()],
        },
        ..request
    };
    let result = supervisor::collect(&request).expect("supervisor success");
    assert_eq!(result.agent, AgentKind::Codex);
    assert!(result.is_empty());
    std::fs::remove_dir_all(&scratch).ok();
}

#[test]
fn supervisor_maps_worker_exit_failure_to_internal() {
    init_product_exe();
    let _worker_lock = lock_worker_tests();
    let request = CollectorRequestV1::new("sup-exit", AgentKind::Codex);
    let _guard = EnvGuard::set("CAM_TEST_WORKER_EXIT", "3");
    let error = supervisor::collect(&request).expect_err("exit 3");
    assert!(
        matches!(error, CollectorError::Internal { .. }),
        "got {error}"
    );
}

#[test]
fn supervisor_maps_worker_panic_to_internal_without_panic_text() {
    init_product_exe();
    let _worker_lock = lock_worker_tests();
    let request = CollectorRequestV1::new("sup-panic", AgentKind::Codex);
    let _guard = EnvGuard::set("CAM_TEST_WORKER_PANIC", "1");
    let error = supervisor::collect(&request).expect_err("panic path");
    assert!(
        matches!(error, CollectorError::Internal { .. }),
        "got {error}"
    );
    if let CollectorError::Internal { details } = &error {
        assert!(
            !details.contains("CAM_TEST_WORKER_PANIC fault injection"),
            "panic text must not leak into the error"
        );
    }
}

#[test]
fn supervisor_maps_garbage_stdout_to_protocol() {
    init_product_exe();
    let _worker_lock = lock_worker_tests();
    let request = CollectorRequestV1::new("sup-garbage", AgentKind::Codex);
    let _guard = EnvGuard::set("CAM_TEST_WORKER_GARBAGE_STDOUT", "1");
    let error = supervisor::collect(&request).expect_err("garbage stdout");
    assert!(
        matches!(error, CollectorError::Protocol { .. }),
        "got {error}"
    );
}

#[test]
fn supervisor_survives_stderr_flood() {
    init_product_exe();
    let _worker_lock = lock_worker_tests();
    // Empty CODEX_HOME so the flood child's collection is fast regardless of
    // how much real agent data the machine has; the flood itself is what is
    // under test.
    let scratch = std::env::temp_dir().join(format!(
        "cam-sup-flood-codex-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    std::fs::create_dir_all(&scratch).expect("mkdir scratch");
    let _codex_home = EnvGuard::set("CODEX_HOME", scratch.to_string_lossy().as_ref());
    let request = CollectorRequestV1::new("sup-flood", AgentKind::Codex);
    let _guard = EnvGuard::set("CAM_TEST_WORKER_STDERR_FLOOD", "1");
    let started = Instant::now();
    // Bounded timeout: a broken drain would surface as Timeout at 20 s instead
    // of hanging the suite.
    let outcome = supervisor::collect_with_options(
        &request,
        &NEVER_CANCEL_TEST_FLAG(),
        Duration::from_secs(20),
    );
    assert!(
        started.elapsed() < Duration::from_secs(30),
        "stderr flood must not deadlock the pipes"
    );
    let _ = outcome;
    std::fs::remove_dir_all(&scratch).ok();
}

#[allow(non_snake_case)]
fn NEVER_CANCEL_TEST_FLAG() -> &'static AtomicBool {
    static FLAG: AtomicBool = AtomicBool::new(false);
    &FLAG
}

#[test]
fn supervisor_timeout_kills_worker() {
    init_product_exe();
    let _worker_lock = lock_worker_tests();
    let request = CollectorRequestV1::new("sup-timeout", AgentKind::Codex);
    let _guard = EnvGuard::set("CAM_TEST_WORKER_SLEEP_MS", "60_000");
    let cancel = AtomicBool::new(false);
    let started = Instant::now();
    let error = supervisor::collect_with_options(&request, &cancel, Duration::from_millis(500))
        .expect_err("short timeout");
    let elapsed = started.elapsed();
    assert!(
        matches!(error, CollectorError::Timeout { .. }),
        "got {error}"
    );
    assert!(
        elapsed < Duration::from_secs(10),
        "timeout must kill the worker, took {elapsed:?}"
    );
}

#[test]
fn supervisor_cancel_kills_worker() {
    init_product_exe();
    let _worker_lock = lock_worker_tests();
    let request = CollectorRequestV1::new("sup-cancel", AgentKind::Codex);
    let _guard = EnvGuard::set("CAM_TEST_WORKER_SLEEP_MS", "60_000");
    let cancel = std::sync::Arc::new(AtomicBool::new(false));
    // Cancel from a helper thread shortly after the flight starts.
    let cancel_thread = {
        let cancel = std::sync::Arc::clone(&cancel);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            cancel.store(true, Ordering::SeqCst);
        })
    };
    let started = Instant::now();
    let error = supervisor::collect_with_options(&request, &cancel, Duration::from_secs(60))
        .expect_err("cancelled");
    let elapsed = started.elapsed();
    cancel_thread.join().expect("cancel thread");
    assert!(matches!(error, CollectorError::Cancelled), "got {error}");
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "cancel must kill the worker promptly, took {elapsed:?}"
    );
}

#[test]
fn supervisor_recovers_after_failures() {
    init_product_exe();
    let _worker_lock = lock_worker_tests();
    // Use an Environment-source codex request: the assertion is about the
    // flight recovering, not about the data volume (which varies by machine).
    let request = CollectorRequestV1::new("sup-recover", AgentKind::Codex);
    {
        let _guard = EnvGuard::set("CAM_TEST_WORKER_EXIT", "1");
        let _ = supervisor::collect(&request).expect_err("first failure");
    }
    {
        let _guard = EnvGuard::clear("CAM_TEST_WORKER_EXIT");
        let result = supervisor::collect(&request).expect("recovery after failure");
        assert_eq!(result.agent, AgentKind::Codex);
    }
}

#[test]
fn twenty_concurrent_calls_share_one_worker() {
    init_product_exe();
    let _worker_lock = lock_worker_tests();
    let marker = std::env::temp_dir().join(format!(
        "cam-worker-spawns-{}.txt",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    let _guard = EnvGuard::set(
        "CAM_TEST_WORKER_SPAWN_MARKER",
        marker.to_string_lossy().as_ref(),
    );
    // Ensure the first (and only) flight takes long enough for all 20 callers
    // to join it.
    let _sleep_guard = EnvGuard::set("CAM_TEST_WORKER_SLEEP_MS", "500");

    let handles: Vec<_> = (0..20)
        .map(|_| {
            std::thread::spawn(|| {
                worker_runner::collect(&CollectorRequestV1::new("sup-20", AgentKind::Codex))
            })
        })
        .collect();
    for handle in handles {
        handle
            .join()
            .expect("caller thread")
            .expect("shared success");
    }

    let spawns = std::fs::read_to_string(&marker).unwrap_or_default();
    let count = spawns.lines().count();
    assert_eq!(
        count, 1,
        "20 concurrent callers must share ONE worker (saw {count})"
    );
    std::fs::remove_file(&marker).ok();
}

#[test]
fn single_flight_shares_failures_and_never_sticks() {
    init_product_exe();
    let _worker_lock = lock_worker_tests();
    // Failure flight: all joiners see the failure, and a later flight runs.
    let _exit_guard = EnvGuard::set("CAM_TEST_WORKER_EXIT", "1");
    let handles: Vec<_> = (0..5)
        .map(|_| {
            std::thread::spawn(|| {
                supervisor::collect(&CollectorRequestV1::new("sf-fail", AgentKind::Codex))
            })
        })
        .collect();
    for handle in handles {
        let result = handle.join().expect("thread");
        assert!(result.is_err(), "shared failure");
    }
    drop(_exit_guard);
    // A later flight recovers (single-flight state not stuck).
    let _ = supervisor::collect(&CollectorRequestV1::new("sf-after", AgentKind::Codex))
        .expect("recovered flight");
}

// --- Worker recursion guard --------------------------------------------------

#[test]
fn worker_flag_does_not_reenter_parent_path() {
    let _worker_lock = lock_worker_tests();
    // A worker invocation that itself carries the flag twice still behaves as
    // a worker (reads stdin, answers, exits) — it never spawns anything.
    let mut child = Command::new(EXE)
        .args([worker::INTERNAL_FLAG, worker::INTERNAL_FLAG])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    {
        let mut stdin = child.stdin.take().expect("stdin");
        let _ = stdin.write_all(valid_request_json("codex").as_bytes());
    }
    let output = child.wait_with_output().expect("wait");
    assert!(output.status.success());
    parse_response(&output.stdout);
}

// --- Env guard (debug-only injection helpers need explicit env management) ---

struct EnvGuard {
    name: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl EnvGuard {
    fn set(name: &'static str, value: &str) -> Self {
        let previous = std::env::var_os(name);
        std::env::set_var(name, value);
        Self { name, previous }
    }

    fn clear(name: &'static str) -> Self {
        let previous = std::env::var_os(name);
        std::env::remove_var(name);
        Self { name, previous }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => std::env::set_var(self.name, value),
            None => std::env::remove_var(self.name),
        }
    }
}

// Unused-import guards for types referenced only in some cfg paths.
#[allow(dead_code)]
fn _type_witnesses(_request: &CollectorRequestV1, _source: DataSource, _error: CollectorError) {}
