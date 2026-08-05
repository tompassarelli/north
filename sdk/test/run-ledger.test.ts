import { createHash } from "node:crypto";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { expect, test } from "bun:test";
import {
  AGENT_RUN_EVENT_TYPES,
  AGENT_RUN_LEDGER_CONTRACT,
  AGENT_RUN_LEDGER_VERSION,
  AgentRunLedger,
  eventFacts,
  publishRunLifecycleLedger,
  recordRunEvents,
  type AgentRunEvent,
} from "../src/run-ledger";
import { runFacts } from "../src/telemetry";
import { FramRpcClient } from "../src/framrpc-client";
import { FramTriple } from "../src/framrpc-codec";

const digest = (value: string) => createHash("sha256").update(value).digest("hex");
const repo = resolve(import.meta.dir, "../..");
// The fixture server and the bb writer child must share one Fram vocabulary.
const frozenFramHome = process.env.FRAM_TEST_CHECKOUT
  ?? process.env.FRAM_HOME
  ?? "/home/tom/code/fram/wt-core-target-production-5db9b38";
const identity = {
  run: "@run:lane-ledger-001",
  thread: "@019f89ac-a86a-7399-b915-358d44a1be15",
  agent: "lane-ledger",
  parentRun: "@run:parent-001",
  parentThread: "@019f89ac-parent",
  coordinator: "north-root",
};

const examples: Record<string, Record<string, string | number>> = {
  admission_received: { receiptDigest: digest("receipt"), policyVersion: "routing-v2" },
  provider_routed: {
    provider: "openai", account: "codex-1", model: "gpt-5.6-terra", effort: "medium",
    reasonCode: "entitlement-aware",
  },
  prompt_constructed: {
    compositionVersion: "orchestration-v3", compositionDigest: digest("composition"),
    capabilityClass: "authoring", capabilityCount: 4,
    stablePrefixBytes: 1200, uniqueTailBytes: 300, totalBytes: 1500,
    byteMeasurementSource: "node-buffer-byte-length:utf8",
    tokenMeasurementStatus: "unknown",
    tokenMeasurementSource: "authoritative-tokenizer-unavailable",
    contextWindowStatus: "observed", contextWindowSource: "orchestration-provider-catalog",
    providerContextWindowTokens: 400000,
    contextWindowEffectiveFrom: "2026-01-01",
    contextBudgetStatus: "unknown", contextBudgetSource: "north-harness-unconfigured",
    compactionPolicy: "native-auto-compact-enabled",
    compactionPolicyVersion: "north-native-auto-compact:v1",
  },
  tool_activity: { serverName: "north", toolName: "tell", callCount: 2 },
  tool_observed: { toolName: "mcp__north__tell", activity: "completed", successCount: 2, errorCount: 0 },
  usage_observed: { inputTokens: 120, outputTokens: 40, totalTokens: 160, terminalCount: 1 },
  cache_observed: { cacheReadTokens: 80, cacheCreateTokens: 20, cachedInputTokens: 50 },
  compaction_observed: {
    compactionCount: 0, compactionPolicy: "native-auto-compact-enabled",
    compactionPolicyVersion: "north-native-auto-compact:v1",
    compactionEvidence: "sdk-compact-boundary",
  },
  escalation_observed: { fromTier: "standard", toTier: "senior", reasonCode: "scope-overrun" },
  child_settled: {
    childRun: "@run:child-001", childThread: "@019f89ac-child", outcome: "ran",
    settlementDigest: digest("settlement"),
  },
  terminal_cleanup: { outcome: "ran", cleanupStatus: "complete", childCount: 1 },
};

const mcpActivity = {
  source: "anthropic-agent-sdk:assistant-tool-use", coverage: "exact" as const,
  totalCalls: 2, tools: [{ server: "north", tool: "tell", count: 2 }],
};
test("the shared v1 contract covers every required AgentRun forensic observation", () => {
  expect(AGENT_RUN_LEDGER_VERSION).toBe("north-agent-run-ledger:v1");
  expect(new Set(AGENT_RUN_EVENT_TYPES)).toEqual(new Set(Object.keys(examples)));
  expect(new Set(AGENT_RUN_LEDGER_CONTRACT.coverage)).toEqual(
    new Set(["exact", "partial", "unknown"]),
  );
});

