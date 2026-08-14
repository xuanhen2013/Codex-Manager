use std::collections::HashMap;

use codexmanager_core::{
    rpc::types::UsageWindowUsageSummaryResult,
    storage::{now_ts, Storage, UsageSnapshotRecord},
};

const SECONDS_PER_MINUTE: i64 = 60;
const MINUTES_PER_DAY: i64 = 24 * 60;
const LONG_WINDOW_ROUNDING_BIAS_MINUTES: i64 = 5;

#[derive(Debug, Clone, Copy)]
struct UsageWindowBounds {
    start_at: i64,
    resets_at: i64,
    used_percent: Option<f64>,
}

#[derive(Debug, Clone, Copy, Default)]
struct ObservedCapacity {
    total_tokens: Option<i64>,
    cost_usd: Option<f64>,
}

fn current_secondary_window_bounds(
    snapshot: &UsageSnapshotRecord,
    now: i64,
) -> Option<UsageWindowBounds> {
    let secondary = snapshot
        .secondary_window_minutes
        .zip(snapshot.secondary_resets_at)
        .map(|(minutes, resets_at)| (minutes, resets_at, snapshot.secondary_used_percent))
        .filter(|(minutes, _, _)| *minutes > 0);
    let primary_is_long = snapshot
        .window_minutes
        .is_some_and(|minutes| minutes > MINUTES_PER_DAY + LONG_WINDOW_ROUNDING_BIAS_MINUTES);
    let primary = primary_is_long
        .then_some(())
        .and_then(|_| snapshot.window_minutes.zip(snapshot.resets_at))
        .map(|(minutes, resets_at)| (minutes, resets_at, snapshot.used_percent))
        .filter(|(minutes, _, _)| *minutes > 0);
    let (window_minutes, resets_at, used_percent) = secondary.or(primary)?;
    if resets_at <= now {
        return None;
    }
    let window_seconds = window_minutes.checked_mul(SECONDS_PER_MINUTE)?;
    let start_at = resets_at.checked_sub(window_seconds)?;
    (start_at <= now).then_some(UsageWindowBounds {
        start_at,
        resets_at,
        used_percent,
    })
}

fn observed_capacity(
    total_tokens: i64,
    cost_usd: f64,
    used_percent: f64,
) -> Option<ObservedCapacity> {
    if !used_percent.is_finite() || !(1.0..=100.0).contains(&used_percent) {
        return None;
    }
    let fraction = used_percent / 100.0;
    let total_tokens = (total_tokens > 0)
        .then(|| ((total_tokens as f64 / fraction).round()).clamp(0.0, i64::MAX as f64) as i64);
    let cost_usd = (cost_usd.is_finite() && cost_usd > 0.0).then(|| cost_usd / fraction);
    (total_tokens.is_some() || cost_usd.is_some()).then_some(ObservedCapacity {
        total_tokens,
        cost_usd,
    })
}

fn capacity_change_ratio(current: ObservedCapacity, previous: ObservedCapacity) -> Option<f64> {
    if let (Some(current), Some(previous)) = (current.cost_usd, previous.cost_usd) {
        return (previous > 0.0).then_some((current - previous) / previous);
    }
    let (current, previous) = (current.total_tokens?, previous.total_tokens?);
    (previous > 0).then_some((current as f64 - previous as f64) / previous as f64)
}

fn capacity_trend(change_ratio: f64) -> &'static str {
    if change_ratio >= 0.15 {
        "up"
    } else if change_ratio <= -0.15 {
        "down"
    } else {
        "stable"
    }
}

pub(crate) fn summaries_for_snapshots(
    storage: &Storage,
    snapshots: &[UsageSnapshotRecord],
) -> Result<HashMap<String, UsageWindowUsageSummaryResult>, String> {
    summaries_for_snapshots_at(storage, snapshots, now_ts())
}

