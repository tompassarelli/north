import { afterEach, beforeEach, expect, test } from "bun:test";
import type { Options } from "@anthropic-ai/claude-agent-sdk";
import { spawn as spawnChild } from "node:child_process";
import {
  chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readlinkSync, readdirSync,
  realpathSync, rmSync, symlinkSync, writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createServer } from "node:net";
import type { AddressInfo } from "node:net";
import {
  codexHarnessArguments, internalOpenAIProviderWithManagedHooksProbeForTest,
  openaiProvider as rawOpenAIProvider,
} from "../src/providers/openai";
import {
  MANAGED_CODEX_DISABLED_FEATURES, MANAGED_CODEX_ENABLED_FEATURES,
  ManagedCodexAppServerRun, ManagedCodexHarvestError,
  type ManagedCodexAppServerOptions, type ManagedCodexNextInput, type ManagedCodexResult,
} from "../src/providers/codex-app-server";
import { ProviderRetrySafeError } from "../src/providers";
import { anthropicProvider } from "../src/providers/anthropic";
import { routedQueryWithRegistry } from "../src/providers/internal-router";
import { harnessOptions } from "../src/harness";
import { applyOrchestrationStaffing } from "../src/orchestration-staffing";
import { markCoordinationOptional, markExecutionAdmission } from "../src/execution-admission";
import { selectProviderFromAvailability } from "../src/provider-routing";
import { providerEnvironmentForTarget } from "../src/accounts";
import {
  modelAdmissionReceipt,
  writeProviderModelObservation,
} from "../src/provider-model-observation-store";
import { normalizeCodexSupportedModels } from "../src/providers/codex-models";
import { scrubAmbientGraphEnv } from "./support/managed-env";
import { gatedTest } from "./support/capabilities";
import { providerSessionKey } from "../src/providers/provider-join";
import { causeChain } from "../src/death";
import { kw } from "../src/coord-wire";
import {
  decodeFrame, encodeResponseFrame, rpcRecord, RPC_V2_HEADER_BYTES,
} from "../src/store-rpc-codec";
import {
  WireEventWriter,
  wireEventId,
  wireRunId,
  type WireEvent,
  type WireQueryInput,
} from "../src/wire";
import type { AgentProvider, RoutingTarget } from "../src/providers/types";

let wireQuerySequence = 0;

function testWireContext() {
  const sequence = wireQuerySequence++;
  const writer = new WireEventWriter({
    runId: wireRunId(`run:openai-provider-test:${sequence}`),
    eventId: (index) => wireEventId(`event:openai-provider-test:${sequence}:${index}`),
  });
  writer.append({ kind: "run.started", lifecycle: "running", owner: "test" });
  return {
    writer,
    route: {
      model: { provider: "openai" as const, capabilityClass: "unknown" as const },
      effort: "medium" as const,
      attempt: 1,
    },
  };
}

interface LegacyOpenAIQueryArguments {
  prompt: WireQueryInput;
  options: Options;
  target?: RoutingTarget;
}

function testProvider(provider: AgentProvider) {
  return {
    ...provider,
    query(args: LegacyOpenAIQueryArguments) {
      return provider.query({
        input: args.prompt,
        options: args.options,
        ...(args.target === undefined ? {} : { target: args.target }),
        context: testWireContext(),
      });
    },
  };
}

const openaiProvider = testProvider(rawOpenAIProvider);

// The proxy tracks the declared `web` capability, not the sandbox. Spelled out
// rather than imported from managedCodexNetworkArguments: an independent statement
// of the contract, not a restatement of the producer.
function expectedCodexFeatureArgs(web: boolean): string[] {
  return [
    ...MANAGED_CODEX_ENABLED_FEATURES.flatMap((name) => ["--enable", name]),
    ...(web
      ? [
        "-c", "features.network_proxy.enabled=true",
        "-c", 'features.network_proxy.domains={"chromium.googlesource.com"="allow"}',
      ]
      : ["--disable", "network_proxy"]),
    ...MANAGED_CODEX_DISABLED_FEATURES.flatMap((name) => ["--disable", name]),
  ];
}

// When this suite runs inside a managed north lane, the ambient graph identity
// (AGENT_COORDINATOR, BEAGLE_STORE_*, NORTH_AUTHOR/DRIVER/LEAD/…) is on the harness MCP
// env whitelist and leaks into the compiled Codex MCP args, breaking the exact
// hermetic env assertion below. Scrub the inherited pollution around every test;
// each test that needs specific graph state sets it explicitly afterward.
let restoreGraphEnv: (() => void) | undefined;

const savedBin = process.env.NORTH_CODEX_BIN;
const savedHome = process.env.HOME;
const savedAgentLawsPath = process.env.AGENT_LAWS_PATH;
const savedPort = process.env.NORTH_PORT;
const savedLaws = process.env.AGENT_LAWS;
const savedOrchestration = process.env.NORTH_ORCHESTRATION_HOME;
const savedModelObservations = process.env.NORTH_PROVIDER_MODEL_OBSERVATIONS;
const northRoot = realpathSync(join(import.meta.dir, "../.."));
const temporary: string[] = [];
beforeEach(() => {
  restoreGraphEnv = scrubAmbientGraphEnv();
});
const liveProcessPidFiles = new Set<string>();
function leakedPromptDescriptors(): string[] {
  if (process.platform !== "linux") return [];
  return readdirSync("/proc/self/fd").flatMap((name) => {
    try {
      const target = readlinkSync(`/proc/self/fd/${name}`);
      return target.includes("north-codex-prompt-") ? [target] : [];
    } catch { return []; }
  });
}
const codexThreadStarted = JSON.stringify({
  type: "thread.started",
  thread_id: "67e55044-10b1-426f-9247-bb680e5fe0c8",
});
const codexTurnStarted = JSON.stringify({ type: "turn.started" });
const codexTerminal = (usage: Record<string, unknown> = {
  input_tokens: 1,
  cached_input_tokens: 0,
  output_tokens: 1,
  reasoning_output_tokens: 0,
}): string => JSON.stringify({ type: "turn.completed", usage });
const codexSuccess = (
  middle: string[] = [],
  usage?: Record<string, unknown>,
): string[] => [
  codexThreadStarted,
  codexTurnStarted,
  ...middle,
  codexTerminal(usage),
];

const RESPAWN_FAILURE_THREAD = "019f7abc-0000-7000-8000-00000000feed";
const RESPAWN_FAILURE_TURN = "019f7abc-0000-7000-8000-00000000beef";
const RESPAWN_FAILURE_TOOL = "command-private-before-commit";

class RespawnThenPreflightFailureRun extends ManagedCodexAppServerRun {
  readonly #runOptions: ManagedCodexAppServerOptions;

  constructor(options: ManagedCodexAppServerOptions) {
    super(options);
    this.#runOptions = options;
  }

