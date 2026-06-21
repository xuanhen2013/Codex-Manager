use codexmanager_core::{rpc::types::ApiKeyUsageStatSummary, storage::ApiKeyTokenUsageSummary};

use crate::storage_helpers::open_storage;
use crate::RpcActor;

pub(crate) fn read_api_key_usage_stats_for_actor(
    actor: &RpcActor,
) -> Result<Vec<ApiKeyUsageStatSummary>, String> {
    let storage = open_storage().ok_or_else(|| "open storage failed".to_string())?;
    if actor.is_admin() {
        let items = storage
            .summarize_request_token_stats_by_key()
            .map_err(|err| format!("summarize api key token stats failed: {err}"))?;
        return Ok(map_api_key_usage_stats(items));
    }
    let user_id = actor
        .user_id
        .as_deref()
        .ok_or_else(|| "permission_denied: apikey usage requires user session".to_string())?;
    let items = storage
        .summarize_request_token_stats_by_key_for_user(user_id)
        .map_err(|err| format!("summarize api key token stats failed: {err}"))?;

    Ok(map_api_key_usage_stats(items))
}

fn map_api_key_usage_stats(items: Vec<ApiKeyTokenUsageSummary>) -> Vec<ApiKeyUsageStatSummary> {
    items
        .into_iter()
        .map(|item| ApiKeyUsageStatSummary {
            key_id: item.key_id,
            total_tokens: item.total_tokens.max(0),
            estimated_cost_usd: item.estimated_cost_usd.max(0.0),
        })
        .collect()
}
