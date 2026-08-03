import { connect } from "node:net";
import { resolve } from "node:path";
import { bridgeSocketPath } from "./protocol";

const northBin = resolve(import.meta.dir, "../../..", "bin", "north");

/** @typedef {import("./journal").JournalRecord} JournalRecord */
/** @typedef {import("./journal").TornTail} TornTail */
/** @typedef {import("./protocol").BridgeRequest} BridgeRequest */

/**
 * @typedef {{
 *   launched?: (executionId: string) => void,
 *   controlled?: (executionId: string, control: string, delivery: string) => void,
 *   event?: (record: JournalRecord) => void,
 *   barrier?: (executionId: string, cursor: number, tornTail?: TornTail) => void,
 *   error?: (error: Error) => void,
 *   close?: () => void,
 * }} BridgeCallbacks
 */

/**
 * @typedef {{ close: () => void }} BridgeStream
 */

function asError(value) {
  return value instanceof Error ? value : new Error(String(value));
}

function emit(callback, ...args) {
  try { callback?.(...args); }
  catch (error) { queueMicrotask(() => { throw error; }); }
}

/**
 * Connect one callback-oriented client to the existing North bridge socket.
 * @param {BridgeRequest} request
 * @param {BridgeCallbacks} [callbacks]
 * @param {string} [socketPath]
 * @returns {Promise<BridgeStream>}
 */
export function streamBridge(request, callbacks = {}, socketPath = bridgeSocketPath()) {
  return new Promise((resolveStream, reject) => {
    const socket = connect(socketPath);
    let settled = false;
    let buffer = "";
    const failConnect = (error) => {
      if (!settled) { settled = true; reject(asError(error)); }
      else emit(callbacks.error, asError(error));
    };
    socket.setEncoding("utf8");
    socket.once("connect", () => {
      if (settled) return;
      settled = true;
      socket.write(`${JSON.stringify(request)}\n`);
      resolveStream({ close: () => socket.destroy() });
    });
    socket.on("error", failConnect);
    socket.on("close", () => emit(callbacks.close));
    socket.on("data", (chunk) => {
      buffer += chunk;
      while (true) {
        const newline = buffer.indexOf("\n");
        if (newline < 0) return;
        const line = buffer.slice(0, newline);
        buffer = buffer.slice(newline + 1);
        if (!line) continue;
        try {
          const message = JSON.parse(line);
          if (message.type === "launched") emit(callbacks.launched, message.executionId);
          else if (message.type === "controlled")
            emit(callbacks.controlled, message.executionId, message.control, message.delivery);
          else if (message.type === "event") emit(callbacks.event, message.record);
          else if (message.type === "barrier")
            emit(callbacks.barrier, message.executionId, message.cursor, message.tornTail);
          else if (message.type === "error") emit(callbacks.error, new Error(message.message));
          else emit(callbacks.error, new Error("unknown bridge server message"));
        } catch (error) { emit(callbacks.error, asError(error)); }
      }
    });
  });
}

/** @param {string} prompt @param {string} cwd @param {BridgeCallbacks} [callbacks] */
export function launchBridge(prompt, cwd, callbacks) {
  return streamBridge({ op: "launch", prompt, cwd }, callbacks);
}

/** @param {string} executionId @param {number} cursor @param {BridgeCallbacks} [callbacks] */
export function attachBridge(executionId, cursor, callbacks) {
  return streamBridge({ op: "attach", executionId, cursor }, callbacks);
}

/** @param {string} executionId @param {string} input @param {BridgeCallbacks} [callbacks] */
export function steerBridge(executionId, input, callbacks) {
  return streamBridge({ op: "submitInput", executionId, input }, callbacks);
}

/** @param {string} executionId @param {BridgeCallbacks} [callbacks] */
export function interruptBridge(executionId, callbacks) {
  return streamBridge({ op: "interruptTurn", executionId }, callbacks);
}

async function north(args) {
  const child = Bun.spawn({ cmd: [northBin, ...args], stdout: "pipe", stderr: "pipe" });
  const [stdout, stderr, exitCode] = await Promise.all([
    new Response(child.stdout).text(), new Response(child.stderr).text(), child.exited,
  ]);
  if (exitCode !== 0)
    throw new Error(`north ${args[0]} failed (${exitCode}): ${stderr.trim() || stdout.trim()}`);
  return stdout;
}

async function northJson(args) {
  const output = await north(args);
  try { return JSON.parse(output); }
  catch (error) { throw new Error(`north ${args.join(" ")} returned invalid JSON`, { cause: error }); }
}

function boardIds(board) {
  if (!Array.isArray(board)) return [];
  return board.flatMap((row) => typeof row?.id === "string" ? [row.id.replace(/^@/, "")] : []);
}

/**
 * @typedef {{ agents: unknown, board: unknown, facts: unknown, selectedFacts?: unknown }} NorthSnapshot
 */

/**
 * Read the current North machine view. The callback keeps rendering code out of
 * this transport module while all command argv remains explicit.
 * @param {{ snapshot?: (snapshot: NorthSnapshot) => void, error?: (error: Error) => void }} [callbacks]
 * @param {string} [selectedThreadId]
 * @returns {Promise<NorthSnapshot>}
 */
export async function snapshotNorth(callbacks = {}, selectedThreadId) {
  try {
    const [agents, board] = await Promise.all([
      northJson(["agents", "--json"]), northJson(["json", "board", "--all"]),
    ]);
    const ids = boardIds(board);
    const facts = ids.length ? await northJson(["json", "show-many", ids.join(",")]) : {};
    const selectedFacts = selectedThreadId
      ? await northJson(["json", "show", selectedThreadId.replace(/^@/, "")]) : undefined;
    const snapshot = selectedFacts === undefined ? { agents, board, facts } : { agents, board, facts, selectedFacts };
    emit(callbacks.snapshot, snapshot);
    return snapshot;
  } catch (error) {
    const failure = asError(error);
    emit(callbacks.error, failure);
    throw failure;
  }
}

/** @param {string} threadId @param {string} input */
export function steerNorth(threadId, input) {
  return north(["steer", threadId.replace(/^@/, ""), input]);
}

/**
 * Reassignment intentionally exposes the retract-then-tell gap: North has no
 * single atomic driver handoff command.
 * @param {string} threadId
 * @param {string} priorDriver Explicit current driver required for safe retract.
 * @param {string} nextDriver
 * @param {{ warning?: (message: string) => void }} [callbacks]
 */
export async function reassignNorthDriver(threadId, priorDriver, nextDriver, callbacks = {}) {
  if (!priorDriver) throw new Error("driver reassignment requires an explicit prior driver");
  if (!nextDriver) throw new Error("driver reassignment requires a new driver");
  const subject = threadId.replace(/^@/, "");
  emit(callbacks.warning, "Driver reassignment is not atomic: the prior driver is retracted before the new driver is told.");
  await north(["retract", subject, "driver", priorDriver]);
  await north(["tell", subject, "driver", nextDriver]);
}
