use rusqlite::{params, OptionalExtension, Result, Transaction};
use serde::{Deserialize, Serialize};

use super::{now_ts, Connection, Storage};

const HARDENING_MIGRATION_VERSION: &str = "113_model_billing_v2_hardening";
const CANONICAL_TOKEN_ACCOUNTING_MIGRATION_VERSION: &str =
    "xuanhen_canonical_token_accounting_v1";
const ACTUAL_USAGE_BILLING_MIGRATION_VERSION: &str = "codexmanager_actual_usage_billing_v1";

#[derive(Debug)]
struct EstimatedHourlyContribution {
    bucket_start: i64,
    key_id: String,
    account_id: String,
    model: String,
    actual_source_kind: String,
    actual_source_id: String,
    owner_user_id: String,
    input_tokens: i64,
    cached_input_tokens: i64,
    output_tokens: i64,
    total_tokens: i64,
    reasoning_output_tokens: i64,
    estimated_cost_microusd: i64,
    request_count: i64,
    success_count: i64,
    error_count: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModelPriceTierV2 {
    #[serde(alias = "min_input_tokens")]
    pub min_input_tokens: i64,
    #[serde(alias = "input_microusd_per_1m")]
    pub input_microusd_per_1m: i64,
    #[serde(alias = "cached_input_microusd_per_1m")]
    pub cached_input_microusd_per_1m: i64,
    #[serde(default, alias = "cache_creation_microusd_per_1m")]
    pub cache_creation_microusd_per_1m: i64,
    #[serde(alias = "output_microusd_per_1m")]
    pub output_microusd_per_1m: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChargeComputationV2 {
    pub uncached_input_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    pub numerator: i128,
    pub base_cost_microusd: i64,
    pub charged_cost_microusd: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChargeSnapshotInputV2 {
    pub request_log_id: i64,
    pub model_slug: String,
    pub usage_source: String,
    pub input_tokens: i64,
    pub cached_input_tokens: i64,
    #[serde(default)]
    pub cache_creation_input_tokens: i64,
    pub output_tokens: i64,
    #[serde(default)]
    pub reasoning_output_tokens: i64,
    #[serde(default)]
    pub unclassified_tokens: i64,
    #[serde(default = "default_usage_quality")]
    pub usage_quality: String,
    pub rate_multiplier_millis: i64,
    #[serde(default)]
    pub wallet_id: Option<String>,
    #[serde(default)]
    pub api_key_id: Option<String>,
    #[serde(default)]
    pub pricing_rule_id: Option<String>,
    #[serde(default)]
    pub raw_usage_json: Option<String>,
    #[serde(default)]
    pub ledger_note: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChargeSnapshotV2 {
    pub request_log_id: i64,
    pub model_id: Option<String>,
    pub model_slug: String,
    pub tier_min_input_tokens: i64,
    pub usage_source: String,
    pub input_tokens: i64,
    pub cached_input_tokens: i64,
    pub cache_creation_input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_output_tokens: i64,
    pub unclassified_tokens: i64,
    pub input_microusd_per_1m: i64,
    pub cached_input_microusd_per_1m: i64,
    pub cache_creation_microusd_per_1m: i64,
    pub output_microusd_per_1m: i64,
    pub usage_quality: String,
    pub rate_multiplier_millis: i64,
    pub base_cost_microusd: i64,
    pub charged_cost_microusd: i64,
    pub currency: String,
    pub created_at: i64,
}

fn default_usage_quality() -> String {
    "complete".to_string()
}

fn checked_i64(value: i128, label: &str) -> Result<i64> {
    i64::try_from(value).map_err(|_| {
        rusqlite::Error::InvalidParameterName(format!("{label} exceeds SQLite INTEGER range"))
    })
}

fn ceil_div(value: i128, divisor: i128) -> i128 {
    if value <= 0 {
        0
    } else {
        value / divisor + i128::from(value % divisor != 0)
    }
}

pub fn compute_charge_v2(
    input_tokens: i64,
    cached_input_tokens: i64,
    cache_creation_input_tokens: i64,
    output_tokens: i64,
    tier: &ModelPriceTierV2,
    rate_multiplier_millis: i64,
) -> Result<ChargeComputationV2> {
    if input_tokens < 0
        || cached_input_tokens < 0
        || cache_creation_input_tokens < 0
        || output_tokens < 0
        || tier.input_microusd_per_1m < 0
        || tier.cached_input_microusd_per_1m < 0
        || tier.cache_creation_microusd_per_1m < 0
        || tier.output_microusd_per_1m < 0
        || rate_multiplier_millis < 0
    {
        return Err(rusqlite::Error::InvalidParameterName(
            "tokens, rates, and multiplier must be non-negative".to_string(),
        ));
    }
    let cached_tokens_for_charge = cached_input_tokens.min(input_tokens);
    let cache_creation_tokens_for_charge = cache_creation_input_tokens
        .min(input_tokens.saturating_sub(cached_tokens_for_charge));
    let uncached_input_tokens = input_tokens
        .saturating_sub(cached_tokens_for_charge)
        .saturating_sub(cache_creation_tokens_for_charge)
        .max(0);
    let input_part = i128::from(uncached_input_tokens)
        .checked_mul(i128::from(tier.input_microusd_per_1m))
        .ok_or_else(|| {
            rusqlite::Error::InvalidParameterName("input charge overflow".to_string())
        })?;
    let cached_part = i128::from(cached_tokens_for_charge)
        .checked_mul(i128::from(tier.cached_input_microusd_per_1m))
        .ok_or_else(|| {
            rusqlite::Error::InvalidParameterName("cached charge overflow".to_string())
        })?;
    let cache_creation_part = i128::from(cache_creation_tokens_for_charge)
        .checked_mul(i128::from(tier.cache_creation_microusd_per_1m))
        .ok_or_else(|| {
            rusqlite::Error::InvalidParameterName("cache creation charge overflow".to_string())
        })?;
    let output_part = i128::from(output_tokens)
        .checked_mul(i128::from(tier.output_microusd_per_1m))
        .ok_or_else(|| {
            rusqlite::Error::InvalidParameterName("output charge overflow".to_string())
        })?;
    let numerator = input_part
        .checked_add(cached_part)
        .and_then(|value| value.checked_add(cache_creation_part))
        .and_then(|value| value.checked_add(output_part))
        .ok_or_else(|| rusqlite::Error::InvalidParameterName("charge overflow".to_string()))?;
    let charged_numerator = numerator
        .checked_mul(i128::from(rate_multiplier_millis))
        .ok_or_else(|| {
            rusqlite::Error::InvalidParameterName("multiplied charge overflow".to_string())
        })?;
    Ok(ChargeComputationV2 {
        uncached_input_tokens,
        cache_read_tokens: cached_tokens_for_charge,
        cache_creation_tokens: cache_creation_tokens_for_charge,
        numerator,
        base_cost_microusd: checked_i64(ceil_div(numerator, 1_000_000), "base cost")?,
        charged_cost_microusd: checked_i64(
            ceil_div(charged_numerator, 1_000_000_000),
            "charged cost",
        )?,
    })
}

fn map_snapshot(row: &rusqlite::Row<'_>) -> Result<ChargeSnapshotV2> {
    Ok(ChargeSnapshotV2 {
        request_log_id: row.get(0)?,
        model_id: row.get(1)?,
        model_slug: row.get(2)?,
        tier_min_input_tokens: row.get(3)?,
        usage_source: row.get(4)?,
        input_tokens: row.get(5)?,
        cached_input_tokens: row.get(6)?,
        cache_creation_input_tokens: row.get(7)?,
        output_tokens: row.get(8)?,
        reasoning_output_tokens: row.get(9)?,
        unclassified_tokens: row.get(10)?,
        input_microusd_per_1m: row.get(11)?,
        cached_input_microusd_per_1m: row.get(12)?,
        cache_creation_microusd_per_1m: row.get(13)?,
        output_microusd_per_1m: row.get(14)?,
        usage_quality: row.get(15)?,
        rate_multiplier_millis: row.get(16)?,
        base_cost_microusd: row.get(17)?,
        charged_cost_microusd: row.get(18)?,
        currency: row.get(19)?,
        created_at: row.get(20)?,
    })
}

const SNAPSHOT_SELECT: &str = "SELECT request_log_id,model_id,model_slug,tier_min_input_tokens,
    usage_source,input_tokens,cached_input_tokens,cache_creation_input_tokens,output_tokens,
    reasoning_output_tokens,unclassified_tokens,
    input_microusd_per_1m,cached_input_microusd_per_1m,cache_creation_microusd_per_1m,
    output_microusd_per_1m,usage_quality,rate_multiplier_millis,
    base_cost_microusd,charged_cost_microusd,currency,created_at
  FROM request_charge_snapshots";

impl Storage {
    pub(super) fn apply_canonical_token_accounting_migration(&self) -> Result<()> {
        if self.has_migration(CANONICAL_TOKEN_ACCOUNTING_MIGRATION_VERSION)? {
            return Ok(());
        }

        self.ensure_column(
            "model_prices",
            "cache_creation_microusd_per_1m",
            "INTEGER",
        )?;
        self.ensure_column(
            "model_price_tiers",
            "cache_creation_microusd_per_1m",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        self.ensure_column(
            "request_charge_snapshots",
            "cache_creation_input_tokens",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        self.ensure_column(
            "request_charge_snapshots",
            "cache_creation_microusd_per_1m",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        self.ensure_column(
            "request_charge_snapshots",
            "reasoning_output_tokens",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        self.ensure_column(
            "request_charge_snapshots",
            "unclassified_tokens",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        self.ensure_column(
            "request_charge_snapshots",
            "usage_quality",
            "TEXT NOT NULL DEFAULT 'complete'",
        )?;
        for table in [
            "request_token_stats",
            "request_token_stat_rollups",
            "request_token_stat_hourly_rollups",
        ] {
            self.ensure_column(
                table,
                "cache_creation_input_tokens",
                "INTEGER NOT NULL DEFAULT 0",
            )?;
            self.ensure_column(
                table,
                "unclassified_tokens",
                "INTEGER NOT NULL DEFAULT 0",
            )?;
        }
        self.ensure_column(
            "request_token_stats",
            "usage_quality",
            "TEXT NOT NULL DEFAULT 'complete'",
        )?;

        let tx = self.conn.unchecked_transaction()?;
        // Existing catalogs predate an explicit cache-write bucket. Charging
        // those writes as ordinary input preserves the old behavior until an
        // administrator configures a provider-specific cache-write price.
        tx.execute(
            "UPDATE model_prices
             SET cache_creation_microusd_per_1m=input_microusd_per_1m
             WHERE price_status<>'missing'",
            [],
        )?;
        tx.execute(
            "UPDATE model_price_tiers
             SET cache_creation_microusd_per_1m=input_microusd_per_1m",
            [],
        )?;
        tx.execute(
            "UPDATE request_token_stats
             SET usage_quality=CASE
                 WHEN usage_source='actual' THEN 'complete'
                 ELSE 'unclassified'
             END
             WHERE usage_quality IS NULL
                OR TRIM(usage_quality)=''
                OR usage_quality NOT IN ('complete','inconsistent','unclassified')",
            [],
        )?;
        tx.execute(
            "INSERT INTO schema_migrations(version,applied_at) VALUES(?1,?2)",
            params![CANONICAL_TOKEN_ACCOUNTING_MIGRATION_VERSION, now_ts()],
        )?;
        tx.commit()?;
        if let Some(migrations) = self.applied_migrations.borrow_mut().as_mut() {
            migrations.insert(CANONICAL_TOKEN_ACCOUNTING_MIGRATION_VERSION.to_string());
        }
        Ok(())
    }

    fn load_estimated_hourly_contributions(
        conn: &Connection,
    ) -> Result<Vec<EstimatedHourlyContribution>> {
        let mut stmt = conn.prepare(
                "WITH estimated_requests AS (
                    SELECT
                        CAST(COALESCE(t.created_at, r.created_at, s.created_at) / 3600 AS INTEGER) * 3600 AS bucket_start,
                        COALESCE(NULLIF(TRIM(t.key_id), ''), NULLIF(TRIM(r.key_id), ''), '') AS key_id,
                        COALESCE(NULLIF(TRIM(t.account_id), ''), NULLIF(TRIM(r.account_id), ''), '') AS account_id,
                        COALESCE(NULLIF(TRIM(t.model), ''), NULLIF(TRIM(r.model), ''), NULLIF(TRIM(s.model_slug), ''), '') AS model,
                        COALESCE(NULLIF(TRIM(t.actual_source_kind), ''), NULLIF(TRIM(r.actual_source_kind), ''), '') AS actual_source_kind,
                        COALESCE(NULLIF(TRIM(t.actual_source_id), ''), NULLIF(TRIM(r.actual_source_id), ''), '') AS actual_source_id,
                        COALESCE(
                            (
                                SELECT MIN(NULLIF(TRIM(w.owner_id), ''))
                                FROM app_wallet_ledger_entries l
                                JOIN app_wallets w ON w.id = l.wallet_id
                                WHERE l.request_log_id = s.request_log_id
                                  AND l.entry_kind = 'request_charge'
                                  AND w.owner_kind = 'user'
                            ),
                            NULLIF(TRIM(owner.owner_user_id), ''),
                            NULLIF(TRIM(stat_owner.owner_user_id), ''),
                            ''
                        ) AS owner_user_id,
                        CASE
                            WHEN COALESCE(t.input_tokens, s.input_tokens, 0) > 0
                                THEN COALESCE(t.input_tokens, s.input_tokens, 0)
                            ELSE 0
                        END AS input_tokens,
                        CASE
                            WHEN COALESCE(t.cached_input_tokens, s.cached_input_tokens, 0) > 0
                                THEN COALESCE(t.cached_input_tokens, s.cached_input_tokens, 0)
                            ELSE 0
                        END AS cached_input_tokens,
                        CASE
                            WHEN COALESCE(t.output_tokens, s.output_tokens, 0) > 0
                                THEN COALESCE(t.output_tokens, s.output_tokens, 0)
                            ELSE 0
                        END AS output_tokens,
                        CASE
                            WHEN t.input_tokens IS NOT NULL OR t.output_tokens IS NOT NULL THEN
                                CASE
                                    WHEN COALESCE(t.input_tokens, s.input_tokens, 0) > 0
                                        THEN COALESCE(t.input_tokens, s.input_tokens, 0)
                                    ELSE 0
                                END
                                + CASE
                                    WHEN COALESCE(t.output_tokens, s.output_tokens, 0) > 0
                                        THEN COALESCE(t.output_tokens, s.output_tokens, 0)
                                    ELSE 0
                                END
                            WHEN t.total_tokens IS NOT NULL THEN
                                CASE WHEN t.total_tokens > 0 THEN t.total_tokens ELSE 0 END
                            WHEN COALESCE(t.input_tokens, s.input_tokens, 0)
                               + COALESCE(t.output_tokens, s.output_tokens, 0) > 0
                                THEN COALESCE(t.input_tokens, s.input_tokens, 0)
                                   + COALESCE(t.output_tokens, s.output_tokens, 0)
                            ELSE 0
                        END AS total_tokens,
                        CASE
                            WHEN COALESCE(t.reasoning_output_tokens, 0) > 0
                                THEN t.reasoning_output_tokens
                            ELSE 0
                        END AS reasoning_output_tokens,
                        s.base_cost_microusd AS estimated_cost_microusd,
                        1 AS request_count,
                        CASE WHEN r.status_code >= 200 AND r.status_code <= 299 THEN 1 ELSE 0 END AS success_count,
                        CASE WHEN IFNULL(r.status_code, 0) >= 400 OR TRIM(IFNULL(r.error, '')) <> '' THEN 1 ELSE 0 END AS error_count
                    FROM request_charge_snapshots s
                    LEFT JOIN request_logs r ON r.id = s.request_log_id
                    LEFT JOIN request_token_stats t ON t.request_log_id = s.request_log_id
                    LEFT JOIN api_key_owners owner
                        ON owner.key_id = r.key_id AND owner.owner_kind = 'user'
                    LEFT JOIN api_key_owners stat_owner
                        ON stat_owner.key_id = t.key_id AND stat_owner.owner_kind = 'user'
                    WHERE s.usage_source = 'estimated'
                )
                SELECT
                    bucket_start,
                    key_id,
                    account_id,
                    model,
                    actual_source_kind,
                    actual_source_id,
                    owner_user_id,
                    SUM(input_tokens),
                    SUM(cached_input_tokens),
                    SUM(output_tokens),
                    SUM(total_tokens),
                    SUM(reasoning_output_tokens),
                    SUM(estimated_cost_microusd),
                    SUM(request_count),
                    SUM(success_count),
                    SUM(error_count)
                FROM estimated_requests
                GROUP BY
                    bucket_start,
                    key_id,
                    account_id,
                    model,
                    actual_source_kind,
                    actual_source_id,
                    owner_user_id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(EstimatedHourlyContribution {
                bucket_start: row.get(0)?,
                key_id: row.get(1)?,
                account_id: row.get(2)?,
                model: row.get(3)?,
                actual_source_kind: row.get(4)?,
                actual_source_id: row.get(5)?,
                owner_user_id: row.get(6)?,
                input_tokens: row.get(7)?,
                cached_input_tokens: row.get(8)?,
                output_tokens: row.get(9)?,
                total_tokens: row.get(10)?,
                reasoning_output_tokens: row.get(11)?,
                estimated_cost_microusd: row.get(12)?,
                request_count: row.get(13)?,
                success_count: row.get(14)?,
                error_count: row.get(15)?,
            })
        })?;
        rows.collect()
    }

    fn remove_estimated_hourly_rollup_usage(
        tx: &Transaction<'_>,
        contributions: Vec<EstimatedHourlyContribution>,
    ) -> Result<(usize, usize, usize)> {
        let mut cleaned = 0_usize;
        let mut ambiguous = 0_usize;
        let mut not_rolled_up = 0_usize;
        for contribution in contributions {
            let changed = tx.execute(
                "UPDATE request_token_stat_hourly_rollups
                 SET input_tokens=input_tokens-?8,
                     cached_input_tokens=cached_input_tokens-?9,
                     output_tokens=output_tokens-?10,
                     total_tokens=total_tokens-?11,
                     reasoning_output_tokens=reasoning_output_tokens-?12,
                     estimated_cost_usd=(
                         CAST(ROUND(estimated_cost_usd * 1000000.0) AS INTEGER)-?13
                     ) / 1000000.0,
                     request_count=request_count-?14,
                     success_count=success_count-?15,
                     error_count=error_count-?16,
                     updated_at=?17
                 WHERE bucket_start=?1
                   AND key_id=?2
                   AND account_id=?3
                   AND model=?4
                   AND actual_source_kind=?5
                   AND actual_source_id=?6
                   AND owner_user_id=?7
                   AND input_tokens>=?8
                   AND cached_input_tokens>=?9
                   AND output_tokens>=?10
                   AND total_tokens>=?11
                   AND reasoning_output_tokens>=?12
                   AND CAST(ROUND(estimated_cost_usd * 1000000.0) AS INTEGER)>=?13
                   AND request_count>=?14
                   AND success_count>=?15
                   AND error_count>=?16",
                params![
                    contribution.bucket_start,
                    contribution.key_id,
                    contribution.account_id,
                    contribution.model,
                    contribution.actual_source_kind,
                    contribution.actual_source_id,
                    contribution.owner_user_id,
                    contribution.input_tokens,
                    contribution.cached_input_tokens,
                    contribution.output_tokens,
                    contribution.total_tokens,
                    contribution.reasoning_output_tokens,
                    contribution.estimated_cost_microusd,
                    contribution.request_count,
                    contribution.success_count,
                    contribution.error_count,
                    now_ts(),
                ],
            )?;
            if changed == 1 {
                cleaned = cleaned.saturating_add(1);
                tx.execute(
                    "DELETE FROM request_token_stat_hourly_rollups
                     WHERE bucket_start=?1
                       AND key_id=?2
                       AND account_id=?3
                       AND model=?4
                       AND actual_source_kind=?5
                       AND actual_source_id=?6
                       AND owner_user_id=?7
                       AND request_count=0
                       AND success_count=0
                       AND error_count=0
                       AND input_tokens=0
                       AND cached_input_tokens=0
                       AND output_tokens=0
                       AND total_tokens=0
                       AND reasoning_output_tokens=0
                       AND CAST(ROUND(estimated_cost_usd * 1000000.0) AS INTEGER)=0",
                    params![
                        contribution.bucket_start,
                        contribution.key_id,
                        contribution.account_id,
                        contribution.model,
                        contribution.actual_source_kind,
                        contribution.actual_source_id,
                        contribution.owner_user_id,
                    ],
                )?;
            } else {
                let target_exists: bool = tx.query_row(
                    "SELECT EXISTS(
                         SELECT 1 FROM request_token_stat_hourly_rollups
                         WHERE bucket_start=?1
                           AND key_id=?2
                           AND account_id=?3
                           AND model=?4
                           AND actual_source_kind=?5
                           AND actual_source_id=?6
                           AND owner_user_id=?7
                     )",
                    params![
                        contribution.bucket_start,
                        contribution.key_id,
                        contribution.account_id,
                        contribution.model,
                        contribution.actual_source_kind,
                        contribution.actual_source_id,
                        contribution.owner_user_id,
                    ],
                    |row| row.get(0),
                )?;
                if target_exists {
                    ambiguous = ambiguous.saturating_add(1);
                } else {
                    not_rolled_up = not_rolled_up.saturating_add(1);
                }
            }
        }
        Ok((cleaned, ambiguous, not_rolled_up))
    }

    pub(super) fn apply_actual_usage_billing_migration(&self) -> Result<()> {
        if self.has_migration(ACTUAL_USAGE_BILLING_MIGRATION_VERSION)? {
            return Ok(());
        }
        let tx = self.conn.unchecked_transaction()?;
        // The transaction owns this connection; reading through the same
        // connection keeps the contribution snapshot consistent with the
        // cleanup writes below.
        let estimated_contributions = Self::load_estimated_hourly_contributions(&self.conn)?;

        // Estimates produced by the old gateway were diagnostic guesses, not
        // authoritative provider usage. Remove them from token/cost rollups;
        // retain an estimate snapshot only when a wallet ledger already refers
        // to it, so the financial audit trail is not silently rewritten.
        let (cleaned_rollups, ambiguous_rollups, not_rolled_up) =
            Self::remove_estimated_hourly_rollup_usage(&tx, estimated_contributions)?;
        tx.execute(
            "UPDATE request_token_stats
             SET usage_source='estimated',
                 input_tokens=NULL,
                 cached_input_tokens=NULL,
                 output_tokens=NULL,
                 total_tokens=NULL,
                 reasoning_output_tokens=NULL,
                 estimated_cost_usd=NULL
             WHERE request_log_id IN (
                 SELECT request_log_id
                 FROM request_charge_snapshots
                 WHERE usage_source='estimated'
             )",
            [],
        )?;
        tx.execute(
            "DELETE FROM request_charge_snapshots
             WHERE usage_source='estimated'
               AND NOT EXISTS (
                   SELECT 1
                   FROM app_wallet_ledger_entries ledger
                   WHERE ledger.request_log_id=request_charge_snapshots.request_log_id
                     AND ledger.entry_kind='request_charge'
               )",
            [],
        )?;
        for (key, value) in [
            (
                "actual_usage_billing_migration_cleaned_rollups",
                cleaned_rollups,
            ),
            (
                "actual_usage_billing_migration_ambiguous_rollups",
                ambiguous_rollups,
            ),
            (
                "actual_usage_billing_migration_not_rolled_up",
                not_rolled_up,
            ),
        ] {
            tx.execute(
                "INSERT INTO model_catalog_v2_meta(key,value) VALUES(?1,?2)
                 ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                params![key, value.to_string()],
            )?;
        }
        tx.execute(
            "INSERT INTO schema_migrations(version,applied_at) VALUES(?1,?2)",
            params![ACTUAL_USAGE_BILLING_MIGRATION_VERSION, now_ts()],
        )?;
        tx.commit()?;
        if let Some(migrations) = self.applied_migrations.borrow_mut().as_mut() {
            migrations.insert(ACTUAL_USAGE_BILLING_MIGRATION_VERSION.to_string());
        }
        Ok(())
    }

    pub(super) fn apply_model_billing_v2_hardening_migration(&self) -> Result<()> {
        if self.has_migration(HARDENING_MIGRATION_VERSION)? {
            return Ok(());
        }
        let tx = self.conn.unchecked_transaction()?;
        tx.execute_batch(include_str!(
            "../../migrations/113_model_billing_v2_hardening.sql"
        ))?;
        let invalid_snapshots: i64 = tx.query_row(
            "SELECT COUNT(*) FROM request_charge_snapshots
             WHERE cached_input_tokens>input_tokens OR rate_multiplier_millis<0",
            [],
            |row| row.get(0),
        )?;
        if invalid_snapshots != 0 {
            return Err(rusqlite::Error::SqliteFailure(
                (),
                Some("model billing V2 hardening smoke check failed".to_string()),
            ));
        }
        tx.execute(
            "INSERT INTO model_catalog_v2_meta(key,value)
             VALUES('billing_hardening_state','complete')
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            [],
        )?;
        tx.execute(
            "INSERT INTO schema_migrations(version,applied_at) VALUES(?1,?2)",
            params![HARDENING_MIGRATION_VERSION, now_ts()],
        )?;
        tx.commit()?;
        if let Some(migrations) = self.applied_migrations.borrow_mut().as_mut() {
            migrations.insert(HARDENING_MIGRATION_VERSION.to_string());
        }
        Ok(())
    }

    pub fn select_model_price_tier_v2(
        &self,
        model_slug: &str,
        input_tokens: i64,
    ) -> Result<Option<(String, ModelPriceTierV2)>> {
        if input_tokens < 0 {
            return Err(rusqlite::Error::InvalidParameterName(
                "input tokens cannot be negative".to_string(),
            ));
        }
        self.conn
            .query_row(
                "SELECT m.id,t.min_input_tokens,t.input_microusd_per_1m,
                        t.cached_input_microusd_per_1m,
                        t.cache_creation_microusd_per_1m,t.output_microusd_per_1m
                 FROM models m
                 JOIN model_prices p ON p.model_id=m.id AND p.price_status<>'missing'
                 JOIN model_price_tiers t ON t.model_id=m.id AND t.min_input_tokens<=?2
                 WHERE m.slug=?1 COLLATE NOCASE
                 ORDER BY t.min_input_tokens DESC LIMIT 1",
                params![model_slug.trim(), input_tokens],
                |row| {
                    Ok((
                        row.get(0)?,
                        ModelPriceTierV2 {
                            min_input_tokens: row.get(1)?,
                            input_microusd_per_1m: row.get(2)?,
                            cached_input_microusd_per_1m: row.get(3)?,
                            cache_creation_microusd_per_1m: row.get(4)?,
                            output_microusd_per_1m: row.get(5)?,
                        },
                    ))
                },
            )
            .optional()
    }

    pub fn get_charge_snapshot_v2(&self, request_log_id: i64) -> Result<Option<ChargeSnapshotV2>> {
        self.conn
            .query_row(
                &format!("{SNAPSHOT_SELECT} WHERE request_log_id=?1"),
                [request_log_id],
                map_snapshot,
            )
            .optional()
    }

    fn select_charge_tier_v2(
        tx: &Transaction<'_>,
        model_slug: &str,
        input_tokens: i64,
    ) -> Result<(String, ModelPriceTierV2)> {
        let price_status: Option<String> = tx
            .query_row(
                "SELECT p.price_status FROM models m JOIN model_prices p ON p.model_id=m.id
                 WHERE m.slug=?1 COLLATE NOCASE",
                [model_slug],
                |row| row.get(0),
            )
            .optional()?;
        match price_status.as_deref() {
            None => return Err(rusqlite::Error::QueryReturnedNoRows),
            Some("missing") => {
                return Err(rusqlite::Error::InvalidParameterName(
                    "model_price_missing".to_string(),
                ))
            }
            _ => {}
        }
        tx.query_row(
            "SELECT m.id,t.min_input_tokens,t.input_microusd_per_1m,
                    t.cached_input_microusd_per_1m,
                    t.cache_creation_microusd_per_1m,t.output_microusd_per_1m
             FROM models m JOIN model_price_tiers t ON t.model_id=m.id AND t.min_input_tokens<=?2
             WHERE m.slug=?1 COLLATE NOCASE ORDER BY t.min_input_tokens DESC LIMIT 1",
            params![model_slug, input_tokens],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    ModelPriceTierV2 {
                        min_input_tokens: row.get(1)?,
                        input_microusd_per_1m: row.get(2)?,
                        cached_input_microusd_per_1m: row.get(3)?,
                        cache_creation_microusd_per_1m: row.get(4)?,
                        output_microusd_per_1m: row.get(5)?,
                    },
                ))
            },
        )
    }

    fn snapshot_matches_input(snapshot: &ChargeSnapshotV2, input: &ChargeSnapshotInputV2) -> bool {
        snapshot
            .model_slug
            .eq_ignore_ascii_case(input.model_slug.trim())
            && snapshot.input_tokens == input.input_tokens
            && snapshot.cached_input_tokens == input.cached_input_tokens.min(input.input_tokens)
            && snapshot.cache_creation_input_tokens
                == input
                    .cache_creation_input_tokens
                    .min(input.input_tokens.saturating_sub(snapshot.cached_input_tokens))
            && snapshot.output_tokens == input.output_tokens
            && snapshot.reasoning_output_tokens == input.reasoning_output_tokens
            && snapshot.unclassified_tokens == input.unclassified_tokens
            && snapshot.usage_quality == input.usage_quality
            && snapshot.rate_multiplier_millis == input.rate_multiplier_millis
    }

    fn update_request_token_stat_actual(
        tx: &Transaction<'_>,
        input: &ChargeSnapshotInputV2,
        base_cost_microusd: i64,
    ) -> Result<()> {
        tx.execute(
            "UPDATE request_token_stats
             SET usage_source='actual',
                 input_tokens=COALESCE(input_tokens, ?2),
                 cached_input_tokens=COALESCE(cached_input_tokens, ?3),
                 cache_creation_input_tokens=COALESCE(cache_creation_input_tokens, ?4),
                 output_tokens=COALESCE(output_tokens, ?5),
                 reasoning_output_tokens=COALESCE(reasoning_output_tokens, ?6),
                 unclassified_tokens=COALESCE(unclassified_tokens, ?7),
                 total_tokens=COALESCE(total_tokens, ?2 + ?5 + ?7),
                 usage_quality=?8,
                 estimated_cost_usd=CAST(?9 AS REAL)/1000000.0
             WHERE request_log_id=?1",
            params![
                input.request_log_id,
                input.input_tokens,
                input.cached_input_tokens.min(input.input_tokens),
                input.cache_creation_input_tokens.min(
                    input
                        .input_tokens
                        .saturating_sub(input.cached_input_tokens.min(input.input_tokens)),
                ),
                input.output_tokens,
                input.reasoning_output_tokens,
                input.unclassified_tokens,
                input.usage_quality,
                base_cost_microusd,
            ],
        )?;
        Ok(())
    }

    fn apply_wallet_charge_tx(
        tx: &Transaction<'_>,
        input: &ChargeSnapshotInputV2,
        charge_microusd: i64,
        now: i64,
    ) -> Result<()> {
        let Some(wallet_id) = input
            .wallet_id
            .as_deref()
            .filter(|id| !id.trim().is_empty())
        else {
            return Ok(());
        };

        // The request id is the billing idempotency key. A repeated callback
        // must validate the existing effect and then become a no-op.
        let prior_ledger: Option<(String, i64)> = tx
            .query_row(
                "SELECT wallet_id,amount_credit_micros
                 FROM app_wallet_ledger_entries
                 WHERE request_log_id=?1 AND entry_kind='request_charge' LIMIT 1",
                [input.request_log_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((prior_wallet_id, amount)) = prior_ledger {
            if prior_wallet_id != wallet_id || amount != -charge_microusd {
                return Err(rusqlite::Error::InvalidParameterName(
                    "request_charge_idempotency_conflict".to_string(),
                ));
            }
            return Ok(());
        }

        let changed = tx.execute(
            "UPDATE app_wallets SET balance_credit_micros=balance_credit_micros-?2,updated_at=?3
             WHERE id=?1 AND status='active'
               AND balance_credit_micros-?2>=frozen_credit_micros",
            params![wallet_id, charge_microusd, now],
        )?;
        if changed != 1 {
            return Err(rusqlite::Error::InvalidParameterName(
                "wallet_insufficient_balance".to_string(),
            ));
        }
        let balance_after: i64 = tx.query_row(
            "SELECT balance_credit_micros FROM app_wallets WHERE id=?1",
            [wallet_id],
            |row| row.get(0),
        )?;
        tx.execute(
            "INSERT INTO app_wallet_ledger_entries(id,wallet_id,entry_kind,
               amount_credit_micros,balance_after_credit_micros,request_log_id,api_key_id,
               pricing_rule_id,raw_usage_json,note,created_by_user_id,created_at)
             VALUES(?1,?2,'request_charge',?3,?4,?5,?6,?7,?8,?9,NULL,?10)",
            params![
                format!("wl_request_{}", input.request_log_id),
                wallet_id,
                -charge_microusd,
                balance_after,
                input.request_log_id,
                input.api_key_id,
                input.pricing_rule_id,
                input.raw_usage_json,
                input.ledger_note.as_deref().unwrap_or("model_catalog_v2"),
                now
            ],
        )?;
        Ok(())
    }

    pub fn record_charge_snapshot_v2(
        &self,
        input: &ChargeSnapshotInputV2,
    ) -> Result<ChargeSnapshotV2> {
        if !matches!(input.usage_source.as_str(), "actual" | "estimated") {
            return Err(rusqlite::Error::InvalidParameterName(
                "usage_source must be actual or estimated".to_string(),
            ));
        }
        if input.request_log_id <= 0
            || input.input_tokens < 0
            || input.cached_input_tokens < 0
            || input.cache_creation_input_tokens < 0
            || input.output_tokens < 0
            || input.reasoning_output_tokens < 0
            || input.unclassified_tokens < 0
            || input.rate_multiplier_millis < 0
        {
            return Err(rusqlite::Error::InvalidParameterName(
                "tokens, request id, and multiplier must be non-negative".to_string(),
            ));
        }
        let model_slug = input.model_slug.trim();
        if model_slug.is_empty() {
            return Err(rusqlite::Error::InvalidParameterName(
                "model_slug_required".to_string(),
            ));
        }
        if !matches!(
            input.usage_quality.as_str(),
            "complete" | "inconsistent" | "unclassified"
        ) {
            return Err(rusqlite::Error::InvalidParameterName(
                "usage_quality must be complete, inconsistent, or unclassified".to_string(),
            ));
        }
        let tx = self.conn.unchecked_transaction()?;
        let existing = tx
            .query_row(
                &format!("{SNAPSHOT_SELECT} WHERE request_log_id=?1"),
                [input.request_log_id],
                map_snapshot,
            )
            .optional()?;
        if let Some(existing) = existing {
            if input.usage_source == "estimated" {
                if existing.usage_source == "actual"
                    || Self::snapshot_matches_input(&existing, input)
                {
                    tx.commit()?;
                    return Ok(existing);
                }
                return Err(rusqlite::Error::InvalidParameterName(
                    "request_charge_usage_conflict".to_string(),
                ));
            }
            if existing.usage_source == "actual" {
                if !Self::snapshot_matches_input(&existing, input) {
                    return Err(rusqlite::Error::InvalidParameterName(
                        "request_charge_usage_conflict".to_string(),
                    ));
                }
                Self::update_request_token_stat_actual(&tx, input, existing.base_cost_microusd)?;
                let now = now_ts();
                Self::apply_wallet_charge_tx(&tx, input, existing.charged_cost_microusd, now)?;
                tx.commit()?;
                return Ok(existing);
            }
            let has_prior_ledger: bool = tx.query_row(
                "SELECT EXISTS(
                   SELECT 1 FROM app_wallet_ledger_entries
                   WHERE request_log_id=?1 AND entry_kind='request_charge'
                 )",
                [input.request_log_id],
                |row| row.get(0),
            )?;
            if has_prior_ledger {
                return Err(rusqlite::Error::InvalidParameterName(
                    "cannot_promote_billed_estimate".to_string(),
                ));
            }
            let (model_id, tier) =
                Self::select_charge_tier_v2(&tx, model_slug, input.input_tokens)?;
            let computation = compute_charge_v2(
                input.input_tokens,
                input.cached_input_tokens,
                input.cache_creation_input_tokens,
                input.output_tokens,
                &tier,
                input.rate_multiplier_millis,
            )?;
            let cached_input_tokens = input.cached_input_tokens.min(input.input_tokens);
            let cache_creation_input_tokens = input
                .cache_creation_input_tokens
                .min(input.input_tokens.saturating_sub(cached_input_tokens));
            let now = now_ts();
            tx.execute(
                "UPDATE request_charge_snapshots
                 SET model_id=?2,model_slug=?3,tier_min_input_tokens=?4,
                     usage_source='actual',input_tokens=?5,cached_input_tokens=?6,
                     cache_creation_input_tokens=?7,output_tokens=?8,
                     reasoning_output_tokens=?9,unclassified_tokens=?10,input_microusd_per_1m=?11,
                     cached_input_microusd_per_1m=?12,cache_creation_microusd_per_1m=?13,
                     output_microusd_per_1m=?14,usage_quality=?15,
                     rate_multiplier_millis=?16,base_cost_microusd=?17,
                     charged_cost_microusd=?18,created_at=?19
                 WHERE request_log_id=?1",
                params![
                    input.request_log_id,
                    model_id,
                    model_slug,
                    tier.min_input_tokens,
                    input.input_tokens,
                    cached_input_tokens,
                    cache_creation_input_tokens,
                    input.output_tokens,
                    input.reasoning_output_tokens,
                    input.unclassified_tokens,
                    tier.input_microusd_per_1m,
                    tier.cached_input_microusd_per_1m,
                    tier.cache_creation_microusd_per_1m,
                    tier.output_microusd_per_1m,
                    input.usage_quality,
                    input.rate_multiplier_millis,
                    computation.base_cost_microusd,
                    computation.charged_cost_microusd,
                    now,
                ],
            )?;
            Self::update_request_token_stat_actual(&tx, input, computation.base_cost_microusd)?;
            Self::apply_wallet_charge_tx(&tx, input, computation.charged_cost_microusd, now)?;
            tx.commit()?;
            return self
                .get_charge_snapshot_v2(input.request_log_id)?
                .ok_or(rusqlite::Error::QueryReturnedNoRows);
        }

        let (model_id, tier) = Self::select_charge_tier_v2(&tx, model_slug, input.input_tokens)?;
        let computation = compute_charge_v2(
            input.input_tokens,
            input.cached_input_tokens,
            input.cache_creation_input_tokens,
            input.output_tokens,
            &tier,
            input.rate_multiplier_millis,
        )?;
        let cached_input_tokens = input.cached_input_tokens.min(input.input_tokens);
        let cache_creation_input_tokens = input
            .cache_creation_input_tokens
            .min(input.input_tokens.saturating_sub(cached_input_tokens));
        let now = now_ts();
        tx.execute(
            "INSERT INTO request_charge_snapshots(request_log_id,model_id,model_slug,
               tier_min_input_tokens,usage_source,input_tokens,cached_input_tokens,
               cache_creation_input_tokens,output_tokens,reasoning_output_tokens,
               unclassified_tokens,input_microusd_per_1m,
               cached_input_microusd_per_1m,cache_creation_microusd_per_1m,
               output_microusd_per_1m,usage_quality,rate_multiplier_millis,
               base_cost_microusd,charged_cost_microusd,currency,created_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,'USD',?20)",
            params![
                input.request_log_id,
                model_id,
                model_slug,
                tier.min_input_tokens,
                input.usage_source,
                input.input_tokens,
                cached_input_tokens,
                cache_creation_input_tokens,
                input.output_tokens,
                input.reasoning_output_tokens,
                input.unclassified_tokens,
                tier.input_microusd_per_1m,
                tier.cached_input_microusd_per_1m,
                tier.cache_creation_microusd_per_1m,
                tier.output_microusd_per_1m,
                input.usage_quality,
                input.rate_multiplier_millis,
                computation.base_cost_microusd,
                computation.charged_cost_microusd,
                now
            ],
        )?;
        if input.usage_source == "actual" {
            Self::update_request_token_stat_actual(&tx, input, computation.base_cost_microusd)?;
        } else {
            tx.execute(
                "UPDATE request_token_stats
                 SET usage_source='estimated',
                     input_tokens=NULL,
                     cached_input_tokens=NULL,
                     output_tokens=NULL,
                     total_tokens=NULL,
                     reasoning_output_tokens=NULL,
                     estimated_cost_usd=NULL
                 WHERE request_log_id=?1",
                [input.request_log_id],
            )?;
        }
        if input.usage_source == "actual" {
            Self::apply_wallet_charge_tx(&tx, input, computation.charged_cost_microusd, now)?;
        }
        tx.commit()?;
        self.get_charge_snapshot_v2(input.request_log_id)?
            .ok_or(rusqlite::Error::QueryReturnedNoRows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn insert_request_log(storage: &Storage) -> i64 {
        storage
            .conn
            .execute(
                "INSERT INTO request_logs(request_path,method,created_at)
                 VALUES('/v1/responses','POST',1)",
                [],
            )
            .unwrap();
        storage.conn.last_insert_rowid()
    }

    fn insert_wallet(storage: &Storage, wallet_id: &str, balance: i64) {
        storage
            .conn
            .execute(
                "INSERT INTO app_wallets(
                    id,owner_kind,owner_id,balance_credit_micros,
                    frozen_credit_micros,status,created_at,updated_at
                 ) VALUES(?1,'user',?2,?3,0,'active',1,1)",
                params![wallet_id, format!("owner-{wallet_id}"), balance],
            )
            .unwrap();
    }

    fn actual_input(request_log_id: i64, input_tokens: i64) -> ChargeSnapshotInputV2 {
        ChargeSnapshotInputV2 {
            request_log_id,
            model_slug: "gpt-5.4-mini".into(),
            usage_source: "actual".into(),
            input_tokens,
            cached_input_tokens: 0,
            output_tokens: 10,
            rate_multiplier_millis: 1_000,
            ..Default::default()
        }
    }

    #[test]
    fn integer_formula_charges_cached_subset_once() {
        let tier = ModelPriceTierV2 {
            min_input_tokens: 0,
            input_microusd_per_1m: 2_000_000,
            cached_input_microusd_per_1m: 200_000,
            output_microusd_per_1m: 10_000_000,
        };
        let result = compute_charge_v2(100, 40, 10, &tier, 1_500).unwrap();
        assert_eq!(result.uncached_input_tokens, 60);
        assert_eq!(result.numerator, 228_000_000);
        assert_eq!(result.base_cost_microusd, 228);
        assert_eq!(result.charged_cost_microusd, 342);
        let free = compute_charge_v2(100, 40, 10, &tier, 0).unwrap();
        assert_eq!(free.base_cost_microusd, 228);
        assert_eq!(free.charged_cost_microusd, 0);
    }

    #[test]
    fn extreme_integer_inputs_return_an_error_without_panicking() {
        let tier = ModelPriceTierV2 {
            min_input_tokens: 0,
            input_microusd_per_1m: i64::MAX,
            cached_input_microusd_per_1m: i64::MAX,
            output_microusd_per_1m: i64::MAX,
        };
        let error = compute_charge_v2(i64::MAX, 0, i64::MAX, &tier, i64::MAX)
            .expect_err("overflow must be rejected");
        assert!(error.to_string().contains("overflow"));
    }

    #[test]
    fn cached_above_input_is_clamped_and_tier_boundary_is_exact() {
        let storage = Storage::open_in_memory().unwrap();
        storage.init().unwrap();
        let (_, low) = storage
            .select_model_price_tier_v2("gpt-5.4", 271_999)
            .unwrap()
            .unwrap();
        let (_, high) = storage
            .select_model_price_tier_v2("gpt-5.4", 272_000)
            .unwrap()
            .unwrap();
        assert_eq!(low.min_input_tokens, 0);
        assert_eq!(high.min_input_tokens, 272_000);
        let result = compute_charge_v2(10, 20, 0, &low, 1_000).unwrap();
        assert_eq!(result.uncached_input_tokens, 0);
        assert_eq!(
            result.numerator,
            10_i128 * i128::from(low.cached_input_microusd_per_1m)
        );
    }

    #[test]
    fn missing_price_is_rejected_and_snapshot_is_idempotent() {
        let storage = Storage::open_in_memory().unwrap();
        storage.init().unwrap();
        storage.conn.execute("INSERT INTO request_logs(request_path,method,created_at) VALUES('/v1/responses','POST',1)",[]).unwrap();
        let request_log_id = storage.conn.last_insert_rowid();
        let mut input = ChargeSnapshotInputV2 {
            request_log_id,
            model_slug: "codex-auto-review".into(),
            usage_source: "actual".into(),
            input_tokens: 1,
            cached_input_tokens: 0,
            output_tokens: 1,
            rate_multiplier_millis: 1_000,
            ..Default::default()
        };
        assert!(storage
            .record_charge_snapshot_v2(&input)
            .unwrap_err()
            .to_string()
            .contains("model_price_missing"));
        input.model_slug = "gpt-5.4-mini".into();
        input.input_tokens = 10;
        input.cached_input_tokens = 20;
        let first = storage.record_charge_snapshot_v2(&input).unwrap();
        let second = storage.record_charge_snapshot_v2(&input).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.cached_input_tokens, 10);
    }

    #[test]
    fn zero_multiplier_records_a_free_ledger_and_is_idempotent() {
        let storage = Storage::open_in_memory().unwrap();
        storage.init().unwrap();
        storage
            .conn
            .execute(
                "INSERT INTO app_wallets(id,owner_kind,owner_id,balance_credit_micros,
                   frozen_credit_micros,status,created_at,updated_at)
                 VALUES('wallet-free','user','free-user',0,0,'active',1,1)",
                [],
            )
            .unwrap();
        storage.conn.execute("INSERT INTO request_logs(request_path,method,created_at) VALUES('/v1/responses','POST',1)",[]).unwrap();
        let request_log_id = storage.conn.last_insert_rowid();
        let input = ChargeSnapshotInputV2 {
            request_log_id,
            model_slug: "gpt-5.4-mini".into(),
            usage_source: "actual".into(),
            input_tokens: 100,
            cached_input_tokens: 0,
            output_tokens: 0,
            rate_multiplier_millis: 0,
            wallet_id: Some("wallet-free".into()),
            ..Default::default()
        };
        let first = storage.record_charge_snapshot_v2(&input).unwrap();
        let second = storage.record_charge_snapshot_v2(&input).unwrap();
        assert_eq!(first, second);
        assert!(first.base_cost_microusd > 0);
        assert_eq!(first.charged_cost_microusd, 0);
        assert_eq!(storage.request_charge_ledger_entry_count().unwrap(), 1);
        let duplicate = storage.conn.execute(
            "INSERT INTO app_wallet_ledger_entries(id,wallet_id,entry_kind,
               amount_credit_micros,balance_after_credit_micros,request_log_id,created_at)
             VALUES('duplicate-charge','wallet-free','request_charge',0,0,?1,2)",
            [request_log_id],
        );
        assert!(
            duplicate.is_err(),
            "partial unique index must reject duplicates"
        );
        let balance: i64 = storage
            .conn
            .query_row(
                "SELECT balance_credit_micros FROM app_wallets WHERE id='wallet-free'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(balance, 0);
    }

    #[test]
    fn repeated_actual_usage_is_idempotent_for_wallet_and_snapshot() {
        let storage = Storage::open_in_memory().unwrap();
        storage.init().unwrap();
        insert_wallet(&storage, "wallet-repeat", 10_000_000);
        let request_log_id = insert_request_log(&storage);
        let mut input = actual_input(request_log_id, 100);
        input.wallet_id = Some("wallet-repeat".into());

        let first = storage.record_charge_snapshot_v2(&input).unwrap();
        let second = storage.record_charge_snapshot_v2(&input).unwrap();
        assert_eq!(first, second);
        let balance: i64 = storage
            .conn
            .query_row(
                "SELECT balance_credit_micros FROM app_wallets WHERE id='wallet-repeat'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(balance, 10_000_000 - first.charged_cost_microusd);
        assert_eq!(storage.request_charge_ledger_entry_count().unwrap(), 1);
    }

    #[test]
    fn conflicting_duplicate_usage_is_rejected_without_mutating_the_snapshot() {
        let storage = Storage::open_in_memory().unwrap();
        storage.init().unwrap();
        let request_log_id = insert_request_log(&storage);
        let first_input = actual_input(request_log_id, 100);
        let first = storage.record_charge_snapshot_v2(&first_input).unwrap();

        let error = storage
            .record_charge_snapshot_v2(&actual_input(request_log_id, 101))
            .expect_err("same request id with different usage must conflict");
        assert!(error.to_string().contains("request_charge_usage_conflict"));
        assert_eq!(
            storage.get_charge_snapshot_v2(request_log_id).unwrap(),
            Some(first)
        );
    }

    #[test]
    fn an_actual_snapshot_can_apply_one_deferred_wallet_charge() {
        let storage = Storage::open_in_memory().unwrap();
        storage.init().unwrap();
        let request_log_id = insert_request_log(&storage);
        let first_input = actual_input(request_log_id, 100);
        let first = storage.record_charge_snapshot_v2(&first_input).unwrap();
        assert_eq!(storage.request_charge_ledger_entry_count().unwrap(), 0);

        insert_wallet(&storage, "wallet-deferred", 10_000_000);
        let mut billed_input = first_input.clone();
        billed_input.wallet_id = Some("wallet-deferred".into());
        let second = storage.record_charge_snapshot_v2(&billed_input).unwrap();
        assert_eq!(second, first);
        assert_eq!(storage.request_charge_ledger_entry_count().unwrap(), 1);

        let balance_before_retry: i64 = storage
            .conn
            .query_row(
                "SELECT balance_credit_micros FROM app_wallets WHERE id='wallet-deferred'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        storage.record_charge_snapshot_v2(&billed_input).unwrap();
        let balance_after_retry: i64 = storage
            .conn
            .query_row(
                "SELECT balance_credit_micros FROM app_wallets WHERE id='wallet-deferred'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(balance_after_retry, balance_before_retry);
    }

    #[test]
    fn estimated_snapshot_promotes_to_actual_and_reprices_without_ledger() {
        let storage = Storage::open_in_memory().unwrap();
        storage.init().unwrap();
        let request_log_id = insert_request_log(&storage);
        storage
            .conn
            .execute(
                "INSERT INTO request_token_stats(request_log_id,created_at)
                 VALUES(?1,1)",
                [request_log_id],
            )
            .unwrap();
        storage
            .record_charge_snapshot_v2(&ChargeSnapshotInputV2 {
                request_log_id,
                model_slug: "gpt-5.4-mini".into(),
                usage_source: "estimated".into(),
                input_tokens: 100,
                cached_input_tokens: 0,
                output_tokens: 0,
                rate_multiplier_millis: 1_000,
                ..Default::default()
            })
            .unwrap();

        let actual = storage
            .record_charge_snapshot_v2(&actual_input(request_log_id, 1_000))
            .unwrap();
        assert_eq!(actual.usage_source, "actual");
        assert_eq!(actual.input_tokens, 1_000);
        assert_eq!(storage.request_charge_ledger_entry_count().unwrap(), 0);
        let source: String = storage
            .conn
            .query_row(
                "SELECT usage_source FROM request_token_stats WHERE request_log_id=?1",
                [request_log_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(source, "actual");
    }

    #[test]
    fn billed_estimated_snapshot_cannot_promote_to_actual() {
        let storage = Storage::open_in_memory().unwrap();
        storage.init().unwrap();
        insert_wallet(&storage, "wallet-estimate", 10_000_000);
        let request_log_id = insert_request_log(&storage);
        storage
            .record_charge_snapshot_v2(&ChargeSnapshotInputV2 {
                request_log_id,
                model_slug: "gpt-5.4-mini".into(),
                usage_source: "estimated".into(),
                input_tokens: 100,
                cached_input_tokens: 0,
                output_tokens: 0,
                rate_multiplier_millis: 1_000,
                ..Default::default()
            })
            .unwrap();
        storage
            .conn
            .execute(
                "INSERT INTO app_wallet_ledger_entries(
                    id,wallet_id,entry_kind,amount_credit_micros,
                    balance_after_credit_micros,request_log_id,created_at
                 ) VALUES('estimate-ledger','wallet-estimate','request_charge',-1,0,?1,1)",
                [request_log_id],
            )
            .unwrap();

        let error = storage
            .record_charge_snapshot_v2(&actual_input(request_log_id, 100))
            .expect_err("an already billed estimate must remain immutable");
        assert!(error.to_string().contains("cannot_promote_billed_estimate"));
        assert_eq!(
            storage
                .get_charge_snapshot_v2(request_log_id)
                .unwrap()
                .unwrap()
                .usage_source,
            "estimated"
        );
    }

    #[test]
    fn estimated_snapshot_is_not_a_billable_stat_or_wallet_charge() {
        let storage = Storage::open_in_memory().unwrap();
        storage.init().unwrap();
        storage
            .conn
            .execute(
                "INSERT INTO request_logs(request_path,method,created_at)
                 VALUES('/v1/responses','POST',1)",
                [],
            )
            .unwrap();
        let request_log_id = storage.conn.last_insert_rowid();
        storage
            .conn
            .execute(
                "INSERT INTO request_token_stats(
                   request_log_id,input_tokens,cached_input_tokens,output_tokens,
                   total_tokens,estimated_cost_usd,created_at
                 ) VALUES(?1,100,0,0,100,1.0,1)",
                [request_log_id],
            )
            .unwrap();

        let snapshot = storage
            .record_charge_snapshot_v2(&ChargeSnapshotInputV2 {
                request_log_id,
                model_slug: "gpt-5.4-mini".into(),
                usage_source: "estimated".into(),
                input_tokens: 100,
                cached_input_tokens: 0,
                output_tokens: 0,
                rate_multiplier_millis: 1_000,
                wallet_id: Some("wallet-must-not-be-read".into()),
                ..Default::default()
            })
            .unwrap();

        assert_eq!(snapshot.usage_source, "estimated");
        let (input, cost): (Option<i64>, Option<f64>) = storage
            .conn
            .query_row(
                "SELECT input_tokens,estimated_cost_usd
                 FROM request_token_stats WHERE request_log_id=?1",
                [request_log_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(input, None);
        assert_eq!(cost, None);
        assert_eq!(storage.request_charge_ledger_entry_count().unwrap(), 0);
    }

    #[test]
    fn actual_usage_migration_removes_unbilled_estimates_but_keeps_ledger_audit() {
        let storage = Storage::open_in_memory().unwrap();
        storage.init().unwrap();

        storage
            .conn
            .execute(
                "INSERT INTO request_logs(request_path,method,created_at)
                 VALUES('/v1/responses','POST',1)",
                [],
            )
            .unwrap();
        let unbilled_request_id = storage.conn.last_insert_rowid();
        storage
            .record_charge_snapshot_v2(&ChargeSnapshotInputV2 {
                request_log_id: unbilled_request_id,
                model_slug: "gpt-5.4-mini".into(),
                usage_source: "estimated".into(),
                input_tokens: 100,
                cached_input_tokens: 0,
                output_tokens: 0,
                rate_multiplier_millis: 1_000,
                ..Default::default()
            })
            .unwrap();

        storage
            .conn
            .execute(
                "INSERT INTO request_logs(request_path,method,created_at)
                 VALUES('/v1/responses','POST',1)",
                [],
            )
            .unwrap();
        let audited_request_id = storage.conn.last_insert_rowid();
        storage
            .record_charge_snapshot_v2(&ChargeSnapshotInputV2 {
                request_log_id: audited_request_id,
                model_slug: "gpt-5.4-mini".into(),
                usage_source: "estimated".into(),
                input_tokens: 100,
                cached_input_tokens: 0,
                output_tokens: 0,
                rate_multiplier_millis: 1_000,
                ..Default::default()
            })
            .unwrap();
        storage
            .conn
            .execute(
                "INSERT INTO app_wallets(
                   id,owner_kind,owner_id,balance_credit_micros,
                   frozen_credit_micros,status,created_at,updated_at
                 ) VALUES('wallet-audit','user','audit-user',0,0,'active',1,1)",
                [],
            )
            .unwrap();
        storage
            .conn
            .execute(
                "INSERT INTO app_wallet_ledger_entries(
                   id,wallet_id,entry_kind,amount_credit_micros,
                   balance_after_credit_micros,request_log_id,created_at
                 ) VALUES('ledger-audit','wallet-audit','request_charge',-1,0,?1,1)",
                [audited_request_id],
            )
            .unwrap();

        for request_log_id in [unbilled_request_id, audited_request_id] {
            storage
                .conn
                .execute(
                    "INSERT INTO request_token_stats(
                       request_log_id,input_tokens,total_tokens,
                       estimated_cost_usd,created_at
                     ) VALUES(?1,100,100,1.0,1)",
                    [request_log_id],
                )
                .unwrap();
        }

        storage
            .conn
            .execute(
                "DELETE FROM schema_migrations WHERE version=?1",
                [ACTUAL_USAGE_BILLING_MIGRATION_VERSION],
            )
            .unwrap();
        storage.applied_migrations.borrow_mut().take();
        storage.apply_actual_usage_billing_migration().unwrap();
        storage.apply_actual_usage_billing_migration().unwrap();

        assert!(storage
            .get_charge_snapshot_v2(unbilled_request_id)
            .unwrap()
            .is_none());
        assert!(storage
            .get_charge_snapshot_v2(audited_request_id)
            .unwrap()
            .is_some());
        for request_log_id in [unbilled_request_id, audited_request_id] {
            let (input, cost): (Option<i64>, Option<f64>) = storage
                .conn
                .query_row(
                    "SELECT input_tokens,estimated_cost_usd
                     FROM request_token_stats WHERE request_log_id=?1",
                    [request_log_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            assert_eq!(input, None);
            assert_eq!(cost, None);
        }
    }

    #[test]
    fn actual_usage_migration_subtracts_estimates_from_mixed_hourly_rollups() {
        let storage = Storage::open_in_memory().unwrap();
        storage.init().unwrap();

        storage
            .conn
            .execute(
                "INSERT INTO request_logs(
                    key_id,account_id,model,request_path,method,status_code,created_at
                 ) VALUES('key-mixed','account-mixed','gpt-5.4-mini','/v1/responses','POST',200,100)",
                [],
            )
            .unwrap();
        let actual_request_id = storage.conn.last_insert_rowid();
        storage
            .conn
            .execute(
                "INSERT INTO request_token_stats(
                    request_log_id,key_id,account_id,model,input_tokens,
                    output_tokens,total_tokens,estimated_cost_usd,created_at
                 ) VALUES(?1,'key-mixed','account-mixed','gpt-5.4-mini',10,0,10,0.25,100)",
                [actual_request_id],
            )
            .unwrap();

        storage
            .conn
            .execute(
                "INSERT INTO request_logs(
                    key_id,account_id,model,request_path,method,status_code,error,created_at
                 ) VALUES('key-mixed','account-mixed','gpt-5.4-mini','/v1/responses','POST',502,'upstream',200)",
                [],
            )
            .unwrap();
        let estimated_request_id = storage.conn.last_insert_rowid();
        storage
            .conn
            .execute(
                "INSERT INTO request_token_stats(
                    request_log_id,key_id,account_id,model,input_tokens,
                    output_tokens,total_tokens,estimated_cost_usd,created_at
                 ) VALUES(?1,'key-mixed','account-mixed','gpt-5.4-mini',100,0,100,1.0,200)",
                [estimated_request_id],
            )
            .unwrap();
        let estimated_snapshot = storage
            .record_charge_snapshot_v2(&ChargeSnapshotInputV2 {
                request_log_id: estimated_request_id,
                model_slug: "gpt-5.4-mini".into(),
                usage_source: "estimated".into(),
                input_tokens: 100,
                cached_input_tokens: 0,
                output_tokens: 0,
                rate_multiplier_millis: 1_000,
                ..Default::default()
            })
            .unwrap();

        storage
            .conn
            .execute(
                "INSERT INTO request_token_stat_hourly_rollups(
                    bucket_start,bucket_end,key_id,account_id,model,
                    actual_source_kind,actual_source_id,owner_user_id,
                    input_tokens,cached_input_tokens,output_tokens,total_tokens,
                    reasoning_output_tokens,estimated_cost_usd,request_count,
                    success_count,error_count,updated_at
                 ) VALUES(0,3600,'key-mixed','account-mixed','gpt-5.4-mini',
                    '','','',110,0,0,110,0,?1,2,1,1,1)",
                [0.25 + estimated_snapshot.base_cost_microusd as f64 / 1_000_000.0],
            )
            .unwrap();

        storage
            .conn
            .execute(
                "DELETE FROM schema_migrations WHERE version=?1",
                [ACTUAL_USAGE_BILLING_MIGRATION_VERSION],
            )
            .unwrap();
        storage.applied_migrations.borrow_mut().take();
        storage.apply_actual_usage_billing_migration().unwrap();

        let (input, total, cost, requests, successes, errors): (i64, i64, f64, i64, i64, i64) =
            storage
                .conn
                .query_row(
                    "SELECT input_tokens,total_tokens,estimated_cost_usd,
                            request_count,success_count,error_count
                     FROM request_token_stat_hourly_rollups
                     WHERE bucket_start=0 AND key_id='key-mixed'",
                    [],
                    |row| {
                        Ok((
                            row.get(0)?,
                            row.get(1)?,
                            row.get(2)?,
                            row.get(3)?,
                            row.get(4)?,
                            row.get(5)?,
                        ))
                    },
                )
                .unwrap();
        assert_eq!(input, 10);
        assert_eq!(total, 10);
        assert!((cost - 0.25).abs() < 1e-9);
        assert_eq!(requests, 1);
        assert_eq!(successes, 1);
        assert_eq!(errors, 0);

        storage.apply_actual_usage_billing_migration().unwrap();
        let (input_after, cost_after): (i64, f64) = storage
            .conn
            .query_row(
                "SELECT input_tokens,estimated_cost_usd
                 FROM request_token_stat_hourly_rollups
                 WHERE bucket_start=0 AND key_id='key-mixed'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(input_after, input);
        assert!((cost_after - cost).abs() < 1e-9);
    }

    #[test]
    fn actual_usage_migration_leaves_ambiguous_rollups_unchanged_and_records_count() {
        let storage = Storage::open_in_memory().unwrap();
        storage.init().unwrap();
        let request_log_id = insert_request_log(&storage);
        storage
            .record_charge_snapshot_v2(&ChargeSnapshotInputV2 {
                request_log_id,
                model_slug: "gpt-5.4-mini".into(),
                usage_source: "estimated".into(),
                input_tokens: 100,
                cached_input_tokens: 0,
                output_tokens: 0,
                rate_multiplier_millis: 1_000,
                ..Default::default()
            })
            .unwrap();
        storage
            .conn
            .execute(
                "INSERT INTO request_token_stat_hourly_rollups(
                    bucket_start,bucket_end,key_id,account_id,model,
                    actual_source_kind,actual_source_id,owner_user_id,
                    input_tokens,cached_input_tokens,output_tokens,total_tokens,
                    reasoning_output_tokens,estimated_cost_usd,request_count,
                    success_count,error_count,updated_at
                 ) VALUES(0,3600,'','','gpt-5.4-mini','','','',0,0,0,0,0,0.0,1,0,1,1)",
                [],
            )
            .unwrap();

        storage
            .conn
            .execute(
                "DELETE FROM schema_migrations WHERE version=?1",
                [ACTUAL_USAGE_BILLING_MIGRATION_VERSION],
            )
            .unwrap();
        storage.applied_migrations.borrow_mut().take();
        storage.apply_actual_usage_billing_migration().unwrap();

        let (input, requests, errors): (i64, i64, i64) = storage
            .conn
            .query_row(
                "SELECT input_tokens,request_count,error_count
                 FROM request_token_stat_hourly_rollups
                 WHERE bucket_start=0 AND model='gpt-5.4-mini'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!((input, requests, errors), (0, 1, 1));
        let ambiguous: String = storage
            .conn
            .query_row(
                "SELECT value FROM model_catalog_v2_meta
                 WHERE key='actual_usage_billing_migration_ambiguous_rollups'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(ambiguous, "1");
    }

    #[test]
    fn insufficient_wallet_balance_rolls_back_snapshot_and_ledger() {
        let storage = Storage::open_in_memory().unwrap();
        storage.init().unwrap();
        storage
            .conn
            .execute(
                "INSERT INTO app_wallets(id,owner_kind,owner_id,balance_credit_micros,
                   frozen_credit_micros,status,created_at,updated_at)
                 VALUES('wallet-low','user','low-user',1,0,'active',1,1)",
                [],
            )
            .unwrap();
        storage.conn.execute("INSERT INTO request_logs(request_path,method,created_at) VALUES('/v1/responses','POST',1)",[]).unwrap();
        let request_log_id = storage.conn.last_insert_rowid();
        let error = storage
            .record_charge_snapshot_v2(&ChargeSnapshotInputV2 {
                request_log_id,
                model_slug: "gpt-5.4-mini".into(),
                usage_source: "actual".into(),
                input_tokens: 1_000,
                cached_input_tokens: 0,
                output_tokens: 1_000,
                rate_multiplier_millis: 1_000,
                wallet_id: Some("wallet-low".into()),
                ..Default::default()
            })
            .expect_err("insufficient wallet must fail");
        assert!(error.to_string().contains("wallet_insufficient_balance"));
        assert!(storage
            .get_charge_snapshot_v2(request_log_id)
            .unwrap()
            .is_none());
        assert_eq!(storage.request_charge_ledger_entry_count().unwrap(), 0);
        let balance: i64 = storage
            .conn
            .query_row(
                "SELECT balance_credit_micros FROM app_wallets WHERE id='wallet-low'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(balance, 1);
    }
}
