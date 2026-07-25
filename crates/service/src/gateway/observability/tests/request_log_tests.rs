use codexmanager_core::storage::{ApiKey, Storage};

fn test_api_key(id: &str) -> ApiKey {
    ApiKey {
        id: id.to_string(),
        name: Some("last used test key".to_string()),
        model_slug: Some("gpt-5.4".to_string()),
        reasoning_effort: None,
        service_tier: None,
        rotation_strategy: "account_rotation".to_string(),
        aggregate_api_id: None,
        account_plan_filter: None,
        aggregate_api_url: None,
        client_type: "codex".to_string(),
        protocol_type: "openai_compat".to_string(),
        auth_scheme: "authorization_bearer".to_string(),
        upstream_base_url: None,
        static_headers_json: None,
        key_hash: format!("hash-{id}"),
        status: "active".to_string(),
        created_at: 1,
        last_used_at: None,
    }
}

#[test]
fn successful_request_log_touches_key_and_records_v2_snapshot() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let key = test_api_key("key-last-used-success");
    storage.insert_api_key(&key).expect("insert api key");

    super::write_request_log(
        &storage,
        super::RequestLogTraceContext {
            trace_id: Some("trace-last-used-success"),
            original_path: Some("/v1/responses"),
            adapted_path: Some("/v1/responses"),
            request_type: Some("http"),
            ..Default::default()
        },
        Some(&key.id),
        None,
        "/v1/responses",
        "POST",
        Some("gpt-5.4"),
        None,
        Some("https://example.test/v1/responses"),
        Some(200),
        super::RequestLogUsage {
            input_tokens: Some(1),
            output_tokens: Some(1),
            total_tokens: Some(2),
            ..Default::default()
        },
        None,
        Some(10),
    );

    let loaded = storage
        .find_api_key_by_id(&key.id)
        .expect("load api key")
        .expect("api key exists");
    assert!(loaded.last_used_at.unwrap_or(0) > 0);
    let snapshot = storage
        .get_charge_snapshot_v2(1)
        .expect("read snapshot")
        .expect("snapshot");
    assert_eq!(snapshot.usage_source, "actual");
    assert_eq!(snapshot.input_tokens, 1);
    assert_eq!(snapshot.output_tokens, 1);
    assert_eq!(snapshot.base_cost_microusd, 18);
    let logs = storage
        .list_request_logs(None, 10)
        .expect("read request logs");
    assert_eq!(logs[0].estimated_cost_usd, Some(0.000_018));
}

#[test]
fn missing_usage_keeps_estimate_diagnostic_only() {
    let estimate = super::estimate_input_tokens_from_body(br#"{"input":"hello world"}"#);
    assert!(estimate > 0);
    assert_eq!(
        estimate,
        super::estimate_input_tokens_from_body(br#"{"input":"hello world"}"#)
    );
    let usage = super::resolve_charge_usage(super::RequestLogUsage {
        estimated_input_tokens: Some(estimate),
        ..Default::default()
    });
    assert_eq!(usage.usage_source, "unavailable");
    assert!(!usage.billable);
    assert_eq!(usage.input_tokens, 0);
    assert_eq!(usage.output_tokens, 0);
}

#[test]
fn failed_request_without_actual_usage_is_not_added_to_token_or_cost_usage() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let key = test_api_key("key-failed-estimate");
    storage.insert_api_key(&key).expect("insert api key");

    super::write_request_log(
        &storage,
        super::RequestLogTraceContext {
            trace_id: Some("trace-failed-estimate"),
            original_path: Some("/v1/responses"),
            adapted_path: Some("/v1/responses"),
            request_type: Some("http"),
            ..Default::default()
        },
        Some(&key.id),
        None,
        "/v1/responses",
        "POST",
        Some("gpt-5.4"),
        None,
        Some("https://example.test/v1/responses"),
        Some(502),
        super::RequestLogUsage {
            estimated_input_tokens: Some(250_000),
            ..Default::default()
        },
        Some("upstream server error"),
        Some(10),
    );

    assert!(storage
        .get_charge_snapshot_v2(1)
        .expect("read charge snapshot")
        .is_none());
    let logs = storage
        .list_request_logs(None, 10)
        .expect("read request logs");
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].input_tokens, None);
    assert_eq!(logs[0].total_tokens, None);
    assert_eq!(logs[0].estimated_cost_usd, None);
    let usage = storage
        .summarize_request_logs_between(0, i64::MAX)
        .expect("summarize request usage");
    assert_eq!(usage.input_tokens, 0);
    assert_eq!(usage.output_tokens, 0);
    assert_eq!(usage.estimated_cost_usd, 0.0);
}

#[test]
fn failed_request_with_actual_usage_keeps_real_usage_for_statistics() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let key = test_api_key("key-failed-actual");
    storage.insert_api_key(&key).expect("insert api key");

    super::write_request_log(
        &storage,
        super::RequestLogTraceContext {
            trace_id: Some("trace-failed-actual"),
            original_path: Some("/v1/responses"),
            adapted_path: Some("/v1/responses"),
            request_type: Some("http"),
            ..Default::default()
        },
        Some(&key.id),
        None,
        "/v1/responses",
        "POST",
        Some("gpt-5.4"),
        None,
        Some("https://example.test/v1/responses"),
        Some(502),
        super::RequestLogUsage {
            input_tokens: Some(100),
            cached_input_tokens: Some(40),
            output_tokens: Some(10),
            total_tokens: Some(110),
            ..Default::default()
        },
        Some("upstream server error after usage"),
        Some(10),
    );

    let snapshot = storage
        .get_charge_snapshot_v2(1)
        .expect("read charge snapshot")
        .expect("actual usage snapshot");
    assert_eq!(snapshot.usage_source, "actual");
    assert_eq!(snapshot.input_tokens, 100);
    assert_eq!(snapshot.cached_input_tokens, 40);
    assert_eq!(snapshot.output_tokens, 10);
    let logs = storage
        .list_request_logs(None, 10)
        .expect("read request logs");
    assert_eq!(logs[0].total_tokens, Some(110));
    assert!(logs[0].estimated_cost_usd.unwrap_or(0.0) > 0.0);
}

