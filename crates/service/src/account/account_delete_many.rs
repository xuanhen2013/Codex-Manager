use codexmanager_core::storage::{now_ts, Event};
use serde::Serialize;
use std::collections::HashSet;

use crate::storage_helpers::open_storage;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeleteManyError {
    account_id: String,
    message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeleteManyResult {
    requested: usize,
    deleted: usize,
    failed: usize,
    deleted_account_ids: Vec<String>,
    errors: Vec<DeleteManyError>,
}

/// 函数 `delete_accounts`
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
pub(crate) fn delete_accounts(account_ids: Vec<String>) -> Result<DeleteManyResult, String> {
    let mut unique = Vec::new();
    let mut seen = HashSet::new();
    for account_id in account_ids {
        let normalized = account_id.trim();
        if normalized.is_empty() {
            continue;
        }
        if seen.insert(normalized.to_string()) {
            unique.push(normalized.to_string());
        }
    }

    if unique.is_empty() {
        return Err("missing accountIds".to_string());
    }

    let mut storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let mut result = DeleteManyResult {
        requested: unique.len(),
        deleted: 0,
        failed: 0,
        deleted_account_ids: Vec::new(),
        errors: Vec::new(),
    };

    let existing_account_ids = storage
        .list_account_ids_for_ids(&unique)
        .map_err(|err| err.to_string())?
        .into_iter()
        .collect::<HashSet<_>>();

    for account_id in unique {
        if !existing_account_ids.contains(&account_id) {
            result.failed += 1;
            result.errors.push(DeleteManyError {
                account_id,
                message: "account not found".to_string(),
            });
            continue;
        }

        match storage.delete_account(&account_id) {
            Ok(_) => {
                let _ = storage.insert_event(&Event {
                    account_id: Some(account_id.clone()),
                    event_type: "account_delete_many".to_string(),
                    message: "account deleted via bulk action".to_string(),
                    created_at: now_ts(),
                });
                result.deleted += 1;
                result.deleted_account_ids.push(account_id);
            }
            Err(err) => {
                result.failed += 1;
                result.errors.push(DeleteManyError {
                    account_id,
                    message: err.to_string(),
                });
            }
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::delete_accounts;
    use codexmanager_core::storage::{Account, Storage};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use crate::test_env_guard;

    static DELETE_MANY_TEST_DIR_SEQ: AtomicUsize = AtomicUsize::new(0);

    fn new_test_dir(prefix: &str) -> PathBuf {
        let seq = DELETE_MANY_TEST_DIR_SEQ.fetch_add(1, Ordering::Relaxed);
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
        fn set(key: &'static str, value: &str) -> Self {
            let original = std::env::var_os(key);
            std::env::set_var(key, value);
            Self { key, original }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(value) = &self.original {
                std::env::set_var(self.key, value);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    fn account(id: &str, sort: i64) -> Account {
        Account {
            id: id.to_string(),
            label: id.to_string(),
            issuer: "chatgpt".to_string(),
            chatgpt_account_id: None,
            workspace_id: None,
            group_name: None,
            sort,
            status: "active".to_string(),
            created_at: 1,
            updated_at: 1,
        }
    }

    #[test]
    fn delete_accounts_dedupes_ids_and_reports_missing_accounts() {
        let _lock = test_env_guard();
        let dir = new_test_dir("delete-many-accounts");
        let db_path = dir.join("codexmanager.db");
        let _guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

        let storage = Storage::open(&db_path).expect("open db");
        storage.init().expect("init db");
        storage
            .insert_account(&account("acc-delete", 1))
            .expect("insert delete target");
        storage
            .insert_account(&account("acc-keep", 2))
            .expect("insert keep target");

        let result = delete_accounts(vec![
            " acc-delete ".to_string(),
            "".to_string(),
            "acc-delete".to_string(),
            " missing ".to_string(),
        ])
        .expect("delete result");

        assert_eq!(result.requested, 2);
        assert_eq!(result.deleted, 1);
        assert_eq!(result.failed, 1);
        assert_eq!(result.deleted_account_ids, vec!["acc-delete".to_string()]);
        assert_eq!(result.errors.len(), 1);
        assert_eq!(result.errors[0].account_id, "missing");
        assert_eq!(result.errors[0].message, "account not found");

        let storage = Storage::open(&db_path).expect("reopen db");
        assert!(!storage
            .account_exists("acc-delete")
            .expect("deleted account existence"));
        assert!(storage
            .account_exists("acc-keep")
            .expect("kept account existence"));
    }

    #[test]
    fn delete_accounts_rejects_empty_input_after_normalization() {
        let err = delete_accounts(vec![" ".to_string(), "".to_string()])
            .expect_err("empty ids should be rejected");

        assert_eq!(err, "missing accountIds");
    }
}
