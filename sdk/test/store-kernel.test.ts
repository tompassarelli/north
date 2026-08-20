import { expect, test } from "bun:test";
import { decodeStoreSnapshot, safeNext, storeReplayPosition, type StoreAttempt, type StoreBridgeCommand, type StoreFact, type StoreSnapshot, type StoreWireEvent } from "../src/store-kernel";

const manifest = "a".repeat(64);
const attemptDigest = "b".repeat(64);
const account = { id: "runner", account_role: "execution" } as const;
const attempt: StoreAttempt = {
  subject: `@attempt:${manifest}`,
  execution_attempt_id: "attempt-1",
  execution_attempt_manifest_sha256: manifest,
  execution_attempt_account_id: account.id,
  execution_attempt_account_role: account.account_role,
};
const command: StoreBridgeCommand = {
  subject: `@bridge-command:${attemptDigest}:4`,
  bridge_command_ordinal: 4,
  bridge_command_attempt_id_sha256: attemptDigest,
};

function fact(kind: StoreFact["kind"], ordinal: number, values: Partial<StoreFact> = {}): StoreFact {
  return { kind, ordinal, account, provenance: "store-writer", attempt, ...values };
}

function wire(sequence: number): StoreWireEvent {
  return {
    wire_event_sequence: sequence,
    wire_event_sha256: sequence.toString(16).padStart(64, "0"),
    wire_event_predecessor_sha256: sequence === 1 ? null : (sequence - 1).toString(16).padStart(64, "0"),
  };
}

function snapshot(facts: readonly StoreFact[] = [], wireEvents: readonly StoreWireEvent[] = []): StoreSnapshot {
  return { account, facts, wireEvents };
}

test("safeNext keeps lifecycle and bridge receipts out of wire replay progress", () => {
  const running = [fact("reserved", 11), fact("launch-intent", 15), fact("started", 20)];
  expect(safeNext(snapshot([fact("reserved", 11)]))).toMatchObject({ kind: "launch", replayPosition: 0 });
  expect(safeNext(snapshot([...running, fact("command-intent", 33, { command })], [wire(1), wire(2)])))
    .toMatchObject({ kind: "send", replayPosition: 2, command });
  expect(safeNext(snapshot([...running, fact("command-intent", 33, { command }), fact("delivery-intent", 34, { command }), fact("delivery-receipt", 35, { command })], [wire(1), wire(2)])))
    .toMatchObject({ kind: "no-op", replayPosition: 2 });
  expect(storeReplayPosition(snapshot([...running, fact("terminal", 99, { terminal: "succeeded" })], [wire(1), wire(2)]))).toBe(2);
  expect(safeNext(snapshot([fact("reserved", 11), fact("launch-intent", 15), fact("proved-unsent", 16)], [wire(1)])))
    .toMatchObject({ kind: "advance", replayPosition: 1 });
});

test("wire replay and Store authority schemas fail closed", () => {
  expect(storeReplayPosition(snapshot([], [wire(1), { ...wire(3), wire_event_predecessor_sha256: wire(1).wire_event_sha256 }]))).toBe(0);
  expect(safeNext(snapshot([], [wire(1), { ...wire(2), wire_event_predecessor_sha256: "f".repeat(64) }]))).toMatchObject({ kind: "invalid", reason: "digest-conflict" });
  expect(decodeStoreSnapshot({ account, facts: [], wireEvents: [], account_role: "executor" })).toEqual({ ok: true, snapshot: { account, facts: [], wireEvents: [] } });
  expect(decodeStoreSnapshot({ account: { id: "runner", account_role: "executor" }, facts: [], wireEvents: [] })).toEqual({ ok: false, reason: "missing-role-or-provenance" });
  expect(safeNext({ account: { id: "auditor", account_role: "oversight" }, facts: [], wireEvents: [wire(1)] }))
    .toEqual({ kind: "no-op", reason: "oversight-account", replayPosition: 1 });
});
