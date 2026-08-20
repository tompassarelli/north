import { expect, test } from "bun:test";
import { runStoreRecoveryCli } from "../src/store-recovery-cli";
import type { StoreSnapshot } from "../src/store-kernel";

const manifest = "a".repeat(64);
const attemptSubject = `@attempt:${manifest}`;
const account = { id: "runner", account_role: "execution" } as const;
const attempt = {
  subject: attemptSubject,
  execution_attempt_id: "attempt-1",
  execution_attempt_manifest_sha256: manifest,
  execution_attempt_account_id: account.id,
  execution_attempt_account_role: account.account_role,
} as const;
const authority = {
  subject: attemptSubject,
  manifestSha256: manifest,
  runId: "run-1",
  threadId: "@thread:one",
  reporterAgentId: "@agent:runner",
  ordinal: 1,
  reservedAt: "2026-08-20T00:00:00Z",
  provider: "openai",
  accountId: "runner",
  model: "gpt-5",
  accountAuthorityReceiptSha256: "b".repeat(64),
  routeObservationReceiptSha256: "c".repeat(64),
  runCapabilitySha256: "9".repeat(64),
  runContractSha256: "8".repeat(64),
  threadLease: { resource: "thread:one:dispatch", holder: "holder", epoch: 1 },
  accountLease: { resource: "codex-account:runner:slot:0", holder: "holder", epoch: 2 },
} as const;

function snapshot(): StoreSnapshot {
  return {
    account,
    facts: [{
      kind: "reserved",
      ordinal: 1,
      account,
      provenance: `${attemptSubject}#execution_attempt_manifest_sha256`,
      attempt,
    }],
    wireEvents: [],
  };
}

test("recovery CLI reports safeNext from one injected Store snapshot", async () => {
  const stdout: string[] = [];
  const selectors: string[] = [];
  let closed = false;
  const code = await runStoreRecoveryCli([attemptSubject], {
    async connect() {
      return {
        async scanAll() { throw new Error("unexpected direct scan"); },
        close() { closed = true; },
      };
    },
    async loadSnapshot(options) {
      selectors.push(options.attemptId ?? "");
      return { servedVersion: 57, snapshot: snapshot(), authority };
    },
    stdout: (output) => stdout.push(output),
  });

  expect(code).toBe(0);
  expect(selectors).toEqual([attemptSubject]);
  expect(closed).toBe(true);
  expect(stdout).toEqual([[
    `attempt ${attemptSubject}`,
    "Store version 57",
    "route openai/runner/gpt-5 (execution)",
    "work @thread:one run=run-1 reporter=@agent:runner",
    `capability ${"9".repeat(64)} contract=${"8".repeat(64)}`,
    `authorization account=${"b".repeat(64)} route=${"c".repeat(64)}`,
    "thread lease thread:one:dispatch holder=holder epoch=1",
    "account lease codex-account:runner:slot:0 holder=holder epoch=2",
    "safe next launch",
    "replay cursor 0",
  ].join("\n")]);
});

test("recovery CLI JSON is a versioned Store report", async () => {
  const stdout: string[] = [];
  const code = await runStoreRecoveryCli([attemptSubject, "--json"], {
    async connect() {
      return { async scanAll() { throw new Error("unexpected direct scan"); }, close() {} };
    },
    async loadSnapshot() { return { servedVersion: 58, snapshot: snapshot(), authority }; },
    stdout: (output) => stdout.push(output),
  });

  expect(code).toBe(0);
  expect(JSON.parse(stdout[0]!)).toEqual({
    version: "north:store-recovery-report:v2",
    attempt: attemptSubject,
    servedVersion: 58,
    account,
    authority,
    action: {
      kind: "launch",
      attempt,
      replayPosition: 0,
    },
  });
});

test("recovery CLI refuses to report an action without immutable attempt authority", async () => {
  const stderr: string[] = [];
  let closed = false;
  const code = await runStoreRecoveryCli([attemptSubject], {
    async connect() {
      return {
        async scanAll() { throw new Error("unexpected direct scan"); },
        close() { closed = true; },
      };
    },
    async loadSnapshot() { return { servedVersion: 59, snapshot: snapshot() }; },
    stderr: (output) => stderr.push(output),
  });

  expect(code).toBe(1);
  expect(closed).toBe(true);
  expect(stderr).toEqual([
    "north recover: Store snapshot omitted the selected attempt authority",
  ]);
});

test("recovery CLI rejects noncanonical selectors before connecting", async () => {
  const stderr: string[] = [];
  let connected = false;
  const code = await runStoreRecoveryCli(["attempt-1"], {
    async connect() { connected = true; throw new Error("must not connect"); },
    stderr: (output) => stderr.push(output),
  });

  expect(code).toBe(2);
  expect(connected).toBe(false);
  expect(stderr[0]).toStartWith("usage: north recover");
});
