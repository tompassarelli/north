import { expect, test } from "bun:test";
import { resolve } from "node:path";
import {
  NORTH_STORE_ACCEPTANCE_JOURNEYS,
  NorthStoreAcceptanceError,
  type NorthStoreAcceptanceJourney,
  type NorthStoreAcceptanceJourneyResult,
  runNorthStoreAcceptancePreflight,
} from "../src/store-acceptance-preflight";

const CLI = resolve(import.meta.dir, "../src/store-acceptance-preflight-cli.ts");
const releaseId = "a".repeat(64);
const socket = "unix:///tmp/north-store-acceptance-fixture.sock";
const cutoverAt = "2026-08-21T01:00:00.000Z";
const observedAt = "2026-08-21T01:00:01.000Z";
const accounts = ["apple", "gmail", "proton"];

function result(journey: NorthStoreAcceptanceJourney): NorthStoreAcceptanceJourneyResult {
  return {
    journey, exitCode: 0, stdout: `${journey} output\n`, stderr: "",
    evidence: { releaseId, socket, persistenceConfirmed: true, routingEligible: true,
      observedAt, evidenceMode: "authoritative" },
    ...(journey === "account-census" ? { censusAccountIds: accounts } : {}),
  };
}

function fixture(overrides: Partial<Record<NorthStoreAcceptanceJourney, NorthStoreAcceptanceJourneyResult>> = {}) {
  const results = new Map(NORTH_STORE_ACCEPTANCE_JOURNEYS.map((journey) => [journey, overrides[journey] ?? result(journey)]));
  const calls: NorthStoreAcceptanceJourney[] = [];
  return {
    calls,
    runtime: {
      async runJourney(journey: NorthStoreAcceptanceJourney) {
        calls.push(journey);
        return results.get(journey)!;
      },
    },
    results: [...results.values()],
  };
}

function options() {
  return { releaseId, socket, cutoverAt, expectedAccountIds: accounts };
}

test("fake Store fixture accepts one coherent six-command post-cutover result", async () => {
  const fakeStore = fixture();
  const output: string[] = [];
  const receipt = await runNorthStoreAcceptancePreflight({ ...options(), output: (line) => output.push(line) }, fakeStore.runtime);

  expect(fakeStore.calls).toEqual(NORTH_STORE_ACCEPTANCE_JOURNEYS);
  expect(receipt.accountIds).toEqual(accounts);
  expect(receipt.journeys).toEqual(fakeStore.results);
  expect(receipt.journeys.every((journey) => journey.stdout.endsWith("output\n"))).toBe(true);
  expect(output).toHaveLength(7);
  expect(output.at(-1)).toBe(`ACCEPTANCE PASS 6/6 release=${releaseId}`);
});

test("release disagreement rejects after retaining every command result", async () => {
  const mismatched = { ...result("dashboard"), evidence: { ...result("dashboard").evidence, releaseId: "b".repeat(64) } };
  const fakeStore = fixture({ dashboard: mismatched });
  const failure = await runNorthStoreAcceptancePreflight(options(), fakeStore.runtime)
    .then(() => null, (error) => error as NorthStoreAcceptanceError);

  expect(failure).toBeInstanceOf(NorthStoreAcceptanceError);
  expect(fakeStore.calls).toEqual(NORTH_STORE_ACCEPTANCE_JOURNEYS);
  expect(failure!.failures).toContain(`dashboard release mismatch (${"b".repeat(64)} != ${releaseId})`);
  expect(failure!.journeys).toHaveLength(6);
  expect(failure!.journeys.find(({ journey }) => journey === "dashboard")?.stdout).toBe("dashboard output\n");
});

test("an observation at or before cutover is rejected as stale", async () => {
  const stale = { ...result("recover"), evidence: { ...result("recover").evidence, observedAt: cutoverAt } };
  const fakeStore = fixture({ recover: stale });
  await expect(runNorthStoreAcceptancePreflight(options(), fakeStore.runtime))
    .rejects.toThrow("recover observation is not after cutover");
});

async function runCli(payload: unknown, args = ["--release-id", releaseId, "--socket", socket, "--cutover-at", cutoverAt]) {
  const child = Bun.spawn([process.execPath, "run", CLI, ...args], {
    stdin: new TextEncoder().encode(JSON.stringify(payload)), stdout: "pipe", stderr: "pipe", env: process.env,
  });
  const [stdout, stderr, exitCode] = await Promise.all([
    new Response(child.stdout).text(), new Response(child.stderr).text(), child.exited,
  ]);
  return { stdout, stderr, exitCode };
}

test("JSON CLI preserves the six full command results and names a mismatch", async () => {
  const fakeStore = fixture();
  const success = await runCli({ expectedAccountIds: accounts, journeys: fakeStore.results });
  expect(success.exitCode, success.stderr).toBe(0);
  expect(JSON.parse(success.stdout)).toMatchObject({ releaseId, socket, journeys: fakeStore.results });

  const mismatch = fakeStore.results.map((entry) => entry.journey === "ready"
    ? { ...entry, evidence: { ...entry.evidence, socket: "unix:///tmp/other.sock" } } : entry);
  const rejected = await runCli({ expectedAccountIds: accounts, journeys: mismatch });
  expect(rejected.exitCode).toBe(1);
  expect(rejected.stderr).toContain("ready socket mismatch");
  expect(JSON.parse(rejected.stdout)).toMatchObject({
    accepted: false,
    journeys: mismatch,
  });
});
