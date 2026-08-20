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
      return { servedVersion: 57, snapshot: snapshot() };
    },
    stdout: (output) => stdout.push(output),
  });

  expect(code).toBe(0);
  expect(selectors).toEqual([attemptSubject]);
  expect(closed).toBe(true);
  expect(stdout).toEqual([[
    `attempt ${attemptSubject}`,
    "Store version 57",
    "account runner (execution)",
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
    async loadSnapshot() { return { servedVersion: 58, snapshot: snapshot() }; },
    stdout: (output) => stdout.push(output),
  });

  expect(code).toBe(0);
  expect(JSON.parse(stdout[0]!)).toEqual({
    version: "north:store-recovery-report:v1",
    attempt: attemptSubject,
    servedVersion: 58,
    account,
    action: {
      kind: "launch",
      attempt,
      replayPosition: 0,
    },
  });
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
