//! Semantic contract tests for the typed Collector API: dedupe/aggregation,
//! date gaps, big-integer tokens, reasoning-token handling, the three cost
//! states (priced / priced-zero / missing pricing), and cross-agent isolation
//! at 0/1/2/6-agent scale.

mod common;

use std::fs;

use coding_agent_monitor_lib::collector::{
    ccusage::AgentCollector, AgentKind, CollectRequest, CollectResult, Collector, CostNanoUsd,
};
use common::{
    claude_line, codex_line, date, fixture_root, isolate_env, model, window, write_antigravity_db,
    write_claude_session, write_codex_session, DAY_1, DAY_1_SECONDS, DAY_2_SECONDS, DAY_3,
};

fn collect(agent: AgentKind, request: &CollectRequest) -> CollectResult {
    AgentCollector::new(agent)
        .collect(request)
        .expect("collect")
}

#[test]
fn duplicate_usage_events_are_not_double_counted() {
    let root = fixture_root("dupes");
    // Identical request ids: the vendored dedupe key must collapse them.
    let line = claude_line(
        "2026-01-02T00:00:00.000Z",
        "req-dup",
        "claude-sonnet-4-20250514",
        100,
        10,
        0,
        Some(0.01),
    );
    write_claude_session(&root, "session-a.jsonl", &[line.clone(), line]);
    let _env = isolate_env(&root);

    let result = collect(AgentKind::Claude, &CollectRequest::new(AgentKind::Claude));
    assert_eq!(result.records.len(), 1);
    assert_eq!(
        result.records[0].input_tokens, 100,
        "duplicate must not add tokens"
    );
}

#[test]
fn same_day_sessions_aggregate_into_one_record() {
    let root = fixture_root("same-day");
    write_claude_session(
        &root,
        "session-a.jsonl",
        &[claude_line(
            "2026-01-02T01:00:00.000Z",
            "req-a",
            "claude-sonnet-4-20250514",
            100,
            10,
            0,
            Some(0.01),
        )],
    );
    write_claude_session(
        &root,
        "session-b.jsonl",
        &[claude_line(
            "2026-01-02T09:00:00.000Z",
            "req-b",
            "claude-sonnet-4-20250514",
            200,
            20,
            0,
            Some(0.02),
        )],
    );
    let _env = isolate_env(&root);

    let result = collect(AgentKind::Claude, &CollectRequest::new(AgentKind::Claude));
    assert_eq!(result.records.len(), 1, "one record per day");
    assert_eq!(result.records[0].input_tokens, 300, "sessions summed");
    assert_eq!(result.records[0].output_tokens, 30);
    assert_eq!(
        result.records[0].cost.map(|c| c.as_nano_usd()),
        Some(30_000_000)
    );
}

#[test]
fn date_gaps_are_explicit_absences() {
    let root = fixture_root("gaps");
    write_claude_session(
        &root,
        "session-a.jsonl",
        &[
            claude_line(
                "2026-01-02T00:00:00.000Z",
                "req-g1",
                "claude-sonnet-4-20250514",
                100,
                1,
                0,
                Some(0.01),
            ),
            // No usage on 2026-01-03: the gap.
            claude_line(
                "2026-01-04T00:00:00.000Z",
                "req-g2",
                "claude-sonnet-4-20250514",
                400,
                4,
                0,
                Some(0.04),
            ),
        ],
    );
    let _env = isolate_env(&root);

    let result = collect(
        AgentKind::Claude,
        &CollectRequest::new(AgentKind::Claude).with_window(window(DAY_1, DAY_3)),
    );
    assert_eq!(result.records.len(), 2);
    assert_eq!(result.records[0].date, date(DAY_1));
    assert_eq!(result.records[1].date, date(DAY_3));
}

#[test]
fn big_integer_tokens_pass_through_exactly() {
    let root = fixture_root("big-int");
    // Above JavaScript's 2^53 safe-integer range.
    let huge = 9_007_199_254_740_993u64;
    write_claude_session(
        &root,
        "session-a.jsonl",
        &[claude_line(
            "2026-01-02T00:00:00.000Z",
            "req-big",
            "claude-sonnet-4-20250514",
            huge,
            1,
            0,
            None,
        )],
    );
    let _env = isolate_env(&root);

    let result = collect(AgentKind::Claude, &CollectRequest::new(AgentKind::Claude));
    assert_eq!(result.records[0].input_tokens, huge);
    assert_eq!(result.records[0].total_tokens, huge + 1);
}

