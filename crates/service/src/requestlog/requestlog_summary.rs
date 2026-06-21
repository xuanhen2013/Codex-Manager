use codexmanager_core::rpc::types::{RequestLogFilterSummaryResult, RequestLogListParams};
use codexmanager_core::storage::{RequestLogQuerySummary, Storage};

use crate::storage_helpers::open_storage;

use super::list::NormalizedRequestLogParams;

/// 函数 `read_request_log_filter_summary`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn read_request_log_filter_summary(
    params: RequestLogListParams,
) -> Result<RequestLogFilterSummaryResult, String> {
    let storage = open_storage().ok_or_else(|| "open storage failed".to_string())?;
    read_request_log_filter_summary_with_storage(&storage, params)
}

pub(crate) fn read_request_log_filter_summary_with_storage(
    storage: &Storage,
    params: RequestLogListParams,
) -> Result<RequestLogFilterSummaryResult, String> {
    let params = NormalizedRequestLogParams::from_params(params);
    let total_count = storage
        .count_request_logs(
            params.query.as_deref(),
            None,
            params.start_ts,
            params.end_ts,
        )
        .map_err(|err| format!("count request logs failed: {err}"))?;
    let filtered = storage
        .summarize_request_logs_filtered(
            params.query.as_deref(),
            params.status_filter.as_deref(),
            params.start_ts,
            params.end_ts,
        )
        .map_err(|err| format!("summarize request logs failed: {err}"))?;

    Ok(map_filter_summary(total_count, filtered))
}

pub(crate) fn read_request_log_filter_summary_for_key_ids_with_storage(
    storage: &Storage,
    params: RequestLogListParams,
    key_ids: &[String],
) -> Result<RequestLogFilterSummaryResult, String> {
    let params = NormalizedRequestLogParams::from_params(params);
    if key_ids.is_empty() {
        return Ok(RequestLogFilterSummaryResult::default());
    }
    let total_count = storage
        .count_request_logs_for_keys(
            params.query.as_deref(),
            None,
            params.start_ts,
            params.end_ts,
            key_ids,
        )
        .map_err(|err| format!("count request logs failed: {err}"))?;
    let filtered = storage
        .summarize_request_logs_filtered_for_keys(
            params.query.as_deref(),
            params.status_filter.as_deref(),
            params.start_ts,
            params.end_ts,
            key_ids,
        )
        .map_err(|err| format!("summarize request logs failed: {err}"))?;

    Ok(map_filter_summary(total_count, filtered))
}

fn map_filter_summary(
    total_count: i64,
    filtered: RequestLogQuerySummary,
) -> RequestLogFilterSummaryResult {
    RequestLogFilterSummaryResult {
        total_count: total_count.max(0),
        filtered_count: filtered.count.max(0),
        success_count: filtered.success_count.max(0),
        error_count: filtered.error_count.max(0),
        total_tokens: filtered.total_tokens.max(0),
        total_cost_usd: filtered.estimated_cost_usd.max(0.0),
    }
}

#[cfg(test)]
mod tests {
    use super::{map_filter_summary, read_request_log_filter_summary_for_key_ids_with_storage};
    use codexmanager_core::rpc::types::RequestLogListParams;
    use codexmanager_core::storage::{RequestLogQuerySummary, Storage};

    #[test]
    fn member_filter_summary_short_circuits_empty_key_ids() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");

        let summary = read_request_log_filter_summary_for_key_ids_with_storage(
            &storage,
            RequestLogListParams {
                query: Some("trace:=ignored".to_string()),
                status_filter: Some("5xx".to_string()),
                start_ts: Some(1_700_000_000),
                end_ts: Some(1_700_086_400),
                ..RequestLogListParams::default()
            },
            &[],
        )
        .expect("read empty member request log summary");

        assert_eq!(summary.total_count, 0);
        assert_eq!(summary.filtered_count, 0);
        assert_eq!(summary.success_count, 0);
        assert_eq!(summary.error_count, 0);
        assert_eq!(summary.total_tokens, 0);
        assert_eq!(summary.total_cost_usd, 0.0);
    }

    #[test]
    fn filter_summary_mapping_clamps_negative_aggregate_values() {
        let summary = map_filter_summary(
            -1,
            RequestLogQuerySummary {
                count: -2,
                success_count: -3,
                error_count: -4,
                total_tokens: -5,
                estimated_cost_usd: -0.25,
            },
        );

        assert_eq!(summary.total_count, 0);
        assert_eq!(summary.filtered_count, 0);
        assert_eq!(summary.success_count, 0);
        assert_eq!(summary.error_count, 0);
        assert_eq!(summary.total_tokens, 0);
        assert_eq!(summary.total_cost_usd, 0.0);
    }
}