  override async *session(
    _nextInput: ManagedCodexNextInput,
  ): AsyncGenerator<ManagedCodexResult> {
    await this.#runOptions.onEvent?.("turn/started", {
      threadId: RESPAWN_FAILURE_THREAD,
      turn: { id: RESPAWN_FAILURE_TURN, status: "inProgress" },
    });
    await this.#runOptions.onEvent?.("item/started", {
      threadId: RESPAWN_FAILURE_THREAD,
      turnId: RESPAWN_FAILURE_TURN,
      item: { id: RESPAWN_FAILURE_TOOL, type: "commandExecution" },
    });
    await this.#runOptions.onRespawn?.();
    throw new ManagedCodexHarvestError({
      turnIds: [],
      completedTurns: 0,
      text: "",
      mcp: {
        source: "test:replacement-preflight",
        coverage: "unknown",
        tools: [],
        operationReceipts: [],
        operationAggregates: [],
      },
      nativeCommands: {
        source: "test:replacement-preflight",
        coverage: "unknown",
        northBinaryProbe: "not_observed",
        completions: [],
      },
      unsupportedNotifications: {},
      landedWork: true,
      respawnCount: 1,
      respawns: [{
        attempt: 1,
        reason: "CANARY-private-provider-death-reason",
        threadId: RESPAWN_FAILURE_THREAD,
        completedTurns: 0,
      }],
    }, { cause: new Error("replacement managed Codex preflight failed") });
  }
}
afterEach(() => {
  restoreGraphEnv?.();
  restoreGraphEnv = undefined;
  if (savedBin === undefined) delete process.env.NORTH_CODEX_BIN;
  else process.env.NORTH_CODEX_BIN = savedBin;
  if (savedHome === undefined) delete process.env.HOME;
  else process.env.HOME = savedHome;
  if (savedAgentLawsPath === undefined) delete process.env.AGENT_LAWS_PATH;
  else process.env.AGENT_LAWS_PATH = savedAgentLawsPath;
  if (savedPort === undefined) delete process.env.NORTH_PORT;
  else process.env.NORTH_PORT = savedPort;
  if (savedLaws === undefined) delete process.env.AGENT_LAWS;
  else process.env.AGENT_LAWS = savedLaws;
  if (savedOrchestration === undefined) delete process.env.NORTH_ORCHESTRATION_HOME;
  else process.env.NORTH_ORCHESTRATION_HOME = savedOrchestration;
  if (savedModelObservations === undefined) delete process.env.NORTH_PROVIDER_MODEL_OBSERVATIONS;
  else process.env.NORTH_PROVIDER_MODEL_OBSERVATIONS = savedModelObservations;
  for (const path of liveProcessPidFiles) killRecordedProcess(path);
  liveProcessPidFiles.clear();
  for (const path of temporary.splice(0)) rmSync(path, { recursive: true, force: true });
});

async function eventsFromScript(lines: string[]): Promise<WireEvent[]> {
  const directory = mkdtempSync(join(tmpdir(), "north-codex-usage-"));
  temporary.push(directory);
  const command = join(directory, "fake-codex");
  writeFileSync(command, `#!/usr/bin/env bash\n${lines.map((line) => `printf '%s\\n' '${line}'`).join("\n")}\n`);
  chmodSync(command, 0o700);
  process.env.NORTH_CODEX_BIN = command;
  const events: WireEvent[] = [];
  for await (const event of openaiProvider.query({ prompt: "x", options: {} })) {
    events.push(event);
  }
  return events;
}

async function resultFromScript(lines: string[]): Promise<WireEvent | undefined> {
  return (await eventsFromScript(lines)).at(-1);
}

async function resultFromScriptBody(body: string): Promise<WireEvent | undefined> {
  const directory = mkdtempSync(join(tmpdir(), "north-codex-protocol-"));
  temporary.push(directory);
  const executable = join(directory, "fake-codex");
  writeFileSync(executable, `#!/usr/bin/env bash\nset -eu\n${body}\n`);
  chmodSync(executable, 0o700);
  process.env.NORTH_CODEX_BIN = executable;
  const events: WireEvent[] = [];
  for await (const event of openaiProvider.query({
    prompt: "x",
    options: {},
  })) events.push(event);
  return events.at(-1);
}

async function waitForFile(path: string, timeoutMs = 2_000): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (!existsSync(path) && Date.now() < deadline) await Bun.sleep(10);
  if (!existsSync(path)) throw new Error(`timed out waiting for ${path}`);
}

async function expectProcessGone(pid: number, timeoutMs = 2_000): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  let alive = true;
  while (alive && Date.now() < deadline) {
    try {
      process.kill(pid, 0);
      await Bun.sleep(10);
    } catch {
      alive = false;
    }
  }
  expect(alive).toBe(false);
}

function killRecordedProcess(path: string): void {
  if (!existsSync(path)) return;
  const pid = Number(readFileSync(path, "utf8"));
  if (!Number.isSafeInteger(pid) || pid <= 1) return;
  if (process.platform !== "win32") {
    try { process.kill(-pid, "SIGKILL"); } catch { /* already gone or not a leader */ }
  }
  try { process.kill(pid, "SIGKILL"); } catch { /* already gone */ }
}

async function within<T>(promise: Promise<T>, timeoutMs: number, label: string): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([
      promise,
      new Promise<never>((_, reject) => {
        timer = setTimeout(() => reject(new Error(`${label} timed out`)), timeoutMs);
      }),
    ]);
  } finally {
    if (timer) clearTimeout(timer);
  }
}

test("Codex adapter owns the cumulative total and does not double-count subsets", async () => {
  const result = await resultFromScript(codexSuccess([], {
      input_tokens: 100, cached_input_tokens: 60,
      cache_write_input_tokens: 11, output_tokens: 20, reasoning_output_tokens: 7,
  }));
  expect(result?.kind).toBe("model-call.completed");
  if (result?.kind !== "model-call.completed") throw new Error("expected model terminal");
  expect(result.usage).toEqual({
    lifetime: {
      inputTokens: 100, cacheReadTokens: 60, cacheWriteTokens: 11,
      outputTokens: 20, reasoningTokens: 7, modelCalls: 1,
    },
    context: { tokens: 120 },
  });
  expect(result.evidence?.providerJoin).toEqual({
    version: "north-provider-join:v1",
    sessionKey: providerSessionKey("67e55044-10b1-426f-9247-bb680e5fe0c8"),
    turnKeys: [], sessionPersistence: "persisted", coverage: "partial",
  });
	expect(result.evidence?.providerDurationMs).toBeUndefined();
});

test("Codex never fabricates num_turns, and its honest activity signal reflects tool calls (thread 019f9c36)", async () => {
  // A hardcoded num_turns:1 previously made a zero-tool-call run and a
  // multi-tool-call run indistinguishable, which grounded a real false
  // "Codex never runs a tool loop" quarantine decision. The result must
  // never carry num_turns at all, and the replacement quantity must be
  // named distinctly, marked not comparable, and must actually vary with
  // observed tool activity.
  const idle = await resultFromScript(codexSuccess([]));
  if (idle?.kind !== "model-call.completed") throw new Error("expected idle model terminal");
  expect(idle.evidence?.turns).toEqual({
    unit: "provider-turn", count: 1, toolItems: 0, comparable: false,
  });

  const busyEvents = await eventsFromScript(codexSuccess([
    JSON.stringify({
      type: "item.started",
      item: { id: "item_cmd_1", type: "command_execution", status: "in_progress" },
    }),
    JSON.stringify({
      type: "item.completed",
      item: { id: "item_cmd_1", type: "command_execution", status: "completed" },
    }),
    JSON.stringify({
      type: "item.completed",
      item: { id: "item_cmd_2", type: "file_change", status: "failed" },
    }),
    // Reasoning completes on nearly every turn whether or not a tool ran, so
    // it is NOT a work item on either transport (thread 019f9cc2): counting it
    // would make the number unable to answer "did a tool loop happen".
    JSON.stringify({
      type: "item.completed",
      item: { id: "item_reasoning_1", type: "reasoning" },
    }),
  ]));
  const busy = busyEvents.at(-1);
  if (busy?.kind !== "model-call.completed") throw new Error("expected busy model terminal");
  expect(busy.evidence?.turns).toEqual({
    unit: "provider-turn", count: 1, toolItems: 2, comparable: false,
  });
  expect(busyEvents).toContainEqual(expect.objectContaining({
    kind: "tool.terminal", status: "failed", origin: "provider",
    errorCode: "tool_failed",
  }));

  // The regression this guards against: a 0-tool-call run and a 2-tool-call
  // run must never report the same turn count under the name num_turns
  // (there is none), and their honest activity counts must differ.
  if (idle.evidence?.turns?.unit !== "provider-turn"
      || busy.evidence?.turns?.unit !== "provider-turn") {
    throw new Error("expected Codex turn evidence");
  }
  expect(idle.evidence.turns.toolItems).not.toBe(busy.evidence.turns.toolItems);
});

test("Codex requires one complete terminal and never synthesizes exit-zero success", async () => {
  await expect(resultFromScript([
    codexThreadStarted,
    codexTurnStarted,
    codexTerminal({ input_tokens: 0 }),
  ])).rejects.toThrow("openai_provider_execution_failed");
  await expect(resultFromScript([])).rejects.toThrow("openai_provider_execution_failed");
});

