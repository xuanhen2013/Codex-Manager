use codexmanager_core::rpc::types::RequestLogTodaySummaryResult;
use codexmanager_core::storage::{RequestLogTodaySummary, Storage};

use crate::{storage_helpers::open_storage, time_bounds};

const MAX_REQUESTED_DAY_RANGE_SECS: i64 = 48 * 60 * 60;

/// 函数 `resolve_day_bounds_ts`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-13
///
/// # 参数
/// - day_start_ts: 参数 day_start_ts
/// - day_end_ts: 参数 day_end_ts
///
/// # 返回
/// 返回函数执行结果
fn resolve_day_bounds_ts(
    day_start_ts: Option<i64>,
    day_end_ts: Option<i64>,
) -> Result<(i64, i64), String> {
    time_bounds::resolve_bounded_local_day_bounds_ts(
        day_start_ts,
        day_end_ts,
        MAX_REQUESTED_DAY_RANGE_SECS,
    )
}

/// 函数 `read_requestlog_today_summary`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - day_start_ts: 参数 day_start_ts
/// - day_end_ts: 参数 day_end_ts
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn read_requestlog_today_summary(
    day_start_ts: Option<i64>,
    day_end_ts: Option<i64>,
) -> Result<RequestLogTodaySummaryResult, String> {
    let storage = open_storage().ok_or_else(|| "open storage failed".to_string())?;
    read_requestlog_today_summary_with_storage(&storage, day_start_ts, day_end_ts)
}

pub(crate) fn read_requestlog_today_summary_with_storage(
    storage: &Storage,
    day_start_ts: Option<i64>,
    day_end_ts: Option<i64>,
) -> Result<RequestLogTodaySummaryResult, String> {
    let (start_ts, end_ts) = resolve_day_bounds_ts(day_start_ts, day_end_ts)?;
    let summary = storage
        .summarize_request_logs_between(start_ts, end_ts)
        .map_err(|err| format!("summarize request logs failed: {err}"))?;
    Ok(map_today_summary(summary))
}

pub(crate) fn read_requestlog_today_summary_for_key_ids_with_storage(
    storage: &Storage,
    day_start_ts: Option<i64>,
    day_end_ts: Option<i64>,
    key_ids: &[String],
) -> Result<RequestLogTodaySummaryResult, String> {
    let (start_ts, end_ts) = resolve_day_bounds_ts(day_start_ts, day_end_ts)?;
    if key_ids.is_empty() {
        return Ok(map_today_summary(RequestLogTodaySummary {
            input_tokens: 0,
            cached_input_tokens: 0,
            output_tokens: 0,
            reasoning_output_tokens: 0,
            estimated_cost_usd: 0.0,
        }));
    }
    let summary = storage
        .summarize_request_logs_between_for_keys(start_ts, end_ts, key_ids)
        .map_err(|err| format!("summarize request logs failed: {err}"))?;
    Ok(map_today_summary(summary))
}

fn map_today_summary(summary: RequestLogTodaySummary) -> RequestLogTodaySummaryResult {
    let input_tokens = summary.input_tokens.max(0);
    let cached_input_tokens = summary.cached_input_tokens.max(0);
    let output_tokens = summary.output_tokens.max(0);
    let reasoning_output_tokens = summary.reasoning_output_tokens.max(0);
    let non_cached_input_tokens = input_tokens.saturating_sub(cached_input_tokens);
    RequestLogTodaySummaryResult {
        input_tokens,
        cached_input_tokens,
        output_tokens,
        reasoning_output_tokens,
        today_tokens: non_cached_input_tokens.saturating_add(output_tokens),
        estimated_cost: summary.estimated_cost_usd.max(0.0),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        read_requestlog_today_summary_for_key_ids_with_storage, resolve_day_bounds_ts,
        MAX_REQUESTED_DAY_RANGE_SECS,
    };
    use codexmanager_core::storage::Storage;

    #[test]
    fn resolve_day_bounds_uses_requested_range_when_complete() {
        assert_eq!(
            resolve_day_bounds_ts(Some(1_700_000_000), Some(1_700_086_400)).unwrap(),
            (1_700_000_000, 1_700_086_400)
        );
    }

    #[test]
    fn resolve_day_bounds_rejects_partial_range() {
        let error = resolve_day_bounds_ts(Some(1_700_000_000), None).unwrap_err();
        assert!(error.contains("provided together"));
    }

    #[test]
    fn resolve_day_bounds_rejects_oversized_range() {
        let error =
            resolve_day_bounds_ts(Some(0), Some(MAX_REQUESTED_DAY_RANGE_SECS + 1)).unwrap_err();
        assert!(error.contains("too large"));
    }

    #[test]
    fn today_summary_short_circuits_empty_key_ids() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");

        let summary = read_requestlog_today_summary_for_key_ids_with_storage(
            &storage,
            Some(1_700_000_000),
            Some(1_700_086_400),
            &[],
        )
        .expect("read empty member today summary");

        assert_eq!(summary.input_tokens, 0);
        assert_eq!(summary.cached_input_tokens, 0);
        assert_eq!(summary.output_tokens, 0);
        assert_eq!(summary.reasoning_output_tokens, 0);
        assert_eq!(summary.today_tokens, 0);
        assert_eq!(summary.estimated_cost, 0.0);
    }
}