test("a finalized ledger binds exact lineage, monotone ordering, coverage, and its run header", () => {
  const ledger = new AgentRunLedger(identity);
  for (const [index, type] of AGENT_RUN_EVENT_TYPES.entries()) {
    const event = ledger.append(
      type, examples[type], index === 3 ? "anthropic-adapter" : "north-harness",
      index === 3 ? "partial" : "exact", `2026-07-22T00:00:${String(index).padStart(2, "0")}.000Z`,
    );
    expect(event.sequence).toBe(index);
    expect(event.run).toBe(identity.run);
    expect(event.thread).toBe(identity.thread);
    expect(event.parentRun).toBe(identity.parentRun);
    expect(event.subject).toMatch(/^@run:lane-ledger-001:event:\d{8}$/);
    expect(Object.isFrozen(event)).toBe(true);
    expect(Object.isFrozen(event.payload)).toBe(true);
  }
  const summary = ledger.finalize();
  expect(summary.eventCount).toBe(AGENT_RUN_EVENT_TYPES.length);
  expect(summary.firstSequence).toBe(0);
  expect(summary.lastSequence).toBe(AGENT_RUN_EVENT_TYPES.length - 1);
  expect(summary.terminalSequence).toBe(summary.lastSequence);
  expect(summary.coverage).toEqual([
    { source: "anthropic-adapter", coverage: "partial" },
    { source: "north-harness", coverage: "exact" },
  ]);

  const header = runFacts({
    thread: identity.thread, agent: identity.agent, durationMs: 10,
    posture: "spawn", outcome: "ran", parentRun: identity.parentRun,
    parentThread: identity.parentThread, coordinator: identity.coordinator,
    promptCompositionVersion: "orchestration-v3",
    promptCompositionDigest: digest("composition"), capabilityClass: "authoring",
    runLedger: summary,
  });
  for (const expected of [
    ["agent_run_ledger_version", AGENT_RUN_LEDGER_VERSION],
    ["run_event_status", "complete"],
    ["parent_run", identity.parentRun],
    ["parent_thread", identity.parentThread],
    ["run_coordinator", identity.coordinator],
    ["run_event_count", String(summary.eventCount)],
    ["run_event_terminal_sequence", String(summary.terminalSequence)],
    ["run_event_ledger_sha256", summary.digest],
  ]) expect(header).toContainEqual(expected);
  expect(() => ledger.append("tool_activity", examples.tool_activity, "north-harness", "exact"))
    .toThrow("run ledger is finalized");
});

test("events serialize only fixed content-free predicates and payload keys", () => {
  const ledger = new AgentRunLedger(identity);
  const event = ledger.append(
    "tool_activity", examples.tool_activity, "codex-app-server", "exact",
    "2026-07-22T00:00:00.000Z",
  );
  const facts = eventFacts(event);
  expect(new Set(facts.map(([predicate]) => predicate))).toEqual(new Set([
    "kind", "agent_run_ledger_version", "run", "thread", "agent", "parent_run",
    "parent_thread", "run_coordinator", "run_event_sequence", "run_event_type",
    "run_event_observed_at", "run_event_source", "run_event_coverage",
    "run_event_data", "run_event_sha256",
  ]));
  const encoded = JSON.stringify(facts);
  expect(encoded).not.toContain("tool arguments are private");
  expect(JSON.parse(facts.find(([p]) => p === "run_event_data")![1])).toEqual(examples.tool_activity);
});

test("privacy validation rejects prompt, argument, raw, credential, and free-text payloads", () => {
  for (const forbidden of [
    "prompt", "promptText", "arguments", "rawTranscript", "messageContent",
    "apiKey", "authorization", "credential", "secretValue",
  ]) {
    const ledger = new AgentRunLedger(identity);
    expect(() => ledger.append(
      "tool_activity",
      { ...examples.tool_activity, [forbidden]: "CANARY-private-prompt-and-tool-arguments" },
      "north-harness", "exact",
    )).toThrow();
  }
  const ledger = new AgentRunLedger(identity);
  expect(() => ledger.append(
    "tool_activity", { serverName: "north", toolName: "arbitrary user text with spaces", callCount: 1 },
    "north-harness", "exact",
  )).toThrow("invalid run ledger identifier");
  expect(() => ledger.append(
    "usage_observed", { terminalCount: -1 }, "north-harness", "exact",
  )).toThrow("invalid run ledger count");
  expect(() => ledger.append(
    "admission_received", { receiptDigest: "not-a-digest" }, "north-harness", "exact",
  )).toThrow("invalid run ledger digest");
});

test("a non-terminal or misordered ledger cannot produce a finalized header", () => {
  const noTerminal = new AgentRunLedger(identity);
  noTerminal.append("admission_received", examples.admission_received, "north-harness", "exact");
  expect(() => noTerminal.finalize()).toThrow("requires terminal_cleanup as its final event");

  const terminalFirst = new AgentRunLedger(identity);
  terminalFirst.append("terminal_cleanup", examples.terminal_cleanup, "north-harness", "exact");
  expect(() => terminalFirst.append(
    "usage_observed", examples.usage_observed, "north-harness", "exact",
  )).toThrow("run ledger is finalized");
});

