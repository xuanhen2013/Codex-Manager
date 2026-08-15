"use client";

import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Loader2 } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
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
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";
import { useI18n } from "@/lib/i18n/provider";
import { getAppErrorMessage } from "@/lib/api/transport";
import { PROXY_POOLS_QUERY_KEY, proxyPoolsClient } from "@/lib/api/proxy-pools";
import { PROXY_PROFILES_QUERY_KEY } from "@/lib/api/proxy-profiles";
import type { ProxyBatchImportResult, ProxyPool } from "@/types";

export function ProxyBatchImportModal({
  open,
  onOpenChange,
  pools,
  defaultPoolId,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  pools: ProxyPool[];
  defaultPoolId?: string | null;
}) {
  const { t } = useI18n();
  const queryClient = useQueryClient();
  const [text, setText] = useState("");
  const [poolId, setPoolId] = useState(defaultPoolId || "__none__");
  const [scheme, setScheme] = useState("socks5");
  const [namePrefix, setNamePrefix] = useState("");
  const [result, setResult] = useState<ProxyBatchImportResult | null>(null);

  const mutation = useMutation({
    mutationFn: () =>
      proxyPoolsClient.importBatch({
        text,
        poolId: poolId === "__none__" ? null : poolId,
        defaultScheme: scheme,
        namePrefix: namePrefix.trim() || null,
      }),
    onSuccess: async (next) => {
      setResult(next);
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: PROXY_PROFILES_QUERY_KEY }),
        queryClient.invalidateQueries({ queryKey: PROXY_POOLS_QUERY_KEY }),
      ]);
      toast.success(t("已创建 {count} 条代理", { count: next.created }));
    },
    onError: (error) => toast.error(getAppErrorMessage(error)),
  });

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl">
        <DialogHeader>
          <DialogTitle>{t("批量导入代理")}</DialogTitle>
          <DialogDescription>
            {t("每行一条，支持完整 URL、host:port 和 host:port:user:password。")}
          </DialogDescription>
        </DialogHeader>
        <div className="grid gap-4">
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-3">
            <div className="grid gap-2">
              <Label>{t("目标代理池")}</Label>
              <Select value={poolId} onValueChange={(value) => value && setPoolId(value)}>
                <SelectTrigger><SelectValue /></SelectTrigger>
                <SelectContent>
                  <SelectItem value="__none__">{t("仅创建代理")}</SelectItem>
                  {pools.map((pool) => (
                    <SelectItem key={pool.id} value={pool.id}>{pool.name}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="grid gap-2">
              <Label>{t("默认协议")}</Label>
              <Select value={scheme} onValueChange={(value) => value && setScheme(value)}>
                <SelectTrigger><SelectValue /></SelectTrigger>
                <SelectContent>
                  {["socks5", "socks5h", "http", "https", "socks4"].map((item) => (
                    <SelectItem key={item} value={item}>{item}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="grid gap-2">
              <Label htmlFor="proxy-name-prefix">{t("名称前缀")}</Label>
              <Input
                id="proxy-name-prefix"
                value={namePrefix}
                onChange={(event) => setNamePrefix(event.target.value)}
                placeholder={t("例如：US")}
              />
            </div>
          </div>
          <Textarea
            value={text}
            onChange={(event) => setText(event.target.value)}
            className="min-h-64 font-mono text-xs"
            placeholder={"socks5://user:pass@host:port\nhost:port:user:pass\nname,host:port"}
          />
          {result && (
            <div className="grid grid-cols-4 border text-center text-sm">
              <div className="p-2"><strong>{result.total}</strong><span className="block text-xs text-muted-foreground">{t("总计")}</span></div>
              <div className="border-l p-2"><strong>{result.created}</strong><span className="block text-xs text-muted-foreground">{t("已创建")}</span></div>
              <div className="border-l p-2"><strong>{result.duplicates}</strong><span className="block text-xs text-muted-foreground">{t("重复")}</span></div>
              <div className="border-l p-2"><strong>{result.failed}</strong><span className="block text-xs text-muted-foreground">{t("失败")}</span></div>
            </div>
          )}
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>{t("关闭")}</Button>
          <Button
            onClick={() => mutation.mutate()}
            disabled={!text.trim() || mutation.isPending}
          >
            {mutation.isPending && <Loader2 className="animate-spin" />}
            {t("导入")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
