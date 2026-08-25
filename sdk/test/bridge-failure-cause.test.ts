import { afterEach, expect, test } from "bun:test";
import { mkdtempSync, readFileSync, rmSync, statSync } from "node:fs";
import { connect, type Socket } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Northd } from "../src/bridge/host";
import { MemoryBridgeCommandReceipts } from "../src/bridge/command-receipts";
import type { BridgeProviderExecution } from "../src/bridge/provider";
import type {
  BridgeServerMessage,
} from "../src/bridge/generated/north/bridge/protocol.js";
import { ProviderRetrySafeError } from "../src/providers/types";

const FAILURE_DIAGNOSTIC_DETAIL_BYTES = 4_096;
const ATTEMPT_ID = `@attempt:${"a".repeat(64)}`;
const cleanups: Array<() => Promise<void> | void> = [];
afterEach(async () => {
  for (const cleanup of cleanups.splice(0).reverse()) await cleanup();
});

async function exchange(socketPath: string, request: object): Promise<BridgeServerMessage[]> {
  const socket: Socket = connect(socketPath);
  const connected = Promise.withResolvers<void>();
  socket.once("connect", () => connected.resolve());
  socket.once("error", connected.reject);
  await connected.promise;
  const messages: BridgeServerMessage[] = [];
  let buffer = "";
  socket.setEncoding("utf8");
  socket.on("data", (chunk: string) => {
    buffer += chunk;
    while (true) {
      const newline = buffer.indexOf("\n");
      if (newline < 0) break;
      messages.push(JSON.parse(buffer.slice(0, newline)) as BridgeServerMessage);
      buffer = buffer.slice(newline + 1);
    }
  });
  const closed = Promise.withResolvers<void>();
  socket.once("close", () => closed.resolve());
  socket.write(`${JSON.stringify(request)}\n`);
  await closed.promise;
  return messages;
}

async function launch(open: BridgeProviderExecution["open"]): Promise<{
  messages: BridgeServerMessage[];
  attach: BridgeServerMessage[];
  journal: string;
  diagnostic: string;
  diagnosticMode: number;
}> {
  const root = mkdtempSync(join(tmpdir(), "north-bridge-cause-"));
  const socketPath = join(root, "northd.sock");
  const northd = new Northd({
    socketPath, journalRoot: join(root, "journal"), provider: { open },
    sourceIdentity: () => undefined,
    commandReceipts: new MemoryBridgeCommandReceipts([ATTEMPT_ID]),
  });
  await northd.listen();
  cleanups.push(() => rmSync(root, { recursive: true, force: true }));
  cleanups.push(() => northd.close());

  const messages = await exchange(socketPath, {
    op: "launch", prompt: "go", cwd: root, attemptId: ATTEMPT_ID,
  });
  const launched = messages.find((message) => message.type === "launched");
  if (!launched || launched.type !== "launched") throw new Error("launch id missing");
  const journalPath = join(root, "journal", launched.executionId, "events.log");
  const diagnosticPath = join(
    root,
    "journal",
    launched.executionId,
    "failure-diagnostic.json",
  );
  return {
    messages,
    attach: await exchange(socketPath, {
      op: "attach", executionId: launched.executionId, cursor: 0,
    }),
    journal: readFileSync(journalPath, "utf8"),
    diagnostic: readFileSync(diagnosticPath, "utf8"),
    diagnosticMode: statSync(diagnosticPath).mode & 0o777,
  };
}

function failure(messages: BridgeServerMessage[]): Record<string, unknown> | undefined {
  const event = messages.find((message) =>
    message.type === "event" && message.record.kind === "execution.failure");
  return event?.type === "event" ? event.record.data : undefined;
}

test("provider prose, JSON-RPC payloads, stderr, and cause chains never enter Bridge surfaces", async () => {
  const canaries = [
    "RAW_PROVIDER_JSON_RPC_CANARY",
    "RAW_PROVIDER_STDERR_CANARY",
    "RAW_PROVIDER_CAUSE_CANARY",
  ];
  const result = await launch(async () => {
    throw new Error(`provider failed: {"message":"${canaries[0]}"}`, {
      cause: new Error(`stderr: ${canaries[1]}`, {
        cause: new Error(canaries[2]),
      }),
    });
  });
  expect(failure(result.messages)).toEqual({
    code: "provider_error",
    classification: "provider_failure",
  });
  const publicMaterial = `${JSON.stringify(result.messages)}\n${JSON.stringify(result.attach)}\n${result.journal}`;
  for (const canary of canaries) expect(publicMaterial).not.toContain(canary);
  for (const canary of canaries) expect(result.diagnostic).toContain(canary);
  expect(result.diagnosticMode).toBe(0o600);
});

test("a provider-named typed runtime failure publishes only generic North-owned classification", async () => {
  const canary = "openai_target_PRIVATE_RUNTIME_FAILURE_CANARY";
  const causeCanary = "RAW_PROVIDER_TYPED_CAUSE_CANARY";
  const result = await launch(async () => {
    throw new ProviderRetrySafeError(canary, {
      cause: new Error(causeCanary),
    });
  });
  expect(failure(result.messages)).toEqual({
    code: "provider_error",
    classification: "provider_runtime_failure",
  });
  const terminal = result.messages.find((message) =>
    message.type === "wire" && message.event.kind === "run.terminated");
  expect(terminal?.type === "wire" ? terminal.event : undefined).toMatchObject({
    kind: "run.terminated",
    lifecycle: "failed",
  });
  expect(terminal?.type === "wire" && terminal.event.kind === "run.terminated"
    ? terminal.event.reason
    : undefined).toEqual({ code: "provider_error" });
  const publicMaterial = `${JSON.stringify(result.messages)}\n${JSON.stringify(result.attach)}\n${result.journal}`;
  expect(publicMaterial).not.toContain(canary);
  expect(publicMaterial).not.toContain(causeCanary);
  expect(result.diagnostic).toContain(canary);
  expect(result.diagnostic).toContain(causeCanary);
});

test("private failure diagnostics redact secrets, shorten home paths, and stay bounded", async () => {
  const home = process.env.HOME;
  if (!home) throw new Error("HOME is required by this test");
  const secret = "sk-supervisorsecret012345";
  const result = await launch(async () => {
    throw new Error(`${home}/private ${secret} ${"x".repeat(8_000)}`);
  });
  expect(result.diagnostic).not.toContain(home);
  expect(result.diagnostic).toContain("~/private");
  expect(result.diagnostic).not.toContain(secret);
  expect(result.diagnostic).toContain("sk-REDACTED");
  expect(Buffer.byteLength(JSON.parse(result.diagnostic).detail, "utf8"))
    .toBeLessThanOrEqual(FAILURE_DIAGNOSTIC_DETAIL_BYTES);
});

test("private failure diagnostics redact sensitive values crossing the output boundary", async () => {
  const home = process.env.HOME;
  if (!home) throw new Error("HOME is required by this test");
  const homePrefix = home.slice(0, Math.max(1, home.length - 2));
  const homeResult = await launch(async () => {
    throw new Error(`${"x".repeat(4_090)}${home}/private`);
  });
  expect(homeResult.diagnostic).not.toContain(homePrefix);
  expect(homeResult.diagnostic).toContain("~");

  const secretPrefix = "sk-sup";
  const secretResult = await launch(async () => {
    throw new Error(`${"x".repeat(4_089)} sk-supervisorsecret012345`);
  });
  expect(secretResult.diagnostic).not.toContain(secretPrefix);
  expect(secretResult.diagnostic).toContain("sk-…");
});
