//! Contract tests for the versioned V1 collector transport protocol:
//! lossless round-trips (string-encoded u64/i128), version gating, unknown
//! field policy, and structured error codes. No I/O, no worker, no env.

use coding_agent_monitor_lib::collector::{
    protocol::{
        record_from_v1, CollectorRequestV1, CollectorResponseV1, ErrorCodeV1, OutcomeV1,
        PROTOCOL_VERSION,
    },
    AgentKind, CollectRequest, CollectResult, CollectorError, CostNanoUsd, ModelName, UsageRecord,
};

fn sample_record(input: u64, cost: Option<i128>) -> UsageRecord {
    UsageRecord::from_parts(
        chrono::NaiveDate::from_ymd_opt(2099, 1, 2).expect("date"),
        AgentKind::Claude,
        input,
        50,
        0,
        10,
        0,
        0,
        input + 60,
        cost.map(CostNanoUsd::from_nano),
        vec![ModelName("claude-sonnet-4-20250514".to_string())],
        vec![],
        vec![],
    )
}

#[test]
fn request_round_trips_into_domain() {
    let request = CollectorRequestV1::new("req-1", AgentKind::Antigravity);
    let json = serde_json::to_string(&request).expect("serialize request");
    let parsed: CollectorRequestV1 = serde_json::from_str(&json).expect("deserialize request");
    assert_eq!(parsed, request);

    let domain = parsed.into_domain().expect("domain request");
    assert_eq!(domain.agent, AgentKind::Antigravity);
    assert_eq!(domain.window, None);
    assert_eq!(domain.timezone.0, "UTC");
}

#[test]
fn request_with_paths_source_maps_explicit_roots() {
    let json = format!(
        r#"{{"version":{PROTOCOL_VERSION},"request_id":"req-2","agent":"claude","timezone":"UTC","source":{{"kind":"paths","roots":["C:\\data\\claude root\\中文"]}}}}"#
    );
    let parsed: CollectorRequestV1 = serde_json::from_str(&json).expect("deserialize");
    let domain = parsed.into_domain().expect("domain");
    match domain.source {
        coding_agent_monitor_lib::collector::DataSource::Paths(roots) => {
            assert_eq!(roots.len(), 1);
            assert!(roots[0].to_string_lossy().contains("中文"));
        }
        other => panic!("expected explicit paths, got {other:?}"),
    }
}

#[test]
fn response_round_trips_large_integers_losslessly() {
    // Far beyond JavaScript's 2^53 safe-integer range.
    let huge_input = 9_223_372_036_854_775_807u64;
    let huge_cost = 1_234_567_891_234_567_891_234i128; // > 1e12 USD in nano
    let result = CollectResult::from_parts(
        AgentKind::Claude,
        vec![sample_record(huge_input, Some(huge_cost))],
        vec![],
    );

    let response = CollectorResponseV1::ok("req-big", &result);
    let json = serde_json::to_string(&response).expect("serialize response");
    // Values must be JSON *strings* on the wire, not narrowed numbers.
    assert!(
        json.contains(&huge_input.to_string()),
        "token value must appear verbatim: {json}"
    );
    assert!(
        json.contains(&huge_cost.to_string()),
        "nano-USD value must appear verbatim: {json}"
    );

    let parsed: CollectorResponseV1 = serde_json::from_str(&json).expect("deserialize response");
    assert_eq!(parsed, response, "wire round-trip must be lossless");

    // And wire -> domain must equal the original domain record.
    match &parsed.outcome {
        OutcomeV1::Ok { report } => {
            let rebuilt = record_from_v1(&report.records[0]).expect("rebuild domain record");
            assert_eq!(rebuilt, result.records[0]);
        }
        other => panic!("expected ok outcome, got {other:?}"),
    }
}

