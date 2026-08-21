import { afterEach, expect, test } from "bun:test";
import { chmodSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  emptyFleetDispatchState,
  executeFleetDispatch,
  expireFleetDispatchReservations,
  FLEET_STATE_VERSION,
  reserveFleetDispatchAccount,
  selectFleetDispatchAccount,
  type FleetAccountEvidence,
  type FleetDispatchState,
} from "../src/fleet-dispatch";
import type { ProviderAccount } from "../src/accounts";
import type { AccountUsageReport } from "../src/account-usage";

const temporary: string[] = [];
const now = new Date("2026-08-21T07:00:00.000Z");

afterEach(() => {
  for (const path of temporary.splice(0)) rmSync(path, { recursive: true, force: true });
});

function evidence(
  accountId: string,
  usedPercent: number,
  liveAssignments = 0,
  headroom: FleetAccountEvidence["headroom"] = "plenty",
): FleetAccountEvidence {
  return {
    accountId,
    usedPercent,
    headroom,
    liveAssignments,
    totalTokens: 0,
    windowId: "codex:primary@2026-08-28T07:00:00.000Z",
    windowResetsAt: "2026-08-28T07:00:00.000Z",
  };
}

const census: FleetAccountEvidence[] = [
  evidence("gmail", 44, 3),
  evidence("proton", 48, 4),
  evidence("apple", 72, 0, "normal"),
  evidence("pm", 100, 0, "exhausted"),
  evidence("business", 100, 0, "exhausted"),
];

function providerFixture(rows: readonly FleetAccountEvidence[], root: string): {
  accounts: ProviderAccount[];
  reports: AccountUsageReport[];
} {
  const accounts: ProviderAccount[] = rows.map(({ accountId }) => ({
    id: accountId,
    provider: "openai",
    profile: accountId,
    authMode: "isolated",
    root: join(root, accountId),
  }));
  const reports: AccountUsageReport[] = rows.map(({ accountId, usedPercent }) => ({
    accountId,
    provider: "openai",
    source: "codex-app-server:account-rate-limits",
    status: "observed",
    cached: false,
    observation: {
      targetId: accountId,
      provider: "openai",
      source: "codex-app-server:account-rate-limits",
      observedAt: now.toISOString(),
      windows: [{
        limitId: "codex:primary",
        usedPercent,
        resetsAt: "2026-08-28T07:00:00.000Z",
      }],
    },
    unavailableComponents: [],
  }));
  return { accounts, reports };
}

function liveDependencies(rows: readonly FleetAccountEvidence[], root: string) {
  const fixture = providerFixture(rows, root);
  return {
    accounts: () => fixture.accounts,
    refreshUsage: async () => fixture.reports,
    readActivity: async ({ accountRoot }: { accountRoot: string }) => ({
      hours: 24,
      sessions: 1,
      live: rows.find(({ accountId }) => accountRoot.endsWith(accountId))!.liveAssignments,
      totalTokens: 0,
      outputTokens: 0,
    }),
    now: () => now,
  };
}

test("fresh utilization outranks activity skew at the exact 44/48/72 percentages", () => {
  const selection = selectFleetDispatchAccount(census, emptyFleetDispatchState(), 30_000, now);

  expect(selection.selected.accountId).toBe("gmail");
  expect(selection.closeCandidates.map(({ accountId }) => accountId)).toEqual(["gmail"]);
  expect(selection.eligible.map(({ accountId }) => accountId)).toEqual([
    "gmail", "proton", "apple",
  ]);
  expect(selection.excluded.map(({ accountId, reason }) => [accountId, reason])).toEqual([
    ["business", "low-headroom"],
    ["pm", "low-headroom"],
  ]);
});

test("reservations implement the canonical 45/50 projected-utilization sequence", () => {
  const rows = [evidence("acc1", 50), evidence("acc2", 45)];
  const initial = emptyFleetDispatchState();
  const x = reserveFleetDispatchAccount(rows, initial, "task-x", 30_000, now);
  expect(x.assignment.accountId).toBe("acc2");
  expect(x.assignment.projectedUsedPercent).toBe(45);
  expect(x.assignment.postReservationProjectedUsedPercent).toBe(48);

  const y = reserveFleetDispatchAccount(rows, x.state, "task-y", 30_000, now);
  expect(y.assignment.accountId).toBe("acc2");
  expect(y.assignment.projectedUsedPercent).toBe(48);
  expect(y.assignment.postReservationProjectedUsedPercent).toBe(51);

  const next = selectFleetDispatchAccount(rows, y.state, 30_000, now);
  expect(next.selected.accountId).toBe("acc1");
  expect(next.selected.projectedUsedPercent).toBe(50);
  expect(next.eligible.find(({ accountId }) => accountId === "acc2")?.projectedUsedPercent)
    .toBe(51);
});

test("isolated observed token and percentage deltas produce versioned conservative calibration", () => {
  const rows = [evidence("acc1", 50), evidence("acc2", 45)];
  const reserved = reserveFleetDispatchAccount(
    rows, emptyFleetDispatchState(), "calibration-sample", 30_000, now,
  );
  const sample: FleetDispatchState = {
    ...reserved.state,
    assignments: [{
      ...reserved.assignment,
      status: "reconciled",
      updatedAt: new Date(now.getTime() + 1_000).toISOString(),
      actualTokens: 30_000,
      reconciledUsedPercent: 48,
      reconciliationUncertainty: "isolated-account-window",
    }],
  };

  const selection = selectFleetDispatchAccount(rows, sample, 10_000, now);
  expect(selection.eligible.find(({ accountId }) => accountId === "acc2")?.calibration)
    .toMatchObject({
      version: "north:fleet-token-percent-calibration:v1",
      source: "empirical",
      tokensPerPercent: 7_500,
      samples: 1,
      uncertainty: "high",
    });
});

