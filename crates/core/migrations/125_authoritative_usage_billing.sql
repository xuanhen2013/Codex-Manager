BEGIN IMMEDIATE;

-- Estimated snapshots were created from request-body size rather than an upstream usage payload.
-- Remove only unbilled entries so a pre-existing wallet ledger remains auditable.
CREATE TEMP TABLE authoritative_usage_estimate_cleanup AS
SELECT
  snapshots.request_log_id,
  snapshots.created_at - (snapshots.created_at % 3600) AS bucket_start,
  COALESCE(NULLIF(TRIM(logs.key_id), ''), '') AS key_id,
  COALESCE(NULLIF(TRIM(logs.account_id), ''), '') AS account_id,
  COALESCE(NULLIF(TRIM(logs.model), ''), NULLIF(TRIM(snapshots.model_slug), ''), '') AS model,
  COALESCE(NULLIF(TRIM(logs.actual_source_kind), ''), '') AS actual_source_kind,
  COALESCE(NULLIF(TRIM(logs.actual_source_id), ''), '') AS actual_source_id,
  COALESCE((
    SELECT MIN(NULLIF(TRIM(owner.owner_user_id), ''))
    FROM api_key_owners owner
    WHERE owner.key_id = logs.key_id AND owner.owner_kind = 'user'
  ), '') AS owner_user_id,
  snapshots.input_tokens,
  snapshots.cached_input_tokens,
  snapshots.output_tokens,
  snapshots.input_tokens + snapshots.output_tokens AS total_tokens,
  CAST(snapshots.base_cost_microusd AS REAL) / 1000000.0 AS estimated_cost_usd,
  CASE WHEN logs.status_code BETWEEN 200 AND 299 THEN 1 ELSE 0 END AS success_count,
  CASE WHEN logs.status_code BETWEEN 200 AND 299 THEN 0 ELSE 1 END AS error_count,
  CASE
    WHEN EXISTS (
      SELECT 1 FROM request_token_stats stats
      WHERE stats.request_log_id = snapshots.request_log_id
    ) THEN 'raw'
    WHEN EXISTS (
      SELECT 1 FROM request_token_stat_hourly_rollups hourly
      WHERE hourly.bucket_start = snapshots.created_at - (snapshots.created_at % 3600)
        AND hourly.key_id = COALESCE(NULLIF(TRIM(logs.key_id), ''), '')
        AND hourly.account_id = COALESCE(NULLIF(TRIM(logs.account_id), ''), '')
        AND hourly.model = COALESCE(NULLIF(TRIM(logs.model), ''), NULLIF(TRIM(snapshots.model_slug), ''), '')
        AND hourly.actual_source_kind = COALESCE(NULLIF(TRIM(logs.actual_source_kind), ''), '')
        AND hourly.actual_source_id = COALESCE(NULLIF(TRIM(logs.actual_source_id), ''), '')
        AND hourly.owner_user_id = COALESCE((
          SELECT MIN(NULLIF(TRIM(owner.owner_user_id), ''))
          FROM api_key_owners owner
          WHERE owner.key_id = logs.key_id AND owner.owner_kind = 'user'
        ), '')
    ) THEN 'hourly'
    ELSE 'legacy'
  END AS storage_layer
FROM request_charge_snapshots snapshots
JOIN request_logs logs ON logs.id = snapshots.request_log_id
WHERE snapshots.usage_source = 'estimated'
  AND NOT EXISTS (
    SELECT 1 FROM app_wallet_ledger_entries ledger
    WHERE ledger.request_log_id = snapshots.request_log_id
      AND ledger.entry_kind = 'request_charge'
  );

