BEGIN IMMEDIATE;

-- Restore billing snapshots from persisted upstream usage without charging wallets.
-- cache_write_tokens is intentionally zero because request_token_stats does not
-- persist an independent cache-write field.
CREATE TEMP TABLE _actual_usage_billing_repair AS
WITH normalized AS (
    SELECT
        stats.request_log_id,
        COALESCE(NULLIF(TRIM(stats.model), ''), NULLIF(TRIM(logs.model), '')) AS model_slug,
        MAX(
            0,
            CASE
                WHEN stats.input_tokens IS NOT NULL THEN stats.input_tokens
                WHEN stats.total_tokens IS NOT NULL
                    THEN stats.total_tokens - COALESCE(stats.output_tokens, 0)
                ELSE COALESCE(stats.cached_input_tokens, 0)
            END
        ) AS input_tokens,
        MAX(0, COALESCE(stats.cached_input_tokens, 0)) AS cached_input_tokens,
        MAX(0, COALESCE(stats.output_tokens, 0)) AS output_tokens,
        logs.created_at
    FROM request_token_stats stats
    JOIN request_logs logs ON logs.id = stats.request_log_id
    WHERE stats.usage_included = 1
      AND (
          logs.request_path = '/responses'
          OR logs.request_path LIKE '/responses/%'
          OR logs.request_path LIKE '/responses?%'
          OR logs.request_path = '/v1/responses'
          OR logs.request_path LIKE '/v1/responses/%'
          OR logs.request_path LIKE '/v1/responses?%'
          OR logs.request_path = '/v1/chat/completions'
          OR logs.request_path LIKE '/v1/chat/completions/%'
          OR logs.request_path LIKE '/v1/chat/completions?%'
          OR logs.request_path = '/v1/messages'
          OR logs.request_path LIKE '/v1/messages/%'
          OR logs.request_path LIKE '/v1/messages?%'
      )
      AND logs.request_path NOT LIKE '/v1/messages/count_tokens%'
      AND NOT EXISTS (
          SELECT 1
          FROM request_charge_snapshots snapshots
          WHERE snapshots.request_log_id = stats.request_log_id
      )
      AND (
          stats.input_tokens IS NOT NULL
          OR stats.cached_input_tokens IS NOT NULL
          OR stats.output_tokens IS NOT NULL
          OR stats.total_tokens IS NOT NULL
      )
), clamped AS (
    SELECT
        request_log_id,
        model_slug,
        input_tokens,
        MIN(cached_input_tokens, input_tokens) AS cached_input_tokens,
        output_tokens,
        created_at
    FROM normalized
    WHERE model_slug IS NOT NULL AND TRIM(model_slug) <> ''
), priced AS (
    SELECT
        clamped.request_log_id,
        models.id AS model_id,
        clamped.model_slug,
        clamped.input_tokens,
        clamped.cached_input_tokens,
        clamped.output_tokens,
        tiers.min_input_tokens AS tier_min_input_tokens,
        tiers.input_microusd_per_1m,
        tiers.cached_input_microusd_per_1m,
        tiers.output_microusd_per_1m,
        clamped.created_at
    FROM clamped
    JOIN models ON models.slug = clamped.model_slug COLLATE NOCASE
    JOIN model_prices prices
      ON prices.model_id = models.id
     AND prices.price_status <> 'missing'
    JOIN model_price_tiers tiers
      ON tiers.model_id = models.id
     AND tiers.min_input_tokens <= clamped.input_tokens
    WHERE NOT EXISTS (
        SELECT 1
        FROM model_price_tiers higher
        WHERE higher.model_id = tiers.model_id
          AND higher.min_input_tokens > tiers.min_input_tokens
          AND higher.min_input_tokens <= clamped.input_tokens
    )
)
SELECT
    priced.request_log_id,
    priced.model_id,
    priced.model_slug,
    priced.tier_min_input_tokens,
    priced.input_tokens,
    priced.cached_input_tokens,
    priced.output_tokens,
    CAST((
        (priced.input_tokens - priced.cached_input_tokens) * priced.input_microusd_per_1m
        + priced.cached_input_tokens * priced.cached_input_microusd_per_1m
        + priced.output_tokens * priced.output_microusd_per_1m
        + 999999
    ) / 1000000 AS INTEGER) AS base_cost_microusd,
    priced.input_microusd_per_1m,
    priced.cached_input_microusd_per_1m,
    priced.output_microusd_per_1m,
    priced.created_at
FROM priced;

