//! T07 isolated worker queries and public history invariants. No real sources.
mod common;
use coding_agent_monitor_lib::{
    collector::{
        protocol::{DataSourceV1, DateWindowV1},
        snapshot_protocol::{AgentSpecV1, CollectorSnapshotRequestV1},
        supervisor, worker_runner,
    },
    sidecar::adapter::normalize_history,
    usage::UsageScope,
};

fn request(root: &std::path::Path, days: i64, zone: &str) -> CollectorSnapshotRequestV1 {
    let end = common::date("2026-01-03");
    CollectorSnapshotRequestV1 {
        version: 1,
        request_id: format!("history-{days}-{zone}"),
        agents: vec![AgentSpecV1 {
            agent: "claude".into(),
            source: DataSourceV1::Paths {
                roots: vec![root.join("claude").to_string_lossy().into_owned()],
            },
        }],
        window: Some(DateWindowV1 {
            start_inclusive: (end - chrono::Duration::days(days - 1)).to_string(),
            end_inclusive: end.to_string(),
        }),
        timezone: zone.into(),
    }
}
fn normalized(req: &CollectorSnapshotRequestV1) -> coding_agent_monitor_lib::usage::HistoryUsage {
    let response = worker_runner::collect_snapshot(req).expect("worker fixture");
    let window = req.window.as_ref().unwrap();
    normalize_history(
        &response,
        &UsageScope {
            start_date: window.start_inclusive.clone(),
            end_date: window.end_inclusive.clone(),
            time_zone: req.timezone.clone(),
        },
        "2026-01-03T12:00:00Z",
    )
    .expect("normalize")
}
#[test]
fn concurrent_ranges_timezones_sources_and_unknown_cost_are_isolated() {
    supervisor::set_worker_exe_override(env!("CARGO_BIN_EXE_coding-agent-monitor").into());
    worker_runner::clear_snapshot_result_cache_for_tests();
    let root = common::fixture_root("history");
    common::write_claude_session(
        &root,
        "test.jsonl",
        &[
            common::claude_line(
                "2025-12-15T12:00:00Z",
                "old",
                "unknown-test-model",
                100,
                10,
                0,
                None,
            ),
            common::claude_line(
                "2026-01-02T23:30:00Z",
                "new",
                "claude-sonnet-4-20250514",
                200,
                20,
                0,
                Some(0.01),
            ),
        ],
    );
    let marker = root.join("spawn-marker.txt");
    let mut env = common::EnvGuard::acquire();
    env.set("CAM_TEST_WORKER_SPAWN_MARKER", &marker);
    env.set("CAM_TEST_WORKER_SLEEP_MS", std::path::Path::new("50"));
    let seven = request(&root, 7, "UTC");
    let thirty = request(&root, 30, "UTC");
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(12));
    let threads: Vec<_> = (0..12)
        .map(|i| {
            let req = if i % 2 == 0 {
                seven.clone()
            } else {
                thirty.clone()
            };
            let gate = barrier.clone();
            std::thread::spawn(move || {
                gate.wait();
                (i, normalized(&req))
            })
        })
        .collect();
    for handle in threads {
        let (i, h) = handle.join().unwrap();
        assert_eq!(h.days.len(), if i % 2 == 0 { 7 } else { 30 });
        assert_eq!(
            h.days.iter().map(|d| d.total_tokens).sum::<u64>(),
            if i % 2 == 0 { 220 } else { 330 }
        );
        assert_eq!(h.estimated_cost_usd.is_some(), i % 2 == 0);
        assert_eq!(h.days.last().unwrap().total_tokens, 0);
    }
    assert_eq!(
        std::fs::read_to_string(&marker).unwrap().lines().count(),
        2,
        "twelve callers over two identities require exactly two serial workers"
    );
    let asia = normalized(&request(&root, 30, "Asia/Shanghai"));
    assert_eq!(
        asia.days.last().unwrap().total_tokens,
        220,
        "local midnight changes bucket"
    );
    assert_eq!(asia.days.first().unwrap().date, "2025-12-05");
    let empty = common::fixture_root("history-empty");
    common::write_claude_session(&empty, "empty.jsonl", &[]);
    let zero = normalized(&request(&empty, 30, "UTC"));
    assert!(zero.days.iter().all(|d| d.total_tokens == 0));
    assert_eq!(zero.estimated_cost_usd, None);
    let json = serde_json::to_value(&asia).unwrap();
    assert_eq!(json["scope"]["timeZone"], "Asia/Shanghai");
    assert!(json.get("last7Days").is_none());
    assert!(json.get("days").unwrap().is_array());
    // Invalid protocol cannot borrow the preceding success cache.
    let mut bad = thirty.clone();
    bad.version = 99;
    assert!(worker_runner::collect_snapshot(&bad).is_err());
    assert_eq!(normalized(&thirty).days.len(), 30);
    std::fs::remove_dir_all(root).unwrap();
    std::fs::remove_dir_all(empty).unwrap();
}

