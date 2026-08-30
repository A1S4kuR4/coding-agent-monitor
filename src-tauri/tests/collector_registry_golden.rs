//! Golden parity fixtures for the JSONL/JSON-format agents of the registry
//! (amp, codebuff, droid, gemini, kimi, openclaw, qwen, grok, copilot, pi).
//! Shapes are audited against each vendored adapter's own tests;
//! `expected.json` values are hand-reviewed against the pricing snapshots.
//! Never auto-overwritten at test runtime.

use std::{fs, path::PathBuf};

mod common;

use coding_agent_monitor_lib::collector::{
    ccusage::AgentCollector, protocol::CollectorResponseV1, AgentKind, CollectRequest, Collector,
    DataSource,
};

fn golden_dir(case: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(case)
}

fn run_golden(case: &str, agent: AgentKind) {
    let mut roots = vec![golden_dir(case).join("input")];
    if case == "copilot-basic" {
        // Copilot's data-root override is the OTEL exporter FILE itself.
        roots = vec![roots.pop().expect("root").join("copilot.jsonl")];
    }
    for root in &roots {
        assert!(root.exists(), "missing {}", root.display());
    }
    let request = CollectRequest::new(agent).with_source(DataSource::Paths(roots));
    let result = AgentCollector::new(agent)
        .collect(&request)
        .unwrap_or_else(|error| panic!("{case}: collection failed: {error}"));
    let response = CollectorResponseV1::ok(format!("golden-{case}"), &result);
    let actual = serde_json::to_string_pretty(&response).expect("serialize") + "\n";
    let expected_path = golden_dir(case).join("expected.json");
    let expected = fs::read_to_string(&expected_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", expected_path.display()));
    assert!(
        actual == expected,
        "{case}: golden mismatch.\n--- expected ---\n{expected}\n--- actual ---\n{actual}\nUpdate expected.json by hand after review; never overwrite from tests."
    );
}

#[test]
fn golden_amp_basic() {
    // Amp threads live under `<root>/threads/*.json`; usage comes from the
    // assistant messages' usage block.
    run_golden("amp-basic", AgentKind::Amp);
}

#[test]
fn golden_codebuff_basic() {
    run_golden("codebuff-basic", AgentKind::Codebuff);
}

#[test]
fn golden_droid_basic() {
    // Droid settings files carry the tokenUsage block (incl. thinkingTokens,
    // which the vendor tracks as reasoning-like extra tokens).
    run_golden("droid-basic", AgentKind::Droid);
}

#[test]
fn golden_gemini_basic() {
    run_golden("gemini-basic", AgentKind::Gemini);
}

#[test]
fn golden_kimi_basic() {
    run_golden("kimi-basic", AgentKind::Kimi);
}

#[test]
fn golden_openclaw_basic() {
    run_golden("openclaw-basic", AgentKind::OpenClaw);
}

#[test]
fn golden_qwen_basic() {
    run_golden("qwen-basic", AgentKind::Qwen);
}

#[test]
fn golden_grok_basic() {
    run_golden("grok-basic", AgentKind::Grok);
}

#[test]
fn golden_copilot_basic() {
    // Copilot's override root is the OTEL exporter JSONL file itself (the
    // same shape its env var accepts).
    run_golden("copilot-basic", AgentKind::Copilot);
}

#[test]
fn golden_pi_basic() {
    run_golden("pi-basic", AgentKind::Pi);
}

// --- SQLite-format agents (Phase 2: closes the four golden gaps) ------------
//
// Databases are built at test time through the real vendored schemas
// (tests/common/mod.rs), values hand-audited against the pricing snapshots.
// The `expected.json` files are never auto-overwritten.

fn run_sqlite_golden(case: &str, agent: AgentKind, roots: Vec<PathBuf>) {
    let request = CollectRequest::new(agent).with_source(DataSource::Paths(roots));
    let result = AgentCollector::new(agent)
        .collect(&request)
        .unwrap_or_else(|error| panic!("{case}: collection failed: {error}"));
    let response = CollectorResponseV1::ok(format!("golden-{case}"), &result);
    let actual = serde_json::to_string_pretty(&response).expect("serialize") + "\n";
    let expected_path = golden_dir(case).join("expected.json");
    let expected = fs::read_to_string(&expected_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", expected_path.display()));
    assert!(
        actual == expected,
        "{case}: golden mismatch.\n--- expected ---\n{expected}\n--- actual ---\n{actual}"
    );
}

#[test]
fn golden_goose_basic() {
    let root = golden_dir("goose-basic").join("input");
    fs::create_dir_all(&root).expect("create input");
    let db = root.join("sessions.db");
    common::create_goose_db(&db);
    // total 180 = 100 input + 50 output + 30 unclassified (goose reports no
    // cache columns; the delta rides the reasoning/unclassified buckets as
    // the vendored parser computes it).
    common::insert_goose_session(
        &db,
        "session-a",
        r#"{"model_name":"claude-sonnet-4-20250514"}"#,
        "2026-05-01 01:02:03",
        180,
        100,
        50,
    );
    run_sqlite_golden("goose-basic", AgentKind::Goose, vec![db]);
}

#[test]
fn golden_hermes_basic() {
    let root = golden_dir("hermes-basic").join("input");
    fs::create_dir_all(&root).expect("create input");
    let db = root.join("state.db");
    common::create_hermes_db(&db);
    // cost = actual_cost_usd (0.03) preferred over estimated (0.05).
    common::insert_hermes_session(
        &db,
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
    run_sqlite_golden("hermes-basic", AgentKind::Hermes, vec![db]);
}

#[test]
fn golden_kilo_basic() {
    let root = golden_dir("kilo-basic").join("input");
    fs::create_dir_all(&root).expect("create input");
    let db = root.join("kilo.db");
    common::create_kilo_db(&db);
    // cost comes from the message payload (0.02); reasoning 5 and cache
    // read/write are exercised in the tokens block.
    common::insert_kilo_message(
        &db,
        "msg-1",
        "sess-1",
        r#"{"id":"msg-1","role":"assistant","providerID":"anthropic","modelID":"claude-sonnet-4-20250514","time":{"created":1767312000000},"tokens":{"input":100,"output":50,"reasoning":5,"cache":{"read":10,"write":20}},"cost":0.02}"#,
    );
    run_sqlite_golden("kilo-basic", AgentKind::Kilo, vec![root]);
}

#[test]
fn golden_opencode_basic() {
    let root = golden_dir("opencode-basic").join("input");
    fs::create_dir_all(&root).expect("create input");
    let db = root.join("opencode.db");
    common::create_opencode_db(&db);
    // cost comes from the message payload (0.02); cache read/write exercised.
    common::insert_opencode_message(
        &db,
        "msg-1",
        "sess-1",
        1_767_312_000_000,
        r#"{"id":"msg-1","sessionID":"sess-1","modelID":"claude-sonnet-4-20250514","providerID":"anthropic","time":{"created":1767312000000},"tokens":{"input":100,"output":50,"cache":{"read":10,"write":20}},"cost":0.02}"#,
    );
    run_sqlite_golden("opencode-basic", AgentKind::OpenCode, vec![root]);
}