test("repeated Codex terminals fail instead of selecting a cumulative snapshot", async () => {
  await expect(resultFromScript([
    ...codexSuccess([], {
      input_tokens: 5, cached_input_tokens: 0, output_tokens: 1, reasoning_output_tokens: 0,
    }),
    codexTerminal({
      input_tokens: 9, cached_input_tokens: 4, output_tokens: 2, reasoning_output_tokens: 1,
    }),
  ])).rejects.toThrow("openai_provider_execution_failed");
});

test("Codex lifecycle events are closed, ordered, and terminal exactly once", async () => {
  const validUsage = {
    input_tokens: 1,
    cached_input_tokens: 0,
    output_tokens: 1,
    reasoning_output_tokens: 0,
  };
  const invalidStreams = [
    ["not json"],
    [codexTurnStarted, codexThreadStarted, codexTerminal(validUsage)],
    [codexThreadStarted, codexTerminal(validUsage)],
    [
      codexThreadStarted,
      codexTurnStarted,
      JSON.stringify({ type: "future.event", payload: true }),
      codexTerminal(validUsage),
    ],
    [
      JSON.stringify({
        type: "thread.started",
        thread_id: "67e55044-10b1-426f-9247-bb680e5fe0c8",
        extra: true,
      }),
      codexTurnStarted,
      codexTerminal(validUsage),
    ],
    [
      '{"type":"thread.started","thread_id":"one","thread_id":"two"}',
      codexTurnStarted,
      codexTerminal(validUsage),
    ],
    [
      codexThreadStarted,
      codexTurnStarted,
      codexTerminal(validUsage),
      JSON.stringify({
        type: "item.completed",
        item: { id: "item_0", type: "agent_message", text: "too late" },
      }),
    ],
    [
      codexThreadStarted,
      codexTurnStarted,
      JSON.stringify({ type: "turn.failed", error: { message: "private failure" } }),
    ],
  ];
  for (const events of invalidStreams) {
    await expect(resultFromScript(events)).rejects.toThrow(
      "openai_provider_execution_failed",
    );
  }
});

test("Codex turn.failed records an unavailable provider terminal without exact-zero usage", async () => {
	const directory = mkdtempSync(join(tmpdir(), "north-codex-failed-usage-"));
	temporary.push(directory);
	const command = join(directory, "fake-codex");
	writeFileSync(command, `#!/usr/bin/env bash
printf '%s\n' '${codexThreadStarted}'
printf '%s\n' '${codexTurnStarted}'
printf '%s\n' '${JSON.stringify({ type: "turn.failed", error: { message: "private failure" } })}'
`);
	chmodSync(command, 0o700);
	process.env.NORTH_CODEX_BIN = command;
	const context = testWireContext();
	const events: WireEvent[] = [];
	let failure: unknown;
	try {
		for await (const event of rawOpenAIProvider.query({
			input: "x",
			options: {},
			context,
		})) events.push(event);
	} catch (error) {
		failure = error;
	}
	expect((failure as Error).message).toBe("openai_provider_execution_failed");
	expect(events.at(-1)).toMatchObject({
		kind: "model-call.completed",
		status: "failed",
		origin: "provider",
		usageCoverage: "unavailable",
	});
	expect(context.writer.snapshot()?.usageCoverage).toEqual({
		providerTerminalCount: 1,
		scope: "wire_run_cumulative",
		totalStatus: "unknown_incomplete_terminal",
	});
});

test("Codex terminal usage preserves every reported cache and reasoning counter", async () => {
  const invalidUsage = [
    { input_tokens: 1, output_tokens: 1 },
    {
      input_tokens: -1,
      cached_input_tokens: 0,
      output_tokens: 1,
      reasoning_output_tokens: 0,
    },
    {
      input_tokens: 1.5,
      cached_input_tokens: 0,
      output_tokens: 1,
      reasoning_output_tokens: 0,
    },
    {
      input_tokens: 1,
      cached_input_tokens: 0,
      cache_write_input_tokens: -1,
      output_tokens: 1,
      reasoning_output_tokens: 0,
    },
    {
      input_tokens: Number.MAX_SAFE_INTEGER + 1,
      cached_input_tokens: 0,
      output_tokens: 1,
      reasoning_output_tokens: 0,
    },
    {
      input_tokens: 1,
      cached_input_tokens: 2,
      output_tokens: 1,
      reasoning_output_tokens: 0,
    },
    {
      input_tokens: 1,
      cached_input_tokens: 0,
      output_tokens: 1,
      reasoning_output_tokens: 2,
    },
    {
      input_tokens: 1,
      cached_input_tokens: 0,
      output_tokens: 1,
      reasoning_output_tokens: 0,
      extra: 1,
    },
  ];
  for (const usage of invalidUsage) {
    await expect(resultFromScript([
      codexThreadStarted,
      codexTurnStarted,
      codexTerminal(usage),
    ])).rejects.toThrow("openai_provider_execution_failed");
  }
});

test("Codex item payloads stay opaque except for identity and final agent text", async () => {
  const events = await eventsFromScript(codexSuccess([
	JSON.stringify({
	  type: "item.started",
	  item: {
	    id: "command_0", type: "command_execution", status: "in_progress",
	    command: "printf CANARY-PRIVATE-COMMAND",
	  },
	}),
	JSON.stringify({
	  type: "item.completed",
	  item: {
	    id: "command_0", type: "command_execution", status: "completed",
	    command: "printf CANARY-PRIVATE-COMMAND",
	  },
	}),
    JSON.stringify({
      type: "item.started",
      item: { id: "item_0", type: "future_provider_item" },
    }),
    JSON.stringify({
      type: "item.updated",
      item: {
        id: "item_0",
        type: "future_provider_item",
        provider_may_evolve: { nested: [true, 1, "bounded"] },
      },
    }),
    JSON.stringify({
      type: "item.completed",
      item: { id: "item_0", type: "future_provider_item" },
    }),
    JSON.stringify({
      type: "item.completed",
      item: {
        id: "item_1",
        type: "agent_message",
        text: "final answer",
        incidental_provider_field: true,
      },
    }),
  ]));
  expect(events).toContainEqual(expect.objectContaining({
    kind: "message.recorded", stage: "delta", content: "final answer",
  }));
	const commandAdmission = events.find((event) =>
	  event.kind === "tool.admitted" && event.name === "command");
	if (commandAdmission?.kind !== "tool.admitted") {
	  throw new Error("missing Codex command admission");
	}
	expect(commandAdmission.argumentDigest).toMatch(/^[a-f0-9]{64}$/);
	expect(JSON.stringify(events)).not.toContain("CANARY-PRIVATE-COMMAND");

  await expect(resultFromScript(codexSuccess([
    JSON.stringify({
      type: "item.completed",
      item: { type: "agent_message", text: "missing identity" },
    }),
  ]))).rejects.toThrow("openai_provider_execution_failed");
  await expect(resultFromScript(codexSuccess([
    JSON.stringify({
      type: "item.completed",
      item: { id: "item_0", type: "agent_message", text: 7 },
    }),
  ]))).rejects.toThrow("openai_provider_execution_failed");
});

test("Codex raw JSONL framing rejects invalid UTF-8, partial frames, and line overflow", async () => {
  await expect(resultFromScriptBody("printf '\\377\\n'"))
    .rejects.toThrow("openai_provider_execution_failed");
  await expect(resultFromScriptBody(
    "printf '%s' '{\"type\":\"thread.started\"'",
  )).rejects.toThrow("openai_provider_execution_failed");
  await expect(resultFromScriptBody(
    "head -c 1048577 /dev/zero | tr '\\000' x; printf '\\n'",
  )).rejects.toThrow("openai_provider_execution_failed");
  await expect(resultFromScriptBody([
    `printf '%s\\n' '${codexThreadStarted}'`,
    `printf '%s\\n' '${codexTurnStarted}'`,
    `printf '%s\\n' '${codexTerminal()}'`,
    "printf '%s' '{\"trailing\":\"partial\"'",
  ].join("\n"))).rejects.toThrow("openai_provider_execution_failed");
});

