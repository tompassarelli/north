import { afterEach, expect, test } from "bun:test";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { connect, type Socket } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Northd } from "../src/bridge/host";
import type { BridgeProviderExecution } from "../src/bridge/provider";
import type { BridgeServerMessage } from "../src/bridge/protocol";
import { ProviderRetrySafeError } from "../src/providers/types";

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
}> {
  const root = mkdtempSync(join(tmpdir(), "north-bridge-cause-"));
  const socketPath = join(root, "northd.sock");
  const northd = new Northd({
    socketPath, journalRoot: join(root, "journal"), provider: { open },
    sourceIdentity: () => undefined,
  });
  await northd.listen();
  cleanups.push(() => rmSync(root, { recursive: true, force: true }));
  cleanups.push(() => northd.close());

  const messages = await exchange(socketPath, { op: "launch", prompt: "go", cwd: root });
  const launched = messages.find((message) => message.type === "launched");
  if (!launched || launched.type !== "launched") throw new Error("launch id missing");
  const journalPath = join(root, "journal", launched.executionId, "events.log");
  return {
    messages,
    attach: await exchange(socketPath, {
      op: "attach", executionId: launched.executionId, cursor: 0,
    }),
    journal: readFileSync(journalPath, "utf8"),
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
});