#[test]
fn history_empty_calendar_boundaries_and_failure_never_fill_success() {
    use coding_agent_monitor_lib::collector::snapshot_protocol::CollectorSnapshotResponseV1;
    let empty = CollectorSnapshotResponseV1::ok("empty", vec![]);
    for (start, end, count) in [
        ("2024-02-01", "2024-03-01", 30),
        ("2025-12-28", "2026-01-03", 7),
    ] {
        let scope = UsageScope {
            start_date: start.into(),
            end_date: end.into(),
            time_zone: "UTC".into(),
        };
        let h = normalize_history(&empty, &scope, "2026-01-03T12:00:00Z").unwrap();
        assert_eq!(h.days.len(), count);
        assert_eq!(h.days.first().unwrap().date, start);
        assert_eq!(h.days.last().unwrap().date, end);
        assert!(h
            .days
            .iter()
            .all(|d| d.total_tokens == 0 && d.estimated_cost_usd.is_none()));
    }
    let bad_scope = UsageScope {
        start_date: "2026-01-03".into(),
        end_date: "2026-01-01".into(),
        time_zone: "UTC".into(),
    };
    assert!(normalize_history(&empty, &bad_scope, "now").is_err());
}

#[test]
fn history_rejects_structured_failure_and_preserves_range_diagnostics() {
    use coding_agent_monitor_lib::collector::snapshot_protocol::CollectorSnapshotResponseV1;
    let scope = UsageScope {
        start_date: "2025-12-05".into(),
        end_date: "2026-01-03".into(),
        time_zone: "UTC".into(),
    };
    let failed: CollectorSnapshotResponseV1 = serde_json::from_value(serde_json::json!({
        "version": 1, "request_id": "failed", "agents": [{"agent": "claude", "status": "error",
        "error": {"code": "source_unavailable", "message": "synthetic failure"}}]
    }))
    .unwrap();
    assert!(normalize_history(&failed, &scope, "2026-01-03T12:00:00Z").is_err());
    let skipped: CollectorSnapshotResponseV1 = serde_json::from_value(serde_json::json!({
        "version": 1, "request_id": "skips", "agents": [{"agent": "claude", "status": "ok",
        "report": {"records": [], "diagnostics": [{"kind": "corrupt_record", "file": "synthetic-private-path", "details": "synthetic private text"}]}}]
    })).unwrap();
    let h = normalize_history(&skipped, &scope, "2026-01-03T12:00:00Z").unwrap();
    assert_eq!(
        h.coverage.status,
        coding_agent_monitor_lib::usage::CoverageStatus::PossiblyIncomplete
    );
    assert_eq!(h.coverage.diagnostics[0].count, 1);
    assert!(!serde_json::to_string(&h)
        .unwrap()
        .contains("synthetic-private"));
    assert!(h.days.iter().all(|d| d.total_tokens == 0));
}
