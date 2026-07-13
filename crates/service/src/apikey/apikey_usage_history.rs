use std::collections::BTreeMap;

use codexmanager_core::rpc::types::{
    ApiKeyDailyUsagePoint, ApiKeyUsageHistoryResult, ApiKeyUsageHistoryUsage,
};
use codexmanager_core::storage::{DailyTokenUsageRollup, TokenUsageRollup};

use crate::storage_helpers::open_storage;
use crate::time_bounds;

const MAX_USAGE_HISTORY_DAYS: usize = 366;
const HOUR_SECONDS: i64 = 60 * 60;

pub(crate) fn read_api_key_usage_history(
    key_id: &str,
    start_ts: Option<i64>,
    end_ts: Option<i64>,
    day_boundaries_ts: Vec<i64>,
) -> Result<ApiKeyUsageHistoryResult, String> {
    let key_id = key_id.trim();
    if key_id.is_empty() {
        return Err("keyId is required".to_string());
    }
    let start_ts = start_ts.ok_or_else(|| "startTs is required".to_string())?;
    let end_ts = end_ts.ok_or_else(|| "endTs is required".to_string())?;
    let (range_start_ts, range_end_ts) = time_bounds::resolve_bounded_local_day_bounds_ts(
        Some(start_ts),
        Some(end_ts),
        (MAX_USAGE_HISTORY_DAYS as i64 + 1).saturating_mul(time_bounds::DAY_SECONDS),
    )?;
    validate_day_boundaries(range_start_ts, range_end_ts, &day_boundaries_ts)?;
    let storage = open_storage().ok_or_else(|| "open storage failed".to_string())?;
    let raw_daily_usage = storage
        .summarize_request_token_stats_daily_for_key_boundaries(key_id, &day_boundaries_ts)
        .map_err(|err| format!("summarize api key daily usage failed: {err}"))?;
    let daily_usage = fill_daily_usage(&day_boundaries_ts, raw_daily_usage);
    let usage = daily_usage.iter().fold(
        ApiKeyUsageHistoryUsage::default(),
        |mut total, point| {
            add_usage(&mut total, &point.usage);
            total
        },
    );

    Ok(ApiKeyUsageHistoryResult {
        key_id: key_id.to_string(),
        range_start_ts,
        range_end_ts,
        usage,
        daily_usage,
    })
}

fn validate_day_boundaries(
    range_start_ts: i64,
    range_end_ts: i64,
    day_boundaries_ts: &[i64],
) -> Result<(), String> {
    if day_boundaries_ts.len() < 2 {
        return Err("dayBoundariesTs is required".to_string());
    }
    if day_boundaries_ts.len() - 1 > MAX_USAGE_HISTORY_DAYS {
        return Err("requested day range is too large".to_string());
    }
    if day_boundaries_ts.first() != Some(&range_start_ts)
        || day_boundaries_ts.last() != Some(&range_end_ts)
    {
        return Err("dayBoundariesTs must match startTs and endTs".to_string());
    }
    for window in day_boundaries_ts.windows(2) {
        let start = window[0];
        let end = window[1];
        if start.rem_euclid(HOUR_SECONDS) != 0 || end.rem_euclid(HOUR_SECONDS) != 0 {
            return Err(
                "daily history requires local midnight boundaries aligned to a UTC hour"
                    .to_string(),
            );
        }
        let duration = end.saturating_sub(start);
        if !(22 * HOUR_SECONDS..=26 * HOUR_SECONDS).contains(&duration) {
            return Err("dayBoundariesTs contains an invalid natural day".to_string());
        }
    }
    Ok(())
}

fn usage_response(usage: &TokenUsageRollup) -> ApiKeyUsageHistoryUsage {
    ApiKeyUsageHistoryUsage {
        input_tokens: usage.input_tokens.max(0),
        cached_input_tokens: usage.cached_input_tokens.max(0),
        output_tokens: usage.output_tokens.max(0),
        reasoning_output_tokens: usage.reasoning_output_tokens.max(0),
        total_tokens: usage.total_tokens.max(0),
        estimated_cost_usd: usage.estimated_cost_usd.max(0.0),
        request_count: usage.request_count.max(0),
        success_count: usage.success_count.max(0),
        error_count: usage.error_count.max(0),
    }
}

fn add_usage(target: &mut ApiKeyUsageHistoryUsage, usage: &ApiKeyUsageHistoryUsage) {
    target.input_tokens = target.input_tokens.saturating_add(usage.input_tokens);
    target.cached_input_tokens = target
        .cached_input_tokens
        .saturating_add(usage.cached_input_tokens);
    target.output_tokens = target.output_tokens.saturating_add(usage.output_tokens);
    target.reasoning_output_tokens = target
        .reasoning_output_tokens
        .saturating_add(usage.reasoning_output_tokens);
    target.total_tokens = target.total_tokens.saturating_add(usage.total_tokens);
    target.estimated_cost_usd += usage.estimated_cost_usd;
    target.request_count = target.request_count.saturating_add(usage.request_count);
    target.success_count = target.success_count.saturating_add(usage.success_count);
    target.error_count = target.error_count.saturating_add(usage.error_count);
}

fn fill_daily_usage(
    day_boundaries_ts: &[i64],
    items: Vec<DailyTokenUsageRollup>,
) -> Vec<ApiKeyDailyUsagePoint> {
    let mut by_start = items
        .into_iter()
        .map(|item| (item.day_start_ts, item))
        .collect::<BTreeMap<_, _>>();
    let mut result = Vec::new();
    for window in day_boundaries_ts.windows(2) {
        let start = window[0];
        let end = window[1];
        if let Some(item) = by_start.remove(&start) {
            result.push(ApiKeyDailyUsagePoint {
                day_start_ts: start,
                day_end_ts: end,
                usage: usage_response(&item.usage),
            });
        } else {
            result.push(ApiKeyDailyUsagePoint {
                day_start_ts: start,
                day_end_ts: end,
                usage: ApiKeyUsageHistoryUsage::default(),
            });
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_variable_natural_days_and_rejects_sub_hour_boundaries() {
        assert!(validate_day_boundaries(0, 255_600, &[0, 86_400, 169_200, 255_600]).is_ok());
        assert!(validate_day_boundaries(1_800, 88_200, &[1_800, 88_200])
            .expect_err("reject half-hour boundary")
            .contains("aligned to a UTC hour"));
    }

    #[test]
    fn fills_missing_days_and_sums_daily_usage() {
        let daily_usage = fill_daily_usage(
            &[0, time_bounds::DAY_SECONDS, 169_200, 255_600],
            vec![DailyTokenUsageRollup {
                day_start_ts: time_bounds::DAY_SECONDS,
                day_end_ts: 169_200,
                usage: TokenUsageRollup {
                    input_tokens: 100,
                    total_tokens: 120,
                    estimated_cost_usd: 0.12,
                    request_count: 1,
                    ..TokenUsageRollup::default()
                },
            }],
        );

        assert_eq!(daily_usage.len(), 3);
        assert_eq!(daily_usage[0].usage.total_tokens, 0);
        assert_eq!(daily_usage[1].usage.total_tokens, 120);
        assert_eq!(daily_usage[2].usage.total_tokens, 0);
        let total = daily_usage.iter().fold(
            ApiKeyUsageHistoryUsage::default(),
            |mut total, point| {
                add_usage(&mut total, &point.usage);
                total
            },
        );
        assert_eq!(total.input_tokens, 100);
        assert_eq!(total.total_tokens, 120);
        assert_eq!(total.request_count, 1);
        assert!((total.estimated_cost_usd - 0.12).abs() < f64::EPSILON);
    }
}
