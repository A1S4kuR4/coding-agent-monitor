//! Gate 0 PoC (docs/V0.3_GATE0_VERIFICATION.md): proves the vendored ccusage
//! workspace runs in-process — no sidecar, no CLI parser, no stdout — and that
//! the single SQLite native linkage (sqlite3-src) links and executes alongside
//! the app's own `sqlite`-based DB layer in the same binary.
//!
//! The production sidecar collection path is intentionally NOT switched to
//! this seam yet (Phase 1+ decision).

use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use ccusage_adapter_all::daily_report_json_by_agent;
use ccusage_cli::SharedArgs;

/// The full per-agent data-dir env table used by the vendored workspace's own
/// unified-report tests (ccusage-adapter-all/src/tests.rs). Every agent is
/// pointed at an empty fixture directory so the test cannot ingest real user
/// data; only the two agents under test get populated roots.
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

/// RAII guard restoring every touched env var on drop, so parallel CAM tests
/// are not affected by process-global env mutations.
struct EnvGuard(Vec<&'static str>);

impl EnvGuard {
    fn set(key: &'static str, value: &Path) {
        // SAFETY: single-threaded test setup; the guard restores the previous
        // value (or removes it) when dropped.
        unsafe { std::env::set_var(key, value) };
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for key in &self.0 {
            // SAFETY: see EnvGuard::set.
            unsafe { std::env::remove_var(key) };
        }
    }
}

fn temp_fixture_root() -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos();
    std::env::temp_dir().join(format!("coding-agent-monitor-ccusage-poc-{unique}"))
}

#[test]
fn daily_report_json_by_agent_collects_claude_fixture_in_process() {
    let root = temp_fixture_root();
    fs::create_dir_all(root.join("claude/projects/cam")).expect("create claude fixture");
    fs::create_dir_all(root.join("antigravity")).expect("create antigravity fixture");
    fs::write(
        root.join("claude/projects/cam/session-a.jsonl"),
        r#"{"timestamp":"2099-01-02T00:00:00.000Z","sessionId":"session-a","requestId":"req-direct","costUSD":0.01,"message":{"usage":{"input_tokens":100,"output_tokens":50,"cache_creation_input_tokens":0,"cache_read_input_tokens":10},"model":"claude-sonnet-4-20250514","id":"msg-direct"}}"#,
    )
    .expect("write claude fixture");

    // Isolate every agent at an empty directory; HOME/USERPROFILE/XDG too.
    // CLAUDE_CONFIG_DIR must be the config root containing `projects/`.
    let mut touched = Vec::new();
    for key in AGENT_ENV_KEYS {
        let value = if key == "CLAUDE_CONFIG_DIR" {
            root.join("claude")
        } else {
            root.clone()
        };
        EnvGuard::set(key, &value);
        touched.push(key);
    }
    for (key, value) in [
        ("HOME", root.join("empty-home")),
        ("USERPROFILE", root.join("empty-home")),
        ("XDG_CONFIG_HOME", root.join("empty-xdg-config")),
    ] {
        fs::create_dir_all(&value).expect("create home fixture");
        EnvGuard::set(key, &value);
        touched.push(key);
    }
    let _env = EnvGuard(touched);

    let shared = SharedArgs {
        json: true,
        offline: true,
        timezone: Some("UTC".to_string()),
        single_thread: true,
        ..SharedArgs::default()
    };

    let report =
        daily_report_json_by_agent(&shared).expect("in-process unified daily report");

    let daily = report["daily"].as_array().expect("daily rows");
    assert_eq!(daily.len(), 1, "exactly one daily row from the fixture");
    assert_eq!(daily[0]["period"], "2099-01-02", "daily bucket date");
    assert_eq!(
        daily[0]["agent"], "all",
        "unified daily row aggregates all agents"
    );
    assert_eq!(
        daily[0]["totalTokens"], 160,
        "100 input + 50 output + 10 cache read"
    );
    let breakdowns = daily[0]["agents"].as_array().expect("agent breakdowns");
    let claude = breakdowns
        .iter()
        .find(|b| b["agent"] == "claude")
        .expect("claude agent breakdown");
    assert_eq!(claude["totalTokens"], 160, "claude breakdown tokens");
    assert_eq!(
        claude["modelsUsed"][0], "claude-sonnet-4-20250514",
        "model credited in breakdown"
    );
    let metadata_agents = daily[0]["metadata"]["agents"]
        .as_array()
        .expect("metadata agent ids");
    assert!(
        metadata_agents.iter().any(|id| id == "claude"),
        "claude credited in metadata agent ids"
    );
    assert_eq!(
        report["totals"]["totalTokens"], 160,
        "totals aggregate the fixture"
    );

    fs::remove_dir_all(&root).expect("clean up fixture");
}
