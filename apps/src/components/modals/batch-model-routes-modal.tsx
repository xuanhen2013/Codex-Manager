"use client";

import { useState } from "react";
import { GitBranch, Plus, Trash2 } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button, buttonVariants } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useI18n } from "@/lib/i18n/provider";
import type { AggregateApi } from "@/types/api-key";
import type {
  ManagedModelBatchRouteAssignmentV2,
  ManagedModelBatchRouteResultV2,
  ModelRouteBatchModeV2,
  ModelRouteSourceKindV2,
} from "@/types/model-v2";

type BatchRouteDraft = {
  key: string;
  sourceKind: ModelRouteSourceKindV2;
  sourceId: string;
  priority: string;
  weight: string;
};

interface BatchModelRoutesModalProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  selectedSlugs: string[];
  aggregateApis?: AggregateApi[];
  isSaving?: boolean;
  onApply: (
    input: ManagedModelBatchRouteAssignmentV2,
  ) => Promise<ManagedModelBatchRouteResultV2 | null>;
}

function createRouteDraft(
  sourceKind: ModelRouteSourceKindV2,
  sourceId = "",
): BatchRouteDraft {
  return {
    key: `batch-route-${Date.now()}-${Math.random().toString(36).slice(2)}`,
    sourceKind,
    sourceId: sourceKind === "account_pool" ? "default" : sourceId,
    priority: "0",
    weight: "1",
  };
}

function integer(value: string, minimum?: number): number | null {
  const parsed = Number(value.trim());
  if (!Number.isSafeInteger(parsed) || (minimum != null && parsed < minimum)) {
    return null;
  }
  return parsed;
}