test("provider stdout cannot spoof the supervisor's out-of-band status channel", async () => {
  await expect(resultFromScriptBody([
    "printf '%s\\n' 'NORTH_CODEX_SUPERVISOR 1 EXIT 0'",
    ...codexSuccess().map((line) => `printf '%s\\n' '${line}'`),
  ].join("\n"))).rejects.toThrow("openai_provider_execution_failed");
});

test("provider stderr is drained privately and cannot spoof supervisor status", async () => {
  const result = await resultFromScriptBody([
    "printf '%s\\n' 'NORTH_CODEX_SUPERVISOR 1 UNAVAILABLE' >&2",
    "printf '%s\\n' 'NORTH_CODEX_SUPERVISOR 1 EXIT 0' >&2",
    ...codexSuccess().map((line) => `printf '%s\\n' '${line}'`),
  ].join("\n"));
  expect(result).toMatchObject({ kind: "model-call.completed", status: "succeeded" });
});

test("an immediate terminal frame is drained before supervisor completion", async () => {
  for (let attempt = 0; attempt < 8; attempt++) {
    const result = await resultFromScript(codexSuccess([
      JSON.stringify({
        type: "item.started",
        item: { id: `item_${attempt}`, type: "agent_message", text: "" },
      }),
      JSON.stringify({
        type: "item.completed",
        item: { id: `item_${attempt}`, type: "agent_message", text: `answer-${attempt}` },
      }),
    ]));
    expect(result).toMatchObject({ kind: "model-call.completed", status: "succeeded" });
  }
});

test("Codex JSONL frame-count and cumulative-byte ceilings are enforced", async () => {
  const item = JSON.stringify({
    type: "item.updated",
    item: { id: "item_0", type: "progress" },
  });
  await expect(resultFromScriptBody([
    `printf '%s\\n' '${codexThreadStarted}'`,
    `printf '%s\\n' '${codexTurnStarted}'`,
    "i=0",
    "while [ \"$i\" -lt 10000 ]; do",
    `  printf '%s\\n' '${item}'`,
    "  i=$((i + 1))",
    "done",
  ].join("\n"))).rejects.toThrow("openai_provider_execution_failed");

  await expect(resultFromScriptBody([
    `printf '%s\\n' '${codexThreadStarted}'`,
    `printf '%s\\n' '${codexTurnStarted}'`,
    "padding=\"$(head -c 2048 /dev/zero | tr '\\000' x)\"",
    "i=0",
    "while [ \"$i\" -lt 9000 ]; do",
    "  printf '{\"type\":\"item.updated\",\"item\":{\"id\":\"item_0\",\"type\":\"progress\",\"payload\":\"%s\"}}\\n' \"$padding\"",
    "  i=$((i + 1))",
    "done",
  ].join("\n"))).rejects.toThrow("openai_provider_execution_failed");
});

test("Codex error events terminate and reap the child before propagating", async () => {
  const directory = mkdtempSync(join(tmpdir(), "north-codex-child-"));
  temporary.push(directory);
  const command = join(directory, "fake-codex");
  const terminated = join(directory, "terminated");
  writeFileSync(command, `#!/usr/bin/env bash
trap 'printf terminated > "${terminated}"; exit 0' TERM
printf '%s\\n' '{"type":"error","message":"CODEX_EVENT_CANARY_DO_NOT_EXPOSE"}'
while true; do :; done
`);
  chmodSync(command, 0o700);
  process.env.NORTH_CODEX_BIN = command;
  const query = openaiProvider.query({ prompt: "x", options: {} as any });
  let caught: unknown;
  try { for await (const _ of query as AsyncIterable<any>) {} }
  catch (error) { caught = error; }
  expect((caught as Error).message).toBe("openai_provider_execution_failed");
  expect((caught as Error).message).not.toContain("CODEX_EVENT_CANARY_DO_NOT_EXPOSE");
  // The classification code stays provider-prose-free; the CAUSE CHAIN is where
  // diagnosability lives (thread 019f9cec). Pin the whole legacy-path chain:
  // stable code -> which stage failed -> the provider's own error payload.
  const chain = causeChain(caught);
  expect(chain).toContain("openai_provider_execution_failed");
  expect(chain).toContain(
    "Codex legacy supervisor execution failed while sending or parsing a provider turn",
  );
  expect(chain).toContain("Codex emitted an unrecoverable error");
  expect(chain).toContain("provider error event: CODEX_EVENT_CANARY_DO_NOT_EXPOSE");
  expect(existsSync(terminated)).toBe(true);
  expect(readFileSync(terminated, "utf8")).toBe("terminated");
  expect(leakedPromptDescriptors()).toEqual([]);
});

test("Codex parent exit reaps a TERM-resistant inherited-pipe descendant", async () => {
  if (process.platform === "win32") return;
  const directory = mkdtempSync(join(tmpdir(), "north-codex-exited-parent-"));
  temporary.push(directory);
  const command = join(directory, "fake-codex");
  const parentPath = join(directory, "parent-pid");
  const descendantPath = join(directory, "descendant-pid");
  liveProcessPidFiles.add(parentPath);
  liveProcessPidFiles.add(descendantPath);
  writeFileSync(command, `#!/usr/bin/env bash
set -eu
printf '%s' "$$" > "${parentPath}"
(
  trap '' TERM
  printf '%s' "$BASHPID" > "${descendantPath}"
  while true; do sleep 10; done
) &
while [ ! -s "${descendantPath}" ]; do :; done
printf '%s\\n' '${codexThreadStarted}'
printf '%s\\n' '${codexTurnStarted}'
exit 0
`);
  chmodSync(command, 0o700);
  process.env.NORTH_CODEX_BIN = command;
  try {
    const startedAt = Date.now();
    const caught = await within((async (): Promise<unknown> => {
      try {
        for await (const _ of openaiProvider.query({
          prompt: "x", options: {} as any,
        }) as AsyncIterable<any>) {}
        return undefined;
      } catch (error) {
        return error;
      }
    })(), 3_000, "exited-parent reaper");
    const elapsed = Date.now() - startedAt;
    const parentPid = Number(readFileSync(parentPath, "utf8"));
    const descendantPid = Number(readFileSync(descendantPath, "utf8"));
    expect((caught as Error).message).toBe("openai_provider_execution_failed");
    expect(elapsed).toBeLessThan(2_500);
    await expectProcessGone(parentPid);
    await expectProcessGone(descendantPid);
  } finally {
    killRecordedProcess(parentPath);
    killRecordedProcess(descendantPath);
    liveProcessPidFiles.delete(parentPath);
    liveProcessPidFiles.delete(descendantPath);
  }
});

test("Codex completion reaps a TERM-resistant descendant with closed pipes", async () => {
  if (process.platform === "win32") return;
  const directory = mkdtempSync(join(tmpdir(), "north-codex-closed-pipe-child-"));
  temporary.push(directory);
  const command = join(directory, "fake-codex");
  const parentPath = join(directory, "parent-pid");
  const descendantPath = join(directory, "descendant-pid");
  liveProcessPidFiles.add(parentPath);
  liveProcessPidFiles.add(descendantPath);
  writeFileSync(command, `#!/usr/bin/env bash
set -eu
printf '%s' "$$" > "${parentPath}"
(
  exec </dev/null >/dev/null 2>&1
  trap '' TERM
  printf '%s' "$BASHPID" > "${descendantPath}"
  while true; do sleep 10; done
) &
while [ ! -s "${descendantPath}" ]; do :; done
printf '%s\\n' '${codexThreadStarted}'
printf '%s\\n' '${codexTurnStarted}'
printf '%s\\n' '${codexTerminal()}'
exit 0
`);
  chmodSync(command, 0o700);
  process.env.NORTH_CODEX_BIN = command;
  try {
    const startedAt = Date.now();
    const messages = await within((async (): Promise<any[]> => {
      const result: any[] = [];
      for await (const message of openaiProvider.query({
        prompt: "x", options: {} as any,
      }) as AsyncIterable<any>) result.push(message);
      return result;
    })(), 3_000, "closed-pipe descendant reaper");
    const elapsed = Date.now() - startedAt;
    const parentPid = Number(readFileSync(parentPath, "utf8"));
    const descendantPid = Number(readFileSync(descendantPath, "utf8"));
    expect(messages.at(-1)).toMatchObject({ kind: "model-call.completed", status: "succeeded" });
    expect(elapsed).toBeLessThan(2_500);
    await expectProcessGone(parentPid);
    await expectProcessGone(descendantPid);
  } finally {
    killRecordedProcess(parentPath);
    killRecordedProcess(descendantPath);
    liveProcessPidFiles.delete(parentPath);
    liveProcessPidFiles.delete(descendantPath);
  }
});