test("stale reservations age out explicitly and stop contributing projected load", () => {
  const rows = [evidence("acc1", 50), evidence("acc2", 45)];
  const reserved = reserveFleetDispatchAccount(rows, emptyFleetDispatchState(), "stale", 80_000, now);
  const later = new Date(now.getTime() + 24 * 60 * 60 * 1_000 + 1);
  const expired = expireFleetDispatchReservations(reserved.state, later);

  expect(expired.expired).toBe(1);
  expect(expired.state.assignments[0]?.status).toBe("expired");
  expect(selectFleetDispatchAccount(rows, expired.state, 10_000, later).selected.accountId)
    .toBe("acc2");
});

test("launches with the selected explicit identity and reconciles the reservation", async () => {
  const root = mkdtempSync(join(tmpdir(), "north-fleet-dispatch-"));
  temporary.push(root);
  const path = join(root, "reservations.json");
  const preloaded = reserveFleetDispatchAccount(
    census,
    emptyFleetDispatchState(),
    "existing-gmail",
    50_000,
    now,
  ).state;
  writeFileSync(path, `${JSON.stringify(preloaded, null, 2)}\n`);
  const launcher = join(root, "codex-fixture");
  const capture = join(root, "launch.txt");
  writeFileSync(launcher, `#!/bin/sh
printf '%s\n' "\${CODEX_HOME-unset}|$*" > "$NORTH_FLEET_CAPTURE"
`);
  chmodSync(launcher, 0o755);
  const dependencies = liveDependencies(census, root);
  const result = await executeFleetDispatch({
    assignmentId: "repair-api",
    estimatedTokens: 30_000,
    codexArgs: ["exec", "--model", "gpt-5.6-terra", "-"],
  }, {
    ...dependencies,
    assignmentPath: path,
    env: {
      ...process.env,
      CODEX_HOME: "/wrong/inherited/home",
      NORTH_FLEET_CODEX_COMMAND: launcher,
      NORTH_FLEET_CAPTURE: capture,
    },
  });

  expect(result.selected.accountId).toBe("proton");
  expect(readFileSync(capture, "utf8")).toBe(
    "unset|as proton exec --model gpt-5.6-terra -\n",
  );
  expect(result.assignment).toMatchObject({
    assignmentId: "repair-api",
    accountId: "proton",
    status: "reconciled",
    actualTokens: 0,
    pid: expect.any(Number),
  });
  const state = JSON.parse(readFileSync(path, "utf8")) as FleetDispatchState;
  expect(state.version).toBe(FLEET_STATE_VERSION);
  expect(state.assignments.find(({ assignmentId }) => assignmentId === "repair-api"))
    .toMatchObject({ accountId: "proton", status: "reconciled" });
});

test("the file-lease transaction prevents simultaneous callers sharing a stale winner", async () => {
  const root = mkdtempSync(join(tmpdir(), "north-fleet-concurrent-"));
  temporary.push(root);
  const path = join(root, "reservations.json");
  const rows = [evidence("acc1", 50), evidence("acc2", 45)];
  const dependencies = liveDependencies(rows, root);
  const launched: string[] = [];
  const complete: Array<(code: number) => void> = [];
  let bothStartedResolve!: () => void;
  const bothStarted = new Promise<void>((resolve) => { bothStartedResolve = resolve; });
  const startCodex = async (accountId: string) => {
    launched.push(accountId);
    const completed = new Promise<number>((resolve) => complete.push(resolve));
    if (launched.length === 2) bothStartedResolve();
    return { completed };
  };
  const dispatch = (assignmentId: string) => executeFleetDispatch({
    assignmentId,
    estimatedTokens: 60_000,
    codexArgs: ["exec", "-"],
  }, { ...dependencies, assignmentPath: path, startCodex });

  const pending = Promise.all([dispatch("concurrent-x"), dispatch("concurrent-y")]);
  await bothStarted;
  expect(launched).toEqual(["acc2", "acc1"]);
  for (const resolve of complete) resolve(0);
  const results = await pending;
  expect(results.map(({ assignment }) => assignment?.accountId).sort()).toEqual(["acc1", "acc2"]);
});

test("a failed launch releases its reservation as cancelled", async () => {
  const root = mkdtempSync(join(tmpdir(), "north-fleet-cancel-"));
  temporary.push(root);
  const path = join(root, "reservations.json");
  const rows = [evidence("gmail", 44), evidence("proton", 48)];
  const dependencies = liveDependencies(rows, root);

  await expect(executeFleetDispatch({
    assignmentId: "cancelled-launch",
    estimatedTokens: 20_000,
    codexArgs: ["exec", "-"],
  }, {
    ...dependencies,
    assignmentPath: path,
    startCodex: async () => { throw new Error("fixture launch failed"); },
  })).rejects.toThrow("fixture launch failed");

  const state = JSON.parse(readFileSync(path, "utf8")) as FleetDispatchState;
  expect(state.assignments).toHaveLength(1);
  expect(state.assignments[0]).toMatchObject({
    assignmentId: "cancelled-launch",
    accountId: "gmail",
    status: "cancelled",
  });
});
