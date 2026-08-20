import { afterEach, expect, test } from "bun:test";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import * as acp from "@agentclientprotocol/sdk";
import { createBridgeAcpApplication } from "../src/acp/agent";
import { MemoryBridgeCommandReceipts } from "../src/bridge/command-receipts";
import { Northd } from "../src/bridge/host";
import { scanJournalFile } from "../src/bridge/journal";
import type {
  BridgeProviderExecution,
  BridgeProviderOpenContext,
  BridgeProviderSession,
} from "../src/bridge/provider";
import { wireArtifactId, wireToolCallId } from "../src/wire";
import { BridgeWireTestSession } from "./support/bridge-wire-session";

const cleanups: Array<() => Promise<void> | void> = [];
const attemptId = `@attempt:${"a".repeat(64)}`;

afterEach(async () => {
  for (const cleanup of cleanups.splice(0).reverse()) await cleanup();
});

interface AcpFixture {
  root: string;
  journalRoot: string;
  socketPath: string;
  northd: Northd;
}

async function fixture(provider: BridgeProviderExecution): Promise<AcpFixture> {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "north-bridge-acp-"));
  const socketPath = path.join(root, "northd.sock");
  const journalRoot = path.join(root, "journal");
  const northd = new Northd({
    socketPath,
    journalRoot,
    provider,
    commandReceipts: new MemoryBridgeCommandReceipts([attemptId]),
    selectProvider: async () => "openai",
    sourceIdentity: () => undefined,
  });
  await northd.listen();
  cleanups.push(() => fs.rmSync(root, { recursive: true, force: true }));
  cleanups.push(() => northd.close());
  return { root, journalRoot, socketPath, northd };
}

async function waitFor<T>(read: () => T | undefined, label: string): Promise<T> {
  const deadline = Date.now() + 3_000;
  let value = read();
  while (value === undefined && Date.now() < deadline) {
    await Bun.sleep(5);
    value = read();
  }
  if (value === undefined) throw new Error(`timed out waiting for ${label}`);
  return value;
}

function initializedClient(updates: acp.SessionNotification[]): acp.ClientApp {
  return acp.client({ name: "north-acp-test" })
    .onNotification(acp.methods.client.session.update, ({ params }) => {
      updates.push(structuredClone(params));
    });
}

async function initialize(client: acp.ClientContext): Promise<acp.InitializeResponse> {
  return client.request(acp.methods.agent.initialize, {
    protocolVersion: acp.PROTOCOL_VERSION,
    clientCapabilities: {},
    clientInfo: { name: "north-acp-test", version: "1" },
  });
}

test("ACP refuses session creation without a reserved attempt before provider launch", async () => {
  let providerOpened = false;
  const f = await fixture({
    async open() {
      providerOpened = true;
      throw new Error("provider must not open without a reserved attempt");
    },
  });
  const { app } = createBridgeAcpApplication({ socketPath: f.socketPath, attemptId: "" });

  await initializedClient([]).connectWith(app, async (client) => {
    await initialize(client);
    await expect(client.request(acp.methods.agent.session.new, {
      cwd: f.root,
      mcpServers: [],
    })).rejects.toThrow("bridge launch requires a canonical reserved attempt id");
  });
  expect(providerOpened).toBe(false);
});

