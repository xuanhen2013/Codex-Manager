use super::{rebase_http_body_for_account_change, remove_account_affinity_fields};

#[test]
fn account_rebase_removes_root_and_client_metadata_affinity() {
    let body = br#"{
        "model":"gpt-5.6",
        "previous_response_id":"resp_old",
        "session_id":"session_old",
        "client_metadata":{
            "x-codex-installation-id":"install_keep",
            "thread_id":"thread_old",
            "x-codex-window-id":"thread_old:0",
            "x-codex-turn-metadata":"{\"thread_id\":\"thread_old\"}",
            "sandbox_mode":"workspace-write"
        }
    }"#;

    let rebased = rebase_http_body_for_account_change(body).expect("body should change");
    let value: serde_json::Value = serde_json::from_slice(&rebased).expect("parse rebased body");

    assert!(value.get("previous_response_id").is_none());
    assert!(value.get("session_id").is_none());
    assert_eq!(
        value.pointer("/client_metadata/x-codex-installation-id"),
        Some(&serde_json::Value::String("install_keep".to_string()))
    );
    assert_eq!(
        value.pointer("/client_metadata/sandbox_mode"),
        Some(&serde_json::Value::String("workspace-write".to_string()))
    );
    assert!(value.pointer("/client_metadata/thread_id").is_none());
    assert!(value
        .pointer("/client_metadata/x-codex-turn-metadata")
        .is_none());
}

#[test]
fn account_rebase_preserves_unchanged_payload() {
    let body = br#"{"model":"gpt-5.6","input":"hello"}"#;
    assert!(rebase_http_body_for_account_change(body).is_none());
}

#[test]
fn shared_affinity_match_is_case_insensitive() {
    let mut object = serde_json::json!({
        "X-Codex-Parent-Thread-ID":"parent",
        "tool_namespaces_info":"keep"
    })
    .as_object()
    .expect("object")
    .clone();
    assert!(remove_account_affinity_fields(&mut object));
    assert!(!object.contains_key("X-Codex-Parent-Thread-ID"));
    assert!(object.contains_key("tool_namespaces_info"));
}