test("Codex interrupt is bounded and kills a TERM-resistant process group", async () => {
  if (process.platform === "win32") return;
  const directory = mkdtempSync(join(tmpdir(), "north-codex-interrupt-group-"));
  temporary.push(directory);
  const command = join(directory, "fake-codex");
  const parentPath = join(directory, "parent-pid");
  const descendantPath = join(directory, "descendant-pid");
  liveProcessPidFiles.add(parentPath);
  liveProcessPidFiles.add(descendantPath);
  writeFileSync(command, `#!/usr/bin/env bash
set -eu
trap '' TERM
printf '%s' "$$" > "${parentPath}"
(
  trap '' TERM
  printf '%s' "$BASHPID" > "${descendantPath}"
  while true; do sleep 10; done
) &
while [ ! -s "${descendantPath}" ]; do :; done
printf '%s\\n' '${codexThreadStarted}'
printf '%s\\n' '${codexTurnStarted}'
while true; do sleep 10; done
`);
  chmodSync(command, 0o700);
  process.env.NORTH_CODEX_BIN = command;
  const query = openaiProvider.query({ prompt: "x", options: {} as any });
  const running = (async (): Promise<unknown> => {
    try {
      for await (const _ of query as AsyncIterable<any>) {}
      return undefined;
    } catch (error) {
      return error;
    }
  })();
  try {
    await Promise.all([waitForFile(parentPath), waitForFile(descendantPath)]);
    const parentPid = Number(readFileSync(parentPath, "utf8"));
    const descendantPid = Number(readFileSync(descendantPath, "utf8"));
    const startedAt = Date.now();
    await within(query.interrupt(), 2_500, "Codex process-group interrupt");
    const elapsed = Date.now() - startedAt;
    const caught = await within(running, 2_500, "interrupted query settlement");
    expect((caught as Error).message).toBe("openai_provider_execution_failed");
    expect(elapsed).toBeLessThan(2_500);
    await expectProcessGone(parentPid);
    await expectProcessGone(descendantPid);
  } finally {
    killRecordedProcess(parentPath);
    killRecordedProcess(descendantPath);
    liveProcessPidFiles.delete(parentPath);
    liveProcessPidFiles.delete(descendantPath);
  }
});

test("kernel EOF after a SIGKILLed North host reaps the Codex process tree", async () => {
  if (process.platform === "win32") return;
  const directory = mkdtempSync(join(tmpdir(), "north-codex-host-sigkill-"));
  temporary.push(directory);
  const command = join(directory, "fake-codex");
  const hostScript = join(directory, "host.ts");
  const hostPath = join(directory, "host-pid");
  const supervisorPath = join(directory, "supervisor-pid");
  const providerPath = join(directory, "provider-pid");
  const descendantPath = join(directory, "descendant-pid");
  for (const path of [hostPath, supervisorPath, providerPath, descendantPath])
    liveProcessPidFiles.add(path);
  writeFileSync(command, `#!/usr/bin/env bash
set -eu
cat >/dev/null
trap '' TERM
printf '%s' "$$" > "${providerPath}"
(
  exec </dev/null >/dev/null 2>&1
  trap '' TERM
  printf '%s' "$BASHPID" > "${descendantPath}"
  while true; do sleep 10; done
) &
while [ ! -s "${descendantPath}" ]; do :; done
printf '%s\\n' '${codexThreadStarted}'
printf '%s\\n' '${codexTurnStarted}'
while true; do sleep 10; done
`);
  chmodSync(command, 0o700);
  writeFileSync(hostScript, `
import { writeFileSync } from "node:fs";
import { openaiProvider } from ${JSON.stringify(join(northRoot, "sdk/src/providers/openai.ts"))};
import { WireEventWriter, wireEventId, wireRunId } from ${JSON.stringify(join(northRoot, "sdk/src/wire/index.ts"))};
const writer = new WireEventWriter({
  runId: wireRunId("run:host-death-probe"),
  eventId: (sequence) => wireEventId(\`event:host-death-probe:\${sequence}\`),
});
writer.append({ kind: "run.started", lifecycle: "running", owner: "test" });
writeFileSync(${JSON.stringify(hostPath)}, String(process.pid));
for await (const _ of openaiProvider.query({
  input: "host-death-probe",
  options: { env: { ...process.env, NORTH_CODEX_BIN: ${JSON.stringify(command)} } },
  context: {
    writer,
    route: {
      model: { provider: "openai", capabilityClass: "unknown" },
      effort: "medium",
      attempt: 1,
    },
  },
})) {}
`);
  const host = spawnChild(process.execPath, [hostScript], {
    env: { ...process.env, NORTH_CODEX_BIN: command },
    stdio: "ignore",
  });
  try {
    await Promise.all([
      waitForFile(hostPath),
      waitForFile(providerPath),
      waitForFile(descendantPath),
    ]);
    const providerPid = Number(readFileSync(providerPath, "utf8"));
    const stat = readFileSync(`/proc/${providerPid}/stat`, "utf8");
    const fields = stat.slice(stat.lastIndexOf(") ") + 2).trim().split(/\s+/);
    const supervisorPid = Number(fields[1]);
    expect(Number.isSafeInteger(supervisorPid) && supervisorPid > 1).toBe(true);
    writeFileSync(supervisorPath, String(supervisorPid));

    expect(host.kill("SIGKILL")).toBe(true);
    await within(new Promise<void>((resolve) => host.once("close", () => resolve())),
      2_000, "SIGKILLed North host settlement");
    await expectProcessGone(supervisorPid, 4_000);
    await expectProcessGone(providerPid, 4_000);
    await expectProcessGone(Number(readFileSync(descendantPath, "utf8")), 4_000);
  } finally {
    try { host.kill("SIGKILL"); } catch { /* already gone */ }
    for (const path of [hostPath, supervisorPath, providerPath, descendantPath]) {
      killRecordedProcess(path);
      liveProcessPidFiles.delete(path);
    }
  }
});

test("cleanup failure never replaces the real Codex provider error", async () => {
  const directory = mkdtempSync(join(tmpdir(), "north-codex-cleanup-"));
  temporary.push(directory);
  const command = join(directory, "fake-codex");
  writeFileSync(command, `#!/usr/bin/env bash
printf '%s\\n' '{"type":"error","message":"CODEX_CLEANUP_CANARY_DO_NOT_EXPOSE"}'
exit 2
`);
  chmodSync(command, 0o700);
  process.env.NORTH_CODEX_BIN = command;
  const query = openaiProvider.query({ prompt: "x", options: {} as any });
  query.interrupt = async () => { throw new Error("cleanup failed"); };

  await expect(async () => { for await (const _ of query as AsyncIterable<any>) {} })
    .toThrow("openai_provider_execution_failed");
});

test("Codex nonzero exit redacts stderr and is never retry-safe", async () => {
  const directory = mkdtempSync(join(tmpdir(), "north-codex-reject-"));
  temporary.push(directory);
  const command = join(directory, "fake-codex");
  writeFileSync(command, "#!/usr/bin/env bash\nprintf 'CODEX_STDERR_CANARY_DO_NOT_EXPOSE' >&2\nexit 2\n");
  chmodSync(command, 0o700);
  process.env.NORTH_CODEX_BIN = command;
  const query = openaiProvider.query({ prompt: "x", options: {} as any });
  let caught: unknown;
  try { for await (const _ of query as AsyncIterable<any>) {} }
  catch (error) { caught = error; }
  expect(caught).toBeInstanceOf(Error);
  expect(caught).not.toBeInstanceOf(ProviderRetrySafeError);
  expect((caught as Error).message).toBe("openai_provider_execution_failed");
  expect((caught as Error).message).not.toContain("CODEX_STDERR_CANARY_DO_NOT_EXPOSE");
});

