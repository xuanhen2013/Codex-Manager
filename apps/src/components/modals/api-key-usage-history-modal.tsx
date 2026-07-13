"use client";

import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  BarChart3,
  CalendarDays,
  CircleDollarSign,
  ListChecks,
  RefreshCw,
} from "lucide-react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { accountClient } from "@/lib/api/account-client";
import { getAppErrorMessage } from "@/lib/api/transport";
import { useI18n } from "@/lib/i18n/provider";
import {
  buildApiKeyUsageDateRange,
  createApiKeyUsagePresetRange,
  type ApiKeyUsageDateRange,
  type ApiKeyUsageRangePreset,
} from "@/lib/utils/api-key-usage-range";
import { cn } from "@/lib/utils";
import type { ApiKey, ApiKeyUsageHistoryUsage } from "@/types";

interface ApiKeyUsageHistoryModalProps {
  apiKey: ApiKey | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

interface PresetOption {
  value: ApiKeyUsageRangePreset;
  label: string;
}

function formatInteger(value: number, locale: string): string {
  return Math.max(0, Math.trunc(value)).toLocaleString(locale);
}

function formatSummaryInteger(value: number, locale: string): string {
  return new Intl.NumberFormat(locale, {
    notation: "compact",
    maximumFractionDigits: 2,
  }).format(Math.max(0, value));
}

function formatUsd(value: number): string {
  const normalized = Math.max(0, value);
  return normalized.toLocaleString("en-US", {
    style: "currency",
    currency: "USD",
    minimumFractionDigits: 2,
    maximumFractionDigits: normalized > 0 && normalized < 0.01 ? 6 : 4,
  });
}

function formatSuccessRate(usage: ApiKeyUsageHistoryUsage): string {
  if (usage.requestCount <= 0) return "--";
  return `${((usage.successCount / usage.requestCount) * 100).toFixed(1)}%`;
}

export function ApiKeyUsageHistoryModal({
  apiKey,
  open,
  onOpenChange,
}: ApiKeyUsageHistoryModalProps) {
  const { locale, t } = useI18n();
  const initialRange = useMemo(
    () => createApiKeyUsagePresetRange("this_month"),
    [],
  );
  const [preset, setPreset] =
    useState<ApiKeyUsageRangePreset>("this_month");
  const [startInput, setStartInput] = useState(initialRange.startInput);
  const [endInput, setEndInput] = useState(initialRange.endInput);
  const [appliedRange, setAppliedRange] =
    useState<ApiKeyUsageDateRange>(initialRange);
  const [rangeError, setRangeError] = useState<string | null>(null);
  const keyId = apiKey?.id ?? "";

  const presets: PresetOption[] = [
    { value: "last_7_days", label: t("最近 7 天") },
    { value: "last_30_days", label: t("最近 30 天") },
    { value: "this_month", label: t("本月") },
    { value: "last_month", label: t("上月") },
    { value: "custom", label: t("自定义") },
  ];

  const usageQuery = useQuery({
    queryKey: [
      "apikey-daily-usage",
      keyId,
      appliedRange.startTs,
      appliedRange.endTs,
      appliedRange.dayBoundariesTs,
    ],
    queryFn: () =>
      accountClient.readApiKeyDailyUsage(
        keyId,
        appliedRange.startTs,
        appliedRange.endTs,
        appliedRange.dayBoundariesTs,
      ),
    enabled: open && Boolean(keyId),
    retry: 1,
    staleTime: 30_000,
  });

  const selectPreset = (nextPreset: ApiKeyUsageRangePreset) => {
    setPreset(nextPreset);
    setRangeError(null);
    if (nextPreset === "custom") return;
    const range = createApiKeyUsagePresetRange(nextPreset);
    setStartInput(range.startInput);
    setEndInput(range.endInput);
    setAppliedRange(range);
  };

  const applyCustomRange = () => {
    const range = buildApiKeyUsageDateRange(startInput, endInput);
    if (!range) {
      setRangeError(t("日期范围无效"));
      return;
    }
    if (range.dayBoundariesTs.length - 1 > 366) {
      setRangeError(t("最多可查询 366 天"));
      return;
    }
    setRangeError(null);
    setAppliedRange(range);
  };

  const dailyUsage = useMemo(
    () => [...(usageQuery.data?.dailyUsage ?? [])].reverse(),
    [usageQuery.data?.dailyUsage],
  );
  const hasUsage = Boolean(
    usageQuery.data &&
      (usageQuery.data.usage.totalTokens > 0 ||
        usageQuery.data.usage.estimatedCostUsd > 0 ||
        usageQuery.data.usage.requestCount > 0),
  );

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className="flex max-h-[90dvh] flex-col gap-0 overflow-hidden p-0"
        style={{
          maxWidth: "none",
          width: "min(1000px, calc(100vw - 2rem))",
        }}
        data-testid="api-key-usage-history-modal"
      >
        <DialogHeader className="border-b px-5 py-4 pr-14 sm:px-6">
          <div className="flex items-center gap-3">
            <div className="flex size-9 shrink-0 items-center justify-center rounded-md bg-primary/10 text-primary">
              <BarChart3 className="size-4" />
            </div>
            <div className="min-w-0">
              <DialogTitle>{t("每日用量")}</DialogTitle>
              <DialogDescription className="mt-1 truncate">
                {apiKey?.name || t("未命名")} · {keyId}
              </DialogDescription>
            </div>
          </div>
        </DialogHeader>

        <div className="border-b bg-muted/20 px-5 py-4 sm:px-6">
          <div className="grid grid-cols-3 gap-1 rounded-md bg-muted p-1 sm:flex sm:items-center">
              {presets.map((option) => (
                <Button
                  key={option.value}
                  type="button"
                  size="sm"
                  variant={preset === option.value ? "secondary" : "ghost"}
                  className={cn(
                    "h-7 w-full sm:w-auto",
                    preset === option.value && "bg-background shadow-sm",
                  )}
                  aria-pressed={preset === option.value}
                  onClick={() => selectPreset(option.value)}
                >
                  {option.label}
                </Button>
              ))}
          </div>

          {preset === "custom" ? (
            <div className="mt-3 flex flex-col gap-3 sm:flex-row sm:items-end">
              <div className="grid flex-1 gap-1.5">
                <Label htmlFor="api-key-usage-start-date">{t("开始日期")}</Label>
                <Input
                  id="api-key-usage-start-date"
                  type="date"
                  value={startInput}
                  onChange={(event) => setStartInput(event.target.value)}
                />
              </div>
              <div className="grid flex-1 gap-1.5">
                <Label htmlFor="api-key-usage-end-date">{t("结束日期")}</Label>
                <Input
                  id="api-key-usage-end-date"
                  type="date"
                  value={endInput}
                  onChange={(event) => setEndInput(event.target.value)}
                />
              </div>
              <Button type="button" onClick={applyCustomRange}>
                <CalendarDays className="size-4" />
                {t("应用")}
              </Button>
            </div>
          ) : null}
          {rangeError ? (
            <p className="mt-2 text-xs font-medium text-destructive">
              {rangeError}
            </p>
          ) : null}
        </div>

        <div className="grid grid-cols-2 border-b bg-background md:grid-cols-4">
          <div className="border-b px-5 py-3 md:border-r md:border-b-0 sm:px-6">
            <p className="text-xs text-muted-foreground">{t("总 Token")}</p>
            <p className="mt-1 text-lg font-semibold tabular-nums">
              {usageQuery.isPending ? (
                <Skeleton className="h-6 w-20" />
              ) : (
                formatSummaryInteger(usageQuery.data?.usage.totalTokens ?? 0, locale)
              )}
            </p>
          </div>
          <div className="border-b border-l px-5 py-3 md:border-r md:border-l-0 md:border-b-0 sm:px-6">
            <p className="text-xs text-muted-foreground">{t("估算费用")}</p>
            <p className="mt-1 text-lg font-semibold tabular-nums">
              {usageQuery.isPending ? (
                <Skeleton className="h-6 w-20" />
              ) : (
                formatUsd(usageQuery.data?.usage.estimatedCostUsd ?? 0)
              )}
            </p>
          </div>
          <div className="px-5 py-3 md:border-r sm:px-6">
            <p className="text-xs text-muted-foreground">{t("请求数")}</p>
            <p className="mt-1 text-lg font-semibold tabular-nums">
              {usageQuery.isPending ? (
                <Skeleton className="h-6 w-16" />
              ) : (
                formatInteger(usageQuery.data?.usage.requestCount ?? 0, locale)
              )}
            </p>
          </div>
          <div className="border-l px-5 py-3 md:border-l-0 sm:px-6">
            <p className="text-xs text-muted-foreground">{t("成功率")}</p>
            <p className="mt-1 text-lg font-semibold tabular-nums">
              {usageQuery.isPending ? (
                <Skeleton className="h-6 w-16" />
              ) : (
                formatSuccessRate(
                  usageQuery.data?.usage ?? {
                    inputTokens: 0,
                    cachedInputTokens: 0,
                    outputTokens: 0,
                    reasoningOutputTokens: 0,
                    totalTokens: 0,
                    estimatedCostUsd: 0,
                    requestCount: 0,
                    successCount: 0,
                    errorCount: 0,
                  },
                )
              )}
            </p>
          </div>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4 sm:px-6">
          <div className="mb-3 flex items-center justify-between gap-3">
            <div>
              <h3 className="font-medium">{t("每日明细")}</h3>
              <p className="mt-0.5 text-xs text-muted-foreground">
                {appliedRange.startInput} · {appliedRange.endInput}
              </p>
            </div>
            {usageQuery.isFetching && !usageQuery.isPending ? (
              <RefreshCw className="size-4 animate-spin text-muted-foreground" />
            ) : null}
          </div>

          {usageQuery.isPending ? (
            <div className="space-y-2">
              {Array.from({ length: 6 }).map((_, index) => (
                <Skeleton key={index} className="h-9 w-full" />
              ))}
            </div>
          ) : usageQuery.isError ? (
            <Alert variant="destructive">
              <CircleDollarSign className="size-4" />
              <AlertTitle>{t("加载用量失败")}</AlertTitle>
              <AlertDescription className="flex flex-wrap items-center justify-between gap-3">
                <span>{getAppErrorMessage(usageQuery.error)}</span>
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  onClick={() => void usageQuery.refetch()}
                >
                  <RefreshCw className="size-3.5" />
                  {t("重试")}
                </Button>
              </AlertDescription>
            </Alert>
          ) : !hasUsage ? (
            <div className="flex min-h-48 flex-col items-center justify-center text-center">
              <ListChecks className="size-8 text-muted-foreground/60" />
              <p className="mt-3 font-medium">{t("暂无用量数据")}</p>
              <p className="mt-1 text-xs text-muted-foreground">
                {t("所选区间内没有记录到用量")}
              </p>
            </div>
          ) : (
            <div className="overflow-hidden rounded-md border">
              <Table className="min-w-[760px]">
                <TableHeader>
                  <TableRow className="bg-muted/40">
                    <TableHead>{t("日期")}</TableHead>
                    <TableHead className="text-right">{t("请求数")}</TableHead>
                    <TableHead className="text-right">{t("输入 Token")}</TableHead>
                    <TableHead className="text-right">{t("缓存输入")}</TableHead>
                    <TableHead className="text-right">{t("输出 Token")}</TableHead>
                    <TableHead className="text-right">{t("推理 Token")}</TableHead>
                    <TableHead className="text-right">{t("总 Token")}</TableHead>
                    <TableHead className="text-right">{t("估算费用")}</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {dailyUsage.map((point) => (
                    <TableRow key={point.dayStartTs}>
                      <TableCell className="font-medium">
                        {new Date(point.dayStartTs * 1000).toLocaleDateString(locale, {
                          year: "numeric",
                          month: "2-digit",
                          day: "2-digit",
                        })}
                      </TableCell>
                      <TableCell className="text-right tabular-nums">
                        {formatInteger(point.usage.requestCount, locale)}
                      </TableCell>
                      <TableCell className="text-right tabular-nums">
                        {formatInteger(point.usage.inputTokens, locale)}
                      </TableCell>
                      <TableCell className="text-right tabular-nums text-muted-foreground">
                        {formatInteger(point.usage.cachedInputTokens, locale)}
                      </TableCell>
                      <TableCell className="text-right tabular-nums">
                        {formatInteger(point.usage.outputTokens, locale)}
                      </TableCell>
                      <TableCell className="text-right tabular-nums text-muted-foreground">
                        {formatInteger(point.usage.reasoningOutputTokens, locale)}
                      </TableCell>
                      <TableCell className="text-right font-medium tabular-nums">
                        {formatInteger(point.usage.totalTokens, locale)}
                      </TableCell>
                      <TableCell className="text-right font-medium tabular-nums">
                        {formatUsd(point.usage.estimatedCostUsd)}
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </div>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}
