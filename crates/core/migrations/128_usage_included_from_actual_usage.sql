UPDATE request_token_stats
SET usage_included = CASE
  WHEN EXISTS (
    SELECT 1
    FROM request_charge_snapshots snapshots
    WHERE snapshots.request_log_id = request_token_stats.request_log_id
      AND snapshots.usage_source = 'actual'
  )
    OR input_tokens IS NOT NULL
    OR cached_input_tokens IS NOT NULL
    OR output_tokens IS NOT NULL
    OR total_tokens IS NOT NULL
    OR reasoning_output_tokens IS NOT NULL
  THEN 1
  ELSE 0
END;
