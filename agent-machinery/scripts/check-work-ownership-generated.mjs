#!/usr/bin/env bun
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const SOURCE = "scripts/work-ownership.bjs";
const GENERATED = "scripts/work-ownership.js";
const TYPED_TOOL = ["bea", "gle"].join("");
const temporaryRoot = mkdtempSync(join(tmpdir(), "agent-machinery-ownership-"));
const candidate = join(temporaryRoot, "work-ownership.js");
const decoder = new TextDecoder();

try {
  const result = Bun.spawnSync(
    [TYPED_TOOL, "build", SOURCE, candidate],
    { cwd: ROOT, stdout: "pipe", stderr: "pipe" },
  );
  if (result.exitCode !== 0) {
    const detail = decoder.decode(result.stderr).trim();
    throw new Error(`typed ownership projection failed${detail ? `: ${detail}` : ""}`);
  }
  if (!readFileSync(resolve(ROOT, GENERATED)).equals(readFileSync(candidate)))
    throw new Error(`stale generated ownership projection: run bun run build:ownership`);
  console.log("generated ownership projection is fresh");
} finally {
  rmSync(temporaryRoot, { recursive: true, force: true });
}