UPDATE request_token_stat_hourly_rollups AS hourly
SET
  input_tokens = MAX(0, hourly.input_tokens - COALESCE((
    SELECT SUM(cleanup.input_tokens) FROM authoritative_usage_estimate_cleanup cleanup
    WHERE cleanup.storage_layer = 'hourly'
      AND cleanup.bucket_start = hourly.bucket_start
      AND cleanup.key_id = hourly.key_id
      AND cleanup.account_id = hourly.account_id
      AND cleanup.model = hourly.model
      AND cleanup.actual_source_kind = hourly.actual_source_kind
      AND cleanup.actual_source_id = hourly.actual_source_id
      AND cleanup.owner_user_id = hourly.owner_user_id
  ), 0)),
  cached_input_tokens = MAX(0, hourly.cached_input_tokens - COALESCE((
    SELECT SUM(cleanup.cached_input_tokens) FROM authoritative_usage_estimate_cleanup cleanup
    WHERE cleanup.storage_layer = 'hourly'
      AND cleanup.bucket_start = hourly.bucket_start
      AND cleanup.key_id = hourly.key_id
      AND cleanup.account_id = hourly.account_id
      AND cleanup.model = hourly.model
      AND cleanup.actual_source_kind = hourly.actual_source_kind
      AND cleanup.actual_source_id = hourly.actual_source_id
      AND cleanup.owner_user_id = hourly.owner_user_id
  ), 0)),
  output_tokens = MAX(0, hourly.output_tokens - COALESCE((
    SELECT SUM(cleanup.output_tokens) FROM authoritative_usage_estimate_cleanup cleanup
    WHERE cleanup.storage_layer = 'hourly'
      AND cleanup.bucket_start = hourly.bucket_start
      AND cleanup.key_id = hourly.key_id
      AND cleanup.account_id = hourly.account_id
      AND cleanup.model = hourly.model
      AND cleanup.actual_source_kind = hourly.actual_source_kind
      AND cleanup.actual_source_id = hourly.actual_source_id
      AND cleanup.owner_user_id = hourly.owner_user_id
  ), 0)),
  total_tokens = MAX(0, hourly.total_tokens - COALESCE((
    SELECT SUM(cleanup.total_tokens) FROM authoritative_usage_estimate_cleanup cleanup
    WHERE cleanup.storage_layer = 'hourly'
      AND cleanup.bucket_start = hourly.bucket_start
      AND cleanup.key_id = hourly.key_id
      AND cleanup.account_id = hourly.account_id
      AND cleanup.model = hourly.model
      AND cleanup.actual_source_kind = hourly.actual_source_kind
      AND cleanup.actual_source_id = hourly.actual_source_id
      AND cleanup.owner_user_id = hourly.owner_user_id
  ), 0)),
  estimated_cost_usd = MAX(0.0, hourly.estimated_cost_usd - COALESCE((
    SELECT SUM(cleanup.estimated_cost_usd) FROM authoritative_usage_estimate_cleanup cleanup
    WHERE cleanup.storage_layer = 'hourly'
      AND cleanup.bucket_start = hourly.bucket_start
      AND cleanup.key_id = hourly.key_id
      AND cleanup.account_id = hourly.account_id
      AND cleanup.model = hourly.model
      AND cleanup.actual_source_kind = hourly.actual_source_kind
      AND cleanup.actual_source_id = hourly.actual_source_id
      AND cleanup.owner_user_id = hourly.owner_user_id
  ), 0.0)),
  request_count = MAX(0, hourly.request_count - COALESCE((
    SELECT COUNT(*) FROM authoritative_usage_estimate_cleanup cleanup
    WHERE cleanup.storage_layer = 'hourly'
      AND cleanup.bucket_start = hourly.bucket_start
      AND cleanup.key_id = hourly.key_id
      AND cleanup.account_id = hourly.account_id
      AND cleanup.model = hourly.model
      AND cleanup.actual_source_kind = hourly.actual_source_kind
      AND cleanup.actual_source_id = hourly.actual_source_id
      AND cleanup.owner_user_id = hourly.owner_user_id
  ), 0)),
  success_count = MAX(0, hourly.success_count - COALESCE((
    SELECT SUM(cleanup.success_count) FROM authoritative_usage_estimate_cleanup cleanup
    WHERE cleanup.storage_layer = 'hourly'
      AND cleanup.bucket_start = hourly.bucket_start
      AND cleanup.key_id = hourly.key_id
      AND cleanup.account_id = hourly.account_id
      AND cleanup.model = hourly.model
      AND cleanup.actual_source_kind = hourly.actual_source_kind
      AND cleanup.actual_source_id = hourly.actual_source_id
      AND cleanup.owner_user_id = hourly.owner_user_id
  ), 0)),
  error_count = MAX(0, hourly.error_count - COALESCE((
    SELECT SUM(cleanup.error_count) FROM authoritative_usage_estimate_cleanup cleanup
    WHERE cleanup.storage_layer = 'hourly'
      AND cleanup.bucket_start = hourly.bucket_start
      AND cleanup.key_id = hourly.key_id
      AND cleanup.account_id = hourly.account_id
      AND cleanup.model = hourly.model
      AND cleanup.actual_source_kind = hourly.actual_source_kind
      AND cleanup.actual_source_id = hourly.actual_source_id
      AND cleanup.owner_user_id = hourly.owner_user_id
  ), 0)),
  updated_at = CAST(strftime('%s', 'now') AS INTEGER)
