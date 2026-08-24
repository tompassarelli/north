import { expect, test } from "bun:test";
import {
  prepareManagedBridgeAppLaunch,
} from "../src/bridge/app-launch-reservation";
import {
  parseBridgeAppLaunchArguments,
  settleManagedAppLaunchClose,
} from "../src/bridge/cli";
import { MemoryBridgeCommandReceipts } from "../src/bridge/command-receipts";
import {
  "boot!" as boot,
  "bridge-app-launch-argv!" as bridgeAppLaunchArgv,
} from "../src/bridge/generated/north/bridge/app.js";
import {
  "make-model" as makeModel,
  "select-thread" as selectThread,
} from "../src/bridge/generated/north/bridge/model.js";
import { WireEventWriter, wireRunId } from "../src/wire";

const ATTEMPT_ID = `@attempt:${"a".repeat(64)}`;
const THREAD_ID = "019fa4ec-d2e6-7f8f-b375-a4f2ea407a0c";
const EXECUTION_ID = "123e4567-e89b-42d3-a456-426614174000";
const THREAD_LEASE = Object.freeze({
  resource: `thread:${THREAD_ID}:dispatch`, holder: "bridge-fixture", epoch: 1,
});
const ACCOUNT_LEASE = Object.freeze({
  resource: "codex-account:codex-a:slot:0", holder: "bridge-fixture", epoch: 1,
});
const RESERVATION = Object.freeze({
  contractOrigin: "worker-defined" as const,
  baselineDoneWhen: [],
  attemptId: ATTEMPT_ID,
  attemptOrdinal: 1,
  manifestSha256: "1".repeat(64),
  provider: "openai" as const,
  accountId: "codex-a",
  model: "gpt-5.6-terra",
  accountAuthorityReceiptSha256: "2".repeat(64),
  routeObservationReceiptSha256: "3".repeat(64),
  threadLease: THREAD_LEASE,
  accountLease: ACCOUNT_LEASE,
});
const STORE_ROUTE = Object.freeze({
  attemptId: ATTEMPT_ID,
  provider: "openai" as const,
  accountId: "codex-a",
  credentialProfile: "codex-a",
  model: "gpt-5.6-terra",
  accountAuthorityReceiptSha256: "2".repeat(64),
  routeObservationReceiptSha256: "3".repeat(64),
  launchIntentSha256: "4".repeat(64),
});

function runtime(threadId = "", controlThreadId = "") {
  return {
    model: threadId ? selectThread(makeModel("list"), threadId) : makeModel("list"),
    controlThreadId,
    launchProvider: "",
    launchTier: "",
    launchModel: "",
    launchEffort: "",
  };
}

test("ordinary empty-selection boot spawns no invalid Bridge child", () => {
  const app = runtime();
  let launches = 0;
  boot(app, async () => { launches += 1; });

  expect(launches).toBe(0);
  expect(() => bridgeAppLaunchArgv(app, "supervise", "supervisor"))
    .toThrow("selected or managed control thread");
});

test("authored launches ask the broker to reserve an attempt for the exact thread", () => {
  const argv = bridgeAppLaunchArgv(runtime(THREAD_ID), "implement", "worker") as string[];
  expect(argv.slice(1)).toEqual([
    "bridge", "app-launch", "--thread", THREAD_ID,
    "--role", "implementer", "implement",
  ]);
  expect(argv).not.toContain("--attempt");
});

test("the app-launch parser accepts an exact thread but never a caller attempt", () => {
  expect(parseBridgeAppLaunchArguments([
    "--thread", THREAD_ID, "--role", "director", "supervise",
  ])).toEqual({
    role: "director",
    promptArguments: ["supervise"],
    selectedThreadId: THREAD_ID,
  });
  expect(() => parseBridgeAppLaunchArguments(["supervise"]))
    .toThrow("requires --thread");
  expect(() => parseBridgeAppLaunchArguments([
    "--thread", THREAD_ID, "--attempt", ATTEMPT_ID, "supervise",
  ])).toThrow("reserves its own attempt");
});