#[test]
fn error_response_carries_structured_code_and_clean_message() {
    let error = CollectorError::SourceUnavailable {
        agent: AgentKind::Codex,
        details: "codex data root missing".to_string(),
    };
    let response = CollectorResponseV1::error("req-err", &error);
    let json = serde_json::to_string(&response).expect("serialize");

    let parsed: CollectorResponseV1 = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(parsed.request_id, "req-err");
    match &parsed.outcome {
        OutcomeV1::Error { error } => {
            assert_eq!(error.code, ErrorCodeV1::SourceUnavailable);
            assert!(!error.message.contains("SourceUnavailable {"));
            assert!(!error.message.contains("Some("));
        }
        other => panic!("expected error outcome, got {other:?}"),
    }
    // `as_error` restores the real agent attribution from the structured
    // field; the detail text must be contained in the display message.
    let rebuilt = parsed.as_error().expect("error outcome");
    assert!(matches!(
        rebuilt,
        CollectorError::SourceUnavailable { agent: AgentKind::Codex, ref details }
            if details.contains("codex data root missing")
    ));
}

#[test]
fn unknown_fields_are_ignored_within_a_version() {
    let json = format!(
        r#"{{"version":{PROTOCOL_VERSION},"request_id":"req-3","agent":"codex","timezone":"UTC","source":{{"kind":"environment","future_hint":"ignore me"}},"extra_top_level":42}}"#
    );
    let parsed: CollectorRequestV1 =
        serde_json::from_str(&json).expect("unknown fields must be ignored within a version");
    let domain = parsed.into_domain().expect("domain");
    assert_eq!(domain.agent, AgentKind::Codex);
}

#[test]
fn wrong_protocol_version_is_rejected() {
    let json = r#"{"version":2,"request_id":"req-4","agent":"codex","timezone":"UTC","source":{"kind":"environment"}}"#;
    let parsed: CollectorRequestV1 = serde_json::from_str(json).expect("deserialize");
    let error = parsed.into_domain().expect_err("version must be gated");
    assert!(matches!(error, CollectorError::InvalidRequest { .. }));
}

#[test]
fn unknown_agent_id_is_a_typed_invalid_request() {
    let json = format!(
        r#"{{"version":{PROTOCOL_VERSION},"request_id":"req-5","agent":"not-an-agent","timezone":"UTC","source":{{"kind":"environment"}}}}"#
    );
    let parsed: CollectorRequestV1 = serde_json::from_str(&json).expect("deserialize");
    let error = parsed.into_domain().expect_err("unknown agent");
    assert!(matches!(error, CollectorError::InvalidRequest { .. }));
}

#[test]
fn malformed_window_dates_are_typed_rejections() {
    let json = format!(
        r#"{{"version":{PROTOCOL_VERSION},"request_id":"req-6","agent":"codex","window":{{"start_inclusive":"2099-13-99","end_inclusive":"2099-01-01"}},"timezone":"UTC","source":{{"kind":"environment"}}}}"#
    );
    let parsed: CollectorRequestV1 = serde_json::from_str(&json).expect("deserialize");
    let error = parsed.into_domain().expect_err("bad date");
    assert!(matches!(error, CollectorError::InvalidRequest { .. }));
}

#[test]
fn domain_request_maps_to_v1_request() {
    let domain = CollectRequest::new(AgentKind::Goose);
    let wire = CollectorRequestV1 {
        version: PROTOCOL_VERSION,
        request_id: "req-7".to_string(),
        agent: domain.agent.id().to_string(),
        window: None,
        timezone: domain.timezone.0.clone(),
        source: coding_agent_monitor_lib::collector::protocol::DataSourceV1::Environment,
    };
    assert_eq!(wire.agent, "goose");
    let rebuilt = wire.clone().into_domain().expect("domain");
    assert_eq!(rebuilt, domain);
}

#[test]
fn error_responses_keep_real_agent_attribution() {
    for (agent, code) in [
        (AgentKind::Codex, ErrorCodeV1::SourceUnavailable),
        (AgentKind::Antigravity, ErrorCodeV1::CorruptData),
        (AgentKind::Goose, ErrorCodeV1::DatabaseQuery),
    ] {
        let error = match code {
            ErrorCodeV1::SourceUnavailable => CollectorError::SourceUnavailable {
                agent,
                details: format!("{} data root missing", agent.label()),
            },
            ErrorCodeV1::CorruptData => CollectorError::CorruptData {
                agent,
                details: format!("{} data corrupt", agent.label()),
            },
            _ => CollectorError::DatabaseQuery {
                agent,
                details: format!("{} database failed", agent.label()),
            },
        };
        let response = CollectorResponseV1::error("req-agent", &error);
        let json = serde_json::to_string(&response).expect("serialize");
        let parsed: CollectorResponseV1 = serde_json::from_str(&json).expect("deserialize");
        parsed.validate().expect("attribution survives the wire");
        match parsed.as_error().expect("error outcome") {
            CollectorError::SourceUnavailable { agent: got, .. }
            | CollectorError::CorruptData { agent: got, .. }
            | CollectorError::DatabaseQuery { agent: got, .. } => assert_eq!(got, agent),
            other => panic!("wrong classification: {other:?}"),
        }
    }
}

