use codexmanager_core::rpc::types::UsageSnapshotResult;
use std::collections::HashMap;

use crate::storage_helpers::open_storage;
use crate::usage_read::usage_snapshot_result_from_record;

pub(crate) fn read_usage_snapshots_limited(
    limit: Option<usize>,
) -> Result<Vec<UsageSnapshotResult>, String> {
    if limit == Some(0) {
        return Ok(Vec::new());
    }
    // 读取所有账号最新用量
    let storage = open_storage().ok_or_else(|| "open storage failed".to_string())?;
    let items = storage
        .latest_usage_snapshots_by_account_limited(limit)
        .map_err(|err| format!("list usage snapshots failed: {err}"))?;
    let secondary_window_usage = match super::secondary_window_summary::summaries_for_snapshots(
        &storage,
        &items,
    ) {
        Ok(items) => items,
        Err(err) => {
            log::warn!("list secondary-window usage summaries failed: {err}");
            HashMap::new()
        }
    };

    Ok(items
        .into_iter()
        .map(|snapshot| {
            let account_id = snapshot.account_id.clone();
            let mut result = usage_snapshot_result_from_record(snapshot);
            result.secondary_window_usage = secondary_window_usage.get(&account_id).cloned();
            result
        })
        .collect())
}
