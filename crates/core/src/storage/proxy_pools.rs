use rusqlite::{params, Result, Row};

use super::{
    now_ts, AccountProxyPoolBinding, ProxyPool, ProxyPoolMember, ProxyPoolSwitchLog, Storage,
};

impl Storage {
    pub fn create_proxy_pool(
        &self,
        id: &str,
        name: &str,
        description: Option<&str>,
        enabled: bool,
        failure_threshold: i64,
        recovery_threshold: i64,
        heartbeat_interval_secs: i64,
        cooldown_secs: i64,
    ) -> Result<ProxyPool> {
        let now = now_ts();
        self.conn.execute(
            "INSERT INTO proxy_pools (
                id, name, description, enabled, failure_threshold, recovery_threshold,
                heartbeat_interval_secs, cooldown_secs, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
            params![
                id.trim(),
                name.trim(),
                normalize_optional_text(description),
                bool_int(enabled),
                failure_threshold,
                recovery_threshold,
                heartbeat_interval_secs,
                cooldown_secs,
                now,
            ],
        )?;
        self.find_proxy_pool(id)?
            .ok_or(rusqlite::Error::QueryReturnedNoRows)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_proxy_pool(
        &self,
        id: &str,
        name: &str,
        description: Option<&str>,
        enabled: bool,
        failure_threshold: i64,
        recovery_threshold: i64,
        heartbeat_interval_secs: i64,
        cooldown_secs: i64,
    ) -> Result<Option<ProxyPool>> {
        let changed = self.conn.execute(
            "UPDATE proxy_pools
             SET name = ?2, description = ?3, enabled = ?4, failure_threshold = ?5,
                 recovery_threshold = ?6, heartbeat_interval_secs = ?7,
                 cooldown_secs = ?8, updated_at = ?9
             WHERE id = ?1",
            params![
                id.trim(),
                name.trim(),
                normalize_optional_text(description),
                bool_int(enabled),
                failure_threshold,
                recovery_threshold,
                heartbeat_interval_secs,
                cooldown_secs,
                now_ts(),
            ],
        )?;
        if changed == 0 {
            Ok(None)
        } else {
            self.find_proxy_pool(id)
        }
    }

    pub fn delete_proxy_pool(&self, id: &str) -> Result<bool> {
        Ok(self
            .conn
            .execute("DELETE FROM proxy_pools WHERE id = ?1", [id.trim()])?
            > 0)
    }

    pub fn find_proxy_pool(&self, id: &str) -> Result<Option<ProxyPool>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, description, enabled, failure_threshold, recovery_threshold,
                    heartbeat_interval_secs, cooldown_secs, created_at, updated_at
             FROM proxy_pools WHERE id = ?1",
        )?;
        let mut rows = stmt.query([id.trim()])?;
        rows.next()?.map(map_proxy_pool_row).transpose()
    }

    pub fn list_proxy_pools(&self) -> Result<Vec<ProxyPool>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, description, enabled, failure_threshold, recovery_threshold,
                    heartbeat_interval_secs, cooldown_secs, created_at, updated_at
             FROM proxy_pools ORDER BY created_at ASC, id ASC",
        )?;
        let mut rows = stmt.query([])?;
        let mut items = Vec::new();
        while let Some(row) = rows.next()? {
            items.push(map_proxy_pool_row(row)?);
        }
        Ok(items)
    }

    pub fn add_proxy_pool_member(
        &self,
        pool_id: &str,
        proxy_profile_id: &str,
        sort_order: i64,
    ) -> Result<ProxyPoolMember> {
        let now = now_ts();
        self.conn.execute(
            "INSERT INTO proxy_pool_members (
                pool_id, proxy_profile_id, sort_order, enabled, health_status,
                consecutive_failures, consecutive_successes, created_at, updated_at
             ) VALUES (?1, ?2, ?3, 1, 'unchecked', 0, 0, ?4, ?4)",
            params![pool_id.trim(), proxy_profile_id.trim(), sort_order, now],
        )?;
        self.find_proxy_pool_member(pool_id, proxy_profile_id)?
            .ok_or(rusqlite::Error::QueryReturnedNoRows)
    }

    pub fn remove_proxy_pool_member(&self, pool_id: &str, proxy_profile_id: &str) -> Result<bool> {
        Ok(self.conn.execute(
            "DELETE FROM proxy_pool_members WHERE pool_id = ?1 AND proxy_profile_id = ?2",
            params![pool_id.trim(), proxy_profile_id.trim()],
        )? > 0)
    }

    pub fn set_proxy_pool_member_enabled(
        &self,
        pool_id: &str,
        proxy_profile_id: &str,
        enabled: bool,
    ) -> Result<bool> {
        Ok(self.conn.execute(
            "UPDATE proxy_pool_members SET enabled = ?3, updated_at = ?4
             WHERE pool_id = ?1 AND proxy_profile_id = ?2",
            params![
                pool_id.trim(),
                proxy_profile_id.trim(),
                bool_int(enabled),
                now_ts()
            ],
        )? > 0)
    }

    pub fn find_proxy_pool_member(
        &self,
        pool_id: &str,
        proxy_profile_id: &str,
    ) -> Result<Option<ProxyPoolMember>> {
        let mut stmt = self.conn.prepare(
            "SELECT pool_id, proxy_profile_id, sort_order, enabled, health_status,
                    consecutive_failures, consecutive_successes, cooldown_until,
                    last_check_at, last_error, created_at, updated_at
             FROM proxy_pool_members WHERE pool_id = ?1 AND proxy_profile_id = ?2",
        )?;
        let mut rows = stmt.query(params![pool_id.trim(), proxy_profile_id.trim()])?;
        rows.next()?.map(map_proxy_pool_member_row).transpose()
    }

    pub fn list_proxy_pool_members(&self, pool_id: &str) -> Result<Vec<ProxyPoolMember>> {
        let mut stmt = self.conn.prepare(
            "SELECT pool_id, proxy_profile_id, sort_order, enabled, health_status,
                    consecutive_failures, consecutive_successes, cooldown_until,
                    last_check_at, last_error, created_at, updated_at
             FROM proxy_pool_members WHERE pool_id = ?1
             ORDER BY sort_order ASC, proxy_profile_id ASC",
        )?;
        let mut rows = stmt.query([pool_id.trim()])?;
        let mut items = Vec::new();
        while let Some(row) = rows.next()? {
            items.push(map_proxy_pool_member_row(row)?);
        }
        Ok(items)
    }

    pub fn list_due_proxy_pool_members(&self, now: i64) -> Result<Vec<ProxyPoolMember>> {
        let mut stmt = self.conn.prepare(
            "SELECT m.pool_id, m.proxy_profile_id, m.sort_order, m.enabled, m.health_status,
                    m.consecutive_failures, m.consecutive_successes, m.cooldown_until,
                    m.last_check_at, m.last_error, m.created_at, m.updated_at
             FROM proxy_pool_members m
             JOIN proxy_pools p ON p.id = m.pool_id
             JOIN proxy_profiles pp ON pp.id = m.proxy_profile_id
             WHERE p.enabled = 1 AND m.enabled = 1 AND pp.enabled = 1
               AND (m.last_check_at IS NULL OR m.last_check_at + p.heartbeat_interval_secs <= ?1)
               AND (m.health_status <> 'unhealthy' OR m.cooldown_until IS NULL OR m.cooldown_until <= ?1)
             ORDER BY COALESCE(m.last_check_at, 0) ASC, m.pool_id ASC, m.sort_order ASC",
        )?;
        let mut rows = stmt.query([now])?;
        let mut items = Vec::new();
        while let Some(row) = rows.next()? {
            items.push(map_proxy_pool_member_row(row)?);
        }
        Ok(items)
    }

    pub fn list_active_proxy_pool_members(&self) -> Result<Vec<ProxyPoolMember>> {
        let mut stmt = self.conn.prepare(
            "SELECT m.pool_id, m.proxy_profile_id, m.sort_order, m.enabled, m.health_status,
                    m.consecutive_failures, m.consecutive_successes, m.cooldown_until,
                    m.last_check_at, m.last_error, m.created_at, m.updated_at
             FROM proxy_pool_members m
             JOIN proxy_pools p ON p.id = m.pool_id
             JOIN proxy_profiles pp ON pp.id = m.proxy_profile_id
             WHERE p.enabled = 1 AND m.enabled = 1 AND pp.enabled = 1
             ORDER BY m.pool_id ASC, m.sort_order ASC, m.proxy_profile_id ASC",
        )?;
        let mut rows = stmt.query([])?;
        let mut items = Vec::new();
        while let Some(row) = rows.next()? {
            items.push(map_proxy_pool_member_row(row)?);
        }
        Ok(items)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_proxy_pool_member_health(
        &self,
        pool_id: &str,
        proxy_profile_id: &str,
        health_status: &str,
        consecutive_failures: i64,
        consecutive_successes: i64,
        cooldown_until: Option<i64>,
        last_check_at: i64,
        last_error: Option<&str>,
    ) -> Result<bool> {
        Ok(self.conn.execute(
            "UPDATE proxy_pool_members
             SET health_status = ?3, consecutive_failures = ?4,
                 consecutive_successes = ?5, cooldown_until = ?6,
                 last_check_at = ?7, last_error = ?8, updated_at = ?7
             WHERE pool_id = ?1 AND proxy_profile_id = ?2",
            params![
                pool_id.trim(),
                proxy_profile_id.trim(),
                health_status,
                consecutive_failures,
                consecutive_successes,
                cooldown_until,
                last_check_at,
                normalize_optional_text(last_error),
            ],
        )? > 0)
    }

    pub fn find_account_proxy_pool_binding(
        &self,
        account_id: &str,
    ) -> Result<Option<AccountProxyPoolBinding>> {
        let mut stmt = self.conn.prepare(
            "SELECT account_id, pool_id, current_proxy_profile_id, assigned_at,
                    last_switched_at, last_switch_reason, updated_at
             FROM account_proxy_pool_bindings WHERE account_id = ?1",
        )?;
        let mut rows = stmt.query([account_id.trim()])?;
        rows.next()?.map(map_account_pool_binding_row).transpose()
    }

    pub fn list_account_proxy_pool_bindings_by_pool(
        &self,
        pool_id: &str,
    ) -> Result<Vec<AccountProxyPoolBinding>> {
        let mut stmt = self.conn.prepare(
            "SELECT account_id, pool_id, current_proxy_profile_id, assigned_at,
                    last_switched_at, last_switch_reason, updated_at
             FROM account_proxy_pool_bindings WHERE pool_id = ?1 ORDER BY account_id ASC",
        )?;
        let mut rows = stmt.query([pool_id.trim()])?;
        let mut items = Vec::new();
        while let Some(row) = rows.next()? {
            items.push(map_account_pool_binding_row(row)?);
        }
        Ok(items)
    }

    pub fn list_account_proxy_pool_bindings(&self) -> Result<Vec<AccountProxyPoolBinding>> {
        let mut stmt = self.conn.prepare(
            "SELECT account_id, pool_id, current_proxy_profile_id, assigned_at,
                    last_switched_at, last_switch_reason, updated_at
             FROM account_proxy_pool_bindings ORDER BY account_id ASC",
        )?;
        let mut rows = stmt.query([])?;
        let mut items = Vec::new();
        while let Some(row) = rows.next()? {
            items.push(map_account_pool_binding_row(row)?);
        }
        Ok(items)
    }

    pub fn list_account_proxy_pool_bindings_by_current_profile(
        &self,
        proxy_profile_id: &str,
    ) -> Result<Vec<AccountProxyPoolBinding>> {
        let mut stmt = self.conn.prepare(
            "SELECT account_id, pool_id, current_proxy_profile_id, assigned_at,
                    last_switched_at, last_switch_reason, updated_at
             FROM account_proxy_pool_bindings WHERE current_proxy_profile_id = ?1
             ORDER BY account_id ASC",
        )?;
        let mut rows = stmt.query([proxy_profile_id.trim()])?;
        let mut items = Vec::new();
        while let Some(row) = rows.next()? {
            items.push(map_account_pool_binding_row(row)?);
        }
        Ok(items)
    }

    pub fn upsert_account_proxy_pool_binding(
        &self,
        account_id: &str,
        pool_id: &str,
        current_proxy_profile_id: Option<&str>,
        reason: Option<&str>,
    ) -> Result<AccountProxyPoolBinding> {
        let now = now_ts();
        self.conn.execute(
            "INSERT INTO account_proxy_pool_bindings (
                account_id, pool_id, current_proxy_profile_id, assigned_at,
                last_switched_at, last_switch_reason, updated_at
             ) VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?4)
             ON CONFLICT(account_id) DO UPDATE SET
                pool_id = excluded.pool_id,
                current_proxy_profile_id = excluded.current_proxy_profile_id,
                assigned_at = CASE WHEN account_proxy_pool_bindings.pool_id <> excluded.pool_id
                                   THEN excluded.assigned_at ELSE account_proxy_pool_bindings.assigned_at END,
                last_switch_reason = excluded.last_switch_reason,
                updated_at = excluded.updated_at",
            params![
                account_id.trim(),
                pool_id.trim(),
                normalize_optional_text(current_proxy_profile_id),
                now,
                normalize_optional_text(reason),
            ],
        )?;
        self.find_account_proxy_pool_binding(account_id)?
            .ok_or(rusqlite::Error::QueryReturnedNoRows)
    }

    pub fn switch_account_proxy_pool_binding(
        &self,
        account_id: &str,
        to_proxy_profile_id: Option<&str>,
        reason: &str,
    ) -> Result<Option<AccountProxyPoolBinding>> {
        let Some(previous) = self.find_account_proxy_pool_binding(account_id)? else {
            return Ok(None);
        };
        let now = now_ts();
        self.conn.execute(
            "UPDATE account_proxy_pool_bindings
             SET current_proxy_profile_id = ?2, last_switched_at = ?3,
                 last_switch_reason = ?4, updated_at = ?3
             WHERE account_id = ?1",
            params![
                account_id.trim(),
                normalize_optional_text(to_proxy_profile_id),
                now,
                reason.trim(),
            ],
        )?;
        self.conn.execute(
            "INSERT INTO proxy_pool_switch_logs (
                account_id, pool_id, from_proxy_profile_id, to_proxy_profile_id, reason, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                account_id.trim(),
                previous.pool_id,
                previous.current_proxy_profile_id,
                normalize_optional_text(to_proxy_profile_id),
                reason.trim(),
                now,
            ],
        )?;
        self.find_account_proxy_pool_binding(account_id)
    }

    pub fn delete_account_proxy_pool_binding(&self, account_id: &str) -> Result<bool> {
        Ok(self.conn.execute(
            "DELETE FROM account_proxy_pool_bindings WHERE account_id = ?1",
            [account_id.trim()],
        )? > 0)
    }

    pub fn count_proxy_pool_current_bindings(&self, proxy_profile_id: &str) -> Result<i64> {
        self.conn.query_row(
            "SELECT COUNT(1) FROM account_proxy_pool_bindings WHERE current_proxy_profile_id = ?1",
            [proxy_profile_id.trim()],
            |row| row.get(0),
        )
    }

    pub fn list_proxy_pool_switch_logs(
        &self,
        account_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<ProxyPoolSwitchLog>> {
        let normalized_limit = limit.clamp(1, 200) as i64;
        let mut items = Vec::new();
        if let Some(account_id) = normalize_optional_text(account_id) {
            let mut stmt = self.conn.prepare(
                "SELECT id, account_id, pool_id, from_proxy_profile_id, to_proxy_profile_id,
                        reason, created_at
                 FROM proxy_pool_switch_logs WHERE account_id = ?1
                 ORDER BY created_at DESC, id DESC LIMIT ?2",
            )?;
            let mut rows = stmt.query(params![account_id, normalized_limit])?;
            while let Some(row) = rows.next()? {
                items.push(map_switch_log_row(row)?);
            }
        } else {
            let mut stmt = self.conn.prepare(
                "SELECT id, account_id, pool_id, from_proxy_profile_id, to_proxy_profile_id,
                        reason, created_at
                 FROM proxy_pool_switch_logs ORDER BY created_at DESC, id DESC LIMIT ?1",
            )?;
            let mut rows = stmt.query([normalized_limit])?;
            while let Some(row) = rows.next()? {
                items.push(map_switch_log_row(row)?);
            }
        }
        Ok(items)
    }

    pub(super) fn ensure_proxy_pool_tables(&self) -> Result<()> {
        self.conn.execute_batch(include_str!(
            "../../migrations/custom_proxy_pools_20260815.sql"
        ))
    }
}

