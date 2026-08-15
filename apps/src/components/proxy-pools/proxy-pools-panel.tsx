"use client";

import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  ChevronDown,
  ChevronRight,
  HeartPulse,
  Layers3,
  Loader2,
  Pencil,
  Plus,
  RefreshCw,
  Trash2,
  Upload,
  X,
} from "lucide-react";
import { toast } from "sonner";
import { ConfirmDialog } from "@/components/modals/confirm-dialog";
import { ProxyBatchImportModal } from "@/components/modals/proxy-batch-import-modal";
import { ProxyPoolModal } from "@/components/modals/proxy-pool-modal";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { getAppErrorMessage } from "@/lib/api/transport";
import {
  PROXY_POOLS_QUERY_KEY,
  proxyPoolsClient,
} from "@/lib/api/proxy-pools";
import {
  PROXY_PROFILES_QUERY_KEY,
  proxyProfilesClient,
} from "@/lib/api/proxy-profiles";
import { useI18n } from "@/lib/i18n/provider";
import type { ProxyPool } from "@/types";

function healthVariant(status: string): "default" | "secondary" | "destructive" | "outline" {
  if (status === "healthy") return "default";
  if (["unhealthy", "failed"].includes(status)) return "destructive";
  if (["suspect", "recovering"].includes(status)) return "secondary";
  return "outline";
}

function formatTime(timestamp: number | null): string {
  return timestamp ? new Date(timestamp * 1000).toLocaleString() : "-";
}

