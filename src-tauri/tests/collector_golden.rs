//! Golden parity tests for the typed Collector API.
//!
//! Fixtures are committed under `tests/golden/<case>/input/…` and the
//! expected `CollectorResponseV1` JSON under `expected.json`. Everything is
//! synthetic, offline, and free of real user data. Expected files are never
//! rewritten at test runtime: a mismatch FAILS and the diff must be reviewed
//! and the golden updated by hand (see docs/V0.3_PHASE1_COLLECTOR_DESIGN.md).
//!
//! Parity basis: the values are derived from the same vendored v20.0.20
//! engine the v0.2 sidecar (npm ccusage) reports through, and each golden's
//! arithmetic is documented inline; see the parity matrix in the Phase 1
//! design doc for the per-field audit.

mod common;

use std::{fs, path::PathBuf};

use coding_agent_monitor_lib::collector::{
    ccusage::AgentCollector, protocol::CollectorResponseV1, AgentKind, CollectRequest, Collector,
};

fn golden_dir(case: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(case)
}

/// Runs one golden case: `agent`'s collector against the committed fixture
/// input root, byte-compared against `expected.json`.
fn run_golden(case: &str, agent: AgentKind) {
    let input_root = golden_dir(case).join("input");
    assert!(
        input_root.is_dir(),
        "golden fixture input missing: {}",
        input_root.display()
    );
    // Explicit paths — the golden harness must not depend on env resolution.
    let request = CollectRequest::new(agent).with_source(
        coding_agent_monitor_lib::collector::DataSource::Paths(vec![input_root.clone()]),
    );

    let result = AgentCollector::new(agent)
        .collect(&request)
        .unwrap_or_else(|error| panic!("{case}: collection failed: {error}"));

    let response = CollectorResponseV1::ok(format!("golden-{case}"), &result);
    let actual = serde_json::to_string_pretty(&response).expect("serialize response") + "\n";

    let expected_path = golden_dir(case).join("expected.json");
    let expected = fs::read_to_string(&expected_path)
        .unwrap_or_else(|error| panic!("{case}: read {}: {error}", expected_path.display()));

    assert!(
        actual == expected,
        "{case}: golden mismatch.\n--- expected (committed, audited) ---\n{expected}\n--- actual ---\n{actual}\nIf the diff is an intended engine change, review it and update expected.json by hand; never overwrite goldens from tests."
    );
}

#[test]
fn golden_claude_basic() {
    run_golden("claude-basic", AgentKind::Claude);
}

#[test]
fn golden_codex_basic() {
    run_golden("codex-basic", AgentKind::Codex);
}

#[test]
fn golden_antigravity_basic() {
    // Antigravity input is a binary SQLite fixture, built at test time from
    // the audited generation-blob encoder (common::generation_blob) instead of
    // committing an opaque binary file. The committed expected.json pins the
    // resulting typed report: model gemini-3.1-pro (resolved from
    // gemini-3.1-pro-low), input 1000 system + 6321 fresh = 7321, cache_read
    // 10, output 604, total 7935, cost (7321*2 + 10*0.2 + 604*12)/1e6 USD =
    // 0.021892 USD = 21_892_000 nano (pricing: input $2, cache_read $0.2,
    // output $12 per million tokens).
    let case_dir = golden_dir("antigravity-basic");
    let root = case_dir.join("input");
    common::write_antigravity_db(
        &root,
        "conv-1.db",
        &[common::generation_blob(
            "gemini-3.1-pro-low",
            common::DAY_1_SECONDS,
            1000,
            6321,
            10,
            604,
            "resp-1",
        )],
    );
    run_golden("antigravity-basic", AgentKind::Antigravity);
}

#[test]
fn golden_claude_corrupt_lines_surface_diagnostics() {
    // Mixed file: one valid record plus malformed lines. The valid record is
    // preserved (keep as much data as possible) and each skipped corrupt
    // record surfaces as a structured diagnostic.
    run_golden("claude-corrupt", AgentKind::Claude);
}