test("a genuinely missing Codex executable is handled and retry-safe", async () => {
  process.env.NORTH_CODEX_BIN = join(tmpdir(), `north-no-such-codex-${process.pid}`);
  const query = openaiProvider.query({ prompt: "x", options: {} as any });
  let caught: unknown;
  try { for await (const _ of query as AsyncIterable<any>) {} }
  catch (error) { caught = error; }
  expect(caught).toBeInstanceOf(ProviderRetrySafeError);
  expect((caught as Error).message).toBe("openai_provider_executable_unavailable_before_acceptance");
  expect((caught as Error).message).not.toContain(process.env.NORTH_CODEX_BIN!);
});

test("two same-provider targets execute concurrently in disjoint Codex homes", async () => {
  const home = mkdtempSync(join(tmpdir(), "north-codex-targets-"));
  temporary.push(home);
  process.env.HOME = home;
  mkdirSync(join(home, ".codex"), { recursive: true });
  const command = join(home, "fake-codex");
  writeFileSync(command, `#!/usr/bin/env bash
printf '%s' "$CODEX_HOME" > "$CODEX_HOME/execution-root"
printf '%s\n' "$@" > "$CODEX_HOME/argv"
printf '%s\n' '{"type":"thread.started","thread_id":"67e55044-10b1-426f-9247-bb680e5fe0c8"}'
printf '%s\n' '{"type":"turn.started"}'
printf '{"type":"item.started","item":{"id":"item_0","type":"agent_message","text":""}}\n'
printf '{"type":"item.completed","item":{"id":"item_0","type":"agent_message","text":"%s"}}\n' "$CODEX_HOME"
printf '%s\n' '{"type":"turn.completed","usage":{"input_tokens":1,"cached_input_tokens":0,"output_tokens":1,"reasoning_output_tokens":0}}'
`);
  chmodSync(command, 0o700);
  process.env.NORTH_CODEX_BIN = command;
  const targets = [
    { id: "codex-one", provider: "openai" as const, authMode: "isolated" as const, profile: "one" },
    { id: "codex-two", provider: "openai" as const, authMode: "isolated" as const, profile: "two" },
  ];
  const execute = async (target: typeof targets[number]) => {
    const events: WireEvent[] = [];
    for await (const event of openaiProvider.query({ prompt: target.id, options: {}, target })) {
      events.push(event);
    }
    return events;
  };
  const [first, second] = await Promise.all(targets.map(execute));
  const firstRoot = join(home, ".local/state/north/accounts/openai/one");
  const secondRoot = join(home, ".local/state/north/accounts/openai/two");
  expect(first).toContainEqual(expect.objectContaining({
    kind: "message.recorded", stage: "delta", content: firstRoot,
  }));
  expect(second).toContainEqual(expect.objectContaining({
    kind: "message.recorded", stage: "delta", content: secondRoot,
  }));
  expect(readFileSync(join(firstRoot, "execution-root"), "utf8")).toBe(firstRoot);
  expect(readFileSync(join(secondRoot, "execution-root"), "utf8")).toBe(secondRoot);
  for (const root of [firstRoot, secondRoot]) {
    const argv = readFileSync(join(root, "argv"), "utf8");
    expect(argv).toContain('cli_auth_credentials_store="file"');
    expect(argv).toContain('forced_login_method="chatgpt"');
    expect(argv).toContain('model_provider="openai"');
    expect(argv).toContain(`sqlite_home="${join(root, "sqlite")}"`);
  }
});

test("managed Codex preview is an exact feature manifest and admission fails closed before spawn", async () => {
  const directory = mkdtempSync(join(tmpdir(), "north-codex-capabilities-"));
  temporary.push(directory);
  const command = join(directory, "fake-codex");
  const argvPath = join(directory, "argv");
  const taskPath = join(directory, "task");
  writeFileSync(command, `#!/usr/bin/env bash
printf '%s\n' "$@" > "${argvPath}"
cat > "${taskPath}"
printf '%s\n' '{"type":"thread.started","thread_id":"67e55044-10b1-426f-9247-bb680e5fe0c8"}'
printf '%s\n' '{"type":"turn.started"}'
printf '%s\n' '{"type":"turn.completed","usage":{"input_tokens":1,"cached_input_tokens":0,"output_tokens":1,"reasoning_output_tokens":0}}'
`);
  chmodSync(command, 0o700);
  process.env.NORTH_CODEX_BIN = command;
  const canonical = harnessOptions({
    self: "openai-authority-probe",
    provider: "openai",
    model: "gpt-5.6-terra",
    routingMetadata: applyOrchestrationStaffing({ role: "implementer" }),
    presenceRegistrar: false,
  }) as any;
  // A direct adapter caller cannot clone and weaken a sealed route.
  const weakened = {
    ...canonical,
    disallowedTools: canonical.disallowedTools.filter(
      (toolName: string) => ![
        "Agent", "Task", "Workflow", "mcp__north__spawn", "mcp__north__dispatch",
      ].includes(toolName),
    ),
  };
  expect(() => codexHarnessArguments(weakened))
    .toThrow("openai_harness_authority_seal_missing");
  // This preview is deliberately not an executable argv. The app-server
  // adapter constructs and attests the selected account's complete runtime
  // layer in the same provider process that executes the turn.
  const argv = codexHarnessArguments(canonical);
  const expected = expectedCodexFeatureArgs(false);
  expect(argv).toEqual(expected);
  expect(existsSync(taskPath)).toBe(false);
  expect(argv).not.toContain("exec");
  expect(argv).not.toContain("--sandbox");
  expect(argv).not.toContain("--config");
  expect(argv).not.toContain("--ignore-user-config");
  expect(argv).not.toContain("--ignore-rules");
  expect(argv).not.toContain("--add-dir");
  expect(argv).not.toContain("--dangerously-bypass-approvals-and-sandbox");
  expect(argv).not.toContain("danger-full-access");
  const nestedCwd = join(northRoot, "sdk", "src");
  const nested = harnessOptions({
    self: "openai-nested-authority-probe",
    provider: "openai",
    cwd: nestedCwd,
    model: "gpt-5.6-terra",
    routingMetadata: applyOrchestrationStaffing({ role: "implementer" }),
    presenceRegistrar: false,
  }) as any;
  const nestedArgs = codexHarnessArguments(nested);
  expect(nestedArgs).toEqual(expected);
  await expect(async () => {
    for await (const _ of openaiProvider.query({
      prompt: "must not spawn from a non-root write workspace", options: nested,
    }) as AsyncIterable<any>) {}
  }).toThrow("openai_write_workspace_must_be_project_root");

  rmSync(argvPath, { force: true });
  const web = harnessOptions({
    self: "openai-web-admission-proof",
    provider: "openai",
    model: "gpt-5.6-luna",
    routingMetadata: applyOrchestrationStaffing({ role: "scout" }),
    presenceRegistrar: false,
  }) as any;
  // scout: read-only and web-declaring.
  expect(codexHarnessArguments(web)).toEqual(expectedCodexFeatureArgs(true));
  expect(existsSync(argvPath)).toBe(false);

  const unsupported = openaiProvider.query({
    prompt: "x",
    options: { ...canonical, northCapabilities: ["filesystem.read"] } as any,
  });
  await expect(async () => {
    for await (const _ of unsupported as AsyncIterable<any>) {}
  }).toThrow("openai_harness_authority_seal_missing");
  expect(existsSync(argvPath)).toBe(false);

  const ambientTopology = {
    ...canonical,
    env: { ...canonical.env, AGENT_TOPOLOGY: undefined },
  };
  await expect(async () => {
    for await (const _ of openaiProvider.query({
      prompt: "x", options: ambientTopology,
    }) as AsyncIterable<any>) {}
  }).toThrow("openai_harness_authority_seal_missing");
  expect(existsSync(argvPath)).toBe(false);

  const missingDeveloperInstructions = { ...canonical, systemPrompt: "" };
  await expect(async () => {
    for await (const _ of openaiProvider.query({
      prompt: "x", options: missingDeveloperInstructions,
    }) as AsyncIterable<any>) {}
  }).toThrow("openai_harness_authority_seal_missing");
  expect(existsSync(argvPath)).toBe(false);

  expect(() => { canonical.env.AGENT_TOPOLOGY = undefined; }).toThrow();
});

