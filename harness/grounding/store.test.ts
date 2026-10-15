import { expect, test } from "bun:test";
import { mkdirSync, mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { redact } from "./store";

test("redact hides secrets but keeps paths, commit hashes and refs", () => {
  const text = [
    "river gauge password=synthetic-review-only",
    "token: abc123",
    "key sk-abcdefghijklmnopqrstuvwxyz012345",
    "blob Zx9Qw8Er7Ty6Ui5Op4As3Df2Gh1Jk0LzXcVbNmQwErTy",
    "see harness/skills/orchestrating-codex/references/session-diagnostics.md",
    "landed 4502d16752b0b5a1c3f9e2d8a7b6c5d4e3f2a1b0 and convo:2216789",
  ].join("\n");
  const out = redact(text);
  expect(out).not.toContain("synthetic-review-only");
  expect(out).not.toContain("abc123");
  expect(out).not.toContain("sk-abcdefghij");
  expect(out).not.toContain("Zx9Qw8Er7Ty6");
  expect(out).toContain("harness/skills/orchestrating-codex/references/session-diagnostics.md");
  expect(out).toContain("4502d16752b0b5a1c3f9e2d8a7b6c5d4e3f2a1b0");
  expect(out).toContain("convo:2216789");
});

test("distill run --if-idle exits at once while another run holds the lock", () => {
  const state = mkdtempSync(join(tmpdir(), "grounding-idle-"));
  mkdirSync(join(state, "distill.lock.d"));
  const started = Date.now();
  const proc = Bun.spawnSync(["bun", join(import.meta.dir, "distill.ts"), "run", "--no-refresh", "--if-idle"], {
    env: { ...process.env, NORTH_GROUNDING_STATE: state },
    stderr: "pipe",
  });
  expect(Date.now() - started).toBeLessThan(5000);
  expect(proc.stderr.toString()).toContain("busy");
});
