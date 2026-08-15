"use client";

import { useMemo, useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Loader2 } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { useI18n } from "@/lib/i18n/provider";
import { getAppErrorMessage } from "@/lib/api/transport";
import {
  PROXY_POOLS_QUERY_KEY,
  proxyPoolsClient,
} from "@/lib/api/proxy-pools";
import { PROXY_PROFILES_QUERY_KEY } from "@/lib/api/proxy-profiles";
import type { ProxyPool, ProxyProfile } from "@/types";

export function ProxyPoolModal({
  open,
  onOpenChange,
  pool,
  profiles,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  pool: ProxyPool | null;
  profiles: ProxyProfile[];
}) {
  const { t } = useI18n();
  const queryClient = useQueryClient();
  const [name, setName] = useState(pool?.name ?? "");
  const [description, setDescription] = useState(pool?.description ?? "");
  const [enabled, setEnabled] = useState(pool?.enabled ?? true);
  const [failureThreshold, setFailureThreshold] = useState(pool?.failureThreshold ?? 3);
  const [heartbeatIntervalSecs, setHeartbeatIntervalSecs] = useState(
    pool?.heartbeatIntervalSecs ?? 60,
  );
  const [cooldownSecs, setCooldownSecs] = useState(pool?.cooldownSecs ?? 300);
  const [selectedIds, setSelectedIds] = useState<string[]>(
    pool?.members.map((member) => member.proxyProfileId) ?? [],
  );

  const memberOwner = useMemo(() => {
    const map = new Map<string, string>();
    const cached = queryClient.getQueryData<ProxyPool[]>(PROXY_POOLS_QUERY_KEY) ?? [];
    cached.forEach((item) =>
      item.members.forEach((member) => map.set(member.proxyProfileId, item.id)),
    );
    return map;
  }, [queryClient]);

  const saveMutation = useMutation({
    mutationFn: async () => {
      if (!name.trim()) throw new Error(t("代理池名称不能为空"));
      const saved = pool
        ? await proxyPoolsClient.update({
            id: pool.id,
            name: name.trim(),
            description: description.trim() || null,
            enabled,
            failureThreshold,
            recoveryThreshold: pool.recoveryThreshold,
            heartbeatIntervalSecs,
            cooldownSecs,
          })
        : await proxyPoolsClient.create({
            name: name.trim(),
            description: description.trim() || null,
            enabled,
            failureThreshold,
            recoveryThreshold: 2,
            heartbeatIntervalSecs,
            cooldownSecs,
          });
      const existing = new Set(saved.members.map((member) => member.proxyProfileId));
      const selected = new Set(selectedIds);
      const additions = selectedIds.filter((id) => !existing.has(id));
      const removals = [...existing].filter((id) => !selected.has(id));
      if (additions.length) await proxyPoolsClient.addMembers(saved.id, additions);
      for (const id of removals) {
        await proxyPoolsClient.removeMember(saved.id, id);
      }
      return saved;
    },
    onSuccess: async () => {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: PROXY_POOLS_QUERY_KEY }),
        queryClient.invalidateQueries({ queryKey: PROXY_PROFILES_QUERY_KEY }),
      ]);
      toast.success(pool ? t("代理池已更新") : t("代理池已创建"));
      onOpenChange(false);
    },
    onError: (error) => toast.error(getAppErrorMessage(error)),
  });

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl">
        <DialogHeader>
          <DialogTitle>{pool ? t("编辑代理池") : t("新建代理池")}</DialogTitle>
          <DialogDescription>
            {t("代理池的地区和用途由你维护，账号只会在当前池内切换。")}
          </DialogDescription>
        </DialogHeader>
        <div className="grid gap-4">
          <div className="grid gap-2">
            <Label htmlFor="proxy-pool-name">{t("名称")}</Label>
            <Input
              id="proxy-pool-name"
              value={name}
              onChange={(event) => setName(event.target.value)}
              placeholder={t("例如：美国住宅代理池")}
            />
          </div>
          <div className="grid gap-2">
            <Label htmlFor="proxy-pool-description">{t("备注")}</Label>
            <Textarea
              id="proxy-pool-description"
              value={description}
              onChange={(event) => setDescription(event.target.value)}
              className="min-h-20"
            />
          </div>
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-3">
            <div className="grid gap-2">
              <Label htmlFor="failure-threshold">{t("连续失败次数")}</Label>
              <Input
                id="failure-threshold"
                type="number"
                min={1}
                max={10}
                value={failureThreshold}
                onChange={(event) => setFailureThreshold(Number(event.target.value) || 3)}
              />
            </div>
            <div className="grid gap-2">
              <Label htmlFor="heartbeat-interval">{t("心跳间隔（秒）")}</Label>
              <Input
                id="heartbeat-interval"
                type="number"
                min={30}
                value={heartbeatIntervalSecs}
                onChange={(event) => setHeartbeatIntervalSecs(Number(event.target.value) || 60)}
              />
            </div>
            <div className="grid gap-2">
              <Label htmlFor="cooldown-seconds">{t("故障冷却（秒）")}</Label>
              <Input
                id="cooldown-seconds"
                type="number"
                min={30}
                value={cooldownSecs}
                onChange={(event) => setCooldownSecs(Number(event.target.value) || 300)}
              />
            </div>
          </div>
          <div className="flex items-center justify-between border-y py-3">
            <Label htmlFor="proxy-pool-enabled">{t("启用代理池")}</Label>
            <Switch
              id="proxy-pool-enabled"
              checked={enabled}
              onCheckedChange={setEnabled}
            />
          </div>
          <div className="grid gap-2">
            <Label>{t("池内代理")}</Label>
            <div className="max-h-56 overflow-y-auto border">
              {profiles.length ? (
                profiles.map((profile) => {
                  const owner = memberOwner.get(profile.id);
                  const disabled = Boolean(owner && owner !== pool?.id);
                  const checked = selectedIds.includes(profile.id);
                  return (
                    <label
                      key={profile.id}
                      className="flex min-h-12 items-center gap-3 border-b px-3 py-2 last:border-b-0"
                    >
                      <Checkbox
                        checked={checked}
                        disabled={disabled}
                        onCheckedChange={(next) =>
                          setSelectedIds((current) =>
                            next
                              ? [...current, profile.id]
                              : current.filter((id) => id !== profile.id),
                          )
                        }
                      />
                      <span className="min-w-0 flex-1">
                        <span className="block truncate text-sm font-medium">{profile.name}</span>
                        <span className="block truncate text-xs text-muted-foreground">
                          {disabled ? t("已属于其他代理池") : profile.proxyUrlRedacted}
                        </span>
                      </span>
                    </label>
                  );
                })
              ) : (
                <div className="p-4 text-sm text-muted-foreground">{t("暂无代理配置")}</div>
              )}
            </div>
          </div>
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            {t("取消")}
          </Button>
          <Button
            onClick={() => saveMutation.mutate()}
            disabled={saveMutation.isPending}
          >
            {saveMutation.isPending && <Loader2 className="animate-spin" />}
            {t("保存")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