WHERE EXISTS (
  SELECT 1 FROM authoritative_usage_estimate_cleanup cleanup
  WHERE cleanup.storage_layer = 'hourly'
    AND cleanup.bucket_start = hourly.bucket_start
    AND cleanup.key_id = hourly.key_id
    AND cleanup.account_id = hourly.account_id
    AND cleanup.model = hourly.model
    AND cleanup.actual_source_kind = hourly.actual_source_kind
    AND cleanup.actual_source_id = hourly.actual_source_id
    AND cleanup.owner_user_id = hourly.owner_user_id
);

UPDATE request_token_stat_rollups AS legacy
SET
  input_tokens = MAX(0, legacy.input_tokens - COALESCE((
    SELECT SUM(cleanup.input_tokens) FROM authoritative_usage_estimate_cleanup cleanup
    WHERE cleanup.storage_layer = 'legacy'
      AND cleanup.key_id = legacy.key_id
      AND cleanup.account_id = legacy.account_id
      AND cleanup.model = legacy.model
  ), 0)),
  cached_input_tokens = MAX(0, legacy.cached_input_tokens - COALESCE((
    SELECT SUM(cleanup.cached_input_tokens) FROM authoritative_usage_estimate_cleanup cleanup
    WHERE cleanup.storage_layer = 'legacy'
      AND cleanup.key_id = legacy.key_id
      AND cleanup.account_id = legacy.account_id
      AND cleanup.model = legacy.model
  ), 0)),
  output_tokens = MAX(0, legacy.output_tokens - COALESCE((
    SELECT SUM(cleanup.output_tokens) FROM authoritative_usage_estimate_cleanup cleanup
    WHERE cleanup.storage_layer = 'legacy'
      AND cleanup.key_id = legacy.key_id
      AND cleanup.account_id = legacy.account_id
      AND cleanup.model = legacy.model
  ), 0)),
  total_tokens = MAX(0, legacy.total_tokens - COALESCE((
    SELECT SUM(cleanup.total_tokens) FROM authoritative_usage_estimate_cleanup cleanup
    WHERE cleanup.storage_layer = 'legacy'
      AND cleanup.key_id = legacy.key_id
      AND cleanup.account_id = legacy.account_id
      AND cleanup.model = legacy.model
  ), 0)),
  reasoning_output_tokens = MAX(0, legacy.reasoning_output_tokens),
  estimated_cost_usd = MAX(0.0, legacy.estimated_cost_usd - COALESCE((
    SELECT SUM(cleanup.estimated_cost_usd) FROM authoritative_usage_estimate_cleanup cleanup
    WHERE cleanup.storage_layer = 'legacy'
      AND cleanup.key_id = legacy.key_id
      AND cleanup.account_id = legacy.account_id
      AND cleanup.model = legacy.model
  ), 0.0)),
  source_rows = MAX(0, legacy.source_rows - COALESCE((
    SELECT COUNT(*) FROM authoritative_usage_estimate_cleanup cleanup
    WHERE cleanup.storage_layer = 'legacy'
      AND cleanup.key_id = legacy.key_id
      AND cleanup.account_id = legacy.account_id
      AND cleanup.model = legacy.model
  ), 0)),
  updated_at = CAST(strftime('%s', 'now') AS INTEGER)
WHERE EXISTS (
  SELECT 1 FROM authoritative_usage_estimate_cleanup cleanup
  WHERE cleanup.storage_layer = 'legacy'
    AND cleanup.key_id = legacy.key_id
    AND cleanup.account_id = legacy.account_id
    AND cleanup.model = legacy.model
);

DELETE FROM request_token_stats
WHERE request_log_id IN (
  SELECT request_log_id FROM authoritative_usage_estimate_cleanup
  WHERE storage_layer = 'raw'
);

DELETE FROM request_charge_snapshots
WHERE request_log_id IN (
  SELECT request_log_id FROM authoritative_usage_estimate_cleanup
);

DROP TABLE authoritative_usage_estimate_cleanup;
COMMIT;
