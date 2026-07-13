import type {
  ApiKeyDailyUsagePoint,
  ApiKeyUsageHistory,
  ApiKeyUsageHistoryUsage,
} from "@/types";

function asRecord(value: unknown): Record<string, unknown> {
  return value && typeof value === "object"
    ? (value as Record<string, unknown>)
    : {};
}

function readNumber(
  source: Record<string, unknown>,
  camelKey: string,
  snakeKey: string,
): number {
  const value = Number(source[camelKey] ?? source[snakeKey]);
  return Number.isFinite(value) ? Math.max(0, value) : 0;
}

function readInteger(
  source: Record<string, unknown>,
  camelKey: string,
  snakeKey: string,
): number {
  return Math.trunc(readNumber(source, camelKey, snakeKey));
}

function readUsage(payload: unknown): ApiKeyUsageHistoryUsage {
  const source = asRecord(payload);
  return {
    inputTokens: readInteger(source, "inputTokens", "input_tokens"),
    cachedInputTokens: readInteger(
      source,
      "cachedInputTokens",
      "cached_input_tokens",
    ),
    outputTokens: readInteger(source, "outputTokens", "output_tokens"),
    reasoningOutputTokens: readInteger(
      source,
      "reasoningOutputTokens",
      "reasoning_output_tokens",
    ),
    totalTokens: readInteger(source, "totalTokens", "total_tokens"),
    estimatedCostUsd: readNumber(
      source,
      "estimatedCostUsd",
      "estimated_cost_usd",
    ),
    requestCount: readInteger(source, "requestCount", "request_count"),
    successCount: readInteger(source, "successCount", "success_count"),
    errorCount: readInteger(source, "errorCount", "error_count"),
  };
}

function readDailyPoint(payload: unknown): ApiKeyDailyUsagePoint | null {
  const source = asRecord(payload);
  const dayStartTs = readInteger(source, "dayStartTs", "day_start_ts");
  const dayEndTs = readInteger(source, "dayEndTs", "day_end_ts");
  if (dayEndTs <= dayStartTs) return null;
  return {
    dayStartTs,
    dayEndTs,
    usage: readUsage(source.usage),
  };
}

export function readApiKeyUsageHistory(payload: unknown): ApiKeyUsageHistory {
  const source = asRecord(payload);
  const keyId = String(source.keyId ?? source.key_id ?? "").trim();
  const dailySource = source.dailyUsage ?? source.daily_usage;
  const dailyUsage = Array.isArray(dailySource)
    ? dailySource
        .map(readDailyPoint)
        .filter((item): item is ApiKeyDailyUsagePoint => Boolean(item))
    : [];

  return {
    keyId,
    rangeStartTs: readInteger(source, "rangeStartTs", "range_start_ts"),
    rangeEndTs: readInteger(source, "rangeEndTs", "range_end_ts"),
    usage: readUsage(source.usage),
    dailyUsage,
  };
}
