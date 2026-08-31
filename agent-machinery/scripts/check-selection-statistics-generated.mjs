import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const ROOT = resolve(import.meta.dirname, "..");
const SOURCE = "scripts/selection-statistics.bjs";
const GENERATED = "scripts/selection-statistics.js";
const TYPED_TOOL = ["bea", "gle"].join("");
const scratch = mkdtempSync(join(tmpdir(), "agent-machinery-selection-statistics-"));
const emitted = join(scratch, "selection-statistics.js");
const decoder = new TextDecoder();
try {
  const result = Bun.spawnSync(
    [TYPED_TOOL, "build", SOURCE, emitted],
    { cwd: ROOT, stdout: "pipe", stderr: "pipe" },
  );
  if (result.exitCode !== 0) {
    const detail = decoder.decode(result.stderr).trim();
    throw new Error(`typed selection-statistics projection failed${detail ? `: ${detail}` : ""}`);
  }
  if (readFileSync(emitted, "utf8") !==
      readFileSync(resolve(ROOT, GENERATED), "utf8")) {
    throw new Error("scripts/selection-statistics.js is stale; run bun run build:selection");
  }
  console.log("generated selection-statistics projection is fresh");
} finally {
  rmSync(scratch, { recursive: true, force: true });
}