test("the broker owns Store reservation through the durable provider terminal", async () => {
  const calls: string[] = [];
  const receipts = new MemoryBridgeCommandReceipts([STORE_ROUTE]);
  const bind = receipts.bindExecution.bind(receipts);
  receipts.bindExecution = async (...args) => {
    calls.push("bind");
    return bind(...args);
  };
  const managed = await prepareManagedBridgeAppLaunch({
    role: "implementer",
    prompt: "supervise",
    cwd: "/tmp",
    selectedThreadId: THREAD_ID,
  }, {
    env: { AGENT_ID: "bridge-control" },
    executionId: EXECUTION_ID,
    loadThreadFacts: (actual) => {
      calls.push("thread");
      expect(actual).toBe(THREAD_ID);
      return [{ predicate: "title", value: "Bridge control" }];
    },
    selectProvider: async (requested) => {
      calls.push("select");
      expect(requested).toEqual({ provider: "openai" });
      return {
        provider: "openai",
        target: "codex-a",
        routingTargets: {
          "codex-a": {
            id: "codex-a", provider: "openai", authMode: "isolated", profile: "codex-a",
          },
        },
        executionAccountReceipt: {
          accountAuthority: { digest: "2".repeat(64) },
          usage: { receipt: { digest: "3".repeat(64) } },
        },
      } as never;
    },
    acquireLeases: async () => ({
      threadLease: THREAD_LEASE,
      accountLease: ACCOUNT_LEASE,
      release: async () => { calls.push("release"); },
    }),
    reserve: () => {
      calls.push("reserve");
      return RESERVATION;
    },
    launchIntent: () => {
      calls.push("intent");
      return {
        attemptId: ATTEMPT_ID,
        launchIntentSha256: "4".repeat(64),
        launchedAt: "2026-08-24T00:00:00.000Z",
      };
    },
    providerStart: (_context, _reservation, _intent, receipt) => {
      calls.push("provider-start");
      expect(receipt).toMatch(/^[0-9a-f]{64}$/);
      return {
        attemptId: ATTEMPT_ID,
        providerStartReceiptSha256: receipt,
        providerStartManifestSha256: "5".repeat(64),
        providerStartedAt: "2026-08-24T00:00:01.000Z",
      };
    },
    terminal: (_context, _reservation, _intent, _providerStart, receipt) => {
      calls.push("terminal");
      expect(receipt).toMatch(/^[0-9a-f]{64}$/);
      return {
        attemptId: ATTEMPT_ID,
        terminalReceiptSha256: receipt,
        terminalManifestSha256: "6".repeat(64),
        terminalAt: "2026-08-24T00:00:02.000Z",
      };
    },
    commandReceipts: receipts,
  });

  const writer = new WireEventWriter({ runId: wireRunId("run:bridge-app-test") });
  writer.append({ kind: "run.started", lifecycle: "running", owner: "bridge:director" });
  await managed.observeDurableWireEvent(writer.append({
    kind: "model-call.started",
    modelCallId: "model-call:bridge-app-test:1" as never,
    model: { provider: "openai", capabilityClass: "authoring" },
    effort: "medium",
    attempt: 1,
  }));
  await managed.observeDurableWireEvent(writer.terminate({
    lifecycle: "failed",
    reason: { code: "provider_error" },
  }).at(-1)!);

  expect(managed.settled).toBe(true);
  expect(calls).toEqual([
    "thread", "select", "reserve", "intent", "bind",
    "provider-start", "terminal", "release",
  ]);
});

test("only a client close before daemon launch is proved unsent", async () => {
  const calls: string[] = [];
  const managed = await prepareManagedBridgeAppLaunch({
    role: "implementer",
    prompt: "implement",
    cwd: "/tmp",
    selectedThreadId: THREAD_ID,
  }, {
    executionId: EXECUTION_ID,
    loadThreadFacts: () => [{ predicate: "title", value: "Bridge control" }],
    selectProvider: async () => ({
      provider: "openai",
      target: "codex-a",
      routingTargets: {},
      executionAccountReceipt: {
        accountAuthority: { digest: "2".repeat(64) },
        usage: { receipt: { digest: "3".repeat(64) } },
      },
    } as never),
    acquireLeases: async () => ({
      threadLease: THREAD_LEASE,
      accountLease: ACCOUNT_LEASE,
      release: async () => { calls.push("release"); },
    }),
    reserve: () => RESERVATION,
    launchIntent: () => ({
      attemptId: ATTEMPT_ID,
      launchIntentSha256: "4".repeat(64),
      launchedAt: "2026-08-24T00:00:00.000Z",
    }),
    provedUnsent: (_context, _reservation, _intent, receipt) => {
      calls.push("proved-unsent");
      expect(receipt).toMatch(/^[0-9a-f]{64}$/);
      return {
        attemptId: ATTEMPT_ID,
        unsentReceiptSha256: receipt,
        unsentManifestSha256: "5".repeat(64),
        unsentAt: "2026-08-24T00:00:01.000Z",
      };
    },
    commandReceipts: new MemoryBridgeCommandReceipts([STORE_ROUTE]),
  });

  await settleManagedAppLaunchClose(managed, true);
  expect(managed.settled).toBe(false);
  expect(calls).toEqual([]);

  await settleManagedAppLaunchClose(managed, false);
  expect(managed.settled).toBe(true);
  expect(calls).toEqual(["proved-unsent", "release"]);
});

test("an unregistered TUI selection cannot reach provider routing", async () => {
  let routed = false;
  await expect(prepareManagedBridgeAppLaunch({
    role: "implementer",
    prompt: "implement",
    cwd: "/tmp",
    selectedThreadId: "missing-thread",
  }, {
    loadThreadFacts: () => [],
    selectProvider: async () => { routed = true; throw new Error("must not route"); },
  })).rejects.toThrow("not registered in Store");
  expect(routed).toBe(false);
});
