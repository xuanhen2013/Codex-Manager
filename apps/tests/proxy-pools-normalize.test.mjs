import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import test from "node:test";
import { pathToFileURL } from "node:url";
import ts from "../node_modules/typescript/lib/typescript.js";

const appsRoot = path.resolve(import.meta.dirname, "..");
const sourcePath = path.join(appsRoot, "src", "lib", "api", "proxy-pools.ts");
const source = (await fs.readFile(sourcePath, "utf8")).replace(
  /import \{ invoke, withAddr \} from "\.\/transport";/,
  "const invoke = async () => undefined; const withAddr = (value = {}) => value;",
);
const compiled = ts.transpileModule(source, {
  compilerOptions: {
    module: ts.ModuleKind.ES2022,
    target: ts.ScriptTarget.ES2022,
  },
});
const tempFile = path.join(appsRoot, "tests", ".temp-proxy-pools.mjs");
await fs.writeFile(tempFile, compiled.outputText, "utf8");
const proxyPools = await import(`${pathToFileURL(tempFile).href}?${Date.now()}`);
await fs.unlink(tempFile);

test("代理池列表 normalizer 同时支持 snake_case 与成员健康数据", () => {
  const result = proxyPools.normalizeProxyPool({
    id: "pool-us",
    name: "US",
    failure_threshold: 3,
    recovery_threshold: 2,
    heartbeat_interval_secs: 60,
    cooldown_secs: 300,
    accounts_count: 4,
    healthy_members_count: 1,
    members: [{
      proxy_profile_id: "proxy-1",
      proxy_profile_name: "US-1",
      health_status: "healthy",
      current_accounts_count: 4,
    }],
  });

  assert.equal(result.id, "pool-us");
  assert.equal(result.failureThreshold, 3);
  assert.equal(result.accountsCount, 4);
  assert.equal(result.members[0].proxyProfileId, "proxy-1");
  assert.equal(result.members[0].healthStatus, "healthy");
});

test("批量导入 normalizer 清洗结果和 snake_case 代理字段", () => {
  const result = proxyPools.normalizeProxyBatchImportResult({
    total: 2,
    created: 1,
    duplicates: 1,
    failed: 0,
    items: [{
      line: 1,
      status: "created",
      name: "US-1",
      proxy_profile_id: "proxy-1",
      proxy_url_redacted: "socks5://host:1",
    }],
  });

  assert.equal(result.created, 1);
  assert.equal(result.items[0].proxyProfileId, "proxy-1");
  assert.equal(result.items[0].proxyUrlRedacted, "socks5://host:1");
});

test("健康周期 normalizer 读取 switched_accounts", () => {
  assert.deepEqual(
    proxyPools.normalizeProxyPoolHealthCycleResult({
      checked: 2,
      healthy: 1,
      failed: 1,
      switched_accounts: 3,
    }),
    { checked: 2, healthy: 1, failed: 1, switchedAccounts: 3 },
  );
});
