import assert from "node:assert/strict";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { pathToFileURL } from "node:url";
import ts from "../node_modules/typescript/lib/typescript.js";

const appsRoot = path.resolve(import.meta.dirname, "..");
const sourcePath = path.join(appsRoot, "src", "lib", "api", "usage-reset-credits.ts");

async function loadModule() {
  const source = await fs.readFile(sourcePath, "utf8");
  const compiled = ts.transpileModule(source, {
    compilerOptions: {
      module: ts.ModuleKind.ES2022,
      target: ts.ScriptTarget.ES2022,
    },
    fileName: sourcePath,
  });
  const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), "codexmanager-reset-credits-"));
  const tempFile = path.join(tempDir, "usage-reset-credits.mjs");
  await fs.writeFile(tempFile, compiled.outputText, "utf8");
  return import(pathToFileURL(tempFile).href);
}

const resetCredits = await loadModule();

test("readUsageResetCredits normalizes count and expiry records", () => {
  assert.deepEqual(
    resetCredits.readUsageResetCredits({
      available_count: "2",
      credits: [
        {
          id: "credit-1",
          status: "available",
          granted_at: "2026-07-01T00:00:00Z",
          expires_at: "2026-08-01T00:00:00Z",
        },
      ],
    }),
    {
      availableCount: 2,
      credits: [
        {
          id: "credit-1",
          status: "available",
          grantedAt: "2026-07-01T00:00:00Z",
          expiresAt: "2026-08-01T00:00:00Z",
        },
      ],
    },
  );
});

test("readUsageResetConsumeResult preserves partial refresh warning", () => {
  assert.deepEqual(
    resetCredits.readUsageResetConsumeResult({
      resetApplied: true,
      resetCredits: { availableCount: 1, credits: [] },
      usageRefreshed: false,
      warning: "usage refresh failed",
    }),
    {
      resetApplied: true,
      resetCredits: { availableCount: 1, credits: [] },
      usageRefreshed: false,
      warning: "usage refresh failed",
    },
  );
});

test("readUsageResetCredits falls back to available credit records", () => {
  assert.equal(
    resetCredits.readUsageResetCredits({
      credits: [{ id: "credit-1", expiresAt: "2026-08-01T00:00:00Z" }],
    }).availableCount,
    1,
  );
});
