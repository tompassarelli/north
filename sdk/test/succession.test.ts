import { afterEach, expect, test } from "bun:test";
import { mkdtempSync, rmSync, statSync, utimesSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { runSuccessionCli } from "../src/succession-cli";
import {
  classifyAccountCooked,
  decideSuccession,
  parseAvailabilityDocument,
  readHeartbeatEvidence,
  recordPulse,
  type AvailabilityAccount,
  type AvailabilityDocument,
  type CommandRunner,
  type HeartbeatEvidence,
} from "../src/succession";

const observedAt = "2026-07-28T00:00:00.000Z";
const resetsAt = "2026-08-01T00:00:00.000Z";
const freshHeartbeat: HeartbeatEvidence = {
  source: "graph", observedAt, ageMs: 60_000, stale: false, daemonReachable: true,
};
const staleHeartbeat: HeartbeatEvidence = {
  source: "graph", observedAt, ageMs: 6_000_000, stale: true, daemonReachable: true,
};
const scratch: string[] = [];
afterEach(() => {
  while (scratch.length) rmSync(scratch.pop()!, { recursive: true, force: true });
});

function account(
  accountId: string,
  values: { week?: number; window?: number; model?: number; stale?: boolean },
): AvailabilityAccount {
  const modelPct = values.model ?? 20;
  const weekPct = values.week ?? 20;
  const windowPct = values.window ?? 20;
  return {
    account: accountId,
    provider: "anthropic",
    observedAt,
    stale: values.stale ?? false,
    rungs: {
      week: { pct: weekPct, resetsAt },
      window: { name: "five_hour", pct: windowPct, resetsAt },
      models: { fable: { pct: modelPct, resetsAt } },
    },
    verdict: weekPct >= 98 ? "cooked-week"
      : windowPct >= 98 ? "cooked-window"
        : modelPct >= 98 ? "model-cooked[fable]" : "available",
    usableModels: modelPct >= 98 ? [] : ["fable"],
  };
}

function document(...accounts: AvailabilityAccount[]): AvailabilityDocument {
  return { schemaVersion: 1, accounts };
}

test("pinned availability JSON parses account window, week, and model rungs", () => {
  const parsed = parseAvailabilityDocument(JSON.stringify([
    account("a", { week: 98, window: 10, model: 20 }),
  ]));
  expect(parsed.accounts[0]).toMatchObject({
    account: "a",
    provider: "anthropic",
    observedAt,
    stale: false,
    rungs: {
      week: { pct: 98, resetsAt },
      window: { name: "five_hour", pct: 10, resetsAt },
      models: { fable: { pct: 20, resetsAt } },
    },
  });
});

test("each cooked rung class independently trips an account", () => {
  expect(classifyAccountCooked(account("week", { week: 98 }), 98, "fable"))
    .toMatchObject({ cooked: true, rung: "week", pct: 98 });
  expect(classifyAccountCooked(account("window", { window: 99 }), 98, "fable"))
    .toMatchObject({ cooked: true, rung: "window", pct: 99 });
  expect(classifyAccountCooked(account("model", { model: 100 }), 98, "fable"))
    .toMatchObject({ cooked: true, rung: "model", pct: 100, model: "fable" });
  expect(classifyAccountCooked(account("fresh", { week: 97, window: 97, model: 97 }), 98, "fable"))
    .toEqual({ accountId: "fresh", cooked: false });
});

test("fresh evidence fires only when every eligible anthropic account is cooked", () => {
  const fired = decideSuccession(document(
    account("week", { week: 98 }),
    account("window", { window: 99 }),
    account("model", { model: 100 }),
    { ...account("openai", {}), provider: "openai" },
  ), staleHeartbeat);
  expect(fired).toMatchObject({
    action: "fire",
    reason: "all-anthropic-accounts-cooked",
    evidenceStale: false,
  });
  expect(fired.accounts.map(({ accountId }) => accountId))
    .toEqual(["week", "window", "model"]);

  const held = decideSuccession(document(
    account("cooked", { week: 98 }),
    account("not-cooked", { window: 97 }),
  ), staleHeartbeat);
  expect(held).toMatchObject({
    action: "hold",
    reason: "anthropic-account-not-cooked",
    evidenceStale: false,
  });
});

test("stale provider evidence delegates the decision to coordinator heartbeat", () => {
  const stale = document(account("stale", { stale: true }));
  expect(decideSuccession(stale, freshHeartbeat)).toMatchObject({
    action: "hold",
    reason: "stale-evidence-heartbeat-fresh",
  });
  expect(decideSuccession(stale, staleHeartbeat)).toMatchObject({
    action: "fire",
    reason: "stale-evidence-heartbeat-stale",
  });
  expect(decideSuccession(document(), staleHeartbeat)).toMatchObject({
    action: "fire",
    reason: "stale-evidence-heartbeat-stale",
  });
});

test("an explicit unknown availability verdict remains stale evidence", () => {
  const unknown = {
    ...account("unknown", {}),
    verdict: "unknown" as const,
  };
  const parsed = parseAvailabilityDocument(JSON.stringify([unknown]));
  expect(decideSuccession(parsed, freshHeartbeat)).toMatchObject({
    action: "hold",
    reason: "stale-evidence-heartbeat-fresh",
    evidenceStale: true,
  });
});

test("graph heartbeat is primary and fallback file is read only when the daemon command fails", () => {
  const now = new Date("2026-07-28T02:00:00.000Z");
  let calls = 0;
  const graphRunner: CommandRunner = () => {
    calls++;
    return {
      status: 0,
      timedOut: false,
      stderr: "",
      stdout: JSON.stringify([{ predicate: "coordinator_pulse", value: "2026-07-28T01:59:00.000Z" }]),
    };
  };
  expect(readHeartbeatEvidence({
    northBin: "north",
    thread: "thread",
    fallbackFile: "/path/that/must/not/be/read",
    now,
    staleMs: 90 * 60 * 1_000,
    run: graphRunner,
  })).toMatchObject({
    source: "graph",
    daemonReachable: true,
    stale: false,
    ageMs: 60_000,
  });
  expect(calls).toBe(1);

  const unavailable: CommandRunner = () => ({
    status: null, timedOut: true, stdout: "", stderr: "",
  });
  const dir = mkdtempSync(join(tmpdir(), "north-succession-test-"));
  scratch.push(dir);
  const fallbackFile = join(dir, "heartbeat");
  writeFileSync(fallbackFile, "");
  const fallbackTime = new Date("2026-07-28T00:00:00.000Z");
  utimesSync(fallbackFile, fallbackTime, fallbackTime);
  expect(readHeartbeatEvidence({
    northBin: "north",
    thread: "thread",
    fallbackFile,
    now,
    staleMs: 90 * 60 * 1_000,
    run: unavailable,
  })).toMatchObject({
    source: "file",
    daemonReachable: false,
    stale: true,
    ageMs: 2 * 60 * 60 * 1_000,
  });
  const missing = readHeartbeatEvidence({
    northBin: "north",
    thread: "thread",
    fallbackFile: "/path/that/does/not/exist",
    now,
    run: unavailable,
  });
  expect(missing).toEqual({
    source: "missing",
    stale: true,
    daemonReachable: false,
  });
});

test("pulse mirrors the fallback file and writes one superseding graph fact", () => {
  const dir = mkdtempSync(join(tmpdir(), "north-succession-pulse-"));
  scratch.push(dir);
  const fallbackFile = join(dir, "heartbeat");
  const commands: string[][] = [];
  const run: CommandRunner = (_command, args) => {
    commands.push(args);
    return { status: 0, timedOut: false, stdout: "", stderr: "" };
  };
  const now = new Date("2026-07-28T03:00:00.000Z");
  recordPulse({
    northBin: "north",
    thread: "thread",
    fallbackFile,
    now,
    run,
  });
  expect(statSync(fallbackFile).mtime.toISOString()).toBe(now.toISOString());
  expect(commands).toEqual([
    ["tell", "coordinator_pulse", "cardinality", "single"],
    ["tell", "thread", "coordinator_pulse", now.toISOString()],
  ]);
});

test("check consumes only cached availability and the injected handoff fire surface", () => {
  const dir = mkdtempSync(join(tmpdir(), "north-succession-cli-"));
  scratch.push(dir);
  const commands: Array<{ command: string; args: string[]; timeoutMs: number }> = [];
  const run: CommandRunner = (command, args, timeoutMs) => {
    commands.push({ command, args, timeoutMs });
    if (args.join(" ") === "account availability --json")
      return { status: 0, timedOut: false, stderr: "", stdout: JSON.stringify(
        [account("claude", { week: 98 })],
      ) };
    if (args[0] === "json")
      return {
        status: 0,
        timedOut: false,
        stderr: "",
        stdout: JSON.stringify([{ predicate: "coordinator_pulse", value: observedAt }]),
      };
    return { status: 0, timedOut: false, stderr: "", stdout: "" };
  };
  const status = runSuccessionCli(["check"], {
    HOME: dir,
    NORTH_BIN: "/store/north/bin/north",
    NORTH_SUCCESSION_THREAD: "thread",
    NORTH_SUCCESSION_MARKER_FILE: join(dir, "marker"),
    NORTH_SUCCESSION_PENDING_FILE: join(dir, "pending"),
    NORTH_SUCCESSION_FIRE_COMMAND: "/store/north/bin/north",
    NORTH_SUCCESSION_FIRE_ARGS: [
      "handoff", "fire", "--thread", "program-root", "--brief", "/store/succession.md",
    ].join("\u001f"),
  }, run, new Date("2026-07-28T02:00:00.000Z"));
  expect(status).toBe(0);
  expect(commands[0]).toEqual({
    command: "/store/north/bin/north",
    args: ["account", "availability", "--json"],
    timeoutMs: 2_000,
  });
  expect(commands.some(({ args }) => args.includes("--refresh"))).toBe(false);
  expect(commands.some(({ args }) =>
    args.join(" ") === "handoff fire --thread program-root --brief /store/succession.md")).toBe(true);
  expect(commands.filter(({ args }) => args[0] === "tell").map(({ args }) => args[2]))
    .toEqual(["succession_decision", "succession_fire"]);
});
