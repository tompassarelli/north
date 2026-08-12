import { randomUUID } from "node:crypto";
import type { Socket } from "node:net";
import * as path from "node:path";
import * as acp from "@agentclientprotocol/sdk";
import { verifiedSocket, type BridgeConnectionOutput } from "../bridge/cli";
import type { JournalRecord } from "../bridge/journal";
import {
  parseBridgeLaunchExecutionId,
  bridgeSocketPath,
  type BridgeRequest,
  type BridgeServerMessage,
} from "../bridge/protocol";
import { wireRunId, type WireEvent, type WireRunTerminatedEvent } from "../wire";
import { projectWireEvent } from "./wire";

const MAX_BRIDGE_LINE_BYTES = 4 * 1024 * 1024;
const DEFAULT_CONTROL_TIMEOUT_MS = 3_000;

const stderrBridgeOutput: BridgeConnectionOutput = {
  info: (message) => process.stderr.write(`north acp: ${message}\n`),
  error: (message) => process.stderr.write(`north acp: ${message}\n`),
};

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function deferred<T>(): PromiseWithResolvers<T> {
  return Promise.withResolvers<T>();
}

interface ActivePrompt {
  response: PromiseWithResolvers<acp.PromptResponse>;
  ready: PromiseWithResolvers<void>;
  ended: PromiseWithResolvers<void>;
  responseSettled: boolean;
  readySettled: boolean;
  endedSettled: boolean;
  cancelRequested: boolean;
  cleanup?: Promise<void>;
}

interface ManagedSession {
  id: string;
  cwd: string;
  client: acp.AgentContext;
  launched: boolean;
  terminal: boolean;
  closing: boolean;
  closed: boolean;
  feed?: BridgeFeed;
  active?: ActivePrompt;
  closePromise?: Promise<void>;
  unavailable?: Error;
}

interface BridgeFeedCallbacks {
  launched(): void;
  wire(event: WireEvent): Promise<void>;
  idle(record: JournalRecord): void;
  terminal(event: WireRunTerminatedEvent): void;
  failure(error: Error): void;
}

class BridgeFeed {
  readonly ready: Promise<void>;
  #socket: Socket;
  #executionId: string;
  #cwd: string;
  #callbacks: BridgeFeedCallbacks;
  #readyState: PromiseWithResolvers<void>;
  #readySettled = false;
  #barrier = false;
  #terminal = false;
  #detached = false;
  #failed = false;
  #launched = false;
  #expectsLaunch: boolean;
  #cursor = 0;
  #buffer = "";
  #acceptedCwd?: string;
  #beforeBarrier: BridgeServerMessage[] = [];
  #processing = Promise.resolve();

