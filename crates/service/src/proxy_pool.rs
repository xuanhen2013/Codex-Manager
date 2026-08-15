use codexmanager_core::storage::{
    now_ts, ProxyPool, ProxyPoolMember, ProxyProfile, ProxyProfileCreateInput,
    ProxyProfileUpdateInput,
};
use serde::{Deserialize, Serialize};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use crate::storage_helpers::{generate_proxy_pool_id, generate_proxy_profile_id, open_storage};

const STATUS_HEALTHY: &str = "healthy";
const STATUS_SUSPECT: &str = "suspect";
const STATUS_UNHEALTHY: &str = "unhealthy";
const STATUS_RECOVERING: &str = "recovering";
const HEALTH_MONITOR_TICK_SECS: u64 = 15;
const MAX_HEALTH_CHECKS_PER_TICK: usize = 8;

static HEALTH_MONITOR_STARTED: OnceLock<()> = OnceLock::new();
static HEALTH_CYCLE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProxyPoolEntry {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub enabled: bool,
    pub failure_threshold: i64,
    pub recovery_threshold: i64,
    pub heartbeat_interval_secs: i64,
    pub cooldown_secs: i64,
    pub accounts_count: i64,
    pub healthy_members_count: i64,
    pub members: Vec<ProxyPoolMemberEntry>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProxyPoolMemberEntry {
    pub proxy_profile_id: String,
    pub proxy_profile_name: String,
    pub proxy_url_redacted: String,
    pub sort_order: i64,
    pub enabled: bool,
    pub profile_enabled: bool,
    pub health_status: String,
    pub consecutive_failures: i64,
    pub consecutive_successes: i64,
    pub cooldown_until: Option<i64>,
    pub last_check_at: Option<i64>,
    pub last_error: Option<String>,
    pub latency_ms: Option<i64>,
    pub current_accounts_count: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProxyBatchImportRequest {
    pub text: String,
    pub pool_id: Option<String>,
    pub default_scheme: Option<String>,
    pub name_prefix: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProxyBatchImportResult {
    pub total: usize,
    pub created: usize,
    pub duplicates: usize,
    pub failed: usize,
    pub items: Vec<ProxyBatchImportItemResult>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProxyBatchImportItemResult {
    pub line: usize,
    pub status: String,
    pub name: String,
    pub proxy_profile_id: Option<String>,
    pub proxy_url_redacted: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProxyPoolBindingEntry {
    pub account_id: String,
    pub pool_id: String,
    pub pool_name: String,
    pub current_proxy_profile_id: Option<String>,
    pub current_proxy_profile_name: Option<String>,
    pub assigned_at: i64,
    pub last_switched_at: Option<i64>,
    pub last_switch_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProxyPoolSwitchLogEntry {
    pub id: i64,
    pub account_id: String,
    pub pool_id: String,
    pub from_proxy_profile_id: Option<String>,
    pub to_proxy_profile_id: Option<String>,
    pub reason: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProxyPoolHealthCycleResult {
    pub checked: usize,
    pub healthy: usize,
    pub failed: usize,
    pub switched_accounts: usize,
}

pub(crate) fn list_proxy_pools() -> Result<Vec<ProxyPoolEntry>, String> {
    let storage = storage()?;
    let pools = storage
        .list_proxy_pools()
        .map_err(|err| format!("list proxy pools failed: {err}"))?;
    pools
        .into_iter()
        .map(|pool| proxy_pool_entry(&storage, pool))
        .collect()
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn create_proxy_pool(
    name: Option<String>,
    description: Option<String>,
    enabled: Option<bool>,
    failure_threshold: Option<i64>,
    recovery_threshold: Option<i64>,
    heartbeat_interval_secs: Option<i64>,
    cooldown_secs: Option<i64>,
) -> Result<ProxyPoolEntry, String> {
    let storage = storage()?;
    let pool = storage
        .create_proxy_pool(
            &generate_proxy_pool_id(),
            &required_text("name", name)?,
            optional_text(description).as_deref(),
            enabled.unwrap_or(true),
            normalize_threshold("failureThreshold", failure_threshold, 3)?,
            normalize_threshold("recoveryThreshold", recovery_threshold, 2)?,
            normalize_interval(heartbeat_interval_secs, 60, 30, 3600),
            normalize_interval(cooldown_secs, 300, 30, 86400),
        )
        .map_err(|err| format!("create proxy pool failed: {err}"))?;
    proxy_pool_entry(&storage, pool)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn update_proxy_pool(
    id: &str,
    name: Option<String>,
    description: Option<String>,
    enabled: Option<bool>,
    failure_threshold: Option<i64>,
    recovery_threshold: Option<i64>,
    heartbeat_interval_secs: Option<i64>,
    cooldown_secs: Option<i64>,
) -> Result<ProxyPoolEntry, String> {
    let storage = storage()?;
    let current = find_pool(&storage, id)?;
    let pool = storage
        .update_proxy_pool(
            &current.id,
            name.as_deref().unwrap_or(&current.name),
            description.as_deref().or(current.description.as_deref()),
            enabled.unwrap_or(current.enabled),
            normalize_threshold(
                "failureThreshold",
                failure_threshold,
                current.failure_threshold,
            )?,
            normalize_threshold(
                "recoveryThreshold",
                recovery_threshold,
                current.recovery_threshold,
            )?,
            normalize_interval(
                heartbeat_interval_secs,
                current.heartbeat_interval_secs,
                30,
                3600,
            ),
            normalize_interval(cooldown_secs, current.cooldown_secs, 30, 86400),
        )
        .map_err(|err| format!("update proxy pool failed: {err}"))?
        .ok_or_else(|| "proxy pool not found".to_string())?;
    proxy_pool_entry(&storage, pool)
}

pub(crate) fn delete_proxy_pool(id: &str) -> Result<(), String> {
    let storage = storage()?;
    let pool = find_pool(&storage, id)?;
    let bindings = storage
        .list_account_proxy_pool_bindings_by_pool(&pool.id)
        .map_err(|err| format!("read proxy pool bindings failed: {err}"))?;
    if !bindings.is_empty() {
        return Err(format!(
            "proxy pool is still bound to {} account(s); unbind them first",
            bindings.len()
        ));
    }
    if storage
        .delete_proxy_pool(&pool.id)
        .map_err(|err| format!("delete proxy pool failed: {err}"))?
    {
        Ok(())
    } else {
        Err("proxy pool not found".to_string())
    }
}

pub(crate) fn add_proxy_pool_members(
    pool_id: &str,
    proxy_profile_ids: Vec<String>,
) -> Result<ProxyPoolEntry, String> {
    let storage = storage()?;
    let pool = find_pool(&storage, pool_id)?;
    let mut next_order = storage
        .list_proxy_pool_members(&pool.id)
        .map_err(|err| format!("list proxy pool members failed: {err}"))?
        .into_iter()
        .map(|item| item.sort_order)
        .max()
        .unwrap_or(-1)
        + 1;
    for profile_id in proxy_profile_ids {
        let profile_id = profile_id.trim();
        if profile_id.is_empty() {
            continue;
        }
        storage
            .find_proxy_profile(profile_id)
            .map_err(|err| format!("read proxy profile failed: {err}"))?
            .ok_or_else(|| format!("proxy profile not found: {profile_id}"))?;
        storage
            .add_proxy_pool_member(&pool.id, profile_id, next_order)
            .map_err(|err| {
                format!(
                    "add proxy pool member failed for {profile_id}; the proxy may already belong to a pool: {err}"
                )
            })?;
        next_order += 1;
    }
    proxy_pool_entry(&storage, pool)
}

pub(crate) fn remove_proxy_pool_member(
    pool_id: &str,
    proxy_profile_id: &str,
) -> Result<ProxyPoolEntry, String> {
    let storage = storage()?;
    let pool = find_pool(&storage, pool_id)?;
    let active = storage
        .list_account_proxy_pool_bindings_by_current_profile(proxy_profile_id)
        .map_err(|err| format!("read active proxy bindings failed: {err}"))?
        .into_iter()
        .filter(|binding| binding.pool_id == pool.id)
        .count();
    if active > 0 {
        return Err(format!(
            "proxy is currently assigned to {active} account(s); switch or unbind them first"
        ));
    }
    if !storage
        .remove_proxy_pool_member(&pool.id, proxy_profile_id)
        .map_err(|err| format!("remove proxy pool member failed: {err}"))?
    {
        return Err("proxy pool member not found".to_string());
    }
    proxy_pool_entry(&storage, pool)
}

pub(crate) fn set_proxy_pool_member_enabled(
    pool_id: &str,
    proxy_profile_id: &str,
    enabled: bool,
) -> Result<ProxyPoolEntry, String> {
    let storage = storage()?;
    let pool = find_pool(&storage, pool_id)?;
    if !enabled {
        let active = storage
            .list_account_proxy_pool_bindings_by_current_profile(proxy_profile_id)
            .map_err(|err| format!("read active proxy bindings failed: {err}"))?
            .into_iter()
            .filter(|binding| binding.pool_id == pool.id)
            .count();
        if active > 0 {
            return Err(format!(
                "proxy is currently assigned to {active} account(s); switch or unbind them first"
            ));
        }
    }
    if !storage
        .set_proxy_pool_member_enabled(&pool.id, proxy_profile_id, enabled)
        .map_err(|err| format!("update proxy pool member failed: {err}"))?
    {
        return Err("proxy pool member not found".to_string());
    }
    proxy_pool_entry(&storage, pool)
}

pub(crate) fn import_proxy_profiles_batch(
    request: ProxyBatchImportRequest,
) -> Result<ProxyBatchImportResult, String> {
    let storage = storage()?;
    let pool = request
        .pool_id
        .as_deref()
        .map(|id| find_pool(&storage, id))
        .transpose()?;
    let default_scheme = normalize_import_scheme(request.default_scheme.as_deref())?;
    let name_prefix = optional_text(request.name_prefix)
        .or_else(|| pool.as_ref().map(|value| value.name.clone()))
        .unwrap_or_else(|| "Proxy".to_string());
    let existing = storage
        .list_proxy_profiles()
        .map_err(|err| format!("list proxy profiles failed: {err}"))?;
    let mut known_urls = existing
        .iter()
        .map(|profile| profile.proxy_url.trim().to_string())
        .collect::<std::collections::HashSet<_>>();
    let mut sort_order = if let Some(pool) = pool.as_ref() {
        storage
            .list_proxy_pool_members(&pool.id)
            .map_err(|err| format!("list proxy pool members failed: {err}"))?
            .into_iter()
            .map(|item| item.sort_order)
            .max()
            .unwrap_or(-1)
            + 1
    } else {
        0
    };

    let source_lines = request
        .text
        .lines()
        .enumerate()
        .filter_map(|(index, raw)| {
            let value = raw.trim();
            (!value.is_empty() && !value.starts_with('#')).then_some((index + 1, value))
        })
        .collect::<Vec<_>>();
    if source_lines.is_empty() {
        return Err("no proxy entries found".to_string());
    }

    let mut result = ProxyBatchImportResult {
        total: source_lines.len(),
        created: 0,
        duplicates: 0,
        failed: 0,
        items: Vec::with_capacity(source_lines.len()),
    };

    for (position, (line_number, raw)) in source_lines.into_iter().enumerate() {
        let (custom_name, raw_proxy) = split_import_name_and_proxy(raw);
        let name = custom_name
            .map(str::to_string)
            .unwrap_or_else(|| format!("{name_prefix} {:02}", position + 1));
        let normalized = normalize_import_proxy_url(raw_proxy, &default_scheme);
        let proxy_url = match normalized {
            Ok(value) => value,
            Err(error) => {
                result.failed += 1;
                result.items.push(ProxyBatchImportItemResult {
                    line: line_number,
                    status: "failed".to_string(),
                    name,
                    proxy_profile_id: None,
                    proxy_url_redacted: None,
                    error: Some(error),
                });
                continue;
            }
        };
        if !known_urls.insert(proxy_url.clone()) {
            result.duplicates += 1;
            result.items.push(ProxyBatchImportItemResult {
                line: line_number,
                status: "duplicate".to_string(),
                name,
                proxy_profile_id: None,
                proxy_url_redacted: Some(
                    codexmanager_core::storage::derive_proxy_profile_url_metadata(&proxy_url)
                        .proxy_url_redacted,
                ),
                error: None,
            });
            continue;
        }

        let profile_id = generate_proxy_profile_id();
        let created = storage.create_proxy_profile(&ProxyProfileCreateInput {
            id: profile_id.clone(),
            name: name.clone(),
            proxy_url: proxy_url.clone(),
            enabled: true,
            tags_json: None,
            notes: None,
        });
        let profile = match created {
            Ok(value) => value,
            Err(err) => {
                known_urls.remove(&proxy_url);
                result.failed += 1;
                result.items.push(ProxyBatchImportItemResult {
                    line: line_number,
                    status: "failed".to_string(),
                    name,
                    proxy_profile_id: None,
                    proxy_url_redacted: None,
                    error: Some(format!("create proxy profile failed: {err}")),
                });
                continue;
            }
        };
        if let Some(pool) = pool.as_ref() {
            if let Err(err) = storage.add_proxy_pool_member(&pool.id, &profile.id, sort_order) {
                let _ = storage.delete_proxy_profile(&profile.id);
                known_urls.remove(&proxy_url);
                result.failed += 1;
                result.items.push(ProxyBatchImportItemResult {
                    line: line_number,
                    status: "failed".to_string(),
                    name,
                    proxy_profile_id: None,
                    proxy_url_redacted: Some(profile.proxy_url_redacted),
                    error: Some(format!("add proxy to pool failed: {err}")),
                });
                continue;
            }
            sort_order += 1;
        }
        result.created += 1;
        result.items.push(ProxyBatchImportItemResult {
            line: line_number,
            status: "created".to_string(),
            name,
            proxy_profile_id: Some(profile.id),
            proxy_url_redacted: Some(profile.proxy_url_redacted),
            error: None,
        });
    }
    Ok(result)
}

pub(crate) fn bind_account_to_proxy_pool(
    account_id: &str,
    pool_id: &str,
) -> Result<ProxyPoolBindingEntry, String> {
    let storage = storage()?;
    let account_id = account_id.trim();
    if account_id.is_empty() {
        return Err("accountId is required".to_string());
    }
    storage
        .find_account_by_id(account_id)
        .map_err(|err| format!("read account failed: {err}"))?
        .ok_or_else(|| "account not found".to_string())?;
    let pool = find_pool(&storage, pool_id)?;
    if !pool.enabled {
        return Err("proxy pool is disabled".to_string());
    }
    let current = storage
        .find_account_proxy_pool_binding(account_id)
        .map_err(|err| format!("read account proxy pool binding failed: {err}"))?;
    let selected = select_pool_candidate(&storage, &pool, None)?;
    storage
        .upsert_account_proxy_pool_binding(
            account_id,
            &pool.id,
            Some(&selected.id),
            Some("initial_assignment"),
        )
        .map_err(|err| format!("store account proxy pool binding failed: {err}"))?;
    if let Err(err) =
        crate::account_proxy::set_account_proxy_profile_for_pool(account_id, &selected.id)
    {
        if let Some(previous) = current {
            let _ = storage.upsert_account_proxy_pool_binding(
                &previous.account_id,
                &previous.pool_id,
                previous.current_proxy_profile_id.as_deref(),
                previous.last_switch_reason.as_deref(),
            );
        } else {
            let _ = storage.delete_account_proxy_pool_binding(account_id);
        }
        return Err(err);
    }
    binding_entry(&storage, account_id)
}

pub(crate) fn list_proxy_pool_switch_logs(
    account_id: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<ProxyPoolSwitchLogEntry>, String> {
    let storage = storage()?;
    storage
        .list_proxy_pool_switch_logs(account_id, limit.unwrap_or(50))
        .map_err(|err| format!("list proxy pool switch logs failed: {err}"))
        .map(|items| {
            items
                .into_iter()
                .map(|item| ProxyPoolSwitchLogEntry {
                    id: item.id,
                    account_id: item.account_id,
                    pool_id: item.pool_id,
                    from_proxy_profile_id: item.from_proxy_profile_id,
                    to_proxy_profile_id: item.to_proxy_profile_id,
                    reason: item.reason,
                    created_at: item.created_at,
                })
                .collect()
        })
}

pub(crate) fn ensure_proxy_pool_health_monitor() {
    HEALTH_MONITOR_STARTED.get_or_init(|| {
        let _ = thread::Builder::new()
            .name("proxy-pool-health".to_string())
            .spawn(|| loop {
                if let Err(err) = run_proxy_pool_health_cycle() {
                    log::warn!("proxy pool health cycle failed: {err}");
                }
                thread::sleep(Duration::from_secs(HEALTH_MONITOR_TICK_SECS));
            });
    });
}

pub(crate) fn run_proxy_pool_health_cycle() -> Result<ProxyPoolHealthCycleResult, String> {
    run_proxy_pool_health_cycle_mode(false)
}

pub(crate) fn run_proxy_pool_health_cycle_now() -> Result<ProxyPoolHealthCycleResult, String> {
    run_proxy_pool_health_cycle_mode(true)
}

fn run_proxy_pool_health_cycle_mode(force_all: bool) -> Result<ProxyPoolHealthCycleResult, String> {
    let _cycle_guard = HEALTH_CYCLE_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let storage = storage()?;
    let now = now_ts();
    run_proxy_pool_health_cycle_with(
        &storage,
        now,
        force_all,
        |profile| {
            crate::account::proxy_health::check_account_proxy(&profile.proxy_url, |country_code| {
                storage
                    .find_cached_proxy_flag_by_country(country_code)
                    .unwrap_or(None)
            })
        },
        |account_id, profile_id| {
            crate::account_proxy::set_account_proxy_profile_for_pool(account_id, profile_id)
                .map(|_| ())
        },
    )
}

fn run_proxy_pool_health_cycle_with<Check, Assign>(
    storage: &codexmanager_core::storage::Storage,
    now: i64,
    force_all: bool,
    mut check: Check,
    mut assign: Assign,
) -> Result<ProxyPoolHealthCycleResult, String>
where
    Check: FnMut(&ProxyProfile) -> crate::account::proxy_health::ProxyHealthCheckResult,
    Assign: FnMut(&str, &str) -> Result<(), String>,
{
    let members = if force_all {
        storage
            .list_active_proxy_pool_members()
            .map_err(|err| format!("list active proxy pool members failed: {err}"))?
    } else {
        storage
            .list_due_proxy_pool_members(now)
            .map_err(|err| format!("list due proxy pool members failed: {err}"))?
    };
    let mut result = ProxyPoolHealthCycleResult {
        checked: 0,
        healthy: 0,
        failed: 0,
        switched_accounts: 0,
    };

    let max_checks = if force_all {
        usize::MAX
    } else {
        MAX_HEALTH_CHECKS_PER_TICK
    };
    for member in members.into_iter().take(max_checks) {
        let pool = match storage
            .find_proxy_pool(&member.pool_id)
            .map_err(|err| format!("read proxy pool failed: {err}"))?
        {
            Some(value) if value.enabled => value,
            _ => continue,
        };
        let profile = match storage
            .find_proxy_profile(&member.proxy_profile_id)
            .map_err(|err| format!("read proxy profile failed: {err}"))?
        {
            Some(value) if value.enabled => value,
            _ => continue,
        };
        let outcome = check(&profile);
        result.checked += 1;
        let succeeded = outcome.status == "ok";
        let health = next_member_health(&pool, &member, succeeded, now);
        storage
            .update_proxy_pool_member_health(
                &member.pool_id,
                &member.proxy_profile_id,
                health.status,
                health.failures,
                health.successes,
                health.cooldown_until,
                now,
                outcome.last_error.as_deref(),
            )
            .map_err(|err| format!("update proxy pool health failed: {err}"))?;
        update_profile_from_health_outcome(&storage, profile, &outcome)?;

        if succeeded {
            result.healthy += 1;
        } else {
            result.failed += 1;
        }
        if health.status == STATUS_UNHEALTHY && member.health_status != STATUS_UNHEALTHY {
            result.switched_accounts += switch_accounts_from_failed_member(
                &storage,
                &pool,
                &member.proxy_profile_id,
                outcome
                    .last_error
                    .as_deref()
                    .unwrap_or("proxy health check failed"),
                &mut assign,
            )?;
        } else if health.status == STATUS_HEALTHY && member.health_status != STATUS_HEALTHY {
            result.switched_accounts +=
                reconcile_stranded_pool_bindings(storage, &pool, &mut assign)?;
        }
    }
    Ok(result)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MemberHealthUpdate {
    status: &'static str,
    failures: i64,
    successes: i64,
    cooldown_until: Option<i64>,
}

fn next_member_health(
    pool: &ProxyPool,
    member: &ProxyPoolMember,
    succeeded: bool,
    now: i64,
) -> MemberHealthUpdate {
    if succeeded {
        let was_recovering = matches!(
            member.health_status.as_str(),
            STATUS_UNHEALTHY | STATUS_RECOVERING
        );
        let successes = if was_recovering {
            member.consecutive_successes.saturating_add(1)
        } else {
            pool.recovery_threshold
        };
        let recovered = !was_recovering || successes >= pool.recovery_threshold;
        MemberHealthUpdate {
            status: if recovered {
                STATUS_HEALTHY
            } else {
                STATUS_RECOVERING
            },
            failures: 0,
            successes,
            cooldown_until: if recovered {
                None
            } else {
                member.cooldown_until
            },
        }
    } else {
        let failures = member.consecutive_failures.saturating_add(1);
        let unhealthy = failures >= pool.failure_threshold;
        MemberHealthUpdate {
            status: if unhealthy {
                STATUS_UNHEALTHY
            } else {
                STATUS_SUSPECT
            },
            failures,
            successes: 0,
            cooldown_until: unhealthy.then_some(now.saturating_add(pool.cooldown_secs)),
        }
    }
}

fn switch_accounts_from_failed_member<Assign>(
    storage: &codexmanager_core::storage::Storage,
    pool: &ProxyPool,
    failed_profile_id: &str,
    error: &str,
    assign: &mut Assign,
) -> Result<usize, String>
where
    Assign: FnMut(&str, &str) -> Result<(), String>,
{
    let bindings = storage
        .list_account_proxy_pool_bindings_by_current_profile(failed_profile_id)
        .map_err(|err| format!("list failed proxy bindings failed: {err}"))?;
    let mut switched = 0;
    for binding in bindings.into_iter().filter(|item| item.pool_id == pool.id) {
        let candidate = select_pool_candidate(storage, pool, Some(failed_profile_id));
        match candidate {
            Ok(profile) => {
                let reason = format!("health_check_failed: {error}");
                assign(&binding.account_id, &profile.id)?;
                storage
                    .switch_account_proxy_pool_binding(
                        &binding.account_id,
                        Some(&profile.id),
                        &reason,
                    )
                    .map_err(|err| format!("switch proxy pool binding failed: {err}"))?;
                switched += 1;
            }
            Err(_) => {
                let reason = format!("pool_unavailable: {error}");
                storage
                    .switch_account_proxy_pool_binding(
                        &binding.account_id,
                        Some(failed_profile_id),
                        &reason,
                    )
                    .map_err(|err| format!("mark proxy pool unavailable failed: {err}"))?;
            }
        }
    }
    Ok(switched)
}

fn reconcile_stranded_pool_bindings<Assign>(
    storage: &codexmanager_core::storage::Storage,
    pool: &ProxyPool,
    assign: &mut Assign,
) -> Result<usize, String>
where
    Assign: FnMut(&str, &str) -> Result<(), String>,
{
    let bindings = storage
        .list_account_proxy_pool_bindings_by_pool(&pool.id)
        .map_err(|err| format!("list proxy pool bindings failed: {err}"))?;
    let mut switched = 0;
    for binding in bindings {
        let current_id = binding.current_proxy_profile_id.as_deref();
        if current_id.is_some_and(|id| pool_member_is_currently_selectable(storage, pool, id)) {
            continue;
        }
        let Ok(profile) = select_pool_candidate(storage, pool, current_id) else {
            continue;
        };
        assign(&binding.account_id, &profile.id)?;
        storage
            .switch_account_proxy_pool_binding(
                &binding.account_id,
                Some(&profile.id),
                "pool_recovered",
            )
            .map_err(|err| format!("reconcile proxy pool binding failed: {err}"))?;
        switched += 1;
    }
    Ok(switched)
}

fn pool_member_is_currently_selectable(
    storage: &codexmanager_core::storage::Storage,
    pool: &ProxyPool,
    profile_id: &str,
) -> bool {
    let Ok(Some(member)) = storage.find_proxy_pool_member(&pool.id, profile_id) else {
        return false;
    };
    if !member.enabled || !member_is_selectable(&member) {
        return false;
    }
    storage
        .find_proxy_profile(profile_id)
        .ok()
        .flatten()
        .is_some_and(|profile| profile.enabled)
}

fn select_pool_candidate(
    storage: &codexmanager_core::storage::Storage,
    pool: &ProxyPool,
    exclude_profile_id: Option<&str>,
) -> Result<ProxyProfile, String> {
    let members = storage
        .list_proxy_pool_members(&pool.id)
        .map_err(|err| format!("list proxy pool members failed: {err}"))?;
    let mut candidates = Vec::new();
    for member in members {
        if !member.enabled || exclude_profile_id == Some(member.proxy_profile_id.as_str()) {
            continue;
        }
        if !member_is_selectable(&member) {
            continue;
        }
        let Some(profile) = storage
            .find_proxy_profile(&member.proxy_profile_id)
            .map_err(|err| format!("read proxy profile failed: {err}"))?
        else {
            continue;
        };
        if !profile.enabled {
            continue;
        }
        let count = storage
            .count_proxy_pool_current_bindings(&profile.id)
            .map_err(|err| format!("count proxy assignments failed: {err}"))?;
        candidates.push((
            member_health_rank(&member),
            count,
            member.sort_order,
            profile,
        ));
    }
    candidates.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| left.1.cmp(&right.1))
            .then_with(|| left.2.cmp(&right.2))
            .then_with(|| {
                left.3
                    .last_url_latency_ms
                    .unwrap_or(i64::MAX)
                    .cmp(&right.3.last_url_latency_ms.unwrap_or(i64::MAX))
            })
            .then_with(|| left.3.id.cmp(&right.3.id))
    });
    candidates
        .into_iter()
        .next()
        .map(|(_, _, _, profile)| profile)
        .ok_or_else(|| "proxy pool has no available proxy".to_string())
}

fn member_health_rank(member: &ProxyPoolMember) -> u8 {
    match member.health_status.as_str() {
        STATUS_HEALTHY => 0,
        "unchecked" => 1,
        STATUS_SUSPECT => 2,
        _ => 3,
    }
}

fn member_is_selectable(member: &ProxyPoolMember) -> bool {
    match member.health_status.as_str() {
        STATUS_UNHEALTHY | STATUS_RECOVERING => false,
        _ => true,
    }
}

fn update_profile_from_health_outcome(
    storage: &codexmanager_core::storage::Storage,
    profile: ProxyProfile,
    outcome: &crate::account::proxy_health::ProxyHealthCheckResult,
) -> Result<(), String> {
    let geo = outcome.geo.as_ref();
    storage
        .update_proxy_profile(&ProxyProfileUpdateInput {
            id: profile.id,
            name: None,
            proxy_url: None,
            enabled: None,
            status: Some(outcome.status.to_string()),
            last_error: Some(outcome.last_error.clone().unwrap_or_default()),
            last_url_latency_ms: outcome.latency_ms,
            last_download_mbps: None,
            last_upload_mbps: None,
            last_tested_at: Some(now_ts()),
            ip: geo.and_then(|value| value.ip.clone()),
            country_code: geo.and_then(|value| value.country_code.clone()),
            country_name: geo.and_then(|value| value.country_name.clone()),
            region_name: geo.and_then(|value| value.region_name.clone()),
            city_name: geo.and_then(|value| value.city_name.clone()),
            asn: geo.and_then(|value| value.asn),
            as_org: geo.and_then(|value| value.as_org.clone()),
            isp: geo.and_then(|value| value.isp.clone()),
            as_domain: geo.and_then(|value| value.as_domain.clone()),
            flag_img_url: geo.and_then(|value| value.flag_img_url.clone()),
            flag_emoji: geo.and_then(|value| value.flag_emoji.clone()),
            timezone_id: geo.and_then(|value| value.timezone_id.clone()),
            timezone_offset: geo.and_then(|value| value.timezone_offset),
            timezone_utc: geo.and_then(|value| value.timezone_utc.clone()),
            tags_json: None,
            notes: None,
        })
        .map_err(|err| format!("update proxy profile health failed: {err}"))?;
    Ok(())
}

fn proxy_pool_entry(
    storage: &codexmanager_core::storage::Storage,
    pool: ProxyPool,
) -> Result<ProxyPoolEntry, String> {
    let bindings = storage
        .list_account_proxy_pool_bindings_by_pool(&pool.id)
        .map_err(|err| format!("list proxy pool bindings failed: {err}"))?;
    let members = storage
        .list_proxy_pool_members(&pool.id)
        .map_err(|err| format!("list proxy pool members failed: {err}"))?;
    let mut entries = Vec::new();
    for member in members {
        let Some(profile) = storage
            .find_proxy_profile(&member.proxy_profile_id)
            .map_err(|err| format!("read proxy profile failed: {err}"))?
        else {
            continue;
        };
        entries.push(ProxyPoolMemberEntry {
            current_accounts_count: storage
                .count_proxy_pool_current_bindings(&profile.id)
                .unwrap_or(0),
            proxy_profile_id: profile.id,
            proxy_profile_name: profile.name,
            proxy_url_redacted: profile.proxy_url_redacted,
            sort_order: member.sort_order,
            enabled: member.enabled,
            profile_enabled: profile.enabled,
            health_status: member.health_status,
            consecutive_failures: member.consecutive_failures,
            consecutive_successes: member.consecutive_successes,
            cooldown_until: member.cooldown_until,
            last_check_at: member.last_check_at,
            last_error: member.last_error,
            latency_ms: profile.last_url_latency_ms,
        });
    }
    let healthy_members_count = entries
        .iter()
        .filter(|item| item.enabled && item.profile_enabled && item.health_status == STATUS_HEALTHY)
        .count() as i64;
    Ok(ProxyPoolEntry {
        id: pool.id,
        name: pool.name,
        description: pool.description,
        enabled: pool.enabled,
        failure_threshold: pool.failure_threshold,
        recovery_threshold: pool.recovery_threshold,
        heartbeat_interval_secs: pool.heartbeat_interval_secs,
        cooldown_secs: pool.cooldown_secs,
        accounts_count: bindings.len() as i64,
        healthy_members_count,
        members: entries,
        created_at: pool.created_at,
        updated_at: pool.updated_at,
    })
}

fn binding_entry(
    storage: &codexmanager_core::storage::Storage,
    account_id: &str,
) -> Result<ProxyPoolBindingEntry, String> {
    let binding = storage
        .find_account_proxy_pool_binding(account_id)
        .map_err(|err| format!("read account proxy pool binding failed: {err}"))?
        .ok_or_else(|| "account proxy pool binding not found".to_string())?;
    let pool = find_pool(storage, &binding.pool_id)?;
    let profile = binding
        .current_proxy_profile_id
        .as_deref()
        .map(|id| storage.find_proxy_profile(id))
        .transpose()
        .map_err(|err| format!("read current proxy profile failed: {err}"))?
        .flatten();
    Ok(ProxyPoolBindingEntry {
        account_id: binding.account_id,
        pool_id: pool.id,
        pool_name: pool.name,
        current_proxy_profile_id: binding.current_proxy_profile_id,
        current_proxy_profile_name: profile.map(|item| item.name),
        assigned_at: binding.assigned_at,
        last_switched_at: binding.last_switched_at,
        last_switch_reason: binding.last_switch_reason,
    })
}

fn storage() -> Result<crate::storage_helpers::StorageHandle, String> {
    open_storage().ok_or_else(|| "storage unavailable".to_string())
}

fn find_pool(storage: &codexmanager_core::storage::Storage, id: &str) -> Result<ProxyPool, String> {
    let id = id.trim();
    if id.is_empty() {
        return Err("poolId is required".to_string());
    }
    storage
        .find_proxy_pool(id)
        .map_err(|err| format!("read proxy pool failed: {err}"))?
        .ok_or_else(|| "proxy pool not found".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use codexmanager_core::storage::{Account, ProxyProfileCreateInput, Storage};

    fn account(id: &str) -> Account {
        Account {
            id: id.to_string(),
            label: id.to_string(),
            issuer: "chatgpt".to_string(),
            chatgpt_account_id: None,
            workspace_id: None,
            group_name: None,
            sort: 0,
            status: "active".to_string(),
            created_at: 1,
            updated_at: 1,
        }
    }

    fn profile(storage: &Storage, id: &str, port: u16) -> ProxyProfile {
        storage
            .create_proxy_profile(&ProxyProfileCreateInput {
                id: id.to_string(),
                name: id.to_string(),
                proxy_url: format!("socks5://127.0.0.1:{port}"),
                enabled: true,
                tags_json: None,
                notes: None,
            })
            .expect("create proxy profile")
    }

    fn set_health(storage: &Storage, pool_id: &str, profile_id: &str, status: &str) {
        storage
            .update_proxy_pool_member_health(
                pool_id,
                profile_id,
                status,
                if status == STATUS_UNHEALTHY { 3 } else { 0 },
                if status == STATUS_HEALTHY { 2 } else { 0 },
                None,
                100,
                None,
            )
            .expect("update member health");
    }

    fn member(status: &str, failures: i64, successes: i64) -> ProxyPoolMember {
        ProxyPoolMember {
            pool_id: "pool".to_string(),
            proxy_profile_id: "proxy".to_string(),
            sort_order: 0,
            enabled: true,
            health_status: status.to_string(),
            consecutive_failures: failures,
            consecutive_successes: successes,
            cooldown_until: None,
            last_check_at: None,
            last_error: None,
            created_at: 1,
            updated_at: 1,
        }
    }

    #[test]
    fn health_transition_honors_failure_and_recovery_thresholds() {
        let pool = ProxyPool {
            id: "pool".to_string(),
            name: "US".to_string(),
            description: None,
            enabled: true,
            failure_threshold: 3,
            recovery_threshold: 2,
            heartbeat_interval_secs: 60,
            cooldown_secs: 300,
            created_at: 1,
            updated_at: 1,
        };

        let first_failure = next_member_health(&pool, &member(STATUS_HEALTHY, 0, 2), false, 100);
        assert_eq!(first_failure.status, STATUS_SUSPECT);
        assert_eq!(first_failure.failures, 1);
        let threshold_failure =
            next_member_health(&pool, &member(STATUS_SUSPECT, 2, 0), false, 100);
        assert_eq!(threshold_failure.status, STATUS_UNHEALTHY);
        assert_eq!(threshold_failure.cooldown_until, Some(400));

        let first_recovery = next_member_health(&pool, &member(STATUS_UNHEALTHY, 3, 0), true, 500);
        assert_eq!(first_recovery.status, STATUS_RECOVERING);
        let recovered = next_member_health(&pool, &member(STATUS_RECOVERING, 0, 1), true, 560);
        assert_eq!(recovered.status, STATUS_HEALTHY);
    }

    #[test]
    fn manual_health_cycle_checks_all_active_members_while_background_waits_until_due() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        let first = profile(&storage, "us-1", 7001);
        let second = profile(&storage, "us-2", 7002);
        let pool = storage
            .create_proxy_pool("pool-us", "US", None, true, 3, 2, 60, 300)
            .expect("create pool");
        storage
            .add_proxy_pool_member(&pool.id, &first.id, 0)
            .unwrap();
        storage
            .add_proxy_pool_member(&pool.id, &second.id, 1)
            .unwrap();
        set_health(&storage, &pool.id, &first.id, STATUS_HEALTHY);
        set_health(&storage, &pool.id, &second.id, STATUS_HEALTHY);

        let background = run_proxy_pool_health_cycle_with(
            &storage,
            101,
            false,
            |_profile| panic!("background cycle must not check members before they are due"),
            |_account_id, _profile_id| Ok(()),
        )
        .expect("run background cycle");
        assert_eq!(background.checked, 0);

        let manual = run_proxy_pool_health_cycle_with(
            &storage,
            101,
            true,
            |_profile| crate::account::proxy_health::ProxyHealthCheckResult {
                status: "ok",
                latency_ms: Some(1),
                last_error: None,
                geo: None,
            },
            |_account_id, _profile_id| Ok(()),
        )
        .expect("run manual cycle");
        assert_eq!(manual.checked, 2);
        assert_eq!(manual.healthy, 2);
    }

    #[test]
    fn failed_member_switches_accounts_only_within_its_pool() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        storage
            .insert_account(&account("account-1"))
            .expect("insert account");
        let first = profile(&storage, "us-1", 7001);
        let second = profile(&storage, "us-2", 7002);
        let other = profile(&storage, "de-1", 7003);
        let us_pool = storage
            .create_proxy_pool("pool-us", "US", None, true, 3, 2, 60, 300)
            .expect("create US pool");
        let de_pool = storage
            .create_proxy_pool("pool-de", "DE", None, true, 3, 2, 60, 300)
            .expect("create DE pool");
        storage
            .add_proxy_pool_member(&us_pool.id, &first.id, 0)
            .unwrap();
        storage
            .add_proxy_pool_member(&us_pool.id, &second.id, 1)
            .unwrap();
        storage
            .add_proxy_pool_member(&de_pool.id, &other.id, 0)
            .unwrap();
        set_health(&storage, &us_pool.id, &first.id, STATUS_UNHEALTHY);
        set_health(&storage, &us_pool.id, &second.id, STATUS_HEALTHY);
        set_health(&storage, &de_pool.id, &other.id, STATUS_HEALTHY);
        storage
            .upsert_account_proxy_pool_binding(
                "account-1",
                &us_pool.id,
                Some(&first.id),
                Some("initial_assignment"),
            )
            .unwrap();

        let mut assignments = Vec::new();
        let switched = switch_accounts_from_failed_member(
            &storage,
            &us_pool,
            &first.id,
            "TLS failed",
            &mut |account_id, profile_id| {
                assignments.push((account_id.to_string(), profile_id.to_string()));
                Ok(())
            },
        )
        .expect("switch failed member");

        assert_eq!(switched, 1);
        assert_eq!(
            assignments,
            vec![("account-1".to_string(), second.id.clone())]
        );
        let binding = storage
            .find_account_proxy_pool_binding("account-1")
            .unwrap()
            .unwrap();
        assert_eq!(binding.pool_id, us_pool.id);
        assert_eq!(
            binding.current_proxy_profile_id.as_deref(),
            Some(second.id.as_str())
        );
    }

    #[test]
    fn unavailable_pool_fails_closed_then_reconciles_without_switchback() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        storage
            .insert_account(&account("account-1"))
            .expect("insert account");
        let first = profile(&storage, "us-1", 7101);
        let second = profile(&storage, "us-2", 7102);
        let pool = storage
            .create_proxy_pool("pool-us", "US", None, true, 3, 2, 60, 300)
            .expect("create pool");
        storage
            .add_proxy_pool_member(&pool.id, &first.id, 0)
            .unwrap();
        storage
            .add_proxy_pool_member(&pool.id, &second.id, 1)
            .unwrap();
        set_health(&storage, &pool.id, &first.id, STATUS_UNHEALTHY);
        set_health(&storage, &pool.id, &second.id, STATUS_UNHEALTHY);
        storage
            .upsert_account_proxy_pool_binding(
                "account-1",
                &pool.id,
                Some(&first.id),
                Some("initial_assignment"),
            )
            .unwrap();

        let mut assignments = Vec::new();
        let switched = switch_accounts_from_failed_member(
            &storage,
            &pool,
            &first.id,
            "all failed",
            &mut |account_id, profile_id| {
                assignments.push((account_id.to_string(), profile_id.to_string()));
                Ok(())
            },
        )
        .expect("mark pool unavailable");
        assert_eq!(switched, 0);
        assert!(assignments.is_empty());
        let unavailable = storage
            .find_account_proxy_pool_binding("account-1")
            .unwrap()
            .unwrap();
        assert_eq!(
            unavailable.current_proxy_profile_id.as_deref(),
            Some(first.id.as_str())
        );
        assert!(unavailable
            .last_switch_reason
            .as_deref()
            .unwrap_or_default()
            .starts_with("pool_unavailable:"));

        set_health(&storage, &pool.id, &second.id, STATUS_HEALTHY);
        let reconciled =
            reconcile_stranded_pool_bindings(&storage, &pool, &mut |account_id, profile_id| {
                assignments.push((account_id.to_string(), profile_id.to_string()));
                Ok(())
            })
            .expect("reconcile recovered pool");
        assert_eq!(reconciled, 1);
        assert_eq!(
            assignments,
            vec![("account-1".to_string(), second.id.clone())]
        );

        let no_switchback =
            reconcile_stranded_pool_bindings(&storage, &pool, &mut |_account_id, _profile_id| {
                panic!("healthy binding must stay in place")
            })
            .expect("leave healthy binding unchanged");
        assert_eq!(no_switchback, 0);
    }
}

fn required_text(field: &str, value: Option<String>) -> Result<String, String> {
    optional_text(value).ok_or_else(|| format!("{field} is required"))
}

fn optional_text(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn normalize_threshold(field: &str, value: Option<i64>, default: i64) -> Result<i64, String> {
    let value = value.unwrap_or(default);
    if (1..=10).contains(&value) {
        Ok(value)
    } else {
        Err(format!("{field} must be between 1 and 10"))
    }
}

fn normalize_interval(value: Option<i64>, default: i64, min: i64, max: i64) -> i64 {
    value.unwrap_or(default).clamp(min, max)
}

fn normalize_import_scheme(value: Option<&str>) -> Result<String, String> {
    let scheme = value.unwrap_or("socks5").trim().to_ascii_lowercase();
    match scheme.as_str() {
        "http" | "https" | "socks4" | "socks5" | "socks5h" => Ok(scheme),
        _ => Err("defaultScheme must be http, https, socks4, socks5, or socks5h".to_string()),
    }
}

fn split_import_name_and_proxy(value: &str) -> (Option<&str>, &str) {
    if let Some((name, proxy)) = value.split_once(',') {
        let name = name.trim();
        let proxy = proxy.trim();
        if !name.is_empty() && !proxy.is_empty() {
            return (Some(name), proxy);
        }
    }
    (None, value.trim())
}

fn normalize_import_proxy_url(value: &str, default_scheme: &str) -> Result<String, String> {
    let value = value.trim();
    if value.contains("://") {
        return crate::proxy_registry::validation::normalize_proxy_profile_url(value);
    }
    let parts = value.split(':').collect::<Vec<_>>();
    let candidate = match parts.as_slice() {
        [host, port] if !host.trim().is_empty() && port.parse::<u16>().is_ok() => {
            format!("{default_scheme}://{}:{}", host.trim(), port.trim())
        }
        [host, port, username, password]
            if !host.trim().is_empty()
                && port.parse::<u16>().is_ok()
                && !username.trim().is_empty() =>
        {
            format!(
                "{default_scheme}://{}:{}@{}:{}",
                urlencoding::encode(username.trim()),
                urlencoding::encode(password.trim()),
                host.trim(),
                port.trim()
            )
        }
        _ => {
            return Err(
                "unsupported proxy format; use URL, host:port, or host:port:user:password"
                    .to_string(),
            )
        }
    };
    crate::proxy_registry::validation::normalize_proxy_profile_url(&candidate)
}
