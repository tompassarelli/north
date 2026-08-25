import { afterEach, expect, test } from "bun:test";
import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { ExecutionJournal, LANE_LIFECYCLE_KINDS } from "../src/bridge/journal";
import {
  "parse-bridge-launch-arguments!" as parseBridgeLaunchArguments,
} from "../src/bridge/generated/north/bridge/cli.js";

const roots: string[] = [];
const ATTEMPT_ID = `@attempt:${"a".repeat(64)}`;
afterEach(() => {
  for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true });
});

test("bridge launch CLI requires an attempt, defaults to implementer, and accepts explicit roles", () => {
  expect(parseBridgeLaunchArguments(["--attempt", ATTEMPT_ID, "ship", "it"])).toEqual({
    role: "implementer", attemptId: ATTEMPT_ID, promptArguments: ["ship", "it"],
  });
  expect(parseBridgeLaunchArguments([
    "--role", "director", "--attempt", ATTEMPT_ID, "supervise",
  ])).toEqual({
    role: "director", attemptId: ATTEMPT_ID, promptArguments: ["supervise"],
  });
  expect(parseBridgeLaunchArguments([
    "--attempt", ATTEMPT_ID, "--role", "implementer", "build",
  ])).toEqual({
    role: "implementer", attemptId: ATTEMPT_ID, promptArguments: ["build"],
  });
  expect(() => parseBridgeLaunchArguments(["ship", "it"]))
    .toThrow("bridge launch requires --attempt");
  expect(() => parseBridgeLaunchArguments([
    "--attempt", ATTEMPT_ID, "--role", "portfolio", "plan",
  ]))
    .toThrow("bridge launch role must be director or implementer");
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

  const cli = resolve(import.meta.dir, "../src/bridge/generated/north/bridge/cli.js");
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