test("the executable Codex adapter admits exact managed orchestrator authority", () => {
  const options = harnessOptions({
    self: "openai-orchestrator-admission-proof",
    provider: "openai",
    cwd: northRoot,
    routingMetadata: applyOrchestrationStaffing({ role: "director" }),
    presenceRegistrar: false,
  }) as any;
  // director: read-only and web-declaring, like every orchestrator template.
  expect(codexHarnessArguments(options)).toEqual(expectedCodexFeatureArgs(true));
});

test("managed events cross the commit barrier before publication or iterator rejection", async () => {
  const home = mkdtempSync(join(tmpdir(), "north-openai-respawn-preflight-wire-"));
  temporary.push(home);
  process.env.HOME = home;
  process.env.AGENT_LAWS = "on";
  process.env.NORTH_ORCHESTRATION_HOME = realpathSync(
    savedOrchestration ?? join(northRoot, "orchestration"),
  );

  const canonicalAgents = join(home, ".agents", "AGENTS.md");
  mkdirSync(join(home, ".agents"), { recursive: true });
  writeFileSync(canonicalAgents, "RESPAWN_PREFLIGHT_CANONICAL\n");
  process.env.AGENT_LAWS_PATH = canonicalAgents;
  const codexHome = join(home, ".codex");
  mkdirSync(codexHome);
  symlinkSync(canonicalAgents, join(codexHome, "AGENTS.md"));
  const auth = join(codexHome, "auth.json");
  writeFileSync(auth, "{}\n", { mode: 0o600 });
  chmodSync(auth, 0o600);

  const options = harnessOptions({
    self: "openai-respawn-preflight-wire-proof",
    provider: "openai",
    cwd: northRoot,
    model: "gpt-5.6-luna",
    modelAvailability: { exactModelPinned: false, targetId: "openai" },
    routingMetadata: applyOrchestrationStaffing({ role: "scout" }),
    presenceRegistrar: false,
  });
  markCoordinationOptional(options as object);
  const toolCommitReached = Promise.withResolvers<void>();
  const releaseToolCommit = Promise.withResolvers<void>();
  const context = {
    ...testWireContext(),
    eventCommitter: {
      async commitThrough(event: WireEvent): Promise<void> {
        if (event.kind !== "tool.admitted") return;
        toolCommitReached.resolve();
        await releaseToolCommit.promise;
      },
    },
  };
  const provider = internalOpenAIProviderWithManagedHooksProbeForTest(
    () => {},
    {
      resolveManagedCommand: () => "/bin/true",
      createManagedRun: (runOptions) => new RespawnThenPreflightFailureRun(runOptions),
    },
  );
  const query = provider.query({ input: "perform managed work", options, context });
  const observed: WireEvent[] = [];
  const unsubscribe = query.subscribeProviderEvents?.((event) => observed.push(event));
  const yielded: WireEvent[] = [];
  const execution = (async (): Promise<unknown> => {
    try {
      for await (const event of query) yielded.push(event);
    } catch (error) { return error; }
    return undefined;
  })();
  await toolCommitReached.promise;
  expect(observed.some((event) => event.kind === "tool.admitted")).toBe(false);
  expect(yielded).toEqual([]);
  releaseToolCommit.resolve();
  const caught = await execution;
  unsubscribe?.();

  expect(caught).toBeInstanceOf(ManagedCodexHarvestError);
  const yieldedRespawnTerminals = yielded.filter((event) =>
    event.kind === "model-call.completed"
    && event.status === "failed"
    && event.origin === "north"
    && event.errorCode === "provider_session_replaced");
  expect(yieldedRespawnTerminals).toHaveLength(1);
  expect(observed.filter((event) => event.id === yieldedRespawnTerminals[0]!.id))
    .toHaveLength(1);
  expect(context.writer.events().filter((event) => event.id === yieldedRespawnTerminals[0]!.id))
    .toHaveLength(1);
  for (const serialized of [
    JSON.stringify(yielded),
    JSON.stringify(observed),
    JSON.stringify(context.writer.events()),
  ]) {
    expect(serialized).not.toContain("CANARY-private-provider-death-reason");
    expect(serialized).not.toContain(RESPAWN_FAILURE_THREAD);
    expect(serialized).not.toContain(RESPAWN_FAILURE_TURN);
    expect(serialized).not.toContain(RESPAWN_FAILURE_TOOL);
    expect(serialized).not.toContain("gpt-5.6-luna");
  }

  context.writer.append({
    kind: "run.terminated",
    lifecycle: "failed",
    reason: { code: "provider_process_died" },
  });
  const snapshot = context.writer.snapshot()!;
  expect(snapshot.lifecycle).toBe("failed");
  expect(Object.values(snapshot.modelCalls).some((call) => call.status === "running"))
    .toBe(false);
  expect(Object.values(snapshot.toolCalls).some((call) => call.status === "pending"))
    .toBe(false);
  expect(Object.values(snapshot.messages).some((message) => message.stage !== "completed"))
    .toBe(false);
});

test("managed executable resolution fails retry-safe before onRoute or query construction", async () => {
  const home = mkdtempSync(join(tmpdir(), "north-openai-command-preflight-"));
  temporary.push(home);
  process.env.HOME = home;
  process.env.AGENT_LAWS = "on";
  process.env.NORTH_ORCHESTRATION_HOME = realpathSync(savedOrchestration ?? join(northRoot, "orchestration"));
  process.env.NORTH_PORT = "65534";
  const codexHome = join(home, ".codex");
  mkdirSync(codexHome);
  writeFileSync(join(codexHome, "AGENTS.md"), "COMMAND_PREFLIGHT_CANONICAL\n");

  const target = {
    id: "codex-command-preflight",
    provider: "openai" as const,
    authMode: "isolated" as const,
    profile: "command-preflight",
  };
  const options = harnessOptions({
    self: "openai-command-preflight-proof",
    provider: "openai",
    cwd: northRoot,
    routingMetadata: applyOrchestrationStaffing({ role: "executor" }),
    presenceRegistrar: false,
  });
  const decision = selectProviderFromAvailability(
    { provider: "openai", target: target.id },
    [{ targetId: target.id, provider: "openai", available: true, reason: "ready" }],
    {
      mode: "balanced",
      targets: [target],
      targetOrder: [target.id],
      providerOrder: ["openai"],
      pressures: { openai: "normal" },
    },
    "economy",
    "command-preflight-proof",
    "low",
  );
  let resolverCalls = 0;
  let queryConstructed = false;
  let routePublished = false;
  const provider = internalOpenAIProviderWithManagedHooksProbeForTest(
    () => {},
    {
      resolveManagedCommand: () => {
        resolverCalls++;
        throw new Error("trusted resolver unavailable");
      },
      onQueryConstruction: () => { queryConstructed = true; },
    },
  );
  const query = routedQueryWithRegistry(
    decision,
    { input: "must not run", options, writer: testWireContext().writer },
    "economy",
    Object.freeze({ anthropic: anthropicProvider, openai: Object.freeze(provider) }),
    undefined,
    () => { routePublished = true; },
  );

  let caught: unknown;
  try {
    for await (const _ of query as AsyncIterable<any>) {}
  } catch (error) {
    caught = error;
  }
  expect(caught).toBeInstanceOf(ProviderRetrySafeError);
  expect((caught as Error).message)
    .toBe("openai_provider_executable_unavailable_before_acceptance");
  expect(resolverCalls).toBe(1);
  expect(queryConstructed).toBe(false);
  expect(routePublished).toBe(false);
});