fn summaries_for_snapshots_at(
    storage: &Storage,
    snapshots: &[UsageSnapshotRecord],
    now: i64,
) -> Result<HashMap<String, UsageWindowUsageSummaryResult>, String> {
    let mut results = HashMap::new();
    for snapshot in snapshots {
        let account_id = snapshot.account_id.trim();
        let Some(bounds) = current_secondary_window_bounds(snapshot, now) else {
            continue;
        };
        if account_id.is_empty() {
            continue;
        }
        let usage = storage
            .summarize_request_token_stats_for_source_between(
                "openai_account",
                account_id,
                bounds.start_at,
                now,
            )
            .map_err(|err| {
                format!("summarize {account_id} secondary-window usage failed: {err}")
            })?;
        let current_observation = bounds.used_percent.and_then(|used_percent| {
            observed_capacity(usage.total_tokens, usage.estimated_cost_usd, used_percent)
        });
        let previous_snapshot = storage
            .recent_usage_snapshots_for_account(account_id, 8)
            .map_err(|err| format!("load {account_id} usage observation history failed: {err}"))?
            .into_iter()
            .filter(|item| item.captured_at < snapshot.captured_at)
            .find(|item| {
                current_secondary_window_bounds(item, item.captured_at).is_some_and(|previous| {
                    previous.start_at == bounds.start_at && previous.resets_at == bounds.resets_at
                })
            });
        let previous_observation = if let Some(previous_snapshot) = previous_snapshot {
            let previous_bounds =
                current_secondary_window_bounds(&previous_snapshot, previous_snapshot.captured_at);
            if let Some(previous_bounds) = previous_bounds {
                let previous_usage = storage
                    .summarize_request_token_stats_for_source_between(
                        "openai_account",
                        account_id,
                        previous_bounds.start_at,
                        previous_snapshot.captured_at,
                    )
                    .map_err(|err| {
                        format!("summarize {account_id} previous observation failed: {err}")
                    })?;
                previous_bounds.used_percent.and_then(|used_percent| {
                    observed_capacity(
                        previous_usage.total_tokens,
                        previous_usage.estimated_cost_usd,
                        used_percent,
                    )
                })
            } else {
                None
            }
        } else {
            None
        };
        let change_ratio = current_observation
            .zip(previous_observation)
            .and_then(|(current, previous)| capacity_change_ratio(current, previous));
        let snapshot_is_fresh = now.saturating_sub(snapshot.captured_at) <= 30 * 60;
        let confidence = current_observation.map(|_| {
            if snapshot_is_fresh
                && bounds
                    .used_percent
                    .is_some_and(|used_percent| used_percent >= 10.0)
                && previous_observation.is_some()
            {
                "medium"
            } else {
                "low"
            }
        });
        results.insert(
            account_id.to_string(),
            UsageWindowUsageSummaryResult {
                window_start_at: bounds.start_at,
                resets_at: bounds.resets_at,
                total_tokens: usage.total_tokens,
                estimated_cost_usd: usage.estimated_cost_usd,
                observed_token_capacity: current_observation.and_then(|item| item.total_tokens),
                observed_cost_capacity_usd: current_observation.and_then(|item| item.cost_usd),
                previous_observed_token_capacity: previous_observation
                    .and_then(|item| item.total_tokens),
                previous_observed_cost_capacity_usd: previous_observation
                    .and_then(|item| item.cost_usd),
                observed_capacity_change_ratio: change_ratio,
                observed_capacity_trend: change_ratio.map(capacity_trend).map(str::to_string),
                observed_capacity_confidence: confidence.map(str::to_string),
                observed_at: current_observation.map(|_| snapshot.captured_at),
            },
        );
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use codexmanager_core::storage::RequestTokenStat;

    const WEEK_SECONDS: i64 = 7 * 24 * 60 * 60;

    fn snapshot(account_id: &str, resets_at: i64) -> UsageSnapshotRecord {
        UsageSnapshotRecord {
            account_id: account_id.to_string(),
            used_percent: Some(10.0),
            window_minutes: Some(300),
            resets_at: Some(resets_at - WEEK_SECONDS + 300 * SECONDS_PER_MINUTE),
            secondary_used_percent: Some(10.0),
            secondary_window_minutes: Some(WEEK_SECONDS / SECONDS_PER_MINUTE),
            secondary_resets_at: Some(resets_at),
            credits_json: None,
            captured_at: resets_at - WEEK_SECONDS,
        }
    }

    fn insert_usage(
        storage: &Storage,
        request_log_id: i64,
        account_id: &str,
        tokens: i64,
        cost: f64,
        created_at: i64,
    ) {
        storage
            .insert_request_token_stat(&RequestTokenStat {
                request_log_id,
                key_id: Some("key-test".to_string()),
                account_id: Some(account_id.to_string()),
                model: Some("gpt-5".to_string()),
                actual_source_kind: Some("openai_account".to_string()),
                actual_source_id: Some(account_id.to_string()),
                input_tokens: Some(tokens),
                cached_input_tokens: Some(0),
                output_tokens: Some(0),
                total_tokens: Some(tokens),
                reasoning_output_tokens: Some(0),
                estimated_cost_usd: Some(cost),
                created_at,
            })
            .expect("insert token stat");
    }

    #[test]
    fn secondary_window_summary_excludes_usage_before_an_early_reset() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        let now = 2_000_000;
        let reset_at = now + WEEK_SECONDS - 120;
        let item = snapshot("account-early-reset", reset_at);

        insert_usage(
            &storage,
            1,
            "account-early-reset",
            1_000_000,
            2.5,
            now - 180,
        );
        insert_usage(
            &storage,
            2,
            "account-early-reset",
            8_600_000,
            12.34,
            now - 60,
        );

        let summaries =
            summaries_for_snapshots_at(&storage, &[item], now).expect("summarize current window");
        let summary = summaries
            .get("account-early-reset")
            .expect("summary for current window");

        assert_eq!(summary.window_start_at, now - 120);
        assert_eq!(summary.resets_at, reset_at);
        assert_eq!(summary.total_tokens, 8_600_000);
        assert!((summary.estimated_cost_usd - 12.34).abs() < f64::EPSILON);
    }

    #[test]
    fn long_primary_window_is_treated_as_the_weekly_window() {
        let now = 2_000_000;
        let reset_at = now + WEEK_SECONDS;
        let mut item = snapshot("account-free", reset_at);
        item.window_minutes = Some(WEEK_SECONDS / SECONDS_PER_MINUTE);
        item.resets_at = Some(reset_at);
        item.secondary_used_percent = None;
        item.secondary_window_minutes = None;
        item.secondary_resets_at = None;

        let bounds = current_secondary_window_bounds(&item, now).expect("long primary window");
        assert_eq!(bounds.start_at, now);
        assert_eq!(bounds.resets_at, reset_at);
    }

    #[test]
    fn secondary_window_summary_survives_missing_upstream_percent() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        let now = 2_500_000;
        let reset_at = now + WEEK_SECONDS - 120;
        let mut item = snapshot("account-missing-percent", reset_at);
        item.secondary_used_percent = None;
        insert_usage(
            &storage,
            3,
            "account-missing-percent",
            2_000_000,
            3.25,
            now - 60,
        );

        let summaries = summaries_for_snapshots_at(&storage, &[item], now)
            .expect("summarize without upstream percent");
        let summary = summaries
            .get("account-missing-percent")
            .expect("summary without upstream percent");

        assert_eq!(summary.total_tokens, 2_000_000);
        assert!((summary.estimated_cost_usd - 3.25).abs() < f64::EPSILON);
        assert_eq!(summary.observed_token_capacity, None);
        assert_eq!(summary.observed_cost_capacity_usd, None);
    }

    #[test]
    fn secondary_window_observation_reports_stable_capacity_from_history() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        let now = 3_000_000;
        let reset_at = now + WEEK_SECONDS - 120;
        let mut previous = snapshot("account-observed", reset_at);
        previous.secondary_used_percent = Some(5.0);
        previous.captured_at = now - 60;
        let mut current = snapshot("account-observed", reset_at);
        current.secondary_used_percent = Some(10.0);
        current.captured_at = now;
        storage
            .insert_usage_snapshot(&previous)
            .expect("insert previous snapshot");
        storage
            .insert_usage_snapshot(&current)
            .expect("insert current snapshot");
        insert_usage(&storage, 10, "account-observed", 5_000_000, 6.0, now - 90);
        insert_usage(&storage, 11, "account-observed", 5_000_000, 6.0, now - 30);

        let summaries = summaries_for_snapshots_at(&storage, &[current], now)
            .expect("summarize observed capacity");
        let summary = summaries
            .get("account-observed")
            .expect("account observation");

        assert_eq!(summary.observed_token_capacity, Some(100_000_000));
        assert_eq!(summary.previous_observed_token_capacity, Some(100_000_000));
        assert_eq!(summary.observed_capacity_trend.as_deref(), Some("stable"));
        assert_eq!(
            summary.observed_capacity_confidence.as_deref(),
            Some("medium")
        );
        assert!(summary
            .observed_capacity_change_ratio
            .is_some_and(|ratio| ratio.abs() < f64::EPSILON));
        assert!(summary
            .observed_cost_capacity_usd
            .is_some_and(|cost| (cost - 120.0).abs() < f64::EPSILON));
    }
}
