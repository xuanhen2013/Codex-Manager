use serde_json::{Map, Value};

const ACCOUNT_AFFINITY_KEYS: &[&str] = &[
    "session-id",
    "session_id",
    "conversation-id",
    "conversation_id",
    "thread-id",
    "thread_id",
    "client-request-id",
    "client_request_id",
    "window-id",
    "window_id",
    "turn-id",
    "turn_id",
    "turn-state",
    "turn_state",
    "parent-thread-id",
    "parent_thread_id",
    "turn-metadata",
    "turn_metadata",
    "x-client-request-id",
    "x-codex-conversation-id",
    "x-codex-window-id",
    "x-codex-turn-state",
    "x-codex-parent-thread-id",
    "x-codex-turn-metadata",
];

pub(crate) fn remove_account_affinity_fields(object: &mut Map<String, Value>) -> bool {
    let keys = object
        .keys()
        .filter(|key| {
            ACCOUNT_AFFINITY_KEYS
                .iter()
                .any(|candidate| key.eq_ignore_ascii_case(candidate))
        })
        .cloned()
        .collect::<Vec<_>>();
    let changed = !keys.is_empty();
    for key in keys {
        object.remove(&key);
    }
    changed
}

pub(crate) fn rebase_http_body_for_account_change(body: &[u8]) -> Option<Vec<u8>> {
    let mut value = serde_json::from_slice::<Value>(body).ok()?;
    let object = value.as_object_mut()?;
    let mut changed = object.remove("previous_response_id").is_some();
    changed |= remove_account_affinity_fields(object);

    let mut remove_empty_client_metadata = false;
    if let Some(client_metadata) = object.get_mut("client_metadata") {
        if let Some(client_metadata) = client_metadata.as_object_mut() {
            changed |= remove_account_affinity_fields(client_metadata);
            remove_empty_client_metadata = client_metadata.is_empty();
        }
    }
    if remove_empty_client_metadata {
        object.remove("client_metadata");
        changed = true;
    }

    changed.then(|| serde_json::to_vec(&value).ok()).flatten()
}

#[cfg(test)]
#[path = "codex_identity_tests.rs"]
mod tests;