export function ProxyPoolsPanel({ canManage }: { canManage: boolean }) {
  const { t } = useI18n();
  const queryClient = useQueryClient();
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const [modalOpen, setModalOpen] = useState(false);
  const [editingPool, setEditingPool] = useState<ProxyPool | null>(null);
  const [importOpen, setImportOpen] = useState(false);
  const [importPoolId, setImportPoolId] = useState<string | null>(null);
  const [deletePool, setDeletePool] = useState<ProxyPool | null>(null);

  const poolsQuery = useQuery({
    queryKey: PROXY_POOLS_QUERY_KEY,
    queryFn: () => proxyPoolsClient.list(),
    enabled: canManage,
    refetchInterval: 30_000,
  });
  const profilesQuery = useQuery({
    queryKey: PROXY_PROFILES_QUERY_KEY,
    queryFn: () => proxyProfilesClient.listProxyProfiles(),
    enabled: canManage,
  });

  const invalidate = async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: PROXY_POOLS_QUERY_KEY }),
      queryClient.invalidateQueries({ queryKey: PROXY_PROFILES_QUERY_KEY }),
    ]);
  };

  const deleteMutation = useMutation({
    mutationFn: (id: string) => proxyPoolsClient.delete(id),
    onSuccess: async () => {
      await invalidate();
      toast.success(t("代理池已删除"));
    },
    onError: (error) => toast.error(getAppErrorMessage(error)),
  });
  const removeMutation = useMutation({
    mutationFn: ({ poolId, profileId }: { poolId: string; profileId: string }) =>
      proxyPoolsClient.removeMember(poolId, profileId),
    onSuccess: invalidate,
    onError: (error) => toast.error(getAppErrorMessage(error)),
  });
  const healthMutation = useMutation({
    mutationFn: () => proxyPoolsClient.checkHealth(),
    onSuccess: async (result) => {
      await invalidate();
      toast.success(
        t("已检查 {checked} 条代理，切换 {switched} 个账号", {
          checked: result.checked,
          switched: result.switchedAccounts,
        }),
      );
    },
    onError: (error) => toast.error(getAppErrorMessage(error)),
  });

  const pools = poolsQuery.data ?? [];
  const profiles = profilesQuery.data?.items ?? [];

  return (
    <>
      <Card className="glass-card shadow-sm">
        <div className="flex flex-col gap-3 border-b p-6 sm:flex-row sm:items-center sm:justify-between">
          <div className="flex min-w-0 items-center gap-3">
            <Layers3 className="size-5 text-primary" />
            <div className="min-w-0">
              <h3 className="text-sm font-semibold">{t("代理池")}</h3>
              <p className="truncate text-xs text-muted-foreground">
                {t("账号只在绑定的代理池内自动切换。")}
              </p>
            </div>
            <Badge variant="outline">{pools.length}</Badge>
          </div>
          <div className="flex flex-wrap gap-2">
            <Button
              size="sm"
              variant="outline"
              disabled={!canManage || healthMutation.isPending}
              onClick={() => healthMutation.mutate()}
            >
              {healthMutation.isPending ? (
                <Loader2 className="animate-spin" />
              ) : (
                <HeartPulse />
              )}
              {t("立即检测")}
            </Button>
            <Button
              size="sm"
              variant="outline"
              disabled={!canManage}
              onClick={() => {
                setImportPoolId(null);
                setImportOpen(true);
              }}
            >
              <Upload />
              {t("批量导入")}
            </Button>
            <Button
              size="sm"
              disabled={!canManage}
              onClick={() => {
                setEditingPool(null);
                setModalOpen(true);
              }}
            >
              <Plus />
              {t("新建代理池")}
            </Button>
          </div>
        </div>
        <CardContent className="pt-6">
          {poolsQuery.isLoading ? (
            <div className="flex h-32 items-center justify-center text-muted-foreground">
              <Loader2 className="animate-spin" />
            </div>
          ) : poolsQuery.isError ? (
            <div className="flex items-center justify-between border p-4 text-sm text-destructive">
              <span>{getAppErrorMessage(poolsQuery.error)}</span>
              <Button size="sm" variant="outline" onClick={() => poolsQuery.refetch()}>
                <RefreshCw />{t("重试")}
              </Button>
            </div>
          ) : pools.length === 0 ? (
            <div className="flex min-h-32 flex-col items-center justify-center gap-3 border border-dashed text-center">
              <Layers3 className="size-6 text-muted-foreground" />
              <p className="text-sm text-muted-foreground">{t("暂无代理池")}</p>
            </div>
          ) : (
            <div className="overflow-x-auto border">
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead className="w-10" />
                    <TableHead>{t("代理池")}</TableHead>
                    <TableHead>{t("代理")}</TableHead>
                    <TableHead>{t("绑定账号")}</TableHead>
                    <TableHead>{t("心跳策略")}</TableHead>
                    <TableHead className="text-right">{t("操作")}</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {pools.map((pool) => {
                    const expanded = expandedId === pool.id;
                    return (
                      <TableRow key={pool.id}>
                        <TableCell>
                          <Button
                            size="icon-sm"
                            variant="ghost"
                            aria-label={expanded ? t("收起") : t("展开")}
                            onClick={() => setExpandedId(expanded ? null : pool.id)}
                          >
                            {expanded ? <ChevronDown /> : <ChevronRight />}
                          </Button>
                        </TableCell>
                        <TableCell className="min-w-56">
                          <div className="font-medium">{pool.name}</div>
                          <div className="max-w-80 truncate text-xs text-muted-foreground">
                            {pool.description || t("无备注")}
                          </div>
                          {!pool.enabled && <Badge variant="secondary">{t("已禁用")}</Badge>}
                          {expanded && (
                            <div className="mt-3 grid gap-2 border-t pt-3">
                              {pool.members.length ? (
                                pool.members.map((member) => (
                                  <div
                                    key={member.proxyProfileId}
                                    className="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-3 border-b pb-2 last:border-b-0"
                                  >
                                    <div className="min-w-0">
                                      <div className="flex items-center gap-2">
                                        <span className="truncate text-sm">{member.proxyProfileName}</span>
                                        <Badge variant={healthVariant(member.healthStatus)}>
                                          {member.healthStatus}
                                        </Badge>
                                      </div>
                                      <div className="truncate text-xs text-muted-foreground">
                                        {member.proxyUrlRedacted} · {member.currentAccountsCount} {t("个账号")} · {formatTime(member.lastCheckAt)}
                                      </div>
                                      {member.lastError && (
                                        <div className="truncate text-xs text-destructive">{member.lastError}</div>
                                      )}
                                    </div>
                                    <Button
                                      size="icon-sm"
                                      variant="ghost"
                                      aria-label={t("移出代理池")}
                                      disabled={member.currentAccountsCount > 0 || removeMutation.isPending}
                                      onClick={() =>
                                        removeMutation.mutate({
                                          poolId: pool.id,
                                          profileId: member.proxyProfileId,
                                        })
                                      }
                                    >
                                      <X />
                                    </Button>
                                  </div>
                                ))
                              ) : (
                                <p className="text-xs text-muted-foreground">{t("池内暂无代理")}</p>
                              )}
                            </div>
                          )}
                        </TableCell>
                        <TableCell>
                          <span className="font-medium">{pool.members.length}</span>
                          <span className="ml-1 text-xs text-muted-foreground">
                            / {pool.healthyMembersCount} {t("健康")}
                          </span>
                        </TableCell>
                        <TableCell>{pool.accountsCount}</TableCell>
                        <TableCell className="text-xs text-muted-foreground">
                          {pool.heartbeatIntervalSecs}s · {pool.failureThreshold} {t("次失败")}
                        </TableCell>
                        <TableCell>
                          <div className="flex justify-end gap-1">
                            <Button
                              size="icon-sm"
                              variant="ghost"
                              aria-label={t("导入代理")}
                              onClick={() => {
                                setImportPoolId(pool.id);
                                setImportOpen(true);
                              }}
                            >
                              <Upload />
                            </Button>
                            <Button
                              size="icon-sm"
                              variant="ghost"
                              aria-label={t("编辑代理池")}
                              onClick={() => {
                                setEditingPool(pool);
                                setModalOpen(true);
                              }}
                            >
                              <Pencil />
                            </Button>
                            <Button
                              size="icon-sm"
                              variant="ghost"
                              aria-label={t("删除代理池")}
                              disabled={pool.accountsCount > 0}
                              onClick={() => setDeletePool(pool)}
                            >
                              <Trash2 />
                            </Button>
                          </div>
                        </TableCell>
                      </TableRow>
                    );
                  })}
                </TableBody>
              </Table>
            </div>
          )}
        </CardContent>
      </Card>

      {modalOpen && (
        <ProxyPoolModal
          open
          onOpenChange={setModalOpen}
          pool={editingPool}
          profiles={profiles}
        />
      )}
      {importOpen && (
        <ProxyBatchImportModal
          open
          onOpenChange={setImportOpen}
          pools={pools}
          defaultPoolId={importPoolId}
        />
      )}
      <ConfirmDialog
        open={Boolean(deletePool)}
        onOpenChange={(open) => !open && setDeletePool(null)}
        title={t("删除代理池")}
        description={deletePool ? t("确定删除 {name} 吗？", { name: deletePool.name }) : ""}
        confirmText={t("删除")}
        confirmVariant="destructive"
        onConfirm={() => {
          if (deletePool) deleteMutation.mutate(deletePool.id);
          setDeletePool(null);
        }}
      />
    </>
  );
}