INSERT INTO request_charge_snapshots (
    request_log_id, model_id, model_slug, tier_min_input_tokens, usage_source,
    input_tokens, cached_input_tokens, cache_write_tokens, output_tokens,
    input_microusd_per_1m, cached_input_microusd_per_1m, cache_write_microusd_per_1m,
    output_microusd_per_1m, rate_multiplier_millis, base_cost_microusd,
    charged_cost_microusd, currency, created_at
)
SELECT
    repair.request_log_id,
    repair.model_id,
    repair.model_slug,
    repair.tier_min_input_tokens,
    'actual',
    repair.input_tokens,
    repair.cached_input_tokens,
    0,
    repair.output_tokens,
    repair.input_microusd_per_1m,
    repair.cached_input_microusd_per_1m,
    repair.input_microusd_per_1m,
    repair.output_microusd_per_1m,
    1000,
    repair.base_cost_microusd,
    repair.base_cost_microusd,
    'USD',
    repair.created_at
FROM _actual_usage_billing_repair repair
WHERE NOT EXISTS (
    SELECT 1
    FROM request_charge_snapshots snapshots
    WHERE snapshots.request_log_id = repair.request_log_id
);

UPDATE request_token_stats
SET estimated_cost_usd = (
    SELECT CAST(snapshots.base_cost_microusd AS REAL) / 1000000.0
    FROM request_charge_snapshots snapshots
    JOIN _actual_usage_billing_repair repair
      ON repair.request_log_id = snapshots.request_log_id
    WHERE snapshots.request_log_id = request_token_stats.request_log_id
)
WHERE request_log_id IN (
    SELECT request_log_id FROM _actual_usage_billing_repair
);

-- Raw stats may already have been compacted into hourly rollups. Repair only
-- zero-cost rows with persisted tokens, and never overwrite an existing cost.
WITH candidates AS (
    SELECT
        rollups.rowid AS rollup_rowid,
        rollups.input_tokens,
        MIN(MAX(rollups.cached_input_tokens, 0), MAX(rollups.input_tokens, 0)) AS cached_input_tokens,
        MAX(rollups.output_tokens, 0) AS output_tokens,
        tiers.input_microusd_per_1m,
        tiers.cached_input_microusd_per_1m,
        tiers.output_microusd_per_1m
    FROM request_token_stat_hourly_rollups rollups
    JOIN models ON models.slug = rollups.model COLLATE NOCASE
    JOIN model_prices prices
      ON prices.model_id = models.id
     AND prices.price_status <> 'missing'
    JOIN model_price_tiers tiers
      ON tiers.model_id = models.id
     AND tiers.min_input_tokens = 0
    WHERE rollups.estimated_cost_usd <= 0
      AND (rollups.input_tokens > 0 OR rollups.cached_input_tokens > 0 OR rollups.output_tokens > 0)
)
UPDATE request_token_stat_hourly_rollups
SET estimated_cost_usd = (
    SELECT CAST((
        (MAX(candidates.input_tokens, 0) - candidates.cached_input_tokens) * candidates.input_microusd_per_1m
        + candidates.cached_input_tokens * candidates.cached_input_microusd_per_1m
        + candidates.output_tokens * candidates.output_microusd_per_1m
        + 999999
    ) / 1000000 AS REAL) / 1000000.0
    FROM candidates
    WHERE candidates.rollup_rowid = request_token_stat_hourly_rollups.rowid
)
WHERE rowid IN (SELECT rollup_rowid FROM candidates);

WITH candidates AS (
    SELECT
        rollups.rowid AS rollup_rowid,
        MIN(MAX(rollups.cached_input_tokens, 0), MAX(rollups.input_tokens, 0)) AS cached_input_tokens,
        MAX(rollups.input_tokens, 0) AS input_tokens,
        MAX(rollups.output_tokens, 0) AS output_tokens,
        tiers.input_microusd_per_1m,
        tiers.cached_input_microusd_per_1m,
        tiers.output_microusd_per_1m
    FROM request_token_stat_rollups rollups
    JOIN models ON models.slug = rollups.model COLLATE NOCASE
    JOIN model_prices prices
      ON prices.model_id = models.id
     AND prices.price_status <> 'missing'
    JOIN model_price_tiers tiers
      ON tiers.model_id = models.id
     AND tiers.min_input_tokens = 0
    WHERE rollups.estimated_cost_usd <= 0
      AND (rollups.input_tokens > 0 OR rollups.cached_input_tokens > 0 OR rollups.output_tokens > 0)
)
UPDATE request_token_stat_rollups
SET estimated_cost_usd = (
    SELECT CAST((
        (candidates.input_tokens - candidates.cached_input_tokens) * candidates.input_microusd_per_1m
        + candidates.cached_input_tokens * candidates.cached_input_microusd_per_1m
        + candidates.output_tokens * candidates.output_microusd_per_1m
        + 999999
    ) / 1000000 AS REAL) / 1000000.0
    FROM candidates
    WHERE candidates.rollup_rowid = request_token_stat_rollups.rowid
)
WHERE rowid IN (SELECT rollup_rowid FROM candidates);

DROP TABLE _actual_usage_billing_repair;
COMMIT;
