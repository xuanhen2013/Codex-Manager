import { invoke, withAddr } from "./transport";
import type {
  ProxyBatchImportResult,
  ProxyPool,
  ProxyPoolHealthCycleResult,
  ProxyPoolMember,
} from "@/types";

export const PROXY_POOLS_QUERY_KEY = ["proxy-pools"] as const;

export interface ProxyPoolWritePayload {
  id?: string;
  name: string;
  description?: string | null;
  enabled?: boolean;
  failureThreshold?: number;
  recoveryThreshold?: number;
  heartbeatIntervalSecs?: number;
  cooldownSecs?: number;
}

export interface ProxyBatchImportPayload {
  text: string;
  poolId?: string | null;
  defaultScheme?: string;
  namePrefix?: string | null;
}

function object(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};
}

function text(value: unknown): string {
  return typeof value === "string" ? value.trim() : "";
}

function number(value: unknown, fallback = 0): number {
  const parsed = typeof value === "number" ? value : Number(value);
  return Number.isFinite(parsed) ? parsed : fallback;
}

function bool(value: unknown, fallback = false): boolean {
  if (typeof value === "boolean") return value;
  if (typeof value === "number") return value !== 0;
  return fallback;
}

function nullableNumber(value: unknown): number | null {
  if (value == null || value === "") return null;
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : null;
}

function normalizeMember(value: unknown): ProxyPoolMember {
  const source = object(value);
  return {
    proxyProfileId: text(source.proxyProfileId ?? source.proxy_profile_id),
    proxyProfileName: text(source.proxyProfileName ?? source.proxy_profile_name),
    proxyUrlRedacted: text(source.proxyUrlRedacted ?? source.proxy_url_redacted),
    sortOrder: number(source.sortOrder ?? source.sort_order),
    enabled: bool(source.enabled, true),
    profileEnabled: bool(source.profileEnabled ?? source.profile_enabled, true),
    healthStatus: text(source.healthStatus ?? source.health_status) || "unchecked",
    consecutiveFailures: number(
      source.consecutiveFailures ?? source.consecutive_failures,
    ),
    consecutiveSuccesses: number(
      source.consecutiveSuccesses ?? source.consecutive_successes,
    ),
    cooldownUntil: nullableNumber(source.cooldownUntil ?? source.cooldown_until),
    lastCheckAt: nullableNumber(source.lastCheckAt ?? source.last_check_at),
    lastError: text(source.lastError ?? source.last_error) || null,
    latencyMs: nullableNumber(source.latencyMs ?? source.latency_ms),
    currentAccountsCount: number(
      source.currentAccountsCount ?? source.current_accounts_count,
    ),
  };
}

export function normalizeProxyPool(value: unknown): ProxyPool {
  const source = object(value);
  return {
    id: text(source.id),
    name: text(source.name),
    description: text(source.description) || null,
    enabled: bool(source.enabled, true),
    failureThreshold: number(source.failureThreshold ?? source.failure_threshold, 3),
    recoveryThreshold: number(source.recoveryThreshold ?? source.recovery_threshold, 2),
    heartbeatIntervalSecs: number(
      source.heartbeatIntervalSecs ?? source.heartbeat_interval_secs,
      60,
    ),
    cooldownSecs: number(source.cooldownSecs ?? source.cooldown_secs, 300),
    accountsCount: number(source.accountsCount ?? source.accounts_count),
    healthyMembersCount: number(
      source.healthyMembersCount ?? source.healthy_members_count,
    ),
    members: Array.isArray(source.members)
      ? source.members.map(normalizeMember)
      : [],
    createdAt: number(source.createdAt ?? source.created_at),
    updatedAt: number(source.updatedAt ?? source.updated_at),
  };
}

export function normalizeProxyBatchImportResult(value: unknown): ProxyBatchImportResult {
  const source = object(value);
  return {
    total: number(source.total),
    created: number(source.created),
    duplicates: number(source.duplicates),
    failed: number(source.failed),
    items: Array.isArray(source.items)
      ? source.items.map((item) => {
          const entry = object(item);
          const status = text(entry.status);
          return {
            line: number(entry.line),
            status: ["created", "duplicate", "failed"].includes(status)
              ? (status as "created" | "duplicate" | "failed")
              : "failed",
            name: text(entry.name),
            proxyProfileId:
              text(entry.proxyProfileId ?? entry.proxy_profile_id) || null,
            proxyUrlRedacted:
              text(entry.proxyUrlRedacted ?? entry.proxy_url_redacted) || null,
            error: text(entry.error) || null,
          };
        })
      : [],
  };
}

export function normalizeProxyPoolHealthCycleResult(
  value: unknown,
): ProxyPoolHealthCycleResult {
  const source = object(value);
  return {
    checked: number(source.checked),
    healthy: number(source.healthy),
    failed: number(source.failed),
    switchedAccounts: number(
      source.switchedAccounts ?? source.switched_accounts,
    ),
  };
}

export const proxyPoolsClient = {
  async list(): Promise<ProxyPool[]> {
    const result = await invoke<unknown>("service_system_proxy_pool_list", withAddr());
    return Array.isArray(result) ? result.map(normalizeProxyPool) : [];
  },

  async create(payload: ProxyPoolWritePayload): Promise<ProxyPool> {
    return normalizeProxyPool(
      await invoke<unknown>(
        "service_system_proxy_pool_create",
        withAddr({ ...payload }),
      ),
    );
  },

  async update(payload: ProxyPoolWritePayload & { id: string }): Promise<ProxyPool> {
    return normalizeProxyPool(
      await invoke<unknown>(
        "service_system_proxy_pool_update",
        withAddr({ ...payload, poolId: payload.id }),
      ),
    );
  },

  async delete(id: string): Promise<void> {
    await invoke("service_system_proxy_pool_delete", withAddr({ poolId: id }));
  },

  async addMembers(poolId: string, proxyProfileIds: string[]): Promise<ProxyPool> {
    return normalizeProxyPool(
      await invoke<unknown>(
        "service_system_proxy_pool_members_add",
        withAddr({ poolId, proxyProfileIds }),
      ),
    );
  },

  async removeMember(poolId: string, proxyProfileId: string): Promise<ProxyPool> {
    return normalizeProxyPool(
      await invoke<unknown>(
        "service_system_proxy_pool_members_remove",
        withAddr({ poolId, id: proxyProfileId }),
      ),
    );
  },

  async setMemberEnabled(
    poolId: string,
    proxyProfileId: string,
    enabled: boolean,
  ): Promise<ProxyPool> {
    return normalizeProxyPool(
      await invoke<unknown>(
        "service_system_proxy_pool_members_set_enabled",
        withAddr({ poolId, id: proxyProfileId, enabled }),
      ),
    );
  },

  async importBatch(payload: ProxyBatchImportPayload): Promise<ProxyBatchImportResult> {
    return normalizeProxyBatchImportResult(
      await invoke<unknown>(
        "service_system_proxy_import_batch",
        withAddr({ ...payload }),
      ),
    );
  },

  async checkHealth(): Promise<ProxyPoolHealthCycleResult> {
    return normalizeProxyPoolHealthCycleResult(
      await invoke<unknown>(
        "service_system_proxy_pool_health_check",
        withAddr(),
      ),
    );
  },
};