test("ACP initializes, rejects borrowed authority, and isolates concurrent Bridge sessions", async () => {
  const sessions = new Map<string, BridgeWireTestSession>();
  const contexts = new Map<string, BridgeProviderOpenContext>();
  const provider: BridgeProviderExecution = {
    async open(context) {
      contexts.set(context.executionId, context);
      const session = new BridgeWireTestSession(context);
      sessions.set(context.executionId, session);
      return session;
    },
  };
  const f = await fixture(provider);
  const updates: acp.SessionNotification[] = [];
  const { app } = createBridgeAcpApplication({ socketPath: f.socketPath, attemptId });

  await initializedClient(updates).connectWith(app, async (client) => {
    const handshake = await initialize(client);
    expect(handshake).toMatchObject({
      protocolVersion: 1,
      agentCapabilities: {
        loadSession: true,
        promptCapabilities: { image: false, audio: false, embeddedContext: false },
        sessionCapabilities: { close: {} },
      },
    });
    expect(handshake.agentCapabilities?.mcpCapabilities).toBeUndefined();

    await expect(client.request(acp.methods.agent.session.new, {
      cwd: "relative/path",
      mcpServers: [],
    })).rejects.toThrow("ACP cwd must be absolute");
    await expect(client.request(acp.methods.agent.session.new, {
      cwd: f.root,
      mcpServers: [{ name: "borrowed", command: "/bin/false", args: [], env: [] }],
    })).rejects.toThrow("does not accept client-supplied MCP servers");
    await expect(client.request(acp.methods.agent.session.new, {
      cwd: f.root,
      additionalDirectories: [path.join(f.root, "other")],
      mcpServers: [],
    })).rejects.toThrow("does not accept additional workspace directories");
    await expect(client.request(acp.methods.agent.session.load, {
      sessionId: "../another-client",
      cwd: f.root,
      mcpServers: [],
    })).rejects.toThrow("session ID must be a UUIDv4");

    const left = await client.request(acp.methods.agent.session.new, {
      cwd: f.root,
      mcpServers: [],
    });
    const right = await client.request(acp.methods.agent.session.new, {
      cwd: f.root,
      mcpServers: [],
    });
    expect(left.sessionId).not.toBe(right.sessionId);

    const leftPrompt = client.request(acp.methods.agent.session.prompt, {
      sessionId: left.sessionId,
      prompt: [{ type: "text", text: "left prompt" }],
    });
    const rightPrompt = client.request(acp.methods.agent.session.prompt, {
      sessionId: right.sessionId,
      prompt: [{
        type: "resource_link",
        name: "right task",
        uri: "file:///right-task.md",
      }],
    });
    const leftSession = await waitFor(() => sessions.get(left.sessionId), "left provider");
    const rightSession = await waitFor(() => sessions.get(right.sessionId), "right provider");
    expect(contexts.get(left.sessionId)).toMatchObject({
      executionId: left.sessionId,
      prompt: "left prompt",
      cwd: f.root,
    });
    expect(contexts.get(right.sessionId)).toMatchObject({
      executionId: right.sessionId,
      prompt: "right task (file:///right-task.md)",
      cwd: f.root,
    });

    leftSession.assistant("left answer");
    rightSession.assistant("right answer");
    leftSession.complete();
    rightSession.complete();
    expect(await leftPrompt).toEqual({ stopReason: "end_turn" });
    expect(await rightPrompt).toEqual({ stopReason: "end_turn" });

    expect(updates.filter((update) => update.sessionId === left.sessionId)
      .map((update) => update.update)).toEqual([{
        sessionUpdate: "agent_message_chunk",
        messageId: expect.any(String),
        content: { type: "text", text: "left answer" },
      }]);
    expect(updates.filter((update) => update.sessionId === right.sessionId)
      .map((update) => update.update)).toEqual([{
        sessionUpdate: "agent_message_chunk",
        messageId: expect.any(String),
        content: { type: "text", text: "right answer" },
      }]);

    await client.request(acp.methods.agent.session.close, { sessionId: left.sessionId });
    await client.request(acp.methods.agent.session.close, { sessionId: right.sessionId });
    expect(leftSession.effects).toContain("terminate");
    expect(rightSession.effects).toContain("terminate");
  });
});

