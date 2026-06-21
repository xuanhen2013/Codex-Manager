import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import test from "node:test";

const appsRoot = path.resolve(import.meta.dirname, "..");

async function readDashboardSource() {
  return fs.readFile(path.join(appsRoot, "src/app/page.tsx"), "utf8");
}

async function readSource(relativePath) {
  return fs.readFile(path.join(appsRoot, relativePath), "utf8");
}

test("账号直连模式下会遮罩依赖网关请求日志的仪表盘区域", async () => {
  const source = await readDashboardSource();
  assert.match(source, /useCodexProfileModeStatus/);
  assert.match(source, /function DirectModeUnavailable/);
  assert.match(source, /账号直连模式下不可用/);
  assert.match(source, /切换到本地网关后可统计请求日志、Token 和费用/);
  assert.match(source, /buildStaticRouteUrl\("\/platform-mode"\)/);
  assert.match(source, /当前为账号直连模式/);
  assert.match(source, /CodexManager 无法统计 CLI 请求日志和用量。/);
  assert.match(
    source,
    /<DirectModeUnavailable active=\{isDirectAccountMode\}>\s*<AdminUsageAnalyticsCard/s,
  );
  assert.doesNotMatch(source, /当前活跃账号/);
  assert.doesNotMatch(source, /智能推荐/);
});

test("日志页 direct 模式只提示日志口径不遮罩历史日志", async () => {
  const source = await readSource("src/app/logs/page.tsx");
  assert.match(source, /useCodexProfileModeStatus/);
  assert.doesNotMatch(source, /DirectModeUnavailable/);
});

test("启动快照只预取轻量日志样本", async () => {
  const source = await readSource("src/lib/api/startup-snapshot.ts");
  assert.match(source, /STARTUP_SNAPSHOT_REQUEST_LOG_LIMIT = 24/);
});

test("首页仪表盘不再为已移除的活跃账号卡片预取日志样本", async () => {
  const source = await readDashboardSource();
  assert.match(source, /useDashboardStats\(\{\s*requestLogLimit: 0,\s*includeAccountHints: false,\s*\}\)/s);
});