fn map_proxy_pool_row(row: &Row<'_>) -> Result<ProxyPool> {
    Ok(ProxyPool {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        enabled: row.get::<_, i64>(3)? != 0,
        failure_threshold: row.get(4)?,
        recovery_threshold: row.get(5)?,
        heartbeat_interval_secs: row.get(6)?,
        cooldown_secs: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
    })
}

fn map_proxy_pool_member_row(row: &Row<'_>) -> Result<ProxyPoolMember> {
    Ok(ProxyPoolMember {
        pool_id: row.get(0)?,
        proxy_profile_id: row.get(1)?,
        sort_order: row.get(2)?,
        enabled: row.get::<_, i64>(3)? != 0,
        health_status: row.get(4)?,
        consecutive_failures: row.get(5)?,
        consecutive_successes: row.get(6)?,
        cooldown_until: row.get(7)?,
        last_check_at: row.get(8)?,
        last_error: row.get(9)?,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
    })
}

fn map_account_pool_binding_row(row: &Row<'_>) -> Result<AccountProxyPoolBinding> {
    Ok(AccountProxyPoolBinding {
        account_id: row.get(0)?,
        pool_id: row.get(1)?,
        current_proxy_profile_id: row.get(2)?,
        assigned_at: row.get(3)?,
        last_switched_at: row.get(4)?,
        last_switch_reason: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

fn map_switch_log_row(row: &Row<'_>) -> Result<ProxyPoolSwitchLog> {
    Ok(ProxyPoolSwitchLog {
        id: row.get(0)?,
        account_id: row.get(1)?,
        pool_id: row.get(2)?,
        from_proxy_profile_id: row.get(3)?,
        to_proxy_profile_id: row.get(4)?,
        reason: row.get(5)?,
        created_at: row.get(6)?,
    })
}

fn normalize_optional_text(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn bool_int(value: bool) -> i64 {
    if value {
        1
    } else {
        0
    }
}