test("new run headers explicitly distinguish unavailable event evidence from zero events", () => {
  const header = runFacts({
    thread: "(ad-hoc)", agent: "legacy-shape", durationMs: 1,
    posture: "spawn", outcome: "ran",
  });
  expect(header).toContainEqual(["agent_run_ledger_version", AGENT_RUN_LEDGER_VERSION]);
  expect(header).toContainEqual(["run_event_status", "unavailable"]);
  expect(header.some(([predicate]) => predicate === "run_event_count")).toBe(false);
});

const promptEconomics = {
  compositionVersion: "north-harness-prompt:v1",
  compositionDigest: digest("terminal-composition"),
  capabilityClass: "authoring",
  capabilityCount: 4,
  stablePrefixBytes: 1200,
  uniqueTailBytes: 300,
  totalBytes: 1500,
  byteMeasurementSource: "node-buffer-byte-length:utf8",
  tokenMeasurementStatus: "unknown",
  tokenMeasurementSource: "authoritative-tokenizer-unavailable",
  providerContextWindowTokens: 400000,
  contextWindowEffectiveFrom: "2026-01-01",
  contextWindowStatus: "observed",
  contextWindowSource: "orchestration-provider-catalog",
  contextBudgetStatus: "unknown",
  contextBudgetSource: "north-harness-unconfigured",
  compactionPolicy: "native-auto-compact-enabled",
  compactionPolicyVersion: "north-native-auto-compact:v1",
} as const;


async function unusedPort(): Promise<number> {
  const server = createServer();
  await new Promise<void>((resolveListen, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolveListen);
  });
  const address = server.address();
  if (!address || typeof address === "string") throw new Error("failed to allocate test port");
  await new Promise<void>((resolveClose) => server.close(() => resolveClose()));
  return address.port;
}

function framServerFixture(): {
  home: string; bin: string; out: string; server: string;
} {
  const home = frozenFramHome;
  const bin = resolve(home, "bin");
  const out = resolve(home, "out");
  const server = resolve(bin, "fram-server");
  if (!existsSync(server))
    throw new Error("frozen Fram bin/fram-server is unavailable for the run-ledger fixture");
  return { home, bin, out, server };
}

async function waitForFramServer(port: number, spaceId: string): Promise<FramRpcClient> {
  for (let attempt = 0; attempt < 400; attempt += 1) {
    try {
      return await FramRpcClient.connect({
        port, spaceId, connectTimeoutMs: 100, readTimeoutMs: 500,
        maxAttempts: 1, retryDelayMs: 0, jitterMs: 0,
      });
    } catch {}
    await Bun.sleep(25);
  }
  throw new Error("isolated Fram server did not become FRAMRPC-ready");
}

test("one frozen Fram server commits seven ordered events inside the production timeout", async () => {
  const fram = framServerFixture();
  const scratch = mkdtempSync(join(tmpdir(), "north-run-event-batch-"));
  const log = join(scratch, "history.framlog");
  const spaceId = "north-run-event-batch";
  const port = await unusedPort();
  const fixtureEnvironment = {
    NORTH_PORT: String(port),
    FRAM_LOG: log,
    FRAM_SPACE_ID: spaceId,
    FRAM_HOME: fram.home,
    FRAM_BIN: fram.bin,
    FRAM_OUT: fram.out,
  };
  const priorEnvironment = new Map(
    Object.keys(fixtureEnvironment).map((key) => [key, process.env[key]]),
  );
  const serverProcess = Bun.spawn([
    fram.server, "serve", String(port), log, spaceId,
  ], {
    cwd: fram.home,
    env: {
      ...process.env,
      ...fixtureEnvironment,
      FRAM_SERVER_RUNTIME: "jvm-dev",
      FRAM_SERVER_QUIET: "1",
      FRAM_SERVER_XMX: "1g",
      FRAM_SNAPSHOT_BOOT: "0",
    },
    stdout: "pipe",
    stderr: "pipe",
  });
  let client: FramRpcClient | undefined;
  try {
    client = await waitForFramServer(port, spaceId);
    Object.assign(process.env, fixtureEnvironment);
    const versionBefore = await client.version();
    const ledger = new AgentRunLedger({
      run: "@run:batch-writer-integration",
      thread: "(ad-hoc)",
      agent: "batch-writer-integration",
    });
    for (let sequence = 0; sequence < 6; sequence += 1) {
      ledger.append(
        "tool_activity", { totalCalls: sequence }, "north-sdk-test", "exact",
        `2026-07-23T00:00:0${sequence}.000Z`,
      );
    }
    ledger.append(
      "terminal_cleanup", { outcome: "ran", cleanupStatus: "observed" },
      "north-sdk-test", "exact", "2026-07-23T00:00:06.000Z",
    );
    const events = ledger.events();
    const started = performance.now();
    expect(await recordRunEvents(events, 4_000)).toBe("recorded");
    const elapsedMs = performance.now() - started;
    expect(elapsedMs).toBeLessThan(4_000);

    const versionAfter = await client.version();
    expect(versionAfter.servedVersion - versionBefore.servedVersion).toBe(events.length);
    for (const event of events) {
      const stored = await client.scanAll(event.subject, null, null);
      const actualFacts = stored.rows.map((row) => {
        expect(row).toBeInstanceOf(FramTriple);
        const fact = row as FramTriple;
        expect(fact.t1).toBe(event.subject);
        expect(typeof fact.t2).toBe("string");
        expect(typeof fact.t3).toBe("string");
        return JSON.stringify([fact.t2, fact.t3]);
      }).sort();
      const expectedFacts = eventFacts(event)
        .map(([predicate, value]) => JSON.stringify([predicate, value]))
        .sort();
      expect(actualFacts).toEqual(expectedFacts);
    }
  } finally {
    client?.close();
    for (const [key, value] of priorEnvironment) {
      if (value === undefined) delete process.env[key];
      else process.env[key] = value;
    }
    serverProcess.kill("SIGTERM");
    await serverProcess.exited;
    rmSync(scratch, { recursive: true, force: true });
  }
}, 20_000);

