//! Shared helpers for the collector contract/semantics/golden test binaries.
//!
//! Each integration-test binary links this module separately, so the env lock
//! serializes tests within one binary; binaries are separate processes.

#![allow(dead_code)]

use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
    time::{SystemTime, UNIX_EPOCH},
};

use chrono::NaiveDate;
use coding_agent_monitor_lib::collector::{CollectWindow, ModelName};

/// Serializes env-mutating tests in a binary; env is process state.
pub static ENV_LOCK: Mutex<()> = Mutex::new(());

pub const AGENT_ENV_KEYS: [&str; 17] = [
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
pub struct EnvGuard {
    _serial: MutexGuard<'static, ()>,
    saved: Vec<(&'static str, Option<OsString>)>,
}

impl EnvGuard {
    pub fn acquire() -> Self {
        Self {
            _serial: ENV_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
            saved: Vec::new(),
        }
    }

    pub fn set(&mut self, key: &'static str, value: &Path) {
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

pub fn fixture_root(name: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("cam-collector-{name}-{unique}"));
    fs::create_dir_all(&root).expect("create fixture root");
    root
}

/// Points every agent env var at the fixture (empty unless populated) and the
/// home vars at empty home directories. The vendored claude adapter validates
/// its root even when collecting other agents, so an empty `claude/projects/`
/// is always created; CLAUDE_CONFIG_DIR points at the `claude` root.
pub fn isolate_env(root: &Path) -> EnvGuard {
    let mut env = EnvGuard::acquire();
    for key in AGENT_ENV_KEYS {
        let value = if key == "CLAUDE_CONFIG_DIR" {
            root.join("claude")
        } else {
            root.to_path_buf()
        };
        env.set(key, &value);
    }
    // Keep the vendored claude root validation satisfied without data.
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
pub fn claude_line(
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
pub fn write_claude_session(root: &Path, file_name: &str, lines: &[String]) {
    let dir = root.join("claude/projects/cam");
    fs::create_dir_all(&dir).expect("create claude fixture dir");
    fs::write(dir.join(file_name), lines.join("\n")).expect("write claude fixture");
}

/// Codex session JSONL line (shape per the vendored codex adapter tests).
pub fn codex_line(
    timestamp: &str,
    model: &str,
    input: u64,
    cached: u64,
    output: u64,
    reasoning: u64,
) -> String {
    format!(
        r#"{{"timestamp":"{timestamp}","type":"event_msg","payload":{{"type":"token_count","info":{{"model":"{model}","last_token_usage":{{"input_tokens":{input},"cached_input_tokens":{cached},"output_tokens":{output},"reasoning_output_tokens":{reasoning},"total_tokens":{}}}}}}}}}"#,
        input + cached + output + reasoning
    )
}

/// Writes a Codex session JSONL under `<root>/codex/sessions/`.
pub fn write_codex_session(root: &Path, file_name: &str, lines: &[String]) {
    let dir = root.join("codex/sessions");
    fs::create_dir_all(&dir).expect("create codex fixture dir");
    fs::write(dir.join(file_name), lines.join("\n")).expect("write codex fixture");
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

/// Audited generation blob for Antigravity conversation databases.
pub fn generation_blob(
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
pub fn write_antigravity_db(root: &Path, name: &str, generations: &[Vec<u8>]) {
    use sqlite::Connection;
    let dir = root.join("conversations");
    fs::create_dir_all(&dir).expect("create antigravity fixture dir");
    let db_path = dir.join(name);
    // Recreate from scratch so repeated runs (golden fixtures) start clean.
    let _ = fs::remove_file(&db_path);
    let db = Connection::open(&db_path).expect("open antigravity fixture db");
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

pub const DAY_1: &str = "2026-01-02";
pub const DAY_2: &str = "2026-01-03";
pub const DAY_3: &str = "2026-01-04";
pub const DAY_1_SECONDS: u64 = 1_767_312_000; // 2026-01-02T00:00:00Z
pub const DAY_2_SECONDS: u64 = 1_767_398_100; // 2026-01-03T02:35:00Z
pub const DAY_3_SECONDS: u64 = 1_767_484_500; // 2026-01-04T05:15:00Z

pub fn date(text: &str) -> NaiveDate {
    NaiveDate::parse_from_str(text, "%Y-%m-%d").expect("fixture date")
}

pub fn window(start: &str, end: &str) -> CollectWindow {
    CollectWindow::new(date(start), date(end)).expect("fixture window")
}

pub fn model(name: &str) -> ModelName {
    ModelName(name.to_string())
}

// --- SQLite agent schema builders (real vendored schemas, no mocks) --------
//
// Shapes audited against each vendored adapter's loader/parser and its own
// unit tests. All builders create REAL SQLite databases via the sqlite crate.

use sqlite::Connection;

/// Antigravity: `<root>/conversations/<name>.db` (see write_antigravity_db).

/// Goose: `<root>/sessions.db` with the vendored `sessions` schema.
pub fn create_goose_db(path: &Path) {
    // Recreate from scratch so repeated runs start clean.
    let _ = fs::remove_file(path);
    let db = Connection::open(path).expect("open goose db");
    db.execute(
        r#"
CREATE TABLE sessions (
    id TEXT PRIMARY KEY,
    model_config_json TEXT,
    provider_name TEXT,
    created_at TEXT,
    total_tokens INTEGER,
    input_tokens INTEGER,
    output_tokens INTEGER,
    accumulated_total_tokens INTEGER,
    accumulated_input_tokens INTEGER,
    accumulated_output_tokens INTEGER
)
"#,
    )
    .expect("create goose sessions table");
}

pub fn insert_goose_session(
    path: &Path,
    id: &str,
    model_config: &str,
    created_at: &str,
    total: i64,
    input: i64,
    output: i64,
) {
    let db = Connection::open(path).expect("open goose db");
    let mut statement = db
        .prepare("INSERT INTO sessions (id, model_config_json, provider_name, created_at, total_tokens, input_tokens, output_tokens) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)")
        .expect("prepare goose insert");
    statement.bind((1, id)).unwrap();
    statement.bind((2, model_config)).unwrap();
    statement.bind((3, "anthropic")).unwrap();
    statement.bind((4, created_at)).unwrap();
    statement.bind((5, total)).unwrap();
    statement.bind((6, input)).unwrap();
    statement.bind((7, output)).unwrap();
    statement.next().expect("insert goose row");
}

/// Hermes: `<root>/state.db` with the vendored `sessions` schema.
pub fn create_hermes_db(path: &Path) {
    let _ = fs::remove_file(path);
    let db = Connection::open(path).expect("open hermes db");
    db.execute(
        r#"
CREATE TABLE sessions (
    id TEXT PRIMARY KEY,
    source TEXT NOT NULL,
    model TEXT,
    started_at REAL NOT NULL,
    message_count INTEGER DEFAULT 0,
    input_tokens INTEGER DEFAULT 0,
    output_tokens INTEGER DEFAULT 0,
    cache_read_tokens INTEGER DEFAULT 0,
    cache_write_tokens INTEGER DEFAULT 0,
    reasoning_tokens INTEGER DEFAULT 0,
    billing_provider TEXT,
    estimated_cost_usd REAL,
    actual_cost_usd REAL
)
"#,
    )
    .expect("create hermes sessions table");
}

#[allow(clippy::too_many_arguments)]
pub fn insert_hermes_session(
    path: &Path,
    id: &str,
    model: &str,
    started_at: f64,
    input: i64,
    output: i64,
    cache_read: i64,
    cache_write: i64,
    reasoning: i64,
    estimated_cost: f64,
    actual_cost: Option<f64>,
) {
    let db = Connection::open(path).expect("open hermes db");
    let mut statement = db.prepare("INSERT INTO sessions (id, source, model, started_at, message_count, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, reasoning_tokens, billing_provider, estimated_cost_usd, actual_cost_usd) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)").expect("prepare hermes insert");
    statement.bind((1, id)).unwrap();
    statement.bind((2, "cli")).unwrap();
    statement.bind((3, model)).unwrap();
    statement.bind((4, started_at)).unwrap();
    statement.bind((5, 1)).unwrap();
    statement.bind((6, input)).unwrap();
    statement.bind((7, output)).unwrap();
    statement.bind((8, cache_read)).unwrap();
    statement.bind((9, cache_write)).unwrap();
    statement.bind((10, reasoning)).unwrap();
    statement.bind((11, "anthropic")).unwrap();
    statement.bind((12, estimated_cost)).unwrap();
    match actual_cost {
        Some(value) => statement.bind((13, value)).unwrap(),
        None => statement.bind((13, ())).unwrap(),
    }
    statement.next().expect("insert hermes row");
}

/// Kilo: `<root>/kilo.db` with the vendored `message` schema.
pub fn create_kilo_db(path: &Path) {
    let _ = fs::remove_file(path);
    let db = Connection::open(path).expect("open kilo db");
    db.execute("CREATE TABLE message (id TEXT, session_id TEXT, data TEXT)")
        .expect("create kilo message table");
}

pub fn insert_kilo_message(path: &Path, id: &str, session_id: &str, data: &str) {
    let db = Connection::open(path).expect("open kilo db");
    let mut statement = db
        .prepare("INSERT INTO message (id, session_id, data) VALUES (?1, ?2, ?3)")
        .expect("prepare kilo insert");
    statement.bind((1, id)).unwrap();
    statement.bind((2, session_id)).unwrap();
    statement.bind((3, data)).unwrap();
    statement.next().expect("insert kilo row");
}

/// OpenCode: `<root>/opencode.db` with the vendored `message` schema
/// (current layout with the `time_created` column).
pub fn create_opencode_db(path: &Path) {
    let _ = fs::remove_file(path);
    let db = Connection::open(path).expect("open opencode db");
    db.execute(
        "CREATE TABLE IF NOT EXISTS message (id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER NOT NULL DEFAULT 0, data TEXT)",
    )
    .expect("create opencode message table");
}

pub fn insert_opencode_message(
    path: &Path,
    id: &str,
    session_id: &str,
    time_created: i64,
    data: &str,
) {
    let db = Connection::open(path).expect("open opencode db");
    let mut statement = db
        .prepare("INSERT INTO message (id, session_id, time_created, data) VALUES (?1, ?2, ?3, ?4)")
        .expect("prepare opencode insert");
    statement.bind((1, id)).unwrap();
    statement.bind((2, session_id)).unwrap();
    statement.bind((3, time_created)).unwrap();
    statement.bind((4, data)).unwrap();
    statement.next().expect("insert opencode row");
}
