import { expect, test } from "bun:test";
import { decodeStoreSnapshot, safeNext, storeReplayPosition, type StoreFact, type StoreSnapshot } from "../src/store-kernel";

const account = { id: "runner", role: "executor" } as const;

function fact(kind: StoreFact["kind"], sequence: number, values: Partial<StoreFact> = {}): StoreFact {
  return {
    kind,
    sequence,
    digest: `digest-${sequence}`,
    previousDigest: sequence === 1 ? null : `digest-${sequence - 1}`,
    account,
    provenance: "store-writer",
    attempt: "attempt-1",
    ...values,
  };
}

function snapshot(...facts: StoreFact[]): StoreSnapshot { return { account, facts }; }

test("safeNext is conservative across reservation, launch, delivery, cancellation, and settlement", () => {
  expect(safeNext(snapshot())).toEqual({ kind: "reserve", replayPosition: 0 });
  expect(safeNext(snapshot(fact("reserved", 1)))).toEqual({ kind: "launch", attempt: "attempt-1", replayPosition: 1 });
  expect(safeNext(snapshot(fact("reserved", 1), fact("launch-intent", 2))))
    .toEqual({ kind: "reconcile-launch", attempt: "attempt-1", replayPosition: 2 });
  const running = [fact("reserved", 1), fact("launch-intent", 2), fact("started", 3)];
  expect(safeNext(snapshot(...running, fact("command-intent", 4, { command: "deliver" }))))
    .toEqual({ kind: "send", attempt: "attempt-1", command: "deliver", replayPosition: 4 });
  expect(safeNext(snapshot(...running, fact("command-intent", 4, { command: "deliver" }), fact("delivery-intent", 5, { command: "deliver" }))))
    .toEqual({ kind: "reconcile-command", attempt: "attempt-1", command: "deliver", replayPosition: 5 });
  expect(safeNext(snapshot(...running, fact("cancel-request", 4)))).toEqual({ kind: "cancel", attempt: "attempt-1", replayPosition: 4 });
  expect(safeNext(snapshot(...running, fact("cancel-request", 4), fact("cancel-intent", 5))))
    .toEqual({ kind: "reconcile-command", attempt: "attempt-1", command: "cancel", replayPosition: 5 });
  const terminal = [...running, fact("terminal", 4, { terminal: "succeeded" })];
  expect(safeNext(snapshot(...terminal))).toEqual({ kind: "advance", attempt: "attempt-1", replayPosition: 4 });
  expect(safeNext(snapshot(...terminal, fact("advanced", 5)))).toEqual({ kind: "no-op", reason: "settled", replayPosition: 5 });
});

test("Store decoding and decisions fail closed for gaps, digest conflicts, absent proof, combinations, and oversight", () => {
  const reserved = fact("reserved", 1);
  expect(storeReplayPosition(snapshot(reserved, fact("launch-intent", 3, { previousDigest: "digest-1" })))).toBe(0);
  expect(safeNext(snapshot(reserved, fact("launch-intent", 2, { previousDigest: "wrong" })))).toMatchObject({ kind: "invalid", reason: "digest-conflict" });
  expect(safeNext(snapshot(reserved, fact("started", 2)))).toMatchObject({ kind: "invalid", reason: "illegal-combination" });
  expect(decodeStoreSnapshot({ account, facts: [{ ...reserved, provenance: "" }] })).toEqual({ ok: false, reason: "missing-role-or-provenance" });
  expect(safeNext({ account: { id: "auditor", role: "oversight" }, facts: [] })).toEqual({ kind: "no-op", reason: "oversight-account", replayPosition: 0 });
});
