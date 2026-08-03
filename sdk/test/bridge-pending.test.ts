import { afterEach, expect, test } from "bun:test";
import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { ExecutionJournal, LANE_LIFECYCLE_KINDS } from "../src/bridge/journal";

const roots: string[] = [];
afterEach(() => {
  for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true });
});

test("bridge pending is a restart-safe terminal lane queue", () => {
  const state = mkdtempSync(join(tmpdir(), "north-bridge-pending-"));
  roots.push(state);
  const journal = new ExecutionJournal(join(state, "journal"), "lane-ready");
  journal.append(LANE_LIFECYCLE_KINDS.spawnStart, { prompt: "ready to land" });
  journal.append(LANE_LIFECYCLE_KINDS.terminal, {
    processOutcome: "ran", deliveryOutcome: "delivered",
  });
  journal.append(LANE_LIFECYCLE_KINDS.harvest, {
    status: "harvested", branch: "lane-ready", sha: "0123456789abcdef",
  });
  journal.close();

  const cli = resolve(import.meta.dir, "../src/bridge/cli.ts");
  const run = (...args: string[]) => spawnSync(process.execPath, ["run", cli, "pending", ...args], {
    encoding: "utf8",
    env: { ...process.env, NORTH_BRIDGE_STATE_DIR: state },
  });

  const pending = run();
  expect(pending.status).toBe(0);
  expect(pending.stdout).toContain(
    "lane-ready process=ran delivery=delivered branch=lane-ready sha=0123456789abcdef",
  );

  expect(run("--consume", "lane-ready").stdout).toBe("consumed lane-ready\n");
  expect(run().stdout).toBe("");
  expect(run("--consume", "lane-ready").stdout).toBe("already consumed lane-ready\n");
});
