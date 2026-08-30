//! Semantic contract tests for the typed Collector API: dedupe/aggregation,
//! date gaps, big-integer tokens, reasoning-token handling, the three cost
//! states (priced / priced-zero / missing pricing), and cross-agent isolation
//! at 0/1/2/6-agent scale.

mod common;

use coding_agent_monitor_lib::collector::{
    ccusage::AgentCollector, AgentKind, CollectRequest, CollectResult, Collector, CostNanoUsd,
};
use common::{
    claude_line, codex_line, date, fixture_root, isolate_env, model, window, write_antigravity_db,
    write_claude_session, write_codex_session, DAY_1, DAY_1_SECONDS, DAY_3,
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
    assert_eq!(
        record.total_tokens, 1_320,
        "reasoning counts toward total_tokens but has no dedicated bucket: \
         900 input + 100 cache read + 200 output + 20 reasoning"
    );
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