  constructor(
    socket: Socket,
    executionId: string,
    cwd: string,
    request: Extract<BridgeRequest, { op: "launch" | "attach" }>,
    callbacks: BridgeFeedCallbacks,
  ) {
    this.#socket = socket;
    this.#executionId = executionId;
    this.#cwd = cwd;
    this.#callbacks = callbacks;
    this.#expectsLaunch = request.op === "launch";
    this.#readyState = deferred<void>();
    this.ready = this.#readyState.promise;
    this.#socket.setEncoding("utf8");
    this.#socket.on("data", (chunk: string) => this.#receive(chunk));
    this.#socket.once("error", (error) => this.#fail(error));
    this.#socket.once("close", () => {
      void this.#processing.then(() => {
        if (!this.#detached && !this.#terminal && !this.#failed) {
          this.#fail(new Error(`Bridge feed for ${this.#executionId} closed unexpectedly`));
        }
      });
    });
    this.#socket.write(`${JSON.stringify(request)}\n`);
  }

  destroy(): void {
    this.#detached = true;
    this.#socket.destroy();
  }

  #receive(chunk: string): void {
    this.#buffer += chunk;
    if (Buffer.byteLength(this.#buffer, "utf8") > MAX_BRIDGE_LINE_BYTES) {
      this.#fail(new Error("Bridge response line exceeds the ACP adapter bound"));
      return;
    }
    while (true) {
      const newline = this.#buffer.indexOf("\n");
      if (newline < 0) return;
      const line = this.#buffer.slice(0, newline);
      this.#buffer = this.#buffer.slice(newline + 1);
      if (!line) continue;
      let message: BridgeServerMessage;
      try { message = JSON.parse(line) as BridgeServerMessage; }
      catch (error) {
        this.#fail(new Error("Bridge returned malformed JSON", { cause: error }));
        return;
      }
      this.#processing = this.#processing
        .then(() => this.#handle(message))
        .catch((error) => this.#fail(error));
    }
  }

  async #handle(message: BridgeServerMessage): Promise<void> {
    if (this.#failed || this.#detached || message.type === "hello") return;
    if (message.type === "error") throw new Error(message.message);
    if (!this.#barrier) {
      if (message.type === "launched") {
        if (!this.#expectsLaunch || this.#launched) {
          throw new Error("Bridge returned an unexpected ACP launch boundary");
        }
        if (message.executionId !== this.#executionId) {
          throw new Error("Bridge launched a different ACP session");
        }
        this.#launched = true;
        this.#callbacks.launched();
        return;
      }
      if (message.type === "barrier") {
        await this.#commitBarrier(message);
        return;
      }
      this.#beforeBarrier.push(message);
      return;
    }
    if (message.type === "wire") await this.#acceptWire(message.event);
    else if (message.type === "event") this.#acceptControl(message.record);
    else if (message.type === "launched" || message.type === "barrier") {
      throw new Error("Bridge repeated an ACP feed boundary");
    }
  }

  async #commitBarrier(
    barrier: Extract<BridgeServerMessage, { type: "barrier" }>,
  ): Promise<void> {
    if (barrier.executionId !== this.#executionId) {
      throw new Error("Bridge replay belongs to a different ACP session");
    }
    if (this.#expectsLaunch && !this.#launched) {
      throw new Error("Bridge launch replay has no launch boundary");
    }
    if (barrier.tornTail) throw new Error("Bridge control journal has a torn tail");
    for (const message of this.#beforeBarrier) {
      if (message.type === "event") this.#observeReplayControl(message.record);
    }
    if (this.#acceptedCwd === undefined) {
      throw new Error("Bridge replay has no execution acceptance record");
    }
    if (path.resolve(this.#acceptedCwd) !== path.resolve(this.#cwd)) {
      throw new Error("ACP cwd does not match the Bridge session cwd");
    }
    for (const message of this.#beforeBarrier) {
      if (message.type === "wire") await this.#acceptWire(message.event);
    }
    if (barrier.cursor !== this.#cursor) {
      throw new Error("Bridge replay barrier does not match its Wire cursor");
    }
    this.#beforeBarrier = [];
    this.#barrier = true;
    this.#settleReady();
  }

  #observeReplayControl(record: JournalRecord): void {
    if (record.kind !== "execution.accepted") return;
    const cwd = record.data.cwd;
    if (typeof cwd !== "string" || !path.isAbsolute(cwd)) {
      throw new Error("Bridge execution acceptance has no absolute cwd");
    }
    if (this.#acceptedCwd !== undefined && this.#acceptedCwd !== cwd) {
      throw new Error("Bridge execution acceptance changed cwd");
    }
    this.#acceptedCwd = cwd;
  }

  #acceptControl(record: JournalRecord): void {
    if (record.kind === "session.idle") this.#callbacks.idle(record);
  }

  async #acceptWire(event: WireEvent): Promise<void> {
    if (event.runId !== wireRunId(`bridge:${this.#executionId}`)) {
      throw new Error("Bridge Wire event belongs to a different ACP session");
    }
    if (event.sequence !== this.#cursor) {
      throw new Error("Bridge Wire replay is not contiguous");
    }
    this.#cursor += 1;
    await this.#callbacks.wire(event);
    if (event.essential === true && event.kind === "run.terminated") {
      this.#terminal = true;
      this.#callbacks.terminal(event);
    }
  }

  #settleReady(): void {
    if (this.#readySettled) return;
    this.#readySettled = true;
    this.#readyState.resolve();
  }

  #fail(error: unknown): void {
    if (this.#failed || this.#detached) return;
    this.#failed = true;
    const failure = error instanceof Error ? error : new Error(String(error));
    if (!this.#readySettled) {
      this.#readySettled = true;
      this.#readyState.reject(failure);
    }
    this.#callbacks.failure(failure);
    this.#socket.destroy();
  }
}