#[test]
fn reasoning_tokens_are_outside_the_daily_token_totals() {
    let root = fixture_root("reasoning");
    // codex reports reasoning separately from output; the vendored daily
    // aggregation counts input+cached+output in the token totals and tracks
    // reasoning as extra total tokens (not part of the daily aggregates).
    write_codex_session(
        &root,
        "session-a.jsonl",
        &[codex_line(
            "2026-01-02T08:01:00.000Z",
            "gpt-5.2",
            1_000,
            100,
            200,
            20,
        )],
    );
    let _env = isolate_env(&root);

    let result = collect(AgentKind::Codex, &CollectRequest::new(AgentKind::Codex));
    assert_eq!(result.records.len(), 1);
    let record = &result.records[0];
    // Codex reports input including cached tokens; the vendored aggregation
    // splits them into a non-cached input bucket (900) and cache_read (100).
    assert_eq!(record.input_tokens, 900);
    assert_eq!(record.cache_read_tokens, 100);
    assert_eq!(record.output_tokens, 200);
    // The source explicitly reports 20 reasoning tokens; the remaining delta
    // against the agent-reported total (the double-counted cached input) is
    // unclassified, never silently folded into reasoning.
    assert_eq!(record.reasoning_tokens, 20);
    assert_eq!(record.unclassified_tokens, 100);
    assert_eq!(
        record.total_tokens, 1_320,
        "900 input + 100 cache read + 200 output + 20 reasoning + 100 unclassified"
    );
    assert!(coding_agent_monitor_lib::collector::token_bucket_invariant_holds(record));
}

#[test]
fn token_bucket_violation_surfaces_as_diagnostic() {
    let root = fixture_root("invariant");
    // Agent-reported total (150) is LESS than its own buckets
    // (1000 input + 100 cached + 200 output + 20 reasoning): the vendor
    // passes the numbers through, the collector clamps unclassified to 0 and
    // flags the violation — never silently rewriting vendor numbers.
    let line = r#"{"timestamp":"2026-01-02T08:01:00.000Z","type":"event_msg","payload":{"type":"token_count","info":{"model":"gpt-5.2","last_token_usage":{"input_tokens":1000,"cached_input_tokens":100,"output_tokens":200,"reasoning_output_tokens":20,"total_tokens":150}}}}"#;
    write_codex_session(&root, "session-a.jsonl", &[line.to_string()]);
    let _env = isolate_env(&root);

    let result = collect(AgentKind::Codex, &CollectRequest::new(AgentKind::Codex));
    assert_eq!(result.records.len(), 1);
    let record = &result.records[0];
    assert_eq!(record.total_tokens, 150, "vendor total passed through");
    assert_eq!(record.unclassified_tokens, 0, "clamped, not fabricated");
    assert!(result
        .diagnostics
        .iter()
        .any(|diag| diag.kind
            == coding_agent_monitor_lib::collector::DiagnosticKind::InvariantViolation));
    assert!(
        !coding_agent_monitor_lib::collector::token_bucket_invariant_holds(record),
        "violating records do not pretend the invariant holds"
    );
}

#[test]
fn u64_boundary_tokens_do_not_panic_and_use_saturating_semantics() {
    let root = fixture_root("u64-edge");
    // total == u64::MAX with buckets that would overflow when summed: the
    // checked arithmetic clamps and flags instead of panicking or wrapping.
    let line = r#"{"timestamp":"2026-01-02T08:01:00.000Z","type":"event_msg","payload":{"type":"token_count","info":{"model":"gpt-5.2","last_token_usage":{"input_tokens":18446744073709551615,"cached_input_tokens":0,"output_tokens":18446744073709551615,"reasoning_output_tokens":0,"total_tokens":18446744073709551615}}}}"#;
    write_codex_session(&root, "session-a.jsonl", &[line.to_string()]);
    let _env = isolate_env(&root);

    let result = collect(AgentKind::Codex, &CollectRequest::new(AgentKind::Codex));
    assert_eq!(result.records.len(), 1);
    let record = &result.records[0];
    assert_eq!(
        record.input_tokens,
        u64::MAX,
        "non-cached input saturates (u64::MAX - 0 cached)"
    );
    assert!(result
        .diagnostics
        .iter()
        .any(|diag| diag.kind
            == coding_agent_monitor_lib::collector::DiagnosticKind::InvariantViolation));
}

