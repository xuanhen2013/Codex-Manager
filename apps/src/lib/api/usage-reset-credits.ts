import type {
  AccountUsageResetConsumeResult,
  AccountUsageResetCredit,
  AccountUsageResetCredits,
} from "@/types";

function asRecord(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};
}

function asString(value: unknown): string {
  return typeof value === "string" ? value.trim() : "";
}

function asNullableInteger(value: unknown): number | null {
  if (typeof value === "number" && Number.isFinite(value)) {
    return Math.trunc(value);
  }
  if (typeof value === "string" && value.trim()) {
    const parsed = Number(value);
    return Number.isFinite(parsed) ? Math.trunc(parsed) : null;
  }
  return null;
}

function readCredit(value: unknown): AccountUsageResetCredit | null {
  const source = asRecord(value);
  const expiresAt = asString(source.expiresAt ?? source.expires_at);
  if (!expiresAt) return null;
  return {
    id: asString(source.id),
    status: asString(source.status),
    grantedAt: asString(source.grantedAt ?? source.granted_at),
    expiresAt,
  };
}

export function readUsageResetCredits(value: unknown): AccountUsageResetCredits {
  const source = asRecord(value);
  const credits = Array.isArray(source.credits)
    ? source.credits
        .map(readCredit)
        .filter((credit): credit is AccountUsageResetCredit => Boolean(credit))
    : [];
  const availableCount = asNullableInteger(source.availableCount ?? source.available_count);
  return {
    availableCount: availableCount ?? (credits.length > 0 ? credits.length : null),
    credits,
  };
}

export function readUsageResetConsumeResult(
  value: unknown,
): AccountUsageResetConsumeResult {
  const source = asRecord(value);
  const resetCreditsSource = source.resetCredits ?? source.reset_credits;
  return {
    resetApplied: source.resetApplied === true || source.reset_applied === true,
    resetCredits: resetCreditsSource ? readUsageResetCredits(resetCreditsSource) : null,
    usageRefreshed: source.usageRefreshed === true || source.usage_refreshed === true,
    warning: asString(source.warning) || null,
  };
}
