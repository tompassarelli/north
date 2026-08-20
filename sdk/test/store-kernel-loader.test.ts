import { expect, test } from "bun:test";
import { createHash } from "node:crypto";
import { loadStoreSnapshot } from "../src/store-kernel-loader";
import { StoreTriple, type Term } from "../src/store-rpc-codec";
import { safeNext } from "../src/store-kernel";

const manifest = "a".repeat(64);
const attempt = `@attempt:${manifest}`;
const commandDigest = createHash("sha256").update(attempt, "utf8").digest("hex");
const digest = (letter: string) => letter.repeat(64);
const triple = (subject: string, predicate: string, value: Term) => new StoreTriple(subject, predicate, value);

function fixture(): Term[] {
  return [
    triple("@account:runner", "kind", "provider_account"),
    triple("@account:runner", "account_id", "runner"),
    triple("@account:runner", "provider", "openai"),
    triple("@account:runner", "account_role", "execution"),
    triple("@account:runner", "execution_eligible", "true"),
    triple(attempt, "kind", "execution_attempt"),
    triple(attempt, "execution_attempt_version", "north:execution-attempt:v1"),
    triple(attempt, "execution_attempt_manifest_sha256", manifest),
    triple(attempt, "execution_attempt_run", "run-1"),
    triple(attempt, "execution_attempt_thread", "@thread:one"),
    triple(attempt, "execution_attempt_reporter", "@agent:runner"),
    triple(attempt, "execution_attempt_ordinal", "1"),
    triple(attempt, "execution_attempt_account", "runner"),
    triple(attempt, "execution_attempt_provider", "openai"),
    triple(attempt, "execution_attempt_model", "gpt-5"),
    triple(attempt, "execution_attempt_account_authority_sha256", digest("b")),
    triple(attempt, "execution_attempt_route_observation_sha256", digest("c")),
    triple(attempt, "execution_attempt_run_capability_sha256", digest("9")),
    triple(attempt, "execution_attempt_run_contract_sha256", digest("8")),
    triple(attempt, "execution_attempt_reserved_at", "2026-08-20T00:00:00Z"),
    triple(attempt, "execution_attempt_thread_lease", '{"epoch":1,"holder":"holder","resource":"thread:one:dispatch"}'),
    triple(attempt, "execution_attempt_account_lease", '{"epoch":2,"holder":"holder","resource":"codex-account:runner:slot:0"}'),
    triple(attempt, "execution_attempt_launch_intent_sha256", digest("d")),
    triple(attempt, "execution_attempt_launch_intent_version", "north:execution-attempt-launch-intent:v1"),
    triple(attempt, "execution_attempt_launch_intent_at", "2026-08-20T00:00:01Z"),
    triple(attempt, "execution_attempt_provider_start_manifest_sha256", digest("e")),
    triple(attempt, "execution_attempt_provider_start_receipt_sha256", digest("7")),
    triple(attempt, "execution_attempt_provider_started_at", "2026-08-20T00:00:02Z"),
    triple(`@bridge-command:${commandDigest}:1`, "bridge.command/execution-id", "execution-1"),
    triple(`@bridge-command:${commandDigest}:1`, "bridge.command/attempt-id", attempt),
    triple(`@bridge-command:${commandDigest}:1`, "bridge.command/ordinal", 1),
    triple(`@bridge-command:${commandDigest}:1`, "bridge.command/kind", "submit-input"),
    triple(`@bridge-command:${commandDigest}:1`, "bridge.command/payload-digest", digest("f")),
    triple(`@bridge-command:${commandDigest}:1`, "bridge.command/payload-artifact", "bridge-journal:execution-1:events.log#record=1"),
    triple(`@bridge-command:${commandDigest}:1`, "bridge.command/delivery", "queued-next-turn"),
    triple("@run:wire-0", "kind", "wire_event"),
    triple("@run:wire-0", "wire_run_id", "run-1"),
    triple("@run:wire-0", "wire_event_sequence", "0"),
    triple("@run:wire-0", "wire_event_json", "{}"),
    triple("@run:wire-0", "wire_event_sha256", digest("1")),
  ];
}

test("Store snapshot loader reconstructs restart facts from one served scan", async () => {
  let calls = 0;
  const loaded = await loadStoreSnapshot({ accountId: "runner", client: {
    async scanAll() { calls += 1; return { rows: fixture(), servedVersion: 57, pages: 1, attempts: 1 }; },
  } });
  expect(calls).toBe(1);
  expect(loaded.servedVersion).toBe(57);
  expect(loaded.snapshot.wireEvents).toEqual([{ wire_event_sequence: 1, wire_event_sha256: digest("1"), wire_event_predecessor_sha256: null }]);
  expect(safeNext(loaded.snapshot)).toMatchObject({ kind: "send", replayPosition: 1 });
});

test("Store snapshot loader rejects conflicting role and eligibility facts", async () => {
  const rows = fixture();
  const eligibility = rows.findIndex((row) => row instanceof StoreTriple
    && row.t1 === "@account:runner" && row.t2 === "execution_eligible");
  rows[eligibility] = triple("@account:runner", "execution_eligible", "false");
  await expect(loadStoreSnapshot({ attemptId: attempt, client: {
    async scanAll() { return { rows, servedVersion: 57, pages: 1, attempts: 1 }; },
  } })).rejects.toThrow("Store snapshot reconstruction refused");
});

test("Store snapshot loader preserves a valid oversight exclusion", async () => {
  const rows = fixture();
  const role = rows.findIndex((row) => row instanceof StoreTriple
    && row.t1 === "@account:runner" && row.t2 === "account_role");
  const eligibility = rows.findIndex((row) => row instanceof StoreTriple
    && row.t1 === "@account:runner" && row.t2 === "execution_eligible");
  rows[role] = triple("@account:runner", "account_role", "oversight");
  rows[eligibility] = triple("@account:runner", "execution_eligible", "false");
  const loaded = await loadStoreSnapshot({ attemptId: attempt, client: {
    async scanAll() { return { rows, servedVersion: 57, pages: 1, attempts: 1 }; },
  } });
  expect(loaded.snapshot.account).toEqual({ id: "runner", account_role: "oversight" });
  expect(safeNext(loaded.snapshot)).toMatchObject({ kind: "no-op", reason: "oversight-account" });
});