test("terminal success publishes exact ordered lifecycle evidence and a complete summary", async () => {
  const published: AgentRunEvent[] = [];
  let writerCalls = 0;
  const summary = await publishRunLifecycleLedger(identity, {
    promptEconomics,
    tokenUsage: {
      inputTokens: 100, outputTokens: 20, cacheReadTokens: 40, cacheCreateTokens: 10,
      cachedInputTokens: 30, total: 170, terminalCount: 1,
      terminalScope: "anthropic_result_terminal", totalStatus: "exact",
    },
    compactions: 0,
    outcome: "ran",
    mcpActivity,
  }, 1000, async (events, timeoutMs) => {
    writerCalls += 1;
    published.push(...events);
    expect(timeoutMs).toBe(1000);
    return "recorded";
  });
  expect(writerCalls).toBe(1);
  expect(summary?.eventCount).toBe(7);
  expect(published.map(({ type }) => type)).toEqual([
    "tool_activity", "tool_activity", "prompt_constructed", "usage_observed", "cache_observed",
    "compaction_observed", "terminal_cleanup",
  ]);
  expect(published.at(-1)?.payload).toEqual({ outcome: "ran", cleanupStatus: "observed" });
});

test("a provider failure is a terminal lifecycle but missing usage stays explicitly unknown", async () => {
  const published: AgentRunEvent[] = [];
  const summary = await publishRunLifecycleLedger(identity, {
    promptEconomics,
    tokenUsage: { terminalCount: 0, totalStatus: "unknown_no_terminal" },
    compactions: 1,
    outcome: "provider_error",
    mcpActivity,
  }, 1000, async (events) => { published.push(...events); return "recorded"; });
  expect(summary).toBeDefined();
  expect(published.find(({ type }) => type === "usage_observed")?.coverage).toBe("unknown");
  expect(published.at(-1)?.payload.outcome).toBe("provider_error");
});

test("a mid-batch writer failure never manufactures a complete summary", async () => {
  const finalIdentity = { ...identity, run: "@run:lane-ledger-final" };
  const published: AgentRunEvent[] = [];
  const summary = await publishRunLifecycleLedger(finalIdentity, {
    promptEconomics,
    tokenUsage: { terminalCount: 0, totalStatus: "unknown_no_terminal" },
    compactions: 0,
    outcome: "ran",
    mcpActivity,
  }, 1000, async (events) => {
    published.push(...events.slice(0, 5));
    return "unavailable";
  });
  expect(summary).toBeUndefined();
  expect(published.every(({ run, subject }) =>
    run === "@run:lane-ledger-final" && subject.startsWith("@run:lane-ledger-final:event:"),
  )).toBe(true);

  const crashed = await publishRunLifecycleLedger(finalIdentity, {
    tokenUsage: { terminalCount: 0, totalStatus: "unknown_no_terminal" },
    compactions: 0,
    outcome: "died",
    mcpActivity,
  }, 1000, async () => "recorded");
  expect(crashed).toBeUndefined();
});
