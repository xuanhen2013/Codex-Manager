import assert from "node:assert/strict";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { pathToFileURL } from "node:url";
import ts from "../node_modules/typescript/lib/typescript.js";

const appsRoot = path.resolve(import.meta.dirname, "..");

async function loadTsModule(relativePath) {
  const sourcePath = path.join(appsRoot, "src", ...relativePath.split("/"));
  const source = await fs.readFile(sourcePath, "utf8");
  const compiled = ts.transpileModule(source, {
    compilerOptions: {
      module: ts.ModuleKind.ES2022,
      target: ts.ScriptTarget.ES2022,
    },
    fileName: sourcePath,
  });
  const tempDir = await fs.mkdtemp(
    path.join(os.tmpdir(), "codexmanager-api-key-usage-"),
  );
  const tempFile = path.join(tempDir, `${path.basename(relativePath, ".ts")}.mjs`);
  await fs.writeFile(tempFile, compiled.outputText, "utf8");
  return import(pathToFileURL(tempFile).href);
}

const usageReader = await loadTsModule("lib/api/api-key-usage-history.ts");
const ranges = await loadTsModule("lib/utils/api-key-usage-range.ts");

test("readApiKeyUsageHistory 同时支持 camelCase 与 snake_case", () => {
  const result = usageReader.readApiKeyUsageHistory({
    key_id: " key-1 ",
    rangeStartTs: 100,
    range_end_ts: 200,
    usage: {
      input_tokens: 120,
      cachedInputTokens: 20,
      output_tokens: 40,
      totalTokens: 140,
      estimated_cost_usd: 0.25,
      request_count: 3,
      successCount: 2,
      error_count: 1,
    },
    daily_usage: [
      {
        day_start_ts: 100,
        dayEndTs: 200,
        usage: { total_tokens: 140, estimatedCostUsd: 0.25 },
      },
    ],
  });

  assert.equal(result.keyId, "key-1");
  assert.equal(result.usage.inputTokens, 120);
  assert.equal(result.usage.cachedInputTokens, 20);
  assert.equal(result.usage.estimatedCostUsd, 0.25);
  assert.equal(result.dailyUsage[0].dayEndTs, 200);
  assert.equal(result.dailyUsage[0].usage.totalTokens, 140);
});

test("自定义日期结束时间使用次日零点的排他边界", () => {
  const startTs = ranges.parseLocalDateStartTs("2026-07-01");
  const endTs = ranges.parseLocalDateEndExclusiveTs("2026-07-03");
  assert.equal(startTs, Math.floor(new Date(2026, 6, 1).getTime() / 1000));
  assert.equal(endTs, Math.floor(new Date(2026, 6, 4).getTime() / 1000));
  assert.equal(ranges.buildApiKeyUsageDateRange("2026-07-03", "2026-07-01"), null);
  const range = ranges.buildApiKeyUsageDateRange("2026-07-01", "2026-07-03");
  assert.equal(range.dayBoundariesTs.length, 4);
  assert.equal(range.dayBoundariesTs[0], startTs);
  assert.equal(range.dayBoundariesTs.at(-1), endTs);
});

test("自然日边界数组保留夏令时切换日的真实长度", () => {
  const previousTimeZone = process.env.TZ;
  process.env.TZ = "America/New_York";
  try {
    const range = ranges.buildApiKeyUsageDateRange("2026-03-07", "2026-03-09");
    const dayLengths = range.dayBoundariesTs
      .slice(1)
      .map((value, index) => value - range.dayBoundariesTs[index]);
    assert.deepEqual(dayLengths, [86_400, 82_800, 86_400]);
  } finally {
    if (previousTimeZone === undefined) delete process.env.TZ;
    else process.env.TZ = previousTimeZone;
  }
});

test("本月与上月预设覆盖完整自然月范围", () => {
  const now = new Date(2026, 6, 13, 18, 30);
  const thisMonth = ranges.createApiKeyUsagePresetRange("this_month", now);
  const lastMonth = ranges.createApiKeyUsagePresetRange("last_month", now);

  assert.equal(thisMonth.startInput, "2026-07-01");
  assert.equal(thisMonth.endInput, "2026-07-13");
  assert.equal(lastMonth.startInput, "2026-06-01");
  assert.equal(lastMonth.endInput, "2026-06-30");
});