export interface BridgeAcpAgentOptions {
  socketPath?: string;
  connectBridge?: () => Promise<Socket>;
  controlTimeoutMs?: number;
}

export class BridgeAcpAgent {
  #sessions = new Map<string, ManagedSession>();
  #connectBridge: () => Promise<Socket>;
  #controlTimeoutMs: number;
  #initialized = false;
  #disposed = false;

  constructor(options: BridgeAcpAgentOptions = {}) {
    const socketPath = options.socketPath ?? bridgeSocketPath();
    this.#connectBridge = options.connectBridge
      ?? (async () => (await verifiedSocket(socketPath, stderrBridgeOutput)).socket);
    this.#controlTimeoutMs = options.controlTimeoutMs ?? DEFAULT_CONTROL_TIMEOUT_MS;
  }

  initialize(_params: acp.InitializeRequest): acp.InitializeResponse {
    this.#assertAvailable();
    this.#initialized = true;
    return {
      protocolVersion: acp.PROTOCOL_VERSION,
      agentInfo: { name: "north", title: "North Bridge", version: "0.1.0" },
      agentCapabilities: {
        loadSession: true,
        promptCapabilities: { image: false, audio: false, embeddedContext: false },
        sessionCapabilities: { close: {} },
      },
    };
  }

  newSession(
    params: acp.NewSessionRequest,
    client: acp.AgentContext,
  ): acp.NewSessionResponse {
    this.#assertInitialized();
    this.#validateSessionScope(params);
    const id = randomUUID();
    this.#sessions.set(id, {
      id,
      cwd: params.cwd,
      client,
      launched: false,
      terminal: false,
      closing: false,
      closed: false,
    });
    return { sessionId: id };
  }

  async loadSession(
    params: acp.LoadSessionRequest,
    client: acp.AgentContext,
  ): Promise<acp.LoadSessionResponse> {
    this.#assertInitialized();
    this.#validateSessionScope(params);
    let sessionId: string;
    try { sessionId = parseBridgeLaunchExecutionId(params.sessionId); }
    catch {
      throw acp.RequestError.invalidParams(
        undefined,
        "North ACP session ID must be a UUIDv4",
      );
    }
    if (sessionId !== params.sessionId) {
      throw acp.RequestError.invalidParams(
        undefined,
        "North ACP session ID must be a canonical lowercase UUIDv4",
      );
    }
    if (this.#sessions.has(sessionId)) {
      throw new acp.RequestError(
        -32000,
        `ACP session ${sessionId} is already bound to this client`,
      );
    }
    const session: ManagedSession = {
      id: sessionId,
      cwd: params.cwd,
      client,
      launched: true,
      terminal: false,
      closing: false,
      closed: false,
    };
    this.#sessions.set(session.id, session);
    try {
      await this.#openFeed(session, { op: "attach", executionId: session.id, cursor: 0 });
      return {};
    } catch (error) {
      session.feed?.destroy();
      this.#sessions.delete(session.id);
      if (error instanceof Error
        && error.message === "ACP cwd does not match the Bridge session cwd") {
        throw acp.RequestError.invalidParams(undefined, error.message);
      }
      throw error;
    }
  }

  async prompt(params: acp.PromptRequest): Promise<acp.PromptResponse> {
    this.#assertInitialized();
    const session = this.#session(params.sessionId);
    if (session.active) {
      if (!session.active.cleanup) {
        throw new acp.RequestError(
          -32000,
          `ACP session ${session.id} already has an active prompt`,
        );
      }
      await session.active.cleanup;
    }
    this.#assertPromptable(session);
    const prompt = this.#promptText(params.prompt);
    const turn: ActivePrompt = {
      response: deferred<acp.PromptResponse>(),
      ready: deferred<void>(),
      ended: deferred<void>(),
      responseSettled: false,
      readySettled: false,
      endedSettled: false,
      cancelRequested: false,
    };
    session.active = turn;
    void this.#startTurn(session, turn, prompt).catch((error) => {
      this.#failTurn(session, turn, error);
    });
    return turn.response.promise;
  }

  async cancel(params: acp.CancelNotification): Promise<void> {
    this.#assertInitialized();
    const session = this.#sessions.get(params.sessionId);
    const turn = session?.active;
    if (!session || !turn || session.closed) return;
    if (!turn.cancelRequested) {
      turn.cancelRequested = true;
    }
    turn.cleanup ??= this.#cancelTurn(session, turn);
    await turn.cleanup;
  }

  async closeSession(params: acp.CloseSessionRequest): Promise<acp.CloseSessionResponse> {
    this.#assertInitialized();
    const session = this.#sessions.get(params.sessionId);
    if (session) await this.#closeManagedSession(session);
    return {};
  }

  dispose(): void {
    if (this.#disposed) return;
    this.#disposed = true;
    for (const session of this.#sessions.values()) {
      session.closed = true;
      session.feed?.destroy();
      const turn = session.active;
      if (turn) {
        this.#settleReady(turn);
        this.#resolveTurn(turn, { stopReason: "cancelled" });
        this.#settleEnded(turn);
      }
    }
    this.#sessions.clear();
  }

  async #startTurn(session: ManagedSession, turn: ActivePrompt, prompt: string): Promise<void> {
    if (!session.launched) {
      await this.#openFeed(session, {
        op: "launch",
        executionId: session.id,
        prompt,
        cwd: session.cwd,
        role: "implementer",
      });
      session.launched = true;
    } else {
      await this.#control({ op: "submitInput", executionId: session.id, input: prompt });
    }
    this.#settleReady(turn);
  }

  async #openFeed(
    session: ManagedSession,
    request: Extract<BridgeRequest, { op: "launch" | "attach" }>,
  ): Promise<void> {
    const socket = await this.#connectBridge();
    if (session.closed || session.closing || this.#disposed) {
      socket.destroy();
      throw new Error(`ACP session ${session.id} closed before Bridge attachment`);
    }
    const feed = new BridgeFeed(socket, session.id, session.cwd, request, {
      launched: () => { session.launched = true; },
      wire: async (event) => {
        for (const update of projectWireEvent(event)) {
          await session.client.notify(acp.methods.client.session.update, {
            sessionId: session.id,
            update,
          });
        }
      },
      idle: (record) => this.#idle(session, record),
      terminal: (event) => this.#terminal(session, event),
      failure: (error) => this.#feedFailure(session, error),
    });
    session.feed = feed;
    await feed.ready;
  }

  #idle(session: ManagedSession, record: JournalRecord): void {
    const turn = session.active;
    if (!turn) return;
    const interrupted = record.data.disposition === "interrupted";
    this.#resolveTurn(turn, {
      stopReason: turn.cancelRequested || interrupted ? "cancelled" : "end_turn",
    });
    this.#settleEnded(turn);
    if (session.active === turn) session.active = undefined;
  }

  #terminal(session: ManagedSession, event: WireRunTerminatedEvent): void {
    session.terminal = true;
    const turn = session.active;
    if (!turn) return;
    if (turn.cancelRequested || event.lifecycle === "cancelled") {
      this.#resolveTurn(turn, { stopReason: "cancelled" });
    } else {
      this.#rejectTurn(
        turn,
        new Error(`Bridge session ended: ${event.reason.code}`),
      );
    }
    this.#settleEnded(turn);
    if (session.active === turn) session.active = undefined;
  }

  #feedFailure(session: ManagedSession, error: Error): void {
    if (session.closed || session.terminal) return;
    session.unavailable = error;
    const turn = session.active;
    if (!turn) return;
    if (turn.cancelRequested) {
      this.#settleReady(turn);
      return;
    }
    this.#rejectTurn(turn, error);
    this.#settleReady(turn);
    this.#settleEnded(turn);
    if (session.active === turn) session.active = undefined;
  }

  #failTurn(session: ManagedSession, turn: ActivePrompt, error: unknown): void {
    const failure = error instanceof Error ? error : new Error(String(error));
    this.#settleReady(turn);
    if (turn.cancelRequested) {
      session.unavailable = failure;
      return;
    }
    this.#rejectTurn(turn, failure);
    this.#settleEnded(turn);
    if (session.active === turn) session.active = undefined;
  }

  async #cancelTurn(session: ManagedSession, turn: ActivePrompt): Promise<void> {
    try {
      await this.#bounded(turn.ready.promise, this.#controlTimeoutMs, "Bridge turn admission");
      if (session.active !== turn || session.terminal || session.closed) return;
      await this.#control({ op: "interruptTurn", executionId: session.id });
      await this.#bounded(turn.ended.promise, this.#controlTimeoutMs, "Bridge turn cancellation");
    } catch {
      if (!session.closed) {
        try { await this.#closeManagedSession(session); }
        catch { /* cancellation remains bounded after best-effort Bridge cleanup */ }
      }
    }
  }

  #closeManagedSession(session: ManagedSession): Promise<void> {
    if (session.closePromise) return session.closePromise;
    session.closing = true;
    const turn = session.active;
    if (turn) {
      turn.cancelRequested = true;
    }
    session.closePromise = (async () => {
      try {
        if ((session.launched || session.feed !== undefined) && !session.terminal) {
          await this.#control({ op: "terminateSession", executionId: session.id });
        }
      } finally {
        session.closed = true;
        session.feed?.destroy();
        if (turn) {
          this.#resolveTurn(turn, { stopReason: "cancelled" });
          this.#settleEnded(turn);
          if (session.active === turn) session.active = undefined;
        }
        this.#sessions.delete(session.id);
      }
    })();
    return session.closePromise;
  }

  async #control(request: Exclude<BridgeRequest, { op: "launch" | "attach" | "retire" }>): Promise<void> {
    const socket = await this.#bounded(
      this.#connectBridge(),
      this.#controlTimeoutMs,
      `Bridge ${request.op} connection`,
    );
    const result = deferred<void>();
    let settled = false;
    let buffer = "";
    const finish = (error?: Error) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      socket.destroy();
      if (error) result.reject(error);
      else result.resolve();
    };
    const timer = setTimeout(
      () => finish(new Error(`Bridge ${request.op} timed out`)),
      this.#controlTimeoutMs,
    );
    timer.unref();
    socket.setEncoding("utf8");
    socket.on("data", (chunk: string) => {
      buffer += chunk;
      if (Buffer.byteLength(buffer, "utf8") > MAX_BRIDGE_LINE_BYTES) {
        finish(new Error("Bridge control response exceeds the ACP adapter bound"));
        return;
      }
      while (true) {
        const newline = buffer.indexOf("\n");
        if (newline < 0) return;
        const line = buffer.slice(0, newline);
        buffer = buffer.slice(newline + 1);
        if (!line) continue;
        let message: BridgeServerMessage;
        try { message = JSON.parse(line) as BridgeServerMessage; }
        catch (error) {
          finish(new Error("Bridge returned malformed control JSON", { cause: error }));
          return;
        }
        if (message.type === "error") {
          finish(new Error(message.message));
          return;
        }
        if (message.type === "controlled") {
          if (message.executionId !== request.executionId || message.control !== request.op) {
            finish(new Error("Bridge acknowledged a different ACP control"));
          } else {
            finish();
          }
          return;
        }
      }
    });
    socket.once("error", (error) => finish(error));
    socket.once("close", () => {
      if (!settled) finish(new Error(`Bridge ${request.op} closed without acknowledgement`));
    });
    socket.write(`${JSON.stringify(request)}\n`);
    return result.promise;
  }

  async #bounded<T>(promise: Promise<T>, timeoutMs: number, label: string): Promise<T> {
    const timeout = deferred<never>();
    const timer = setTimeout(
      () => timeout.reject(new Error(`${label} timed out`)),
      timeoutMs,
    );
    timer.unref();
    try { return await Promise.race([promise, timeout.promise]); }
    finally { clearTimeout(timer); }
  }

  #validateSessionScope(
    params: Pick<acp.NewSessionRequest, "cwd" | "mcpServers" | "additionalDirectories">,
  ): void {
    if (!path.isAbsolute(params.cwd)) {
      throw acp.RequestError.invalidParams(undefined, `ACP cwd must be absolute: ${params.cwd}`);
    }
    if (params.mcpServers.length > 0) {
      throw acp.RequestError.invalidParams(
        undefined,
        "North ACP does not accept client-supplied MCP servers",
      );
    }
    if ((params.additionalDirectories?.length ?? 0) > 0) {
      throw acp.RequestError.invalidParams(
        undefined,
        "North ACP does not accept additional workspace directories",
      );
    }
  }

  #promptText(blocks: acp.PromptRequest["prompt"]): string {
    const parts: string[] = [];
    for (const block of blocks) {
      if (block.type === "text") parts.push(block.text);
      else if (block.type === "resource_link") {
        parts.push(`${block.title ?? block.name} (${block.uri})`);
      } else {
        throw acp.RequestError.invalidParams(
          undefined,
          `North ACP does not support ${block.type} prompt content`,
        );
      }
    }
    const prompt = parts.join("\n\n").trim();
    if (!prompt) {
      throw acp.RequestError.invalidParams(
        undefined,
        "North ACP prompt must contain text or a resource link",
      );
    }
    return prompt;
  }

  #session(id: string): ManagedSession {
    const session = this.#sessions.get(id);
    if (!session || session.closed) throw acp.RequestError.resourceNotFound(id);
    return session;
  }

  #assertPromptable(session: ManagedSession): void {
    if (session.closing || session.closed) {
      throw new acp.RequestError(-32000, `ACP session ${session.id} is closed`);
    }
    if (session.terminal) {
      throw new acp.RequestError(-32000, `ACP session ${session.id} is terminal`);
    }
    if (session.unavailable) {
      throw new acp.RequestError(
        -32000,
        `ACP session ${session.id} Bridge feed failed: ${session.unavailable.message}`,
      );
    }
  }

  #assertInitialized(): void {
    this.#assertAvailable();
    if (!this.#initialized) throw new Error("ACP connection must be initialized first");
  }

  #assertAvailable(): void {
    if (this.#disposed) throw new Error("ACP connection is closed");
  }

  #resolveTurn(turn: ActivePrompt, response: acp.PromptResponse): void {
    if (turn.responseSettled) return;
    turn.responseSettled = true;
    turn.response.resolve(response);
  }

  #rejectTurn(turn: ActivePrompt, error: unknown): void {
    if (turn.responseSettled) return;
    turn.responseSettled = true;
    turn.response.reject(error);
  }

  #settleReady(turn: ActivePrompt): void {
    if (turn.readySettled) return;
    turn.readySettled = true;
    turn.ready.resolve();
  }

  #settleEnded(turn: ActivePrompt): void {
    if (turn.endedSettled) return;
    turn.endedSettled = true;
    turn.ended.resolve();
  }
}

