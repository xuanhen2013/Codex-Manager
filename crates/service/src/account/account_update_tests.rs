use super::update_account;
use codexmanager_core::storage::{Account, Storage};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

static ACCOUNT_UPDATE_TEST_DIR_SEQ: AtomicUsize = AtomicUsize::new(0);

fn new_test_dir(prefix: &str) -> PathBuf {
    let seq = ACCOUNT_UPDATE_TEST_DIR_SEQ.fetch_add(1, Ordering::Relaxed);
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
fn update_account_preferred_uses_existence_check_without_loading_account() {
    let _lock = crate::test_env_guard();
    let dir = new_test_dir("account-update-preferred");
    let db_path = dir.join("codexmanager.db");
    let _guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    storage
        .insert_account(&account("acc-preferred", 1))
        .expect("insert preferred");

    update_account(
        "acc-preferred",
        None,
        Some(true),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .expect("set preferred");

    assert_eq!(
        Storage::open(&db_path)
            .expect("reopen db")
            .preferred_account_id()
            .expect("read preferred")
            .as_deref(),
        Some("acc-preferred")
    );
    let err = update_account(
        "acc-missing",
        None,
        Some(true),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .expect_err("missing account should fail");
    assert_eq!(err, "account not found");
}
