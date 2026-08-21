import { afterEach, expect, test } from "bun:test";
import { chmodSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  executeFleetDispatch,
  FLEET_ASSIGNMENT_VERSION,
  selectFleetDispatchAccount,
  type FleetAccountEvidence,
  type FleetDispatchAssignment,
} from "../src/fleet-dispatch";
import type { ProviderAccount } from "../src/accounts";
import type { AccountUsageReport } from "../src/account-usage";

const temporary: string[] = [];
const now = new Date("2026-08-21T07:00:00.000Z");

afterEach(() => {
  for (const path of temporary.splice(0)) rmSync(path, { recursive: true, force: true });
});

const census: FleetAccountEvidence[] = [
  { accountId: "gmail", usedPercent: 44, headroom: "plenty", liveAssignments: 1 },
  { accountId: "proton", usedPercent: 48, headroom: "plenty", liveAssignments: 0 },
  { accountId: "apple", usedPercent: 72, headroom: "normal", liveAssignments: 6 },
  { accountId: "pm", usedPercent: 91, headroom: "low", liveAssignments: 0 },
  { accountId: "business", usedPercent: 99, headroom: "low", liveAssignments: 0 },
];

function assignment(accountId: string, index: number): FleetDispatchAssignment {
  return {
    version: FLEET_ASSIGNMENT_VERSION,
    assignmentId: `assignment-${index}`,
    accountId,
    selectedAt: new Date(now.getTime() + index * 1_000).toISOString(),
    usedPercent: 0,
    remainingHeadroom: 100,
    liveAssignments: 0,
    score: 100,
  };
}

test("balances the supplied five-account census deterministically", () => {
  const first = selectFleetDispatchAccount(census);
  expect(first.selected.accountId).toBe("proton");
  expect(first.closeCandidates.map(({ accountId }) => accountId).sort()).toEqual(["gmail", "proton"]);
  expect(first.excluded.map(({ accountId, reason }) => [accountId, reason])).toEqual([
    ["apple", "materially-oversubscribed"],
    ["business", "low-headroom"],
    ["pm", "low-headroom"],
  ]);

  const second = selectFleetDispatchAccount(census, [assignment("proton", 0)]);
  expect(second.selected.accountId).toBe("gmail");
  const third = selectFleetDispatchAccount(census, [assignment("proton", 0), assignment("gmail", 1)]);
  expect(third.selected.accountId).toBe("proton");
});

test("launches with codex as and records the selected account on the assignment", async () => {
  const root = mkdtempSync(join(tmpdir(), "north-fleet-dispatch-"));
  temporary.push(root);
  const path = join(root, "assignments.jsonl");
  const accounts: ProviderAccount[] = census.map(({ accountId }) => ({
    id: accountId,
    provider: "openai",
    profile: accountId,
    authMode: "isolated",
    root: join(root, accountId),
  }));
  const reports: AccountUsageReport[] = census.map(({ accountId, usedPercent }) => ({
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
  const launcher = join(root, "codex-fixture");
  const capture = join(root, "launch.txt");
  writeFileSync(launcher, `#!/bin/sh
printf '%s\n' "\${CODEX_HOME-unset}|$*" > "$NORTH_FLEET_CAPTURE"
`);
  chmodSync(launcher, 0o755);
  const result = await executeFleetDispatch({
    assignmentId: "repair-api",
    codexArgs: ["exec", "--model", "gpt-5.6-terra", "-"],
  }, {
    accounts: () => accounts,
    refreshUsage: async () => reports,
    readActivity: async ({ accountRoot }) => ({
      hours: 24,
      sessions: 1,
      live: census.find(({ accountId }) => accountRoot.endsWith(accountId))!.liveAssignments,
      outputTokens: 0,
    }),
    now: () => now,
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
  expect(JSON.parse(readFileSync(path, "utf8"))).toMatchObject({
    version: FLEET_ASSIGNMENT_VERSION,
    assignmentId: "repair-api",
    accountId: "proton",
    pid: expect.any(Number),
  });
});