#[test]
fn cost_has_three_states_priced_zero_and_missing() {
    // 1. Priced: cost comes from the log (costUSD present) or the model table.
    let priced = {
        let root = fixture_root("cost-priced");
        write_claude_session(
            &root,
            "session-a.jsonl",
            &[claude_line(
                "2026-01-02T00:00:00.000Z",
                "req-p",
                "claude-sonnet-4-20250514",
                100,
                10,
                0,
                Some(0.01),
            )],
        );
        // Block scope: the env guard must be released before the next
        // scenario acquires the env lock again.
        let _env = isolate_env(&root);
        collect(AgentKind::Claude, &CollectRequest::new(AgentKind::Claude))
    };
    assert_eq!(
        priced.records[0].cost,
        Some(CostNanoUsd::from_nano(10_000_000))
    );
    assert!(priced.records[0].models_missing_pricing.is_empty());

    // 2. Explicit zero: a priced model whose computed cost is exactly zero.
    let zero = {
        let root = fixture_root("cost-zero");
        write_claude_session(
            &root,
            "session-a.jsonl",
            &[claude_line(
                "2026-01-02T00:00:00.000Z",
                "req-z",
                "claude-sonnet-4-20250514",
                100,
                10,
                0,
                Some(0.0),
            )],
        );
        let _env = isolate_env(&root);
        collect(AgentKind::Claude, &CollectRequest::new(AgentKind::Claude))
    };
    assert_eq!(
        zero.records[0].cost,
        Some(CostNanoUsd::ZERO),
        "0.0 from a priced model is a genuine zero"
    );
    assert!(zero.records[0].models_missing_pricing.is_empty());

    // 3. Missing pricing: unknown model, no costUSD — cost is None, never 0.
    let missing = {
        let root = fixture_root("cost-missing");
        write_claude_session(
            &root,
            "session-a.jsonl",
            &[claude_line(
                "2026-01-02T00:00:00.000Z",
                "req-m",
                "totally-unknown-model-xyz",
                500,
                100,
                0,
                None,
            )],
        );
        let _env = isolate_env(&root);
        collect(AgentKind::Claude, &CollectRequest::new(AgentKind::Claude))
    };
    assert_eq!(missing.records[0].cost, None);
    assert_eq!(
        missing.records[0].models_missing_pricing,
        vec![model("totally-unknown-model-xyz")]
    );
}

#[test]
fn six_agents_collect_independently_without_cross_talk() {
    // 0/1/2/6-agent isolation scale: with empty fixtures for six agents, each
    // collector must succeed with an empty result and none may observe
    // another's data.
    let root = fixture_root("six-agents");
    let _env = isolate_env(&root);
    let agents = [
        AgentKind::Claude,
        AgentKind::Codex,
        AgentKind::Amp,
        AgentKind::Gemini,
        AgentKind::Kimi,
        AgentKind::Antigravity,
    ];

    // 0 agents' worth of data, then one agent with data, then two — all via
    // the same per-agent contract.
    for agent in agents {
        let result = collect(agent, &CollectRequest::new(agent));
        assert!(result.is_empty(), "{:?} should be empty", agent);
        assert!(result.diagnostics.is_empty());
    }

    write_claude_session(
        &root,
        "session-a.jsonl",
        &[claude_line(
            "2026-01-02T00:00:00.000Z",
            "req-solo",
            "claude-sonnet-4-20250514",
            100,
            10,
            0,
            Some(0.01),
        )],
    );
    let claude = collect(AgentKind::Claude, &CollectRequest::new(AgentKind::Claude));
    assert_eq!(claude.records.len(), 1);
    for agent in agents.iter().skip(1) {
        assert!(
            collect(*agent, &CollectRequest::new(*agent)).is_empty(),
            "{:?} must not see claude data",
            agent
        );
    }
}