#[test]
fn client_cancelled_request_without_actual_usage_is_not_charged() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    super::write_request_log(
        &storage,
        super::RequestLogTraceContext {
            trace_id: Some("trace-cancelled-estimate"),
            original_path: Some("/v1/responses"),
            adapted_path: Some("/v1/responses"),
            request_type: Some("http"),
            ..Default::default()
        },
        None,
        None,
        "/v1/responses",
        "POST",
        Some("gpt-5.4"),
        None,
        Some("https://example.test/v1/responses"),
        Some(499),
        super::RequestLogUsage {
            estimated_input_tokens: Some(90_000),
            ..Default::default()
        },
        Some("client disconnected"),
        Some(10),
    );

    assert!(storage
        .get_charge_snapshot_v2(1)
        .expect("read charge snapshot")
        .is_none());
}

#[test]
fn successful_request_without_actual_usage_is_kept_as_zero_usage() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    super::write_request_log(
        &storage,
        super::RequestLogTraceContext {
            trace_id: Some("trace-success-no-usage"),
            original_path: Some("/v1/responses"),
            adapted_path: Some("/v1/responses"),
            request_type: Some("http"),
            ..Default::default()
        },
        None,
        None,
        "/v1/responses",
        "POST",
        Some("gpt-5.4"),
        None,
        Some("https://example.test/v1/responses"),
        Some(200),
        super::RequestLogUsage {
            estimated_input_tokens: Some(40_000),
            ..Default::default()
        },
        None,
        Some(10),
    );

    let logs = storage
        .list_request_logs(None, 10)
        .expect("read request logs");
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].total_tokens, None);
    assert_eq!(logs[0].estimated_cost_usd, None);
    assert!(storage
        .get_charge_snapshot_v2(1)
        .expect("read charge snapshot")
        .is_none());
}

#[test]
fn cached_tokens_larger_than_total_input_are_not_billable() {
    let usage = super::resolve_charge_usage(super::RequestLogUsage {
        input_tokens: Some(10),
        cached_input_tokens: Some(20),
        output_tokens: Some(3),
        ..Default::default()
    });
    assert_eq!(usage.usage_source, "actual");
    assert_eq!(usage.input_tokens, 10);
    assert_eq!(usage.cached_input_tokens, 10);
    assert_eq!(usage.output_tokens, 3);
    assert_eq!(usage.quality, "inconsistent");
    assert!(!usage.billable);
}

#[test]
fn independent_cache_tokens_are_canonicalized_into_total_input() {
    let usage = super::resolve_charge_usage(super::RequestLogUsage {
        input_tokens: Some(60),
        cached_input_tokens: Some(40),
        cache_creation_input_tokens: Some(20),
        output_tokens: Some(10),
        cache_tokens_are_subset: Some(false),
        ..Default::default()
    });
    assert_eq!(usage.usage_source, "actual");
    assert_eq!(usage.input_tokens, 120);
    assert_eq!(usage.cached_input_tokens, 40);
    assert_eq!(usage.cache_creation_input_tokens, 20);
    assert_eq!(usage.output_tokens, 10);
    assert_eq!(usage.total_tokens, 130);
    assert_eq!(usage.quality, "complete");
    assert!(usage.billable);
}

#[test]
fn adapter_synthesized_zero_usage_is_not_authoritative() {
    let usage = super::resolve_charge_usage(super::RequestLogUsage {
        input_tokens: Some(0),
        cached_input_tokens: Some(0),
        output_tokens: Some(0),
        total_tokens: Some(0),
        reasoning_output_tokens: Some(0),
        usage_authoritative: Some(false),
        ..Default::default()
    });
    assert_eq!(usage.usage_source, "unavailable");
    assert!(!usage.billable);
}

#[test]
fn total_only_actual_usage_is_reported_but_not_billable() {
    let usage = super::resolve_charge_usage(super::RequestLogUsage {
        total_tokens: Some(110),
        usage_authoritative: Some(true),
        ..Default::default()
    });
    assert_eq!(usage.usage_source, "actual");
    assert_eq!(usage.input_tokens, 0);
    assert_eq!(usage.output_tokens, 0);
    assert_eq!(usage.total_tokens, 110);
    assert_eq!(usage.quality, "unclassified");
    assert!(!usage.billable);
}

#[test]
fn request_log_persists_client_ultra_and_effective_max_separately() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    super::write_request_log(
        &storage,
        super::RequestLogTraceContext {
            trace_id: Some("trace-ultra-max"),
            original_path: Some("/v1/responses"),
            adapted_path: Some("/v1/responses"),
            request_type: Some("http"),
            client_model: Some("gpt-5.6-sol"),
            client_reasoning_effort: Some("ultra"),
            ..Default::default()
        },
        None,
        None,
        "/v1/responses",
        "POST",
        Some("gpt-5.6-sol"),
        Some("max"),
        None,
        Some(200),
        super::RequestLogUsage::default(),
        None,
        Some(1),
    );

    let logs = storage
        .list_request_logs(None, 10)
        .expect("read request logs");
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].client_reasoning_effort.as_deref(), Some("ultra"));
    assert_eq!(logs[0].reasoning_effort.as_deref(), Some("max"));
    assert_eq!(
        logs[0].reasoning_source.as_deref(),
        Some("client_request_normalized")
    );
}