#[test]
fn error_response_carries_vendor_label() {
    let error = CollectorError::VendorAdapter {
        vendor: "ccusage v20.0.20".to_string(),
        details: "engine failed".to_string(),
    };
    let response = CollectorResponseV1::error("req-vendor", &error);
    let json = serde_json::to_string(&response).expect("serialize");
    assert!(json.contains("ccusage v20.0.20"));
    let parsed: CollectorResponseV1 = serde_json::from_str(&json).expect("deserialize");
    match parsed.as_error().expect("error") {
        CollectorError::VendorAdapter { vendor, .. } => {
            assert_eq!(vendor, "ccusage v20.0.20")
        }
        other => panic!("wrong classification: {other:?}"),
    }
}

#[test]
fn agent_classified_error_without_attribution_degrades_to_protocol() {
    let json = format!(
        r#"{{"version":{PROTOCOL_VERSION},"request_id":"req-lost","status":"error","error":{{"code":"source_unavailable","message":"root missing"}}}}"#
    );
    let parsed: CollectorResponseV1 = serde_json::from_str(&json).expect("deserialize");
    parsed.validate().expect("structurally valid");
    match parsed.as_error().expect("error") {
        CollectorError::Protocol { .. } => {}
        other => panic!("misattributed error must degrade to Protocol, got {other:?}"),
    }
}

#[test]
fn response_version_mismatch_is_rejected() {
    let json = r#"{"version":99,"request_id":"req-v","status":"ok","report":{"records":[],"diagnostics":[]}}"#;
    let error = CollectorResponseV1::from_wire(json).expect_err("version gate");
    assert!(matches!(error, CollectorError::Protocol { .. }));
}

#[test]
fn malformed_response_maps_to_protocol_error() {
    let error = CollectorResponseV1::from_wire("not json at all").expect_err("malformed");
    assert!(matches!(error, CollectorError::Protocol { .. }));
    let error = CollectorResponseV1::from_wire(
        r#"{"version":1,"request_id":"","status":"ok","report":{"records":[],"diagnostics":[]}}"#,
    )
    .expect_err("empty request id");
    assert!(matches!(error, CollectorError::Protocol { .. }));
    let error = CollectorResponseV1::from_wire(
        r#"{"version":1,"request_id":"req","status":"ok","report":{"records":[{"date":"2099-13-99","agent":"codex","input_tokens":"1","output_tokens":"0","cache_creation_tokens":"0","cache_read_tokens":"0","reasoning_tokens":"0","unclassified_tokens":"0","total_tokens":"1","cost_nano_usd":null,"models_used":[],"model_breakdowns":[],"models_missing_pricing":[]}],"diagnostics":[]"#,
    )
    .expect_err("bad date + truncated");
    assert!(matches!(error, CollectorError::Protocol { .. }));
}

#[test]
fn invariant_violation_diagnostic_passes_validation() {
    // A record that violates the invariant is valid on the wire as long as
    // the violation is flagged — the receiver must not re-derive it.
    let json = format!(
        r#"{{"version":{PROTOCOL_VERSION},"request_id":"req-inv","status":"ok","report":{{"records":[{{"date":"2099-01-02","agent":"codex","input_tokens":"1000","output_tokens":"200","cache_creation_tokens":"0","cache_read_tokens":"100","reasoning_tokens":"20","unclassified_tokens":"0","total_tokens":"150","cost_nano_usd":null,"models_used":[],"model_breakdowns":[],"models_missing_pricing":[]}}],"diagnostics":[{{"kind":"invariant_violation","file":null,"details":"buckets exceed total"}}]}}}}"#
    );
    let parsed = CollectorResponseV1::from_wire(&json).expect("flagged violation is valid");
    assert!(parsed.as_error().is_none());
}