test("ACP cancellation linearizes while the Bridge provider is still opening and remains reusable", async () => {
  const entered = Promise.withResolvers<BridgeProviderOpenContext>();
  const release = Promise.withResolvers<void>();
  let session: BridgeWireTestSession | undefined;
  const provider: BridgeProviderExecution = {
    async open(context) {
      entered.resolve(context);
      await release.promise;
      session = new BridgeWireTestSession(context);
      return session;
    },
  };
  const f = await fixture(provider);
  const updates: acp.SessionNotification[] = [];
  const { app } = createBridgeAcpApplication({
    socketPath: f.socketPath,
    controlTimeoutMs: 1_000,
    attemptId,
  });

  await initializedClient(updates).connectWith(app, async (client) => {
    await initialize(client);
    const created = await client.request(acp.methods.agent.session.new, {
      cwd: f.root,
      mcpServers: [],
    });
    const prompt = client.request(acp.methods.agent.session.prompt, {
      sessionId: created.sessionId,
      prompt: [{ type: "text", text: "cancel during provider open" }],
    });
    await entered.promise;
    const cancellation = client.notify(acp.methods.agent.session.cancel, {
      sessionId: created.sessionId,
    });
    let promptSettled = false;
    void prompt.then(() => { promptSettled = true; });
    await waitFor(() => {
      const records = scanJournalFile(
        path.join(f.journalRoot, created.sessionId, "events.log"),
        created.sessionId,
      ).records;
      return records.some((record) => record.kind === "control.interrupt_turn")
        ? true : undefined;
    }, "durable startup interrupt");
    await Bun.sleep(20);
    expect(promptSettled).toBe(false);

    release.resolve();
    const opened = await waitFor(() => session, "released provider session");
    await waitFor(
      () => opened.effects.includes("interrupt") ? true : undefined,
      "provider interrupt",
    );
    opened.complete(undefined, "cancelled");
    expect(await prompt).toEqual({ stopReason: "cancelled" });
    await cancellation;

    const secondPrompt = client.request(acp.methods.agent.session.prompt, {
      sessionId: created.sessionId,
      prompt: [{ type: "text", text: "second turn" }],
    });
    await waitFor(
      () => opened.effects.includes("submit:second turn") ? true : undefined,
      "second turn delivery",
    );
    opened.assistant("still usable");
    opened.complete();
    expect(await secondPrompt).toEqual({ stopReason: "end_turn" });
    expect(updates.at(-1)).toMatchObject({
      sessionId: created.sessionId,
      update: {
        sessionUpdate: "agent_message_chunk",
        content: { type: "text", text: "still usable" },
      },
    });
    await client.request(acp.methods.agent.session.close, { sessionId: created.sessionId });
  });
});