test("managed Codex admission revalidates an exact-model receipt after target-scoped revocation", async () => {
  const home = mkdtempSync(join(tmpdir(), "north-openai-model-toctou-"));
  temporary.push(home);
  process.env.HOME = home;
  process.env.AGENT_LAWS = "on";
  process.env.NORTH_ORCHESTRATION_HOME = realpathSync(
    savedOrchestration ?? join(northRoot, "orchestration"),
  );
  process.env.NORTH_PORT = "65534";
  const canonicalAgents = join(home, ".agents", "AGENTS.md");
  mkdirSync(join(home, ".agents"), { recursive: true });
  writeFileSync(canonicalAgents, "CODEX_MODEL_TOCTOU_CANONICAL\n");
  process.env.AGENT_LAWS_PATH = canonicalAgents;
  const codexHome = join(home, ".codex");
  mkdirSync(codexHome);
  symlinkSync(canonicalAgents, join(codexHome, "AGENTS.md"));
  const auth = join(codexHome, "auth.json");
  writeFileSync(auth, "{}\n", { mode: 0o600 });
  chmodSync(auth, 0o600);

  const target: RoutingTarget = {
    id: "codex-personal", provider: "openai", authMode: "ambient",
  };
  const modelStorePath = join(home, "model-observations.json");
  process.env.NORTH_PROVIDER_MODEL_OBSERVATIONS = modelStorePath;
  const observedAt = new Date();
  const positive = normalizeCodexSupportedModels(["gpt-5.6-luna"], target, observedAt);
  await writeProviderModelObservation(positive, modelStorePath, observedAt);
  const receipt = modelAdmissionReceipt(positive, target, "gpt-5.6-luna", observedAt)!;
  const options = harnessOptions({
    self: "openai-model-toctou",
    provider: "openai",
    cwd: northRoot,
    model: "gpt-5.6-luna",
    modelAvailability: {
      exactModelPinned: true,
      targetId: target.id,
      receipt,
    },
    routingMetadata: applyOrchestrationStaffing({ role: "scout" }),
    presenceRegistrar: false,
  });
  markCoordinationOptional(options as object);
  let managedHookChecks = 0;
  let managedRuns = 0;
  const provider = internalOpenAIProviderWithManagedHooksProbeForTest(
    () => { managedHookChecks++; },
    {
      resolveManagedCommand: () => "/bin/true",
      createManagedRun: (runOptions) => {
        managedRuns++;
        return new ManagedCodexAppServerRun(runOptions);
      },
    },
  );
  await provider.admit!({ options, target });
  markExecutionAdmission("openai", options);
  await writeProviderModelObservation(
    normalizeCodexSupportedModels([], target, observedAt), modelStorePath, observedAt,
  );
  const query = provider.query({
    input: "must not launch",
    options,
    target,
    context: testWireContext(),
  });
  await expect(async () => {
    for await (const _event of query) { /* no provider event is admissible */ }
  })
    .toThrow("openai_model_availability_unproven");
  expect(managedHookChecks).toBe(2);
  expect(managedRuns).toBe(0);
});

gatedTest("loopback-bind", "selected Codex account bootstrap fails during admission before onRoute or provider spawn", async () => {
  const server = createServer((socket) => {
    const chunks: Buffer[] = [];
    socket.on("data", (chunk) => {
      chunks.push(chunk);
      const buffer = Buffer.concat(chunks);
      if (buffer.length < RPC_V2_HEADER_BYTES) return;
      const bodyLength = new DataView(
        buffer.buffer, buffer.byteOffset, buffer.length,
      ).getUint32(14, true);
      if (buffer.length < RPC_V2_HEADER_BYTES + bodyLength) return;
      const frame = decodeFrame(Uint8Array.from(buffer));
      socket.end(Buffer.from(encodeResponseFrame(frame.requestId, {
        space: frame.request!.space,
        op: frame.request!.op,
        servedVersion: 23,
        page: null,
        error: null,
        payload: rpcRecord(kw("rpc/status"), [
          kw("serving"), 0, kw("native"),
          rpcRecord(kw("rpc/result-cache"), [0, 0, 0, 0]),
        ]),
      })));
    });
  });
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  const home = mkdtempSync(join(tmpdir(), "north-openai-target-admission-"));
  temporary.push(home);
  process.env.HOME = home;
  process.env.AGENT_LAWS = "on";
  process.env.NORTH_ORCHESTRATION_HOME = realpathSync(savedOrchestration ?? join(northRoot, "orchestration"));
  process.env.NORTH_PORT = String((server.address() as AddressInfo).port);
  process.env.BEAGLE_STORE_SERVER_PORT = process.env.NORTH_PORT;
  process.env.BEAGLE_STORE_SPACE_ID = "north-coordination";
  process.env.NORTH_FRAMRPC_HOST = "127.0.0.1";
  const codexHome = join(home, ".codex");
  mkdirSync(codexHome);
  writeFileSync(join(codexHome, "AGENTS.md"), "TARGET_ADMISSION_CANONICAL\n");

  const target = {
    id: "codex-broken-target",
    provider: "openai" as const,
    authMode: "isolated" as const,
    profile: "broken-target",
  };
  const targetRoot = join(home, ".local/state/north/accounts/openai/broken-target");
  mkdirSync(targetRoot, { recursive: true });
  writeFileSync(join(targetRoot, "AGENTS.md"), "TARGET_REPLACEMENT_MUST_FAIL\n");

  const marker = join(home, "provider-spawned");
  const command = join(home, "fake-codex");
  writeFileSync(command, `#!/usr/bin/env bash\nprintf spawned > "${marker}"\n`);
  chmodSync(command, 0o700);
  process.env.NORTH_CODEX_BIN = command;

  const options = harnessOptions({
    self: "openai-target-admission-proof",
    provider: "openai",
    cwd: northRoot,
    routingMetadata: applyOrchestrationStaffing({ role: "executor" }),
    presenceRegistrar: false,
  });
  await expect(openaiProvider.admit!({
    options: {
      ...options,
      env: { ...options.env, AGENT_LAWS: "off" },
    },
    target: { ...target, authMode: "ambient" },
  })).rejects.toThrow("openai_harness_authority_seal_missing");
  expect(existsSync(marker)).toBe(false);

  const decision = selectProviderFromAvailability(
    { provider: "openai", target: target.id },
    [{ targetId: target.id, provider: "openai", available: true, reason: "ready" }],
    {
      mode: "balanced",
      targets: [target],
      targetOrder: [target.id],
      providerOrder: ["openai"],
      pressures: { openai: "normal" },
    },
    "economy",
    "target-admission-proof",
    "low",
  );
  let routePublished = false;
  const query = routedQueryWithRegistry(
    decision,
    { input: "must not run", options, writer: testWireContext().writer },
    "economy",
    Object.freeze({
      anthropic: anthropicProvider,
      openai: Object.freeze(
        internalOpenAIProviderWithManagedHooksProbeForTest(
          () => {},
          { resolveManagedCommand: () => "/nix/store/codex-test/bin/codex" },
        ),
      ),
    }),
    undefined,
    () => { routePublished = true; },
  );
  try {
    await expect(async () => {
      for await (const _ of query as AsyncIterable<any>) {}
    }).toThrow("openai_target_environment_invalid");
    expect(routePublished).toBe(false);
    expect(existsSync(marker)).toBe(false);
  } finally {
    await new Promise<void>((resolve) => server.close(() => resolve()));
  }
});

test("selected Codex account bootstrap refuses to replace an existing account path", () => {
  const home = mkdtempSync(join(tmpdir(), "north-openai-target-replacement-"));
  temporary.push(home);
  process.env.HOME = home;
  const codexHome = join(home, ".codex");
  mkdirSync(codexHome);
  writeFileSync(join(codexHome, "AGENTS.md"), "TARGET_ADMISSION_CANONICAL\n");

  const target = {
    id: "codex-broken-target",
    provider: "openai" as const,
    authMode: "isolated" as const,
    profile: "broken-target",
  };
  const targetRoot = join(home, ".local/state/north/accounts/openai/broken-target");
  mkdirSync(targetRoot, { recursive: true });
  writeFileSync(join(targetRoot, "AGENTS.md"), "TARGET_REPLACEMENT_MUST_FAIL\n");

  expect(() => providerEnvironmentForTarget("openai", target))
    .toThrow(`refusing to replace existing account path ${join(targetRoot, "AGENTS.md")}`);
  expect(readFileSync(join(targetRoot, "AGENTS.md"), "utf8"))
    .toBe("TARGET_REPLACEMENT_MUST_FAIL\n");
});
