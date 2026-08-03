import { afterEach, expect, test } from "bun:test";
import { spawnSync } from "node:child_process";
import {
  mkdirSync, mkdtempSync, rmSync, writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { ExecutionJournal, LANE_LIFECYCLE_KINDS } from "../src/bridge/journal";

const roots: string[] = [];
afterEach(() => {
  for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true });
});

test("bridge dashboard renders fleet from journal and receipt plus queue from work store", () => {
  const home = mkdtempSync(join(tmpdir(), "north-bridge-dashboard-"));
  roots.push(home);
  const state = join(home, ".local/state/north");
  const agents = join(state, "agents");
  const threads = join(state, "threads");
  const cache = join(home, ".cache/north/dashboard-v1");
  mkdirSync(agents, { recursive: true });
  mkdirSync(threads, { recursive: true });
  mkdirSync(cache, { recursive: true });

  writeFileSync(join(agents, "lane-receipt-fixture.log"), "[spawn] starting provider=openai tier=senior\n");
  writeFileSync(
    join(agents, "lane-receipt-fixture.meta.json"),
    `${JSON.stringify({ thread: "receipt-thread", role: "senior", provider: "openai" })}\n`,
  );
  writeFileSync(join(threads, "receipt-thread-fixture.md"), "# Receipt fixture title\n");

  const laneJournal = new ExecutionJournal(join(state, "bridge/journal"), "receipt-fixture");
  laneJournal.append(LANE_LIFECYCLE_KINDS.spawnStart, {
    prompt: "Journal lane fixture title", cwd: home,
  });
  laneJournal.append(LANE_LIFECYCLE_KINDS.identityAdmitted, {
    provider: "journal-provider", role: "integrator", effort: "high", model: "fixture-model",
  });
  laneJournal.append(LANE_LIFECYCLE_KINDS.turnBoundary, { numTurns: 1 });
  laneJournal.append(LANE_LIFECYCLE_KINDS.terminal, {
    processOutcome: "died", deliveryOutcome: "blocked", resultBytes: 0,
  });
  laneJournal.append(LANE_LIFECYCLE_KINDS.harvest, {
    status: "nothing-committed", branch: "lane-receipt-fixture", sha: "abc123",
  });
  laneJournal.close();

  const journal = new ExecutionJournal(join(state, "bridge/journal"), "journal-fixture");
  journal.append("execution.accepted", { prompt: "Journal fixture title", cwd: home });
  journal.append("provider.starting", { adapter: "mock-provider" });
  journal.append("provider.result", { result: "delivered" });
  journal.append("execution.completed");
  journal.close();

  const now = Date.now();
  const board = "THREADS — 1 open thread · 0 active · 1 ready · 0 blocked\n\n"
    + "READY — top 1\n"
    + " unblocks 7  019fc745-40e0-763b-a0b5-b42e99bb6065 Queue fixture title\n";
  writeFileSync(
    join(cache, "board.edn"),
    `{:schema "north.dashboard/panel-v1" :last-attempt {:at ${now} :status "ok"} `
      + `:last-good {:at ${now} :data {:text ${JSON.stringify(board)}}}}`,
  );

  const env = { ...process.env };
  delete env.NORTH_DASHBOARD_LIB;
  const north = resolve(import.meta.dir, "../../bin/north");
  const commandEnv = {
    ...env, HOME: home, NO_COLOR: "1", COLUMNS: "160",
    NORTH_BRIDGE_STATE_DIR: join(state, "bridge"),
    NORTH_BUN: process.execPath,
  };
  const run = (...args: string[]) => spawnSync(north, args, {
    encoding: "utf8",
    env: commandEnv,
  });
  const result = run("bridge", "dashboard", "--once");

  expect(result.stderr).toBe("");
  expect(result.status).toBe(0);
  expect(result.stdout).toContain("FLEET");
  expect(result.stdout).toContain("Journal fixture title");
  expect(result.stdout).toContain("done");
  expect(result.stdout).toContain("delivered");
  expect(result.stdout).toContain("Journal lane fixture title");
  expect(result.stdout).toContain("fixture-model");
  expect(result.stdout).toContain("crashed");
  expect(result.stdout).not.toContain("Receipt fixture title");
  expect(result.stdout).toContain("QUEUE");
  expect(result.stdout).toContain("Queue fixture title");
  expect(result.stdout).not.toContain("\u001b");

  const alias = run("dashboard", "--once");
  expect(alias.status).toBe(0);
  expect(alias.stdout).toContain("Journal fixture title");
  expect(alias.stdout).toContain("Queue fixture title");
});
