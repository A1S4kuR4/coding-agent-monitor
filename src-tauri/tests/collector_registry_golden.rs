//! Golden parity fixtures for the remaining JSONL/JSON-format agents of the
//! registry (amp, codebuff, droid, gemini, kimi, openclaw, qwen, grok,
//! copilot, pi). Shapes are audited against each vendored adapter's own
//! tests; `expected.json` values are hand-reviewed against the pricing
//! snapshots. Never auto-overwritten at test runtime.
//!
//! SQLite-format agents (goose, hermes, kilo, opencode) have no committed
//! non-empty fixture yet — their DB schemas are exercised by the vendored
//! adapters' own tests, and end-to-end goldens for them are owned by the
//! Phase 4 sidecar shadow gate (see the parity matrix in
//! docs/V0.3_PHASE1_COLLECTOR_DESIGN.md).

use std::{fs, path::PathBuf};

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
