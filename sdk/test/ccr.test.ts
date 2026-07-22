import { afterEach, expect, test } from "bun:test";
import { spawnSync } from "node:child_process";
import {
  existsSync, mkdtempSync, readdirSync, readFileSync, rmSync,
  statSync, writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { CCR_COVERAGE, prepareCcrOutput, retrieveCcrOutput } from "../src/ccr";

const roots: string[] = [];
const cli = resolve(import.meta.dir, "../src/ccr-cli.ts");
const north = resolve(import.meta.dir, "../../bin/north");

afterEach(() => {
  for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true });
});

function temporaryRoot(): string {
  const root = mkdtempSync(join(tmpdir(), "north-ccr-"));
  roots.push(root);
  return root;
}

function sampleRows(secret = "PRIVATE-CONTENT", count = 60): Array<Record<string, unknown>> {
  return Array.from({ length: count }, (_, index) => ({
    index,
    level: index === Math.floor(count / 2) ? "warn" : "info",
    message: `${secret}-${index}-${"repeated-value-".repeat(6)}`,
  }));
}

function objectFiles(path: string): string[] {
  if (!existsSync(path)) return [];
  const found: string[] = [];
  for (const entry of readdirSync(path, { withFileTypes: true })) {
    const child = join(path, entry.name);
    if (entry.isDirectory()) found.push(...objectFiles(child));
    else if (/^[0-9a-f]{64}\.json$/.test(entry.name)) found.push(child);
  }
  return found;
}

test("default-off canary preserves exact identities, prefix, and privacy-safe telemetry", async () => {
  const stateDir = temporaryRoot();
  const output = sampleRows("DO-NOT-LOG");
  const stablePrefix = { identity: "DO-NOT-LOG-PREFIX" };
  const result = await prepareCcrOutput({
    output,
    stablePrefix,
    workspaceIdentity: "/private/workspace/name",
    managedRunId: "run-private-name",
    stateDir,
  });

  expect(result.coverage).toBe(CCR_COVERAGE);
  expect(result.transformed).toBe(false);
  expect(result.output).toBe(output);
  expect(result.stablePrefix).toBe(stablePrefix);
  expect(result.telemetry.decision).toBe("disabled");
  expect(result.telemetryRecorded).toBe(true);
  expect(objectFiles(join(stateDir, "objects"))).toEqual([]);

  const telemetryPath = join(stateDir, "telemetry.jsonl");
  const telemetry = readFileSync(telemetryPath, "utf8");
  expect(telemetry).toContain('"coverage":"explicit_canary_only"');
  expect(telemetry).not.toContain("DO-NOT-LOG");
  expect(telemetry).not.toContain("/private/workspace/name");
  expect(telemetry).not.toContain("run-private-name");
  expect(statSync(stateDir).mode & 0o777).toBe(0o700);
  expect(statSync(telemetryPath).mode & 0o777).toBe(0o600);
});

test("enabled canary durably round-trips exact JSON in its workspace/run scope", async () => {
  const stateDir = temporaryRoot();
  const output = sampleRows();
  const stablePrefix = Object.freeze({ prefix: "unchanged" });
  const prepared = await prepareCcrOutput({
    output,
    stablePrefix,
    enabled: true,
    workspaceIdentity: "workspace-a",
    managedRunId: "run-a",
    stateDir,
    now: () => 1_000,
  });

  expect(prepared.transformed).toBe(true);
  expect(prepared.stablePrefix).toBe(stablePrefix);
  expect(prepared.hash).toMatch(/^[0-9a-f]{16}$/);
  expect(prepared.telemetry).toMatchObject({
    decision: "compressed",
    markerOutcome: "emitted",
    storeOutcome: "stored",
    retrievalOutcome: "hit",
  });
  expect(prepared.telemetry.compressedBytes).toBeLessThan(prepared.telemetry.originalBytes!);

  const retrieved = await retrieveCcrOutput({
    workspaceIdentity: "workspace-a",
    managedRunId: "run-a",
    stateDir,
    hash: prepared.hash!,
    now: () => 1_001,
  });
  expect(retrieved.status).toBe("hit");
  expect(Buffer.from(JSON.stringify(retrieved.output))).toEqual(Buffer.from(JSON.stringify(output)));

  const files = objectFiles(join(stateDir, "objects"));
  expect(files).toHaveLength(1);
  expect(statSync(files[0]).mode & 0o777).toBe(0o600);
});

