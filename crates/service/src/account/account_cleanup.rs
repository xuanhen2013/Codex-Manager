use codexmanager_core::storage::{
    now_ts, AccountTokenPlan, Event, UsageSnapshotCleanupRow, UsageSnapshotRecord,
};
use serde::Serialize;
use std::collections::{BTreeSet, HashMap, HashSet};

use crate::account_availability::{evaluate_snapshot, Availability};
use crate::account_plan::ResolvedAccountPlan;
use crate::storage_helpers::open_storage;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeleteUnavailableFreeResult {
    scanned: usize,
    deleted: usize,
    skipped_available: usize,
    skipped_disabled: usize,
    skipped_non_free: usize,
    skipped_missing_usage: usize,
    skipped_missing_token: usize,
    deleted_account_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeleteBannedResult {
    scanned: usize,
    deleted: usize,
    skipped_disabled: usize,
    skipped_not_banned: usize,
    deleted_account_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeleteAccountsByStatusesResult {
    scanned: usize,
    deleted: usize,
    skipped_status: usize,
    target_statuses: Vec<String>,
    deleted_account_ids: Vec<String>,
}

const CLEANUP_STATUS_ALLOWLIST: &[&str] = &[
    "unavailable",
    "banned",
    "limited",
    "disabled",
    "inactive",
    "unknown",
];

/// 函数 `delete_unavailable_free_accounts`
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
pub(crate) fn delete_unavailable_free_accounts() -> Result<DeleteUnavailableFreeResult, String> {
    let mut storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let scanned = storage.account_count().map_err(|err| err.to_string())? as usize;
    let accounts = storage
        .list_account_cleanup_candidates_by_statuses(&cleanup_status_allowlist_vec())
        .map_err(|err| err.to_string())?;
    let account_ids = accounts
        .iter()
        .map(|account| account.id.clone())
        .collect::<Vec<_>>();
    let usage_by_account: HashMap<String, UsageSnapshotCleanupRow> = storage
        .latest_usage_cleanup_rows_for_accounts(&account_ids)
        .map_err(|err| err.to_string())?
        .into_iter()
        .map(|snapshot| (snapshot.account_id.clone(), snapshot))
        .collect();
    let token_plans_by_account: HashMap<String, AccountTokenPlan> = storage
        .list_account_token_plans_for_accounts(&account_ids)
        .map_err(|err| err.to_string())?
        .into_iter()
        .map(|token| (token.account_id.clone(), token))
        .collect();

    let mut result = DeleteUnavailableFreeResult {
        scanned,
        deleted: 0,
        skipped_available: 0,
        skipped_disabled: 0,
        skipped_non_free: 0,
        skipped_missing_usage: 0,
        skipped_missing_token: 0,
        deleted_account_ids: Vec::new(),
    };

    let mut pending_deletes: Vec<(String, String)> = Vec::new();
    for account in accounts {
        let normalized_status = account.status.trim().to_ascii_lowercase();
        if normalized_status == "disabled" {
            result.skipped_disabled += 1;
            continue;
        }

        let snapshot = usage_by_account.get(&account.id);
        let snapshot_record = snapshot.map(cleanup_snapshot_record);
        if normalized_status != "unavailable" && normalized_status != "banned" {
            let Some(snapshot) = snapshot_record.as_ref() else {
                result.skipped_missing_usage += 1;
                continue;
            };
            if matches!(evaluate_snapshot(snapshot), Availability::Available) {
                result.skipped_available += 1;
                continue;
            }
        }

        let token = token_plans_by_account.get(&account.id);
        let resolved_plan =
            crate::account_plan::resolve_account_plan(token, snapshot_record.as_ref());
        let Some(plan) = resolved_plan.as_ref() else {
            if snapshot.is_none() && token.is_none() {
                result.skipped_missing_usage += 1;
            } else if token.is_none() {
                result.skipped_missing_token += 1;
            } else {
                result.skipped_non_free += 1;
            }
            continue;
        };
        if plan.normalized != "free" {
            result.skipped_non_free += 1;
            continue;
        }
        let Some(_token) = token else {
            result.skipped_missing_token += 1;
            continue;
        };

        let event_message = match plan_label_for_event(resolved_plan.as_ref()) {
            Some(plan) => format!("bulk delete unavailable free account: plan={plan}"),
            None => "bulk delete unavailable free account".to_string(),
        };
        pending_deletes.push((account.id, event_message));
    }

    let pending_ids = pending_deletes
        .iter()
        .map(|(account_id, _)| account_id.clone())
        .collect::<Vec<_>>();
    if !pending_ids.is_empty() {
        storage
            .delete_accounts(&pending_ids)
            .map_err(|err| err.to_string())?;
        for (account_id, event_message) in pending_deletes {
            let _ = storage.insert_event(&Event {
                account_id: Some(account_id.clone()),
                event_type: "account_bulk_delete_unavailable_free".to_string(),
                message: event_message,
                created_at: now_ts(),
            });

            result.deleted += 1;
            result.deleted_account_ids.push(account_id);
        }
    }

    Ok(result)
}

/// 函数 `delete_banned_accounts`
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
pub(crate) fn delete_banned_accounts() -> Result<DeleteBannedResult, String> {
    let mut storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let scanned = storage.account_count().map_err(|err| err.to_string())? as usize;
    let accounts = storage
        .list_account_cleanup_candidates_by_statuses(&[
            "banned".to_string(),
            "disabled".to_string(),
        ])
        .map_err(|err| err.to_string())?;

    let mut result = DeleteBannedResult {
        scanned,
        deleted: 0,
        skipped_disabled: 0,
        skipped_not_banned: 0,
        deleted_account_ids: Vec::new(),
    };

    let mut pending_ids = Vec::new();
    for account in accounts {
        let normalized_status = account.status.trim().to_ascii_lowercase();
        if normalized_status == "disabled" {
            result.skipped_disabled += 1;
            continue;
        }
        if normalized_status != "banned" {
            result.skipped_not_banned += 1;
            continue;
        }

        pending_ids.push(account.id);
    }

    if !pending_ids.is_empty() {
        storage
            .delete_accounts(&pending_ids)
            .map_err(|err| err.to_string())?;
        for account_id in pending_ids {
            let _ = storage.insert_event(&Event {
                account_id: Some(account_id.clone()),
                event_type: "account_bulk_delete_banned".to_string(),
                message: "bulk delete banned account".to_string(),
                created_at: now_ts(),
            });

            result.deleted += 1;
            result.deleted_account_ids.push(account_id);
        }
    }
    result.skipped_not_banned = result
        .scanned
        .saturating_sub(result.deleted)
        .saturating_sub(result.skipped_disabled);

    Ok(result)
}

/// 函数 `delete_accounts_by_statuses`
///
/// 作者: gaohongshun
///
/// 时间: 2026-05-04
///
/// # 参数
/// - statuses: 参数 statuses
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn delete_accounts_by_statuses(
    statuses: Vec<String>,
) -> Result<DeleteAccountsByStatusesResult, String> {
    let target_statuses = normalize_cleanup_statuses(statuses)?;
    let mut storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let scanned = storage.account_count().map_err(|err| err.to_string())? as usize;
    let accounts = storage
        .list_account_cleanup_candidates_by_statuses(&target_statuses)
        .map_err(|err| err.to_string())?;

    let mut result = DeleteAccountsByStatusesResult {
        scanned,
        deleted: 0,
        skipped_status: 0,
        target_statuses,
        deleted_account_ids: Vec::new(),
    };

    let mut pending_deletes: Vec<(String, String)> = Vec::new();
    for account in accounts {
        let normalized_status = account.status.trim().to_ascii_lowercase();
        pending_deletes.push((
            account.id,
            format!("bulk delete account by status: status={normalized_status}"),
        ));
    }

    let pending_ids = pending_deletes
        .iter()
        .map(|(account_id, _)| account_id.clone())
        .collect::<Vec<_>>();
    if !pending_ids.is_empty() {
        storage
            .delete_accounts(&pending_ids)
            .map_err(|err| err.to_string())?;
        for (account_id, event_message) in pending_deletes {
            let _ = storage.insert_event(&Event {
                account_id: Some(account_id.clone()),
                event_type: "account_bulk_delete_by_status".to_string(),
                message: event_message,
                created_at: now_ts(),
            });

            result.deleted += 1;
            result.deleted_account_ids.push(account_id);
        }
    }
    result.skipped_status = result.scanned.saturating_sub(result.deleted);

    Ok(result)
}

fn normalize_cleanup_statuses(statuses: Vec<String>) -> Result<Vec<String>, String> {
    let selected = statuses
        .into_iter()
        .filter_map(|status| {
            let normalized = status.trim().to_ascii_lowercase();
            if normalized.is_empty() {
                None
            } else {
                Some(normalized)
            }
        })
        .collect::<BTreeSet<_>>();
    if selected.is_empty() {
        return Err("missing cleanup statuses".to_string());
    }

    let allowed = CLEANUP_STATUS_ALLOWLIST
        .iter()
        .copied()
        .collect::<HashSet<_>>();
    let invalid = selected
        .iter()
        .filter(|status| !allowed.contains(status.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if !invalid.is_empty() {
        return Err(format!(
            "unsupported cleanup statuses: {}",
            invalid.join(",")
        ));
    }

    Ok(CLEANUP_STATUS_ALLOWLIST
        .iter()
        .filter(|status| selected.contains(**status))
        .map(|status| (*status).to_string())
        .collect())
}

fn cleanup_status_allowlist_vec() -> Vec<String> {
    CLEANUP_STATUS_ALLOWLIST
        .iter()
        .map(|status| (*status).to_string())
        .collect()
}

fn cleanup_snapshot_record(row: &UsageSnapshotCleanupRow) -> UsageSnapshotRecord {
    UsageSnapshotRecord {
        account_id: row.account_id.clone(),
        used_percent: row.used_percent,
        window_minutes: row.window_minutes,
        resets_at: None,
        secondary_used_percent: row.secondary_used_percent,
        secondary_window_minutes: row.secondary_window_minutes,
        secondary_resets_at: None,
        credits_json: row.credits_json.clone(),
        captured_at: 0,
    }
}

/// 函数 `plan_label_for_event`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - plan: 参数 plan
///
/// # 返回
/// 返回函数执行结果
fn plan_label_for_event(plan: Option<&ResolvedAccountPlan>) -> Option<&str> {
    plan.and_then(|value| {
        if value.normalized == "unknown" {
            value.raw.as_deref()
        } else {
            Some(value.normalized.as_str())
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{
        delete_accounts_by_statuses, delete_banned_accounts, delete_unavailable_free_accounts,
    };
    use codexmanager_core::storage::{Account, Storage, Token, UsageSnapshotRecord};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use crate::test_env_guard;

    static CLEANUP_TEST_DIR_SEQ: AtomicUsize = AtomicUsize::new(0);

    /// 函数 `new_test_dir`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - prefix: 参数 prefix
    ///
    /// # 返回
    /// 返回函数执行结果
    fn new_test_dir(prefix: &str) -> PathBuf {
        let seq = CLEANUP_TEST_DIR_SEQ.fetch_add(1, Ordering::Relaxed);
        let mut dir = std::env::temp_dir();
        dir.push(format!("{prefix}-{}-{seq}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    struct EnvGuard {
        key: &'static str,
        original: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        /// 函数 `set`
        ///
        /// 作者: gaohongshun
        ///
        /// 时间: 2026-04-02
        ///
        /// # 参数
        /// - key: 参数 key
        /// - value: 参数 value
        ///
        /// # 返回
        /// 返回函数执行结果
        fn set(key: &'static str, value: &str) -> Self {
            let original = std::env::var_os(key);
            std::env::set_var(key, value);
            Self { key, original }
        }
    }

    impl Drop for EnvGuard {
        /// 函数 `drop`
        ///
        /// 作者: gaohongshun
        ///
        /// 时间: 2026-04-02
        ///
        /// # 参数
        /// - self: 参数 self
        ///
        /// # 返回
        /// 无
        fn drop(&mut self) {
            if let Some(value) = &self.original {
                std::env::set_var(self.key, value);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    fn account(id: &str, status: &str, sort: i64) -> Account {
        Account {
            id: id.to_string(),
            label: id.to_string(),
            issuer: "chatgpt".to_string(),
            chatgpt_account_id: None,
            workspace_id: None,
            group_name: None,
            sort,
            status: status.to_string(),
            created_at: 1,
            updated_at: 1,
        }
    }

    fn token(account_id: &str) -> Token {
        Token {
            account_id: account_id.to_string(),
            id_token: "id".to_string(),
            access_token: "access".to_string(),
            refresh_token: "refresh".to_string(),
            api_key_access_token: None,
            last_refresh: 1,
        }
    }

    fn free_usage(account_id: &str) -> UsageSnapshotRecord {
        UsageSnapshotRecord {
            account_id: account_id.to_string(),
            used_percent: Some(95.0),
            window_minutes: Some(300),
            resets_at: None,
            secondary_used_percent: None,
            secondary_window_minutes: None,
            secondary_resets_at: None,
            credits_json: Some(r#"{"planType":"free"}"#.to_string()),
            captured_at: 1,
        }
    }

    #[test]
    fn delete_unavailable_free_accounts_scans_total_but_loads_cleanup_status_candidates() {
        let _lock = test_env_guard();
        let dir = new_test_dir("cleanup-unavailable-free-accounts");
        let db_path = dir.join("codexmanager.db");
        let _guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

        let storage = Storage::open(&db_path).expect("open db");
        storage.init().expect("init db");
        for account in [
            account("acc-active", "active", 1),
            account("acc-unavailable-free", "unavailable", 2),
            account("acc-disabled-free", "disabled", 3),
        ] {
            storage.insert_account(&account).expect("insert account");
            storage
                .insert_token(&token(&account.id))
                .expect("insert token");
            storage
                .insert_usage_snapshot(&free_usage(&account.id))
                .expect("insert usage");
        }

        let result = delete_unavailable_free_accounts().expect("cleanup result");

        assert_eq!(result.scanned, 3);
        assert_eq!(result.deleted, 1);
        assert_eq!(result.skipped_disabled, 1);
        assert_eq!(
            result.deleted_account_ids,
            vec!["acc-unavailable-free".to_string()]
        );
        let remaining = Storage::open(&db_path)
            .expect("reopen db")
            .list_accounts()
            .expect("list accounts")
            .into_iter()
            .map(|account| account.id)
            .collect::<Vec<_>>();
        assert_eq!(
            remaining,
            vec!["acc-active".to_string(), "acc-disabled-free".to_string()]
        );
    }

    /// 函数 `delete_banned_accounts_removes_only_banned_accounts`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// 无
    ///
    /// # 返回
    /// 无
    #[test]
    fn delete_banned_accounts_removes_only_banned_accounts() {
        let _lock = test_env_guard();
        let dir = new_test_dir("cleanup-banned-accounts");
        let db_path = dir.join("codexmanager.db");
        let _guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

        let storage = Storage::open(&db_path).expect("open db");
        storage.init().expect("init db");
        storage
            .insert_account(&Account {
                id: "acc-banned".to_string(),
                label: "Banned".to_string(),
                issuer: "chatgpt".to_string(),
                chatgpt_account_id: None,
                workspace_id: None,
                group_name: None,
                sort: 1,
                status: "banned".to_string(),
                created_at: 1,
                updated_at: 1,
            })
            .expect("insert banned");
        storage
            .insert_account(&Account {
                id: "acc-active".to_string(),
                label: "Active".to_string(),
                issuer: "chatgpt".to_string(),
                chatgpt_account_id: None,
                workspace_id: None,
                group_name: None,
                sort: 2,
                status: "active".to_string(),
                created_at: 1,
                updated_at: 1,
            })
            .expect("insert active");

        let result = delete_banned_accounts().expect("cleanup result");
        assert_eq!(result.deleted, 1);
        assert_eq!(result.deleted_account_ids, vec!["acc-banned".to_string()]);
        assert!(Storage::open(&db_path)
            .expect("reopen db")
            .find_account_by_id("acc-banned")
            .expect("find banned")
            .is_none());
        assert!(Storage::open(&db_path)
            .expect("reopen db")
            .find_account_by_id("acc-active")
            .expect("find active")
            .is_some());
    }

    #[test]
    fn delete_accounts_by_statuses_removes_selected_statuses_only() {
        let _lock = test_env_guard();
        let dir = new_test_dir("cleanup-accounts-by-status");
        let db_path = dir.join("codexmanager.db");
        let _guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

        let storage = Storage::open(&db_path).expect("open db");
        storage.init().expect("init db");
        for (idx, (id, status)) in [
            ("acc-active", "active"),
            ("acc-unavailable", "unavailable"),
            ("acc-banned", "banned"),
            ("acc-limited", "limited"),
            ("acc-disabled", "disabled"),
            ("acc-unknown", "unknown"),
        ]
        .into_iter()
        .enumerate()
        {
            storage
                .insert_account(&Account {
                    id: id.to_string(),
                    label: id.to_string(),
                    issuer: "chatgpt".to_string(),
                    chatgpt_account_id: None,
                    workspace_id: None,
                    group_name: None,
                    sort: idx as i64,
                    status: status.to_string(),
                    created_at: 1,
                    updated_at: 1,
                })
                .expect("insert account");
        }

        let result = delete_accounts_by_statuses(vec![
            "banned".to_string(),
            "limited".to_string(),
            "unknown".to_string(),
            "banned".to_string(),
        ])
        .expect("cleanup result");

        assert_eq!(result.deleted, 3);
        assert_eq!(
            result.target_statuses,
            vec![
                "banned".to_string(),
                "limited".to_string(),
                "unknown".to_string()
            ]
        );
        assert_eq!(
            result.deleted_account_ids,
            vec![
                "acc-banned".to_string(),
                "acc-limited".to_string(),
                "acc-unknown".to_string()
            ]
        );
        let remaining = Storage::open(&db_path)
            .expect("reopen db")
            .list_accounts()
            .expect("list accounts")
            .into_iter()
            .map(|account| account.id)
            .collect::<Vec<_>>();
        assert_eq!(
            remaining,
            vec![
                "acc-active".to_string(),
                "acc-unavailable".to_string(),
                "acc-disabled".to_string()
            ]
        );
    }

    #[test]
    fn delete_accounts_by_statuses_rejects_active_status() {
        let err = delete_accounts_by_statuses(vec!["active".to_string()])
            .expect_err("active should not be cleanup-selectable");

        assert!(err.contains("unsupported cleanup statuses"));
    }
}