export interface BridgeAcpApplication {
  app: acp.AgentApp;
  agent: BridgeAcpAgent;
}

export function createBridgeAcpApplication(
  options: BridgeAcpAgentOptions = {},
): BridgeAcpApplication {
  const agent = new BridgeAcpAgent(options);
  let connectionClaimed = false;
  const app = acp.agent({ name: "north" })
    .onConnect((connection) => {
      if (connectionClaimed) {
        connection.close(new Error("A North ACP app serves exactly one client connection"));
        return;
      }
      connectionClaimed = true;
      void connection.closed.then(
        () => agent.dispose(),
        () => agent.dispose(),
      );
    })
    .onRequest(acp.methods.agent.initialize, ({ params }) => agent.initialize(params))
    .onRequest(acp.methods.agent.session.new, ({ params, client }) =>
      agent.newSession(params, client))
    .onRequest(acp.methods.agent.session.load, ({ params, client }) =>
      agent.loadSession(params, client))
    .onRequest(acp.methods.agent.session.close, ({ params }) => agent.closeSession(params))
    .onRequest(acp.methods.agent.session.prompt, ({ params }) => agent.prompt(params))
    .onNotification(acp.methods.agent.session.cancel, ({ params }) => agent.cancel(params));
  return { app, agent };
}