#[test]
fn antigravity_and_claude_do_not_double_count_the_same_data() {
    // The antigravity adapter is registered once inside the vendored unified
    // loader; per-agent collectors each read only their own registry entry.
    let root = fixture_root("no-double-count");
    write_antigravity_db(
        &root,
        "conv-1.db",
        &[common::generation_blob(
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

    let antigravity = collect(
        AgentKind::Antigravity,
        &CollectRequest::new(AgentKind::Antigravity),
    );
    assert_eq!(antigravity.records[0].total_tokens, 7_935);

    let claude = collect(AgentKind::Claude, &CollectRequest::new(AgentKind::Claude));
    assert!(
        claude.is_empty(),
        "antigravity conversation data must never be attributed to claude"
    );
}

#[test]
fn antigravity_records_keep_deterministic_order_across_databases() {
    let root = fixture_root("ag-order");
    // Deliberately written out of chronological order across two DBs.
    write_antigravity_db(
        &root,
        "conv-2.db",
        &[common::generation_blob(
            "gemini-3.1-pro-low",
            DAY_2_SECONDS,
            500,
            100,
            0,
            50,
            "resp-b",
        )],
    );
    write_antigravity_db(
        &root,
        "conv-1.db",
        &[common::generation_blob(
            "gemini-3.1-pro-low",
            DAY_1_SECONDS,
            1000,
            6321,
            10,
            604,
            "resp-a",
        )],
    );
    let _env = isolate_env(&root);

    let result = collect(
        AgentKind::Antigravity,
        &CollectRequest::new(AgentKind::Antigravity),
    );
    let dates: Vec<_> = result.records.iter().map(|record| record.date).collect();
    let mut sorted = dates.clone();
    sorted.sort();
    assert_eq!(dates, sorted, "records must come back date-ascending");
}

// --- Input bounds (worker stdin hardening groundwork) -----------------------

#[test]
fn empty_paths_source_is_rejected_safely() {
    let root = fixture_root("bounds-empty");
    let _env = isolate_env(&root);
    let request = CollectRequest::new(AgentKind::Claude).with_source(
        coding_agent_monitor_lib::collector::DataSource::Paths(vec![]),
    );
    let error = AgentCollector::new(AgentKind::Claude)
        .collect(&request)
        .expect_err("empty roots must be rejected");
    assert!(matches!(
        error,
        coding_agent_monitor_lib::collector::CollectorError::InvalidRequest { .. }
    ));
}

#[test]
fn oversized_inputs_are_rejected_without_panicking() {
    let root = fixture_root("bounds-oversize");
    let _env = isolate_env(&root);
    let collector = AgentCollector::new(AgentKind::Claude);

    // Too many roots.
    let many = vec![root.clone(); 17];
    let error = collector
        .collect(
            &CollectRequest::new(AgentKind::Claude)
                .with_source(coding_agent_monitor_lib::collector::DataSource::Paths(many)),
        )
        .expect_err("root count cap");
    assert!(matches!(
        error,
        coding_agent_monitor_lib::collector::CollectorError::InvalidRequest { .. }
    ));

    // One oversized root path.
    let long = root.join("x".repeat(5000));
    let error = collector
        .collect(&CollectRequest::new(AgentKind::Claude).with_source(
            coding_agent_monitor_lib::collector::DataSource::Paths(vec![long]),
        ))
        .expect_err("path length cap");
    assert!(matches!(
        error,
        coding_agent_monitor_lib::collector::CollectorError::InvalidRequest { .. }
    ));
}

#[test]
fn invalid_timezone_does_not_panic() {
    let root = fixture_root("bounds-tz");
    let _env = isolate_env(&root);
    write_claude_session(
        &root,
        "session-a.jsonl",
        &[claude_line(
            "2026-01-02T00:00:00.000Z",
            "req-tz",
            "claude-sonnet-4-20250514",
            100,
            10,
            0,
            Some(0.01),
        )],
    );
    let request = CollectRequest::new(AgentKind::Claude).with_timezone(
        coding_agent_monitor_lib::collector::TimeZoneSpec("Not/A-Zone!!".to_string()),
    );
    // The vendor falls back to UTC for an unknown zone; either way this must
    // not panic and must not error.
    let result = collect(AgentKind::Claude, &request);
    assert_eq!(result.records.len(), 1);
}

#[test]
fn multibyte_and_space_paths_and_under_limit_lengths_pass() {
    let root = fixture_root("bounds-ok");
    let nested = root.join("数据 目录/claude with spaces");
    fs::create_dir_all(nested.join("projects/cam")).expect("create non-ASCII dirs");
    fs::write(
        nested.join("projects/cam/session-a.jsonl"),
        claude_line(
            "2026-01-02T00:00:00.000Z",
            "req-uni",
            "claude-sonnet-4-20250514",
            100,
            10,
            0,
            Some(0.01),
        ),
    )
    .expect("write fixture");
    let _env = isolate_env(&root);
    let request = CollectRequest::new(AgentKind::Claude).with_source(
        coding_agent_monitor_lib::collector::DataSource::Paths(vec![nested]),
    );
    let result = collect(AgentKind::Claude, &request);
    assert_eq!(result.records.len(), 1);
}

#[test]
fn diagnostics_never_leak_absolute_paths() {
    // The fixture root name carries a unique marker; if any diagnostic detail
    // or file field leaked the full path, the serialized response would
    // contain the marker.
    const MARKER: &str = "UNIQUEPATHMARKER7f3a";
    let root = fixture_root(MARKER);
    let dir = root.join("conversations");
    fs::create_dir_all(&dir).expect("create antigravity fixture dir");
    fs::write(dir.join("broken.db"), b"definitely not a sqlite database")
        .expect("write corrupt db");
    let _env = isolate_env(&root);

    let result = collect(
        AgentKind::Antigravity,
        &CollectRequest::new(AgentKind::Antigravity),
    );
    let response = coding_agent_monitor_lib::collector::protocol::CollectorResponseV1::ok(
        "req-sanitize",
        &result,
    );
    let json = serde_json::to_string(&response).expect("serialize response");
    assert!(
        !json.contains(MARKER),
        "serialized response must not contain the absolute path marker"
    );
}
