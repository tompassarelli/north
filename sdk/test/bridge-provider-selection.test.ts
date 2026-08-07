import { afterEach, expect, test } from "bun:test";
import { mkdtempSync, rmSync } from "node:fs";
import { connect, type Socket } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Northd } from "../src/bridge/host";
import { parseBridgeLaunchArguments } from "../src/bridge/cli";
import { parseBridgeRequest } from "../src/bridge/protocol";
import type { BridgeProviderExecution } from "../src/bridge/provider";

const cleanups: Array<() => Promise<void> | void> = [];
afterEach(async () => {
  for (const cleanup of cleanups.splice(0).reverse()) await cleanup();
});

test("--claude and --openai pin a provider, and bare launches leave it unset", () => {
  expect(parseBridgeLaunchArguments(["--claude", "go"]))
    .toEqual({ role: "implementer", provider: "anthropic", promptArguments: ["go"] });
  expect(parseBridgeLaunchArguments(["--openai", "go"]))
    .toEqual({ role: "implementer", provider: "openai", promptArguments: ["go"] });
  expect(parseBridgeLaunchArguments(["go"]))
    .toEqual({ role: "implementer", promptArguments: ["go"] });
});

test("a provider flag composes with --role in either order", () => {
  expect(parseBridgeLaunchArguments(["--role", "director", "--claude", "go"]))
    .toEqual({ role: "director", provider: "anthropic", promptArguments: ["go"] });
  expect(parseBridgeLaunchArguments(["--claude", "--role", "director", "go"]))
    .toEqual({ role: "director", provider: "anthropic", promptArguments: ["go"] });
});

test("the wire rejects an unknown provider", () => {
  expect(() => parseBridgeRequest({ op: "launch", prompt: "go", cwd: "/", provider: "gemini" }))
    .toThrow("bridge launch provider must be anthropic or openai");
});

async function launched(request: object, selectProvider: () => Promise<any>): Promise<any> {
  const root = mkdtempSync(join(tmpdir(), "north-bridge-select-"));
  const socketPath = join(root, "northd.sock");
  const opened: any[] = [];
  const provider: BridgeProviderExecution = {
    async open(context) {
      opened.push(context.provider);
      throw new Error("stop after selection");
    },
  };
  const northd = new Northd({
    socketPath, journalRoot: join(root, "journal"), provider,
    sourceIdentity: () => undefined, selectProvider,
  });
  await northd.listen();
  cleanups.push(() => rmSync(root, { recursive: true, force: true }));
  cleanups.push(() => northd.close());

  const socket: Socket = connect(socketPath);
  await new Promise<void>((resolve, reject) => {
    socket.once("connect", resolve);
    socket.once("error", reject);
  });
  const messages: any[] = [];
  let buffer = "";
  socket.setEncoding("utf8");
  socket.on("data", (chunk: string) => {
    buffer += chunk;
    while (true) {
      const newline = buffer.indexOf("\n");
      if (newline < 0) break;
      messages.push(JSON.parse(buffer.slice(0, newline)));
      buffer = buffer.slice(newline + 1);
    }
  });
  const closed = new Promise<void>((resolve) => socket.once("close", () => resolve()));
  socket.write(`${JSON.stringify(request)}\n`);
  await closed;
  const starting = messages.find((message) =>
    message.type === "event" && message.record.kind === "provider.starting");
  return { chosen: opened[0], starting: starting?.record.data };
}

test("an unpinned launch takes the headroom selection", async () => {
  const result = await launched(
    { op: "launch", prompt: "go", cwd: "/" },
    async () => "anthropic",
  );
  expect(result.chosen).toBe("anthropic");
  expect(result.starting.provider).toBe("anthropic");
  expect(result.starting.selection).toBe("headroom");
});

test("a pinned launch never consults headroom", async () => {
  let consulted = 0;
  const result = await launched(
    { op: "launch", prompt: "go", cwd: "/", provider: "openai" },
    async () => { consulted += 1; return "anthropic"; },
  );
  expect(result.chosen).toBe("openai");
  expect(result.starting.selection).toBe("pinned");
  expect(consulted).toBe(0);
});
