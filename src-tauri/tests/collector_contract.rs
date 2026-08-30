//! Contract tests for the typed Collector API (`src/collector`).
//!
//! These tests pin the boundary between CAM and the vendored ccusage sources:
//! fixture-driven typed results, error classification (including the vendor's
//! documented "No valid … data directories" sentinel), precision, ordering,
//! and environment isolation. If an upstream wording or JSON-shape change
//! breaks one of these tests, update the adapter *and* this test together.
//!
//! All env-mutating tests share this binary's `ENV_LOCK` and restore every
//! variable's previous value (see `EnvGuard`).

use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
    time::{SystemTime, UNIX_EPOCH},
};

use chrono::NaiveDate;
use coding_agent_monitor_lib::collector::{
    ccusage::AgentCollector, AgentKind, CollectRequest, CollectWindow, Collector, CollectorError,
    ModelName,
};

/// Serializes env-mutating tests in this binary; env is process state.
static ENV_LOCK: Mutex<()> = Mutex::new(());

const AGENT_ENV_KEYS: [&str; 17] = [
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

/// Saves each var's previous value and restores it exactly on drop.
struct EnvGuard {
    _serial: MutexGuard<'static, ()>,
    saved: Vec<(&'static str, Option<OsString>)>,
}

impl EnvGuard {
    fn acquire() -> Self {
        Self {
            _serial: ENV_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
            saved: Vec::new(),
        }
    }

    fn set(&mut self, key: &'static str, value: &Path) {
        self.saved.push((key, std::env::var_os(key)));
        // SAFETY: the holder of ENV_LOCK is the only thread touching the env.
        unsafe { std::env::set_var(key, value) };
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (key, previous) in self.saved.iter().rev() {
            // SAFETY: see EnvGuard::set.
            unsafe {
                match previous {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
    }
}

fn fixture_root(name: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("cam-collector-{name}-{unique}"));
    fs::create_dir_all(&root).expect("create fixture root");
    root
}

/// Points every agent env var at the fixture (empty unless populated) and the
/// home vars at empty home directories. CLAUDE_CONFIG_DIR gets its own
/// `claude` root (must contain `projects/`).
fn isolate_env(root: &Path) -> EnvGuard {
    let mut env = EnvGuard::acquire();
    for key in AGENT_ENV_KEYS {
        let value = if key == "CLAUDE_CONFIG_DIR" {
            root.join("claude")
        } else {
            root.to_path_buf()
        };
        env.set(key, &value);
    }
    // The vendored claude adapter validates its root even when collecting
    // other agents (one unified pass scans every agent). An empty `projects/`
    // keeps that validation satisfied without contributing data.
    fs::create_dir_all(root.join("claude/projects")).expect("create claude projects dir");
    for key in ["HOME", "USERPROFILE", "XDG_CONFIG_HOME"] {
        let home = root.join("empty-home");
        fs::create_dir_all(&home).expect("create empty home");
        env.set(key, &home);
    }
    env
}

/// Claude JSONL line. `request_id` participates in the vendor's dedupe key —
/// distinct usage events must use distinct ids.
fn claude_line(
    timestamp: &str,
    request_id: &str,
    model: &str,
    input: u64,
    output: u64,
    cache_read: u64,
    cost_usd: Option<f64>,
) -> String {
    let cost = cost_usd
        .map(|value| format!(r#""costUSD":{value},"#))
        .unwrap_or_default();
    format!(
        r#"{{"timestamp":"{timestamp}","sessionId":"session-a","requestId":"{request_id}",{cost}"message":{{"usage":{{"input_tokens":{input},"output_tokens":{output},"cache_creation_input_tokens":0,"cache_read_input_tokens":{cache_read}}},"model":"{model}","id":"msg-{request_id}"}}}}"#
    )
}

/// Writes a Claude session JSONL under `<root>/claude/projects/cam/`.
fn write_claude_session(root: &Path, lines: &[String]) {
    let dir = root.join("claude/projects/cam");
    fs::create_dir_all(&dir).expect("create claude fixture dir");
    fs::write(dir.join("session-a.jsonl"), lines.join("\n")).expect("write claude fixture");
}

// --- Antigravity fixture ----------------------------------------------------
//
// Hand-encoded protobuf, byte-format audited against the vendored parser
// (`vendor/ccusage/rust/adapters/antigravity/src/parser.rs`, `encode` module):
// generation blob = field 1 -> chat_model{ field 19 -> model string,
// field 9 -> info{ field 4 -> timestamp{1: seconds, 2: nanos} },
// field 4 -> usage{ 1: system_input, 2: fresh_input, 5: cache_read,
// 9: output, 10: thinking, 11: response_id } }.

fn varint(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if value == 0 {
            break;
        }
    }
}

fn key(out: &mut Vec<u8>, field: u64, wire: u64) {
    varint(out, field << 3 | wire);
}

fn varint_field(out: &mut Vec<u8>, field: u64, value: u64) {
    key(out, field, 0);
    varint(out, value);
}

fn len_field(out: &mut Vec<u8>, field: u64, bytes: &[u8]) {
    key(out, field, 2);
    varint(out, bytes.len() as u64);
    out.extend_from_slice(bytes);
}

fn generation_blob(
    model: &str,
    timestamp_seconds: u64,
    system_input: u64,
    fresh_input: u64,
    cache_read: u64,
    output: u64,
    response_id: &str,
) -> Vec<u8> {
    let mut chat_model = Vec::new();
    len_field(&mut chat_model, 19, model.as_bytes());
    let mut timestamp = Vec::new();
    varint_field(&mut timestamp, 1, timestamp_seconds);
    varint_field(&mut timestamp, 2, 0);
    let mut info = Vec::new();
    len_field(&mut info, 4, &timestamp);
    len_field(&mut chat_model, 9, &info);
    let mut usage = Vec::new();
    varint_field(&mut usage, 1, system_input);
    varint_field(&mut usage, 2, fresh_input);
    varint_field(&mut usage, 5, cache_read);
    varint_field(&mut usage, 9, output);
    varint_field(&mut usage, 10, 0);
    len_field(&mut usage, 11, response_id.as_bytes());
    len_field(&mut chat_model, 4, &usage);
    let mut blob = Vec::new();
    len_field(&mut blob, 1, &chat_model);
    blob
}

/// Writes an Antigravity conversation database under `<root>/conversations/`.
fn write_antigravity_db(root: &Path, name: &str, generations: &[Vec<u8>]) {
    use sqlite::Connection;
    let dir = root.join("conversations");
    fs::create_dir_all(&dir).expect("create antigravity fixture dir");
    let db = Connection::open(dir.join(name)).expect("open antigravity fixture db");
    db.execute("CREATE TABLE gen_metadata (idx INTEGER, data BLOB)")
        .expect("create gen_metadata");
    let mut statement = db
        .prepare("INSERT INTO gen_metadata (idx, data) VALUES (?1, ?2)")
        .expect("prepare insert");
    for (index, blob) in generations.iter().enumerate() {
        statement.bind((1, index as i64)).expect("bind idx");
        statement.bind((2, blob.as_slice())).expect("bind data");
        statement.next().expect("insert row");
        statement.reset().expect("reset statement");
    }
}

const DAY_1: &str = "2026-01-02";
const DAY_2: &str = "2026-01-03";
const DAY_1_SECONDS: u64 = 1_767_312_000; // 2026-01-02T00:00:00Z

fn date(text: &str) -> NaiveDate {
    NaiveDate::parse_from_str(text, "%Y-%m-%d").expect("fixture date")
}

fn model(name: &str) -> ModelName {
    ModelName(name.to_string())
}

fn window(start: &str, end: &str) -> CollectWindow {
    CollectWindow::new(date(start), date(end)).expect("fixture window")
}

// --- Claude contract --------------------------------------------------------

#[test]
fn claude_collector_returns_typed_sorted_records() {
    let root = fixture_root("claude-ok");
    // Deliberately out of chronological order in the file.
    write_claude_session(
        &root,
        &[
            claude_line(
                "2026-01-03T08:30:00.000Z",
                "req-day2",
                "claude-sonnet-4-20250514",
                300,
                30,
                3,
                Some(0.125),
            ),
            claude_line(
                "2026-01-02T00:00:00.000Z",
                "req-day1",
                "claude-sonnet-4-20250514",
                100,
                50,
                10,
                Some(0.01),
            ),
        ],
    );
    let _env = isolate_env(&root);

    let result = AgentCollector::new(AgentKind::Claude)
        .collect(&CollectRequest::new(AgentKind::Claude))
        .expect("claude collect");

    assert_eq!(result.agent, AgentKind::Claude);
    assert_eq!(result.records.len(), 2, "one record per day");
    assert_eq!(
        result.records[0].date,
        date(DAY_1),
        "records sorted by date"
    );
    assert_eq!(result.records[1].date, date(DAY_2));

    let first = &result.records[0];
    assert_eq!(first.input_tokens, 100);
    assert_eq!(first.output_tokens, 50);
    assert_eq!(first.cache_creation_tokens, 0);
    assert_eq!(first.cache_read_tokens, 10);
    assert_eq!(first.total_tokens, 160);
    assert_eq!(first.models_used, vec![model("claude-sonnet-4-20250514")]);
    assert_eq!(first.cost.map(|cost| cost.as_nano_usd()), Some(10_000_000));
    assert!(first.models_missing_pricing.is_empty());
    assert_eq!(first.model_breakdowns.len(), 1);
    assert_eq!(
        first.model_breakdowns[0].model.0,
        "claude-sonnet-4-20250514"
    );
    assert_eq!(
        first.model_breakdowns[0]
            .cost
            .map(|cost| cost.as_nano_usd()),
        Some(10_000_000)
    );

    // Two identical requests produce identical results (determinism).
    let again = AgentCollector::new(AgentKind::Claude)
        .collect(&CollectRequest::new(AgentKind::Claude))
        .expect("claude collect again");
    assert_eq!(result, again);

    fs::remove_dir_all(&root).expect("clean up fixture");
}

#[test]
fn claude_window_filters_inclusively() {
    let root = fixture_root("claude-window");
    write_claude_session(
        &root,
        &[
            claude_line(
                "2026-01-02T00:00:00.000Z",
                "req-w1",
                "claude-sonnet-4-20250514",
                100,
                1,
                0,
                Some(0.01),
            ),
            claude_line(
                "2026-01-03T00:00:00.000Z",
                "req-w2",
                "claude-sonnet-4-20250514",
                200,
                2,
                0,
                Some(0.02),
            ),
            claude_line(
                "2026-01-04T00:00:00.000Z",
                "req-w3",
                "claude-sonnet-4-20250514",
                400,
                4,
                0,
                Some(0.04),
            ),
        ],
    );
    let _env = isolate_env(&root);
    let collector = AgentCollector::new(AgentKind::Claude);

    // Both bounds inclusive: a single-day window keeps exactly that day.
    let single = collector
        .collect(&CollectRequest::new(AgentKind::Claude).with_window(window(DAY_2, DAY_2)))
        .expect("single-day window");
    assert_eq!(single.records.len(), 1);
    assert_eq!(single.records[0].date, date(DAY_2));
    assert_eq!(single.records[0].input_tokens, 200);

    let range = collector
        .collect(&CollectRequest::new(AgentKind::Claude).with_window(window(DAY_1, DAY_2)))
        .expect("two-day window");
    assert_eq!(range.records.len(), 2);
    assert_eq!(range.records[0].input_tokens, 100);
    assert_eq!(range.records[1].input_tokens, 200);

    fs::remove_dir_all(&root).expect("clean up fixture");
}

#[test]
fn unknown_model_keeps_name_and_reports_missing_pricing() {
    let root = fixture_root("claude-unknown-model");
    // No costUSD in the log: the vendor must price the model itself, and this
    // one is not in any pricing snapshot.
    write_claude_session(
        &root,
        &[claude_line(
            "2026-01-02T00:00:00.000Z",
            "req-unknown",
            "totally-unknown-model-xyz",
            500,
            100,
            0,
            None,
        )],
    );
    let _env = isolate_env(&root);

    let result = AgentCollector::new(AgentKind::Claude)
        .collect(&CollectRequest::new(AgentKind::Claude))
        .expect("collect with unknown model");

    assert_eq!(result.records.len(), 1);
    let record = &result.records[0];
    assert_eq!(record.input_tokens, 500);
    assert!(
        record.cost.is_none(),
        "unpriced model cost must be None, never a fabricated zero"
    );
    assert_eq!(
        record.models_missing_pricing,
        vec![model("totally-unknown-model-xyz")]
    );
    assert_eq!(record.model_breakdowns.len(), 1);
    assert!(record.model_breakdowns[0].cost.is_none());

    fs::remove_dir_all(&root).expect("clean up fixture");
}

#[test]
fn corrupt_claude_lines_are_skipped_and_valid_ones_kept() {
    let root = fixture_root("claude-corrupt");
    write_claude_session(
        &root,
        &[
            "this is not json at all".to_string(),
            r#"{"timestamp":"2026-01-02T00:00:00.000Z","truncated""#.to_string(),
            claude_line(
                "2026-01-02T00:00:00.000Z",
                "req-corrupt-ok",
                "claude-sonnet-4-20250514",
                100,
                10,
                0,
                Some(0.01),
            ),
        ],
    );
    let _env = isolate_env(&root);

    let result = AgentCollector::new(AgentKind::Claude)
        .collect(&CollectRequest::new(AgentKind::Claude))
        .expect("vendor skips malformed lines and keeps valid ones");
    assert_eq!(result.records.len(), 1);
    assert_eq!(result.records[0].input_tokens, 100);

    fs::remove_dir_all(&root).expect("clean up fixture");
}

#[test]
fn claude_missing_data_dir_maps_to_source_unavailable() {
    let root = fixture_root("claude-no-dir");
    // A valid CLAUDE_CONFIG_DIR must contain `projects/`; without it the
    // vendored claude adapter emits its documented sentinel.
    let _env = isolate_env(&root);
    // Undo the empty `projects/` that isolate_env creates: this test pins
    // the sentinel behaviour for a root WITHOUT `projects/`.
    fs::remove_dir_all(root.join("claude/projects")).expect("remove projects dir");

    let error = AgentCollector::new(AgentKind::Claude)
        .collect(&CollectRequest::new(AgentKind::Claude))
        .expect_err("misconfigured claude root must fail");

    assert!(
        matches!(
            error,
            CollectorError::SourceUnavailable {
                agent: AgentKind::Claude,
                ..
            }
        ),
        "expected SourceUnavailable, got: {error}"
    );

    fs::remove_dir_all(&root).expect("clean up fixture");
}

// --- Antigravity contract ---------------------------------------------------

#[test]
fn antigravity_collector_reads_conversation_databases() {
    let root = fixture_root("antigravity-ok");
    // gemini-3.1-pro-low resolves (vendored parser) to gemini-3.1-pro:
    // input $2, output $12, cache_read $0.2 per million tokens.
    // input = system 1000 + fresh 6321 = 7321; output 604; cache_read 10.
    // cost = (7321*2 + 604*12 + 10*0.2) / 1e6 = 0.021892 USD = 21_892_000 nano.
    write_antigravity_db(
        &root,
        "conv-1.db",
        &[generation_blob(
            "gemini-3.1-pro-low",
            DAY_1_SECONDS,
            1000,
            6321,
            10,
            604,
            "resp-1",
        )],
    );
    let _env = isolate_env(&root);

    let result = AgentCollector::new(AgentKind::Antigravity)
        .collect(&CollectRequest::new(AgentKind::Antigravity))
        .expect("antigravity collect");

    assert_eq!(result.records.len(), 1);
    let record = &result.records[0];
    assert_eq!(record.agent, AgentKind::Antigravity);
    assert_eq!(record.date, date(DAY_1));
    assert_eq!(record.input_tokens, 7321, "system + fresh input");
    assert_eq!(record.output_tokens, 604);
    assert_eq!(record.cache_read_tokens, 10);
    assert_eq!(record.cache_creation_tokens, 0);
    assert_eq!(record.total_tokens, 7935);
    assert_eq!(record.models_used, vec![model("gemini-3.1-pro")]);
    assert_eq!(record.cost.map(|cost| cost.as_nano_usd()), Some(21_892_000));

    fs::remove_dir_all(&root).expect("clean up fixture");
}

#[test]
fn antigravity_empty_data_returns_successful_empty_result() {
    let root = fixture_root("antigravity-empty");
    // ANTIGRAVITY_DATA_DIR points at an existing but dataless root: no
    // conversation databases at all.
    let _env = isolate_env(&root);

    let result = AgentCollector::new(AgentKind::Antigravity)
        .collect(&CollectRequest::new(AgentKind::Antigravity))
        .expect("empty antigravity data is success, not error");
    assert!(result.is_empty());
    assert_eq!(result.agent, AgentKind::Antigravity);

    fs::remove_dir_all(&root).expect("clean up fixture");
}

#[test]
fn antigravity_corrupt_database_does_not_crash_collection() {
    let root = fixture_root("antigravity-corrupt");
    let dir = root.join("conversations");
    fs::create_dir_all(&dir).expect("create antigravity fixture dir");
    // Not a SQLite file: the vendored loader must skip or surface a typed
    // error — either way the collector contract holds.
    fs::write(dir.join("broken.db"), b"definitely not a sqlite database")
        .expect("write corrupt db");
    let _env = isolate_env(&root);

    let outcome = AgentCollector::new(AgentKind::Antigravity)
        .collect(&CollectRequest::new(AgentKind::Antigravity));

    match outcome {
        Ok(result) => assert!(result.is_empty(), "corrupt db skipped: no records expected"),
        Err(error) => assert!(
            matches!(
                error,
                CollectorError::DatabaseQuery { .. } | CollectorError::CorruptData { .. }
            ),
            "corrupt db must map to a typed data error, got: {error}"
        ),
    }

    fs::remove_dir_all(&root).expect("clean up fixture");
}

// --- Request validation -----------------------------------------------------

#[test]
fn invalid_window_is_rejected_as_invalid_request() {
    let error = CollectWindow::new(date(DAY_2), date(DAY_1)).expect_err("inverted window");
    assert!(matches!(error, CollectorError::InvalidRequest { .. }));
}

#[test]
fn request_agent_mismatch_is_rejected() {
    let error = AgentCollector::new(AgentKind::Claude)
        .collect(&CollectRequest::new(AgentKind::Antigravity))
        .expect_err("cross-agent request");
    assert!(matches!(error, CollectorError::InvalidRequest { .. }));
}