export function BatchModelRoutesModal({
  open,
  onOpenChange,
  selectedSlugs,
  aggregateApis = [],
  isSaving = false,
  onApply,
}: BatchModelRoutesModalProps) {
  const { t } = useI18n();
  const [mode, setMode] = useState<ModelRouteBatchModeV2>("merge");
  const [routes, setRoutes] = useState<BatchRouteDraft[]>(() => [
    createRouteDraft("account_pool"),
  ]);
  const [error, setError] = useState<string | null>(null);

  const updateRoute = <K extends keyof BatchRouteDraft>(
    index: number,
    key: K,
    value: BatchRouteDraft[K],
  ) => {
    setRoutes((current) =>
      current.map((route, routeIndex) =>
        routeIndex === index ? { ...route, [key]: value } : route,
      ),
    );
  };

  const addRoute = (sourceKind: ModelRouteSourceKindV2) => {
    setRoutes((current) => [
      ...current,
      createRouteDraft(sourceKind, aggregateApis[0]?.id || ""),
    ]);
  };

  const handleApply = async () => {
    try {
      setError(null);
      if (selectedSlugs.length === 0) {
        throw new Error(t("请至少选择一个模型"));
      }
      if (routes.length === 0) {
        throw new Error(t("请至少配置一条路由"));
      }

      const normalizedRoutes = routes.map((route) => {
        const sourceId =
          route.sourceKind === "account_pool" ? "default" : route.sourceId.trim();
        const priority = integer(route.priority);
        const weight = integer(route.weight, 1);
        if (!sourceId) throw new Error(t("请选择聚合 API"));
        if (priority == null) throw new Error(t("路由优先级必须是整数"));
        if (weight == null) throw new Error(t("路由权重必须是正整数"));
        return {
          sourceKind: route.sourceKind,
          sourceId,
          priority,
          weight,
        };
      });
      const uniqueSources = new Set(
        normalizedRoutes.map((route) => `${route.sourceKind}\u0000${route.sourceId}`),
      );
      if (uniqueSources.size !== normalizedRoutes.length) {
        throw new Error(t("不能重复分配同一个路由来源"));
      }

      const result = await onApply({
        slugs: selectedSlugs,
        mode,
        routes: normalizedRoutes,
      });
      if (result && result.failed.length === 0) onOpenChange(false);
    } catch (applyError) {
      setError(
        applyError instanceof Error ? applyError.message : String(applyError),
      );
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="glass-card p-0 sm:max-w-[820px]">
        <div className="max-h-[78vh] space-y-5 overflow-y-auto p-5">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <GitBranch className="h-5 w-5 text-primary" />
              {t("批量分配模型路由")}
            </DialogTitle>
            <DialogDescription>
              {t("已选择 {count} 个模型；每条路由的上游模型名会自动使用对应模型标识。", {
                count: selectedSlugs.length,
              })}
            </DialogDescription>
          </DialogHeader>

          <div className="flex max-h-20 flex-wrap gap-1.5 overflow-y-auto rounded-lg border border-border/60 bg-muted/20 p-2.5">
            {selectedSlugs.map((slug) => (
              <Badge key={slug} variant="secondary" className="font-mono text-[10px]">
                {slug}
              </Badge>
            ))}
          </div>

          <div className="space-y-2">
            <Label htmlFor="batch-route-mode">{t("分配方式")}</Label>
            <Select
              value={mode}
              onValueChange={(value) =>
                setMode((value || "merge") as ModelRouteBatchModeV2)
              }
            >
              <SelectTrigger
                id="batch-route-mode"
                className="w-full"
                aria-label={t("分配方式")}
              >
                <SelectValue>
                  {(value) =>
                    value === "replace" ? t("替换全部现有路由") : t("追加或更新路由")
                  }
                </SelectValue>
              </SelectTrigger>
              <SelectContent>
                <SelectGroup>
                  <SelectItem value="merge">{t("追加或更新路由")}</SelectItem>
                  <SelectItem value="replace">{t("替换全部现有路由")}</SelectItem>
                </SelectGroup>
              </SelectContent>
            </Select>
            <p className="text-xs text-muted-foreground">
              {mode === "replace"
                ? t("将删除所选模型的其他路由，仅保留下方配置。")
                : t("同来源路由会更新，其他现有路由保持不变。")}
            </p>
          </div>

          <div className="space-y-3">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <Label>{t("要分配的路由")}</Label>
              <div className="flex gap-2">
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  onClick={() => addRoute("account_pool")}
                >
                  <Plus className="mr-1 h-3.5 w-3.5" />
                  {t("添加账号池路由")}
                </Button>
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  onClick={() => addRoute("aggregate_api")}
                >
                  <Plus className="mr-1 h-3.5 w-3.5" />
                  {t("添加聚合路由")}
                </Button>
              </div>
            </div>

            {routes.length === 0 ? (
              <div className="rounded-lg border border-dashed p-8 text-center text-sm text-muted-foreground">
                {t("请添加至少一条要分配的路由。")}
              </div>
            ) : (
              routes.map((route, index) => (
                <Card key={route.key} size="sm">
                  <CardContent className="grid items-end gap-3 md:grid-cols-[170px_minmax(0,1fr)_110px_110px_auto]">
                    <div className="space-y-2">
                      <Label htmlFor={`batch-route-kind-${index}`}>
                        {t("来源类型")}
                      </Label>
                      <Select
                        value={route.sourceKind}
                        onValueChange={(value) => {
                          const sourceKind = (value ||
                            "account_pool") as ModelRouteSourceKindV2;
                          updateRoute(index, "sourceKind", sourceKind);
                          updateRoute(
                            index,
                            "sourceId",
                            sourceKind === "account_pool"
                              ? "default"
                              : aggregateApis[0]?.id || "",
                          );
                        }}
                      >
                        <SelectTrigger
                          id={`batch-route-kind-${index}`}
                          className="w-full"
                          aria-label={t("来源类型")}
                        >
                          <SelectValue>
                            {(value) =>
                              value === "aggregate_api" ? t("聚合 API") : t("账号池")
                            }
                          </SelectValue>
                        </SelectTrigger>
                        <SelectContent>
                          <SelectGroup>
                            <SelectItem value="account_pool">{t("账号池")}</SelectItem>
                            <SelectItem value="aggregate_api">{t("聚合 API")}</SelectItem>
                          </SelectGroup>
                        </SelectContent>
                      </Select>
                    </div>

                    <div className="space-y-2">
                      <Label htmlFor={`batch-route-source-${index}`}>
                        {t("来源")}
                      </Label>
                      {route.sourceKind === "aggregate_api" && aggregateApis.length > 0 ? (
                        <Select
                          value={route.sourceId}
                          onValueChange={(value) =>
                            updateRoute(index, "sourceId", value || "")
                          }
                        >
                          <SelectTrigger
                            id={`batch-route-source-${index}`}
                            className="w-full"
                            aria-label={t("来源")}
                          >
                            <SelectValue placeholder={t("选择聚合 API")} />
                          </SelectTrigger>
                          <SelectContent>
                            <SelectGroup>
                              {aggregateApis.map((api) => (
                                <SelectItem key={api.id} value={api.id}>
                                  {api.supplierName || api.id}
                                </SelectItem>
                              ))}
                            </SelectGroup>
                          </SelectContent>
                        </Select>
                      ) : (
                        <Input
                          id={`batch-route-source-${index}`}
                          value={route.sourceId}
                          disabled={route.sourceKind === "account_pool"}
                          placeholder={t("聚合 API ID")}
                          onChange={(event) =>
                            updateRoute(index, "sourceId", event.target.value)
                          }
                        />
                      )}
                    </div>

                    <div className="space-y-2">
                      <Label htmlFor={`batch-route-priority-${index}`}>
                        {t("优先级")}
                      </Label>
                      <Input
                        id={`batch-route-priority-${index}`}
                        type="number"
                        value={route.priority}
                        onChange={(event) =>
                          updateRoute(index, "priority", event.target.value)
                        }
                      />
                    </div>

                    <div className="space-y-2">
                      <Label htmlFor={`batch-route-weight-${index}`}>
                        {t("权重")}
                      </Label>
                      <Input
                        id={`batch-route-weight-${index}`}
                        type="number"
                        min="1"
                        value={route.weight}
                        onChange={(event) =>
                          updateRoute(index, "weight", event.target.value)
                        }
                      />
                    </div>

                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      aria-label={t("删除第 {index} 条批量路由", { index: index + 1 })}
                      onClick={() =>
                        setRoutes((current) =>
                          current.filter((_, routeIndex) => routeIndex !== index),
                        )
                      }
                    >
                      <Trash2 className="h-4 w-4" />
                    </Button>
                  </CardContent>
                </Card>
              ))
            )}
          </div>

          {error ? <p className="text-sm text-destructive">{error}</p> : null}
        </div>

        <div className="border-t border-border/50 px-5 py-3">
          <DialogFooter>
            <DialogClose
              className={buttonVariants({ variant: "ghost" })}
              type="button"
            >
              {t("取消")}
            </DialogClose>
            <Button
              type="button"
              disabled={isSaving || selectedSlugs.length === 0}
              onClick={() => void handleApply()}
            >
              {isSaving ? t("保存中...") : t("应用到 {count} 个模型", { count: selectedSlugs.length })}
            </Button>
          </DialogFooter>
        </div>
      </DialogContent>
    </Dialog>
  );
}