test("retrieval denies cross-workspace and cross-run scopes as misses", async () => {
  const stateDir = temporaryRoot();
  const prepared = await prepareCcrOutput({
    output: sampleRows(), stablePrefix: null, enabled: true,
    workspaceIdentity: "workspace-a", managedRunId: "run-a", stateDir,
  });
  expect(prepared.transformed).toBe(true);

  const wrongWorkspace = await retrieveCcrOutput({
    workspaceIdentity: "workspace-b", managedRunId: "run-a", stateDir, hash: prepared.hash!,
  });
  const wrongRun = await retrieveCcrOutput({
    workspaceIdentity: "workspace-a", managedRunId: "run-b", stateDir, hash: prepared.hash!,
  });
  expect(wrongWorkspace.status).toBe("miss");
  expect(wrongRun.status).toBe("miss");
  expect(wrongWorkspace.output).toBeUndefined();
  expect(wrongRun.output).toBeUndefined();
});

test("retrieval classifies expired and corrupt durable objects", async () => {
  const expiredRoot = temporaryRoot();
  const expired = await prepareCcrOutput({
    output: sampleRows(), stablePrefix: null, enabled: true,
    workspaceIdentity: "workspace", managedRunId: "run", stateDir: expiredRoot,
    ttlMs: 10, now: () => 5_000,
  });
  expect((await retrieveCcrOutput({
    workspaceIdentity: "workspace", managedRunId: "run", stateDir: expiredRoot,
    hash: expired.hash!, now: () => 5_010,
  })).status).toBe("expired");

  const corruptRoot = temporaryRoot();
  const corrupt = await prepareCcrOutput({
    output: sampleRows(), stablePrefix: null, enabled: true,
    workspaceIdentity: "workspace", managedRunId: "run", stateDir: corruptRoot,
  });
  const [file] = objectFiles(join(corruptRoot, "objects"));
  writeFileSync(file, "{}\n", { mode: 0o600 });
  expect((await retrieveCcrOutput({
    workspaceIdentity: "workspace", managedRunId: "run", stateDir: corruptRoot,
    hash: corrupt.hash!,
  })).status).toBe("corrupt");
});

test("canary refuses transforms after provider side effects without writing an object", async () => {
  const stateDir = temporaryRoot();
  const output = sampleRows();
  const result = await prepareCcrOutput({
    output, stablePrefix: "prefix", enabled: true, providerSideEffectsStarted: true,
    workspaceIdentity: "workspace", managedRunId: "run", stateDir,
  });
  expect(result.output).toBe(output);
  expect(result.transformed).toBe(false);
  expect(result.telemetry.fallbackReason).toBe("provider_side_effects_started");
  expect(objectFiles(join(stateDir, "objects"))).toEqual([]);
});

test("CLI is byte-exact on disabled and storage-error fail-open paths", () => {
  const stateDir = temporaryRoot();
  const raw = Buffer.from(` [\n  ${sampleRows("CLI-PRIVATE").map((row) => JSON.stringify(row)).join(",\n  ")}\n] `);
  const disabled = spawnSync(process.execPath, ["run", cli, "prepare", "--workspace", "ws", "--run", "run", "--state-dir", stateDir], {
    input: raw,
  });
  expect(disabled.status).toBe(0);
  expect(disabled.stdout).toEqual(raw);
  expect(JSON.parse(disabled.stderr.toString())).toMatchObject({
    coverage: CCR_COVERAGE, decision: "disabled", transformed: false,
  });

  const blockedState = join(temporaryRoot(), "not-a-directory");
  writeFileSync(blockedState, "block", { mode: 0o600 });
  const failed = spawnSync(process.execPath, [
    "run", cli, "prepare", "--workspace", "ws", "--run", "run",
    "--state-dir", blockedState, "--enable",
  ], { input: raw });
  expect(failed.status).toBe(0);
  expect(failed.stdout).toEqual(raw);
  expect(JSON.parse(failed.stderr.toString())).toMatchObject({
    coverage: CCR_COVERAGE, decision: "fallback", transformed: false,
  });
});

test("north ccr exposes scoped prepare and retrieval without provider wiring", () => {
  const stateDir = temporaryRoot();
  const output = sampleRows("CLI-ROUNDTRIP");
  const prepared = spawnSync(north, [
    "ccr", "prepare", "--workspace", "workspace", "--run", "run",
    "--state-dir", stateDir, "--enable",
  ], { input: JSON.stringify(output) });
  expect(prepared.status).toBe(0);
  const status = JSON.parse(prepared.stderr.toString());
  expect(status).toMatchObject({
    coverage: CCR_COVERAGE, decision: "compressed", transformed: true,
    storeOutcome: "stored", retrievalOutcome: "hit",
  });

  const retrieved = spawnSync(north, [
    "ccr", "retrieve", "--workspace", "workspace", "--run", "run",
    "--state-dir", stateDir, status.hash,
  ]);
  expect(retrieved.status).toBe(0);
  expect(JSON.parse(retrieved.stdout.toString())).toEqual(output);

  const denied = spawnSync(north, [
    "ccr", "retrieve", "--workspace", "workspace", "--run", "other-run",
    "--state-dir", stateDir, status.hash,
  ]);
  expect(denied.status).toBe(3);
  expect(JSON.parse(denied.stderr.toString()).retrievalOutcome).toBe("miss");
});