test("ACP load replays the exact deterministic Wire projection before returning", async () => {
  const sessions = new Map<string, BridgeWireTestSession>();
  const provider: BridgeProviderExecution = {
    async open(context) {
      const session = new BridgeWireTestSession(context);
      sessions.set(context.executionId, session);
      return session;
    },
  };
  const f = await fixture(provider);
  const live: acp.SessionNotification[] = [];
  const first = createBridgeAcpApplication({ socketPath: f.socketPath, attemptId });
  let sessionId = "";

  await initializedClient(live).connectWith(first.app, async (client) => {
    await initialize(client);
    const created = await client.request(acp.methods.agent.session.new, {
      cwd: f.root,
      mcpServers: [],
    });
    sessionId = created.sessionId;
    const prompt = client.request(acp.methods.agent.session.prompt, {
      sessionId,
      prompt: [{ type: "text", text: "produce replay" }],
    });
    const session = await waitFor(() => sessions.get(sessionId), "replay provider");
    session.assistant("durable answer");
    const toolCallId = wireToolCallId(`tool:acp:${sessionId}`);
    session.publish({
      kind: "tool.admitted",
      toolCallId,
      name: "shell_command",
      schema: { status: "unavailable", reason: "test fixture" },
      argumentPreview: "echo durable",
    });
    session.publish({
      kind: "tool.progress",
      toolCallId,
      progress: { phase: "running" },
    });
    const resultArtifactId = wireArtifactId(`artifact:acp:${sessionId}`);
    const resultArtifactDigest = "a".repeat(64);
    session.publish({
      kind: "artifact.published",
      artifactId: resultArtifactId,
      mediaType: "text/plain",
      bytes: 14,
      digest: resultArtifactDigest,
    });
    session.publish({
      kind: "tool.terminal",
      toolCallId,
      status: "succeeded",
      origin: "provider",
      resultPreview: "durable output",
      resultArtifactId,
      resultArtifactDigest,
    });
    session.complete();
    expect(await prompt).toEqual({ stopReason: "end_turn" });
    await client.request(acp.methods.agent.session.close, { sessionId });
  });

  expect(live.map((notification) => notification.update)).toEqual([
    expect.objectContaining({
      sessionUpdate: "agent_message_chunk",
      content: { type: "text", text: "durable answer" },
    }),
    {
      sessionUpdate: "tool_call",
      toolCallId: expect.any(String),
      title: "shell_command",
      kind: "execute",
      status: "pending",
      rawInput: "echo durable",
    },
    {
      sessionUpdate: "tool_call_update",
      toolCallId: expect.any(String),
      status: "in_progress",
      rawOutput: { progress: { phase: "running" } },
    },
    {
      sessionUpdate: "tool_call_update",
      toolCallId: expect.any(String),
      status: "completed",
      content: [{
        type: "content",
        content: { type: "text", text: "durable output" },
      }],
      rawOutput: {
        status: "succeeded",
        preview: "durable output",
        resultArtifactId: wireArtifactId(`artifact:acp:${sessionId}`),
        resultArtifactDigest: "a".repeat(64),
      },
    },
  ]);

  const replay: acp.SessionNotification[] = [];
  const second = createBridgeAcpApplication({ socketPath: f.socketPath, attemptId });
  await initializedClient(replay).connectWith(second.app, async (client) => {
    await initialize(client);
    await expect(client.request(acp.methods.agent.session.load, {
      sessionId,
      cwd: path.join(f.root, "wrong"),
      mcpServers: [],
    })).rejects.toThrow("does not match the Bridge session cwd");
    expect(replay).toEqual([]);

    await client.request(acp.methods.agent.session.load, {
      sessionId,
      cwd: f.root,
      mcpServers: [],
    });
    expect(replay).toEqual(live);
    await expect(client.request(acp.methods.agent.session.prompt, {
      sessionId,
      prompt: [{ type: "text", text: "terminal session" }],
    })).rejects.toThrow("is terminal");
    await client.request(acp.methods.agent.session.close, { sessionId });
  });
});

test("north acp keeps stdout as newline-delimited ACP only", async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "north-acp-stdio-"));
  cleanups.push(() => fs.rmSync(root, { recursive: true, force: true }));
  const north = path.resolve(import.meta.dir, "../../bin/north");
  const child = Bun.spawn([north, "acp"], {
    env: {
      ...process.env,
      BEAGLE_STORE_HOME: root,
      NORTH_BB: process.execPath,
      NORTH_BUN: process.execPath,
    },
    stdin: "pipe",
    stdout: "pipe",
    stderr: "pipe",
  });
  child.stdin.write(`${JSON.stringify({
    jsonrpc: "2.0",
    id: 7,
    method: "initialize",
    params: { protocolVersion: 1, clientCapabilities: {} },
  })}\n`);
  child.stdin.end();
  const [stdout, stderr, exitCode] = await Promise.all([
    new Response(child.stdout).text(),
    new Response(child.stderr).text(),
    child.exited,
  ]);
  expect(exitCode).toBe(0);
  expect(stderr).toBe("");
  const lines = stdout.trimEnd().split("\n");
  expect(lines).toHaveLength(1);
  expect(JSON.parse(lines[0]!)).toEqual({
    jsonrpc: "2.0",
    id: 7,
    result: {
      protocolVersion: 1,
      agentInfo: { name: "north", title: "North Bridge", version: "0.1.0" },
      agentCapabilities: {
        loadSession: true,
        promptCapabilities: { image: false, audio: false, embeddedContext: false },
        sessionCapabilities: { close: {} },
      },
    },
  });
});
