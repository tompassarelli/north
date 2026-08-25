// Reaper false-positive fix (thread 019f6af0-ba69): a finishing lane publishes
// its process/delivery terminal on @agent:<id> synchronously. Production commits
// that projection with a digest marker; the capture fake below asserts its body.
// A committed kind=run row is only a secondary trail.
//
// Hermetic: a fake `north` on PATH + NORTH_BIN captures every tell to a temp log; the
// injected queryFn owns the whole SDK boundary, so no live coordinator / network / model.
// This is the same fake-engine pattern as spawn-boundary.test.ts.
import { test, expect, beforeAll, afterAll, spyOn } from "bun:test";
import {
  chmodSync, copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync,
  rmSync, writeFileSync,
} from "node:fs";
import { join, resolve } from "node:path";
import { tmpdir } from "node:os";
import { createHash } from "node:crypto";
import { ProviderRetrySafeError, type RoutedQueryArguments } from "../src/providers";
import { ManagedCodexHarvestError } from "../src/providers/codex-app-server";
import { managedCodexHarvestEvidence } from "../src/providers/openai";
import { RUN_BAR_EVIDENCE_VERSION } from "../src/delivery-verification";
import type { RoutingAssessment } from "../src/routing-economics";
import type { ShadowReviewerNote, ShadowReviewerUpdate } from "../src/shadow-reviewer";
import {
  DeliveryEvidenceRetryableError,
  DeliveryReservationWriterProcessFailure,
  type DeliveryAttemptLaunchIntent,
  type DeliveryAttemptProviderStart,
  type DeliveryAttemptRoute,
  type DeliveryReservation,
  type DeliveryRunContext,
} from "../src/delivery-evidence";
import { presetRequest } from "./routing-fixtures";
import { applyOrchestrationStaffing } from "../src/orchestration-staffing";
import { HostTerminationCoordinator } from "../src/host-termination";
import { createExecutionActivityEmitter } from "../src/execution-activity";
import type { FeedSubscription, InputAdmission } from "../src/coordination";
import { LANE_LIFECYCLE_KINDS, scanJournalFile } from "../src/bridge/journal";
import {
  decodeWireEvent,
  readWireJsonl,
  WireEventWriter,
  wireEventId,
  wireMessageId,
  wireModelCallId,
  wireRunId,
  wireToolCallId,
  type WireEvent,
  type WireQuery,
  type WireQueryInput,
} from "../src/wire";
import {
  wireTurnEvents,
  wireTurnQuery,
  wireTurnSequenceQuery,
  wireInputIterator,
} from "./support/wire-query";
import { spawn as spawnUnderTest } from "./support/spawn";
import { dispatch as dispatchUnderTest } from "./support/dispatch";

let dir: string;
let log: string;

const MANAGED_ENV = [
  "PATH", "NORTH_BIN", "NORTH_PEER_BB", "NORTH_IDENTITY_TEST_REDIRECT", "NORTH_PORT", "NORTH_STREAM_DIR", "AGENT_LAWS", "AGENT_PRAXIS",
  "AGENT_ID", "NORTH_AGENT_ID", "AGENT_COORDINATOR", "AGENT_MODEL", "AGENT_ROLE",
  "AGENT_IDENTITY_ROLE", "AGENT_TARGET",
  "AGENT_TIER", "AGENT_REASONING", "AGENT_POSTURE", "AGENT_TOPOLOGY", "AGENT_TASK_GRADE",
  "AGENT_DOMAIN_REQUIREMENTS", "AGENT_COMPOSITION",
  "NORTH_ROUTING_POLICY", "NORTH_ENVELOPE_ACCOUNTING",
  "NORTH_BG_MAX_CONTINUATIONS", "NORTH_STALL_MS", "NORTH_TERMINAL_PUBLICATION_BUDGET_MS",
  "NORTH_PROVIDER_OBSERVATIONS", "NORTH_ALLOCATION_MODE", "NORTH_PROVIDER_ORDER",
  "NORTH_PROVIDER_WEIGHTS", "NORTH_RESERVED_FRONTIER_PROVIDER",
  "NORTH_ANTHROPIC_ENTITLEMENT_PRESSURE", "NORTH_OPENAI_ENTITLEMENT_PRESSURE",
  "NORTH_STRUGGLE_POLICY_EXPECTED", "STRUGGLE_ERROR_STREAK",
  "STRUGGLE_LOOP_REPEAT", "STRUGGLE_LOOP_WINDOW", "STRUGGLE_STALL_TURNS",
  "STRUGGLE_STALL_TURNS_ORCHESTRATOR",
  "NORTH_SHADOW_REVIEWER", "NORTH_LEARNING_POLICY",
] as const;
const origEnv: Record<string, string | undefined> = {};
for (const k of MANAGED_ENV) origEnv[k] = process.env[k];

const TEST_COORDINATOR = `test-coordinator-${process.pid}`;

const LOW_RISK_ASSESSMENT: RoutingAssessment = {
  version: "minimum-sufficient-v1",
  signals: {
    decisionOwnership: "none", seamScope: "none",
    errorExposure: "contained-reversible", oracleStrength: "objective-end-to-end",
    foundationalImpact: "none", dependencyShape: "atomic-cohesive",
    reasoningShape: "deterministic",
  },
  derived: {
    minimumTier: "economy", minimumReasoning: "low",
    ruleCodes: ["reasoning-shape:deterministic-tight-strong-oracle"],
  },
  selected: { tier: "economy", reasoning: "low" },
};

function attemptRoute(
  threadId: string,
  provider: "anthropic" | "openai" = "openai",
): DeliveryAttemptRoute {
  const accountId = provider === "openai" ? "codex-test" : "anthropic-test";
  return {
    provider,
    accountId,
    model: provider === "openai" ? "gpt-test" : "claude-test",
    accountAuthorityReceiptSha256: "1".repeat(64),
    routeObservationReceiptSha256: "2".repeat(64),
    threadLease: {
      resource: `thread:${threadId}:dispatch`, holder: "test-holder", epoch: 1,
    },
    accountLease: {
      resource: `codex-account:${accountId}:slot:0`, holder: "test-holder", epoch: 1,
    },
  };
}

function attemptReservation(
  context: DeliveryRunContext,
  route: DeliveryAttemptRoute,
  baselineDoneWhen: string[],
): DeliveryReservation {
  const manifestSha256 = createHash("sha256").update(context.runId).digest("hex");
  return {
    contractOrigin: "accepted",
    baselineDoneWhen,
    attemptId: `@attempt:${manifestSha256}`,
    attemptOrdinal: 1,
    manifestSha256,
    ...route,
  };
}

function linkedAttemptId(
  reservation: DeliveryReservation,
  ...transitions: Array<DeliveryAttemptLaunchIntent | DeliveryAttemptProviderStart>
): string {
  if (transitions.some(({ attemptId }) => attemptId !== reservation.attemptId)) {
    throw new Error("fixture transition does not name its exact reservation attempt");
  }
  return reservation.attemptId;
}

function attemptTransitions() {
  return {
    launchIntent(
      _context: DeliveryRunContext,
      reservation: DeliveryReservation,
    ): DeliveryAttemptLaunchIntent {
      return {
        attemptId: linkedAttemptId(reservation),
        launchIntentSha256: "4".repeat(64),
        launchedAt: "2026-08-25T00:00:00.000Z",
      };
    },
    providerStart(
      _context: DeliveryRunContext,
      reservation: DeliveryReservation,
      launchIntent: DeliveryAttemptLaunchIntent,
      providerStartReceiptSha256: string,
    ): DeliveryAttemptProviderStart {
      return {
        attemptId: linkedAttemptId(reservation, launchIntent),
        providerStartReceiptSha256,
        providerStartManifestSha256: "5".repeat(64),
        providerStartedAt: "2026-08-25T00:00:01.000Z",
      };
    },
    terminal(
      _context: DeliveryRunContext,
      reservation: DeliveryReservation,
      launchIntent: DeliveryAttemptLaunchIntent,
      providerStart: DeliveryAttemptProviderStart,
      terminalReceiptSha256: string,
    ) {
      return {
        attemptId: linkedAttemptId(reservation, launchIntent, providerStart),
        terminalReceiptSha256,
        terminalManifestSha256: "6".repeat(64),
        terminalAt: "2026-08-25T00:00:02.000Z",
      };
    },
  };
}

function pinEvidence(
  provider: "anthropic" | "openai",
  target?: string,
): {
  policyVersion: "north-routing-pin-v1";
  issuedAt: string;
  expiresAt: string;
  reasonCode: "explicit-human-request";
  detail: string;
  pins: Array<{ kind: "provider" | "account"; value: string }>;
} {
  const issuedAt = new Date();
  return {
    policyVersion: "north-routing-pin-v1",
    issuedAt: issuedAt.toISOString(),
    expiresAt: new Date(issuedAt.getTime() + 60 * 60 * 1000).toISOString(),
    reasonCode: "explicit-human-request",
    detail: "completion outcome fixture",
    pins: [
      { kind: "provider", value: provider },
      ...(target ? [{ kind: "account" as const, value: target }] : []),
    ],
  };
}

function fakeTerminationHost() {
  const signals = new Map<string, Set<() => void>>([
    ["SIGTERM", new Set()], ["SIGINT", new Set()],
  ]);
  const exits = new Set<() => void>();
  const exitCodes: number[] = [];
  return {
    control: {
      onSignal: (signal: string, listener: () => void) => { signals.get(signal)!.add(listener); },
      offSignal: (signal: string, listener: () => void) => { signals.get(signal)!.delete(listener); },
      onExit: (listener: () => void) => { exits.add(listener); },
      offExit: (listener: () => void) => { exits.delete(listener); },
      exit: (code: number) => { exitCodes.push(code); },
    },
    emit: (signal: "SIGTERM" | "SIGINT") => {
      for (const listener of [...signals.get(signal)!]) listener();
    },
    listenerCount: () => [...signals.values()].reduce(
      (sum, listeners) => sum + listeners.size, exits.size,
    ),
    exitCodes,
  };
}

function readyFeedSubscription(): FeedSubscription {
  return Object.assign(async () => {}, {
    ready: Promise.resolve(),
    caughtUp: Promise.resolve(),
    replay: async () => {},
    drain: async () => {},
    isArmed: () => true,
  });
}

function subscribeReadyFeed(): FeedSubscription {
  return readyFeedSubscription();
}

async function eventuallyTrue(
  predicate: () => boolean,
  label: string,
  timeoutMs = 2_000,
): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (!predicate()) {
    if (Date.now() >= deadline) throw new Error(`timed out waiting for ${label}`);
    await new Promise((resolve) => setTimeout(resolve, 5));
  }
}

beforeAll(() => {
  dir = mkdtempSync(join(tmpdir(), "north-completion-"));
  log = join(dir, "north.log");
  const fake = join(dir, "north");
  writeFileSync(fake, `#!/usr/bin/env bash
printf '%s\\n' "$*" >> "${log}"
case "$*" in
  *test-terminal-aux-budget*agent_death*) sleep 5 ;;
esac
exit 0
`);
  chmodSync(fake, 0o755);
  const fakeBb = join(dir, "bb");
  writeFileSync(fakeBb, `#!/usr/bin/env bash
printf 'bb %s\\n' "$*" >> "${log}"
case "$*" in
  *run-fact-internal.clj*)
    payload="$(command cat)"
    printf 'run-fact\\t%s\\t%s\\n' "$*" "$payload" >> "${log}"
    ;;
  *msg-cli.clj*test-dispatch-notify-failure*) exit 1 ;;
esac
exit 0
`);
  chmodSync(fakeBb, 0o755);
  const fakeClaude = join(dir, "claude");
  writeFileSync(fakeClaude, `#!/usr/bin/env bash
if [ "$1" = "--version" ]; then
  printf '%s\n' '2.1.0-test'
  exit 0
fi
if [ "$1" = "auth" ] && [ "$2" = "status" ] && [ "$3" = "--json" ]; then
  printf '%s\n' '{"loggedIn":true,"authMethod":"claude.ai","apiProvider":"firstParty"}'
  exit 0
fi
exit 2
`);
  chmodSync(fakeClaude, 0o755);
  const fakeCodex = join(dir, "codex");
  writeFileSync(fakeCodex, `#!/usr/bin/env bash
if [ "$1" = "--version" ]; then
  printf '%s\n' 'codex-test'
  exit 0
fi
if [ "$1" = "login" ] && [ "$2" = "status" ]; then
  printf '%s\n' 'Logged in using ChatGPT'
  exit 0
fi
exit 2
`);
  chmodSync(fakeCodex, 0o755);

  process.env.PATH = `${dir}:${process.env.PATH}`;
  process.env.NORTH_BIN = fake;
  process.env.NORTH_PEER_BB = fakeBb;
  process.env.NORTH_IDENTITY_TEST_REDIRECT = "1";
  process.env.NORTH_PORT = "59999"; // unused -> any stray bb write silently no-ops
  process.env.NORTH_STREAM_DIR = dir;
  process.env.AGENT_LAWS = "off";
  process.env.AGENT_PRAXIS = "off";
  process.env.NORTH_ROUTING_POLICY = join(dir, "absent-routing-policy.json");
  process.env.NORTH_PROVIDER_OBSERVATIONS = join(dir, "absent-provider-observations.json");
  delete process.env.NORTH_ALLOCATION_MODE;
  delete process.env.NORTH_BG_MAX_CONTINUATIONS;
  delete process.env.NORTH_PROVIDER_ORDER;
  delete process.env.NORTH_PROVIDER_WEIGHTS;
  delete process.env.NORTH_RESERVED_FRONTIER_PROVIDER;
  delete process.env.NORTH_ANTHROPIC_ENTITLEMENT_PRESSURE;
  delete process.env.NORTH_OPENAI_ENTITLEMENT_PRESSURE;
  delete process.env.NORTH_STRUGGLE_POLICY_EXPECTED;
  delete process.env.STRUGGLE_ERROR_STREAK;
  delete process.env.STRUGGLE_LOOP_REPEAT;
  delete process.env.STRUGGLE_LOOP_WINDOW;
  delete process.env.STRUGGLE_STALL_TURNS;
  delete process.env.STRUGGLE_STALL_TURNS_ORCHESTRATOR;
  delete process.env.NORTH_SHADOW_REVIEWER;
  delete process.env.NORTH_LEARNING_POLICY;
  delete process.env.AGENT_ID;
  delete process.env.NORTH_AGENT_ID;
  delete process.env.AGENT_MODEL;
  delete process.env.AGENT_ROLE;
  delete process.env.AGENT_IDENTITY_ROLE;
  delete process.env.AGENT_TIER;
  delete process.env.AGENT_REASONING;
  delete process.env.AGENT_POSTURE;
  delete process.env.AGENT_TOPOLOGY;
  delete process.env.AGENT_TASK_GRADE;
  delete process.env.AGENT_DOMAIN_REQUIREMENTS;
  delete process.env.AGENT_COMPOSITION;
  delete process.env.AGENT_TARGET;
  process.env.AGENT_COORDINATOR = TEST_COORDINATOR;
});

afterAll(() => {
  for (const k of MANAGED_ENV) {
    if (origEnv[k] === undefined) delete process.env[k];
    else process.env[k] = origEnv[k];
  }
  try { rmSync(dir, { recursive: true, force: true }); } catch {}
});

test("a clean-finishing lane records outcome=ran ON the lane entity (@agent:<id>)", async () => {
  const { spawn } = await import("./support/spawn");
  let interrupts = 0;
  const journalRoot = join(dir, "lane-journal");

  const cleanQuery = (args: RoutedQueryArguments): WireQuery => ({
    interrupt: async () => { interrupts++; },
    [Symbol.asyncIterator](): AsyncIterator<WireEvent> {
      return wireTurnEvents(args, {
        output: "task done", turns: 1, providerDurationMs: 1,
      });
    },
  });

  const result = await spawn({
    prompt: "do a bounded task", agentId: "test-done-ok",
    coordinator: TEST_COORDINATOR,
    routingMetadata: presetRequest("integrator"), queryFn: cleanQuery, journalRoot,
  });
  expect(result).toBe("task done");
  expect(interrupts).toBe(1);

  const lifecycle = scanJournalFile(
    join(journalRoot, "test-done-ok", "events.log"), "test-done-ok",
  ).records;
  expect(lifecycle.map(({ kind }) => kind)).toEqual([
    LANE_LIFECYCLE_KINDS.spawnStart,
    LANE_LIFECYCLE_KINDS.identityAdmitted,
    LANE_LIFECYCLE_KINDS.turnBoundary,
    LANE_LIFECYCLE_KINDS.terminal,
    LANE_LIFECYCLE_KINDS.harvest,
  ]);
  expect(lifecycle.at(-2)?.data).toMatchObject({
    outcome: "ran", processOutcome: "ran", resultBytes: 9,
  });
  expect(lifecycle.at(-1)?.data).toMatchObject({
    status: "nothing-committed", branch: "lane-test-done-ok",
    sha: expect.stringMatching(/^[0-9a-f]{40}$/),
  });

  expect(existsSync(log)).toBe(true);
  const logged = readFileSync(log, "utf8");
  // The terminal body lands on the lane via the NORTH_BIN-honoring sync write.
  // Coordinator integration separately proves the production digest marker.
  expect(logged).toContain("tell agent:test-done-ok process_outcome ran");
  expect(logged).toContain("tell agent:test-done-ok process_outcome ran");
  expect(logged).toContain("tell agent:test-done-ok delivery_outcome unverified");
  expect(logged).toContain(`send test-done-ok ${TEST_COORDINATOR} AGENT COMPLETE`);
  expect(logged).toContain(
    "tell agent:test-done-ok delivery_reason provider_terminal_success_without_external_verification",
  );
  const runLines = await settledRunLines("test-done-ok");
  expect(runLines.some((line) => line.endsWith(" thread (ad-hoc)"))).toBe(true);
  expect(runLines.some((line) => line.endsWith(" judgment_grade_status unavailable"))).toBe(true);
  expect(runLines.some((line) => line.endsWith(" judgment_grade_source ad-hoc"))).toBe(true);
  expect(runLines.some((line) => line.endsWith(" wire_ledger_status complete"))).toBe(true);
  expect(runLines.some((line) => line.endsWith(" model_call_count 1"))).toBe(true);
  expect(runLines.some((line) => line.endsWith(" struggle_topology worker"))).toBe(true);
  expect(runLines.some((line) => line.endsWith(" struggle_no_progress_turn_threshold 6"))).toBe(true);
});

test("spawn runs its assigned shadow reviewer, publishes the note, and records the summary", async () => {
  writeFileSync(log, "");
  const policyPath = join(dir, "shadow-reviewer-spawn-policy.json");
  writeFileSync(policyPath, JSON.stringify({
    version: 1, mode: "learning", intensity: 1, axes: ["authoring"],
    maxTierDelta: 1, riskCeiling: "p1", seed: "shadow-spawn", epoch: "1",
    evidenceMode: "evaluation",
  }));
  const priorPolicy = process.env.NORTH_LEARNING_POLICY;
  const priorProvider = process.env.AGENT_PROVIDER;
  process.env.NORTH_LEARNING_POLICY = policyPath;
  process.env.AGENT_PROVIDER = "anthropic";
  const updates: ShadowReviewerUpdate[] = [];
  const published: Array<{ source: string; note: ShadowReviewerNote }> = [];
  try {
    const result = await spawnUnderTest({
      prompt: "exercise the spawn shadow reviewer",
      agentId: "test-shadow-reviewer-spawn",
      routingMetadata: presetRequest("scout"),
      routingAssessment: LOW_RISK_ASSESSMENT,
      provider: "anthropic",
      pinEvidence: pinEvidence("anthropic"),
      loadShadowReviewerConfig: () => ({ targetId: "anthropic" }),
      shadowReviewRunner: async (update: ShadowReviewerUpdate) => {
        updates.push(update);
        return {
          runId: wireRunId("run:test-shadow-reviewer-spawn-child"),
          status: "succeeded" as const,
          output: {
            kind: "note",
            severity: "nit",
            issueCode: "unresolved_failure",
            sourceSequence: update.sourceThroughSequence,
          },
          usageStatus: "exact" as const,
          tokens: 7,
          durationMs: 3,
        };
      },
      publishShadowReviewerNote: async (source: string, note: ShadowReviewerNote) => {
        published.push({ source, note });
      },
      queryFn: (args: RoutedQueryArguments) => wireTurnQuery(args, {
        provider: "anthropic", output: "spawn done", turns: 1, providerDurationMs: 1,
      }),
    });
    expect(result).toBe("spawn done");
  } finally {
    if (priorPolicy === undefined) delete process.env.NORTH_LEARNING_POLICY;
    else process.env.NORTH_LEARNING_POLICY = priorPolicy;
    if (priorProvider === undefined) delete process.env.AGENT_PROVIDER;
    else process.env.AGENT_PROVIDER = priorProvider;
  }
  expect(updates).toHaveLength(1);
  expect(published).toHaveLength(1);
  expect(published[0]).toMatchObject({
    source: "test-shadow-reviewer-spawn",
    note: {
      severity: "nit",
      issueCode: "unresolved_failure",
      note: `The latest update contains an unresolved failure (source event ${updates[0]!.sourceThroughSequence}).`,
    },
  });
  const lines = await settledRunLines("test-shadow-reviewer-spawn");
  expect(lines.some((line) => line.endsWith(" shadow_reviewer_status completed"))).toBe(true);
  expect(lines.some((line) => line.endsWith(" shadow_reviewer_eligible_updates 1"))).toBe(true);
  expect(lines.some((line) => line.endsWith(" shadow_reviewer_emitted_notes 1"))).toBe(true);
  expect(lines.some((line) => line.endsWith(" shadow_reviewer_tokens 7"))).toBe(true);
});

test("dispatch runs its assigned shadow reviewer, publishes the note, and records the summary", async () => {
  writeFileSync(log, "");
  const policyPath = join(dir, "shadow-reviewer-dispatch-policy.json");
  writeFileSync(policyPath, JSON.stringify({
    version: 1, mode: "learning", intensity: 1, axes: ["authoring"],
    maxTierDelta: 1, riskCeiling: "p1", seed: "shadow-dispatch", epoch: "1",
    evidenceMode: "evaluation",
  }));
  const priorPolicy = process.env.NORTH_LEARNING_POLICY;
  process.env.NORTH_LEARNING_POLICY = policyPath;
  const priorProvider = process.env.AGENT_PROVIDER;
  process.env.AGENT_PROVIDER = "anthropic";
  const updates: ShadowReviewerUpdate[] = [];
  const published: Array<{ source: string; note: ShadowReviewerNote }> = [];
  try {
    await dispatchUnderTest("test-shadow-reviewer-dispatch-thread", {
      agentId: "test-shadow-reviewer-dispatch",
      routingMetadata: presetRequest("scout"),
      routingAssessment: LOW_RISK_ASSESSMENT,
      pinEvidence: pinEvidence("anthropic"),
      claimDriver: () => ({ release() {} }),
      loadChildren: () => [],
      loadThreadFacts: () => [
        { predicate: "title", value: "Exercise the dispatch shadow reviewer" },
        { predicate: "planned", value: "true" },
        { predicate: "atomic", value: "true" },
        { predicate: "done_when", value: "dispatch finishes" },
      ],
      loadShadowReviewerConfig: () => ({ targetId: "anthropic" }),
      shadowReviewRunner: async (update: ShadowReviewerUpdate) => {
        updates.push(update);
        return {
          runId: wireRunId("run:test-shadow-reviewer-dispatch-child"),
          status: "succeeded" as const,
          output: {
            kind: "note",
            severity: "nit",
            issueCode: "failed_verification",
            sourceSequence: update.sourceThroughSequence,
          },
          usageStatus: "exact" as const,
          tokens: 11,
          durationMs: 5,
        };
      },
      publishShadowReviewerNote: async (source: string, note: ShadowReviewerNote) => {
        published.push({ source, note });
      },
      queryFn: (args: RoutedQueryArguments) => wireTurnQuery(args, {
        provider: "anthropic", output: "dispatch done", turns: 1, providerDurationMs: 1,
      }),
    });
  } finally {
    if (priorPolicy === undefined) delete process.env.NORTH_LEARNING_POLICY;
    else process.env.NORTH_LEARNING_POLICY = priorPolicy;
    if (priorProvider === undefined) delete process.env.AGENT_PROVIDER;
    else process.env.AGENT_PROVIDER = priorProvider;
  }
  expect(updates).toHaveLength(1);
  expect(published).toHaveLength(1);
  expect(published[0]).toMatchObject({
    source: "test-shadow-reviewer-dispatch",
    note: {
      severity: "nit",
      issueCode: "failed_verification",
      note: `The latest update treats a failed verification as successful (source event ${updates[0]!.sourceThroughSequence}).`,
    },
  });
  const lines = await settledRunLines("test-shadow-reviewer-dispatch");
  expect(lines.some((line) => line.endsWith(" shadow_reviewer_status completed"))).toBe(true);
  expect(lines.some((line) => line.endsWith(" shadow_reviewer_eligible_updates 1"))).toBe(true);
  expect(lines.some((line) => line.endsWith(" shadow_reviewer_emitted_notes 1"))).toBe(true);
  expect(lines.some((line) => line.endsWith(" shadow_reviewer_tokens 11"))).toBe(true);
});

test("a lane that dies mid-stream records outcome=died ON the lane entity (reported, not silent)", async () => {
  const { spawn } = await import("./support/spawn");

  // The provider dies mid-turn. The finally path runs, so this
  // is a REPORTED death: outcome=died on @agent:<id> alongside the agent_death fact. The
  // The lane lifecycle janitor then skips its committed terminal — died-unreported is reserved for
  // a hard-kill (or torn publication) with no committed terminal evidence.
  const dyingQuery = (args: RoutedQueryArguments): WireQuery => ({
    [Symbol.asyncIterator](): AsyncIterator<WireEvent> {
      return (async function*(): AsyncGenerator<WireEvent> {
        yield* wireTurnEvents(args, { output: "starting", terminal: false });
        throw new Error("Claude Code process terminated by signal 9");
      })();
    },
  });

  await spawn({ prompt: "dies", agentId: "test-done-died", routingMetadata: presetRequest("integrator"), queryFn: dyingQuery });

  const logged = readFileSync(log, "utf8");
  expect(logged).toContain("tell agent:test-done-died process_outcome died");
  expect(logged).toContain("tell agent:test-done-died process_outcome died");
  expect(logged).toContain("tell agent:test-done-died delivery_outcome blocked");
  expect(logged).toContain("tell @swarm agent_death"); // death path still fires
});

test("a synchronous provider-construction failure records run telemetry", async () => {
  const { spawn } = await import("./support/spawn");
  writeFileSync(log, "");

  const result = await spawn({
    prompt: "fail while constructing the provider query",
    agentId: "test-sync-construction-failure",
    routingMetadata: presetRequest("integrator"),
    thread: "thread-sync-construction",
    queryFn: () => { throw new Error("synchronous adapter construction failure"); },
  });

  expect(result).toBe("");
  const logged = readFileSync(log, "utf8");
  expect(logged).toContain("tell @swarm agent_death");
  expect(logged).toContain("tell agent:test-sync-construction-failure process_outcome died");
  const projection = await waitForRunFactProjection("test-sync-construction-failure");
  expect(projection).toContainEqual(["thread", "@thread-sync-construction"]);
  expect(projection.find(([predicate]) => predicate === "duration_ms")?.[1]).toMatch(/^\d+$/);
});

test("a Orchestration prompt-composition failure is blocked preflight before query construction", async () => {
  const { spawn } = await import("./support/spawn");
  writeFileSync(log, "");
  const sourceRuntime = process.env.NORTH_AGENT_RUNTIME_HOME
    ?? resolve(import.meta.dir, "../..", "agent-runtime/orchestration");
  const brokenRuntime = mkdtempSync(join(tmpdir(), "north-missing-model-delta-"));
  mkdirSync(join(brokenRuntime, "providers"), { recursive: true });
  mkdirSync(join(brokenRuntime, "docs", "deltas"), { recursive: true });
  for (const name of ["anthropic.json", "openai.json"]) {
    copyFileSync(
      join(sourceRuntime, "providers", name),
      join(brokenRuntime, "providers", name),
    );
  }
  const priorRuntimeHome = process.env.NORTH_AGENT_RUNTIME_HOME;
  let queryConstructionCalls = 0;
  try {
    process.env.NORTH_AGENT_RUNTIME_HOME = brokenRuntime;
    const routingMetadata = applyOrchestrationStaffing({
      role: "scout",
      tier: "standard",
      reasoning: "medium",
      composition: {
        kind: "template",
        id: "scout",
        overrides: ["tier", "reasoning"],
        overrideReason: "exercise the Sol prompt-composition preflight boundary",
      },
    });
    const result = await spawn({
      prompt: "prove missing model-delta classification",
      agentId: "test-prompt-composition-preflight",
      routingMetadata,
      provider: "openai",
      pinEvidence: pinEvidence("openai"),
      worktree: false,
      queryFn: () => {
        queryConstructionCalls++;
        return (async function* () {})();
      },
    });
    expect(result).toBe("");
  } finally {
    if (priorRuntimeHome === undefined) delete process.env.NORTH_AGENT_RUNTIME_HOME;
    else process.env.NORTH_AGENT_RUNTIME_HOME = priorRuntimeHome;
    rmSync(brokenRuntime, { recursive: true, force: true });
  }

  expect(queryConstructionCalls).toBe(0);
  const logged = readFileSync(log, "utf8");
  expect(logged).not.toContain(
    "tell agent:test-prompt-composition-preflight process_outcome blocked_preflight",
  );
  expect(logged).not.toContain(
    "tell agent:test-prompt-composition-preflight delivery_outcome blocked",
  );
  expect(logged).not.toContain(
    "tell agent:test-prompt-composition-preflight delivery_reason execution_preflight_blocked",
  );
  expect(logged).not.toContain("tell @swarm agent_death");
  const runLines = await settledRunLines("test-prompt-composition-preflight");
  expect(runLines.some((line) => line.endsWith(" process_outcome blocked_preflight"))).toBe(true);
  expect(runLines.some((line) => line.endsWith(" model_call_count 0"))).toBe(true);
  expect(runLines.some((line) => line.endsWith(" wire_termination_code blocked"))).toBe(true);
});

test("an Anthropic error terminal still records its authoritative usage", async () => {
  const { spawn } = await import("./support/spawn");
  writeFileSync(log, "");
  const queryFn = (args: RoutedQueryArguments): WireQuery => wireTurnQuery(args, {
    provider: "anthropic",
    status: "failed",
    errorCode: "provider_execution_failed",
    failureDetail: "provider_execution_failed",
    providerDurationMs: 1,
    turns: 1,
    usage: {
      lifetime: {
        inputTokens: 11,
        outputTokens: 3,
        cacheWriteTokens: 2,
        cacheReadTokens: 5,
        reasoningTokens: 0,
        modelCalls: 1,
      },
      context: { tokens: 21 },
    },
  });

  await spawn({ prompt: "terminal error usage", agentId: "test-terminal-error",
    routingMetadata: presetRequest("integrator"), provider: "anthropic",
    pinEvidence: pinEvidence("anthropic"), queryFn });
  const lines = await settledRunLines("test-terminal-error", "lifetime_input_tokens 11");
  expect(lines.some((line) => line.endsWith(" lifetime_input_tokens 11"))).toBe(true);
  expect(lines.some((line) => line.endsWith(" lifetime_output_tokens 3"))).toBe(true);
  expect(lines.some((line) => line.endsWith(" lifetime_cache_write_tokens 2"))).toBe(true);
  expect(lines.some((line) => line.endsWith(" lifetime_cache_read_tokens 5"))).toBe(true);
  expect(lines.some((line) => line.endsWith(" model_call_count 1"))).toBe(true);
});

test("a managed Codex provider error becomes privacy-bounded completion evidence", () => {
  const providerPayload = "provider turn error: {\"message\":\"stream disconnected\"}";
  const error = new ManagedCodexHarvestError({
    threadId: "th_fixture", turnIds: ["turn_1"], completedTurns: 0,
    text: "partial first-turn text", landedWork: true,
    mcp: {
      source: "codex-app-server:item-completed", coverage: "exact", totalCalls: 2,
      tools: [], operationReceipts: [], operationAggregates: [],
    },
    nativeCommands: {
      source: "codex-app-server:item-completed", coverage: "exact", totalCommands: 1,
      northBinaryProbe: "not_observed", completions: [],
    },
    unsupportedNotifications: {},
  }, {
    cause: new Error("Codex completed turn reported a provider-side turn error", {
      cause: new Error(providerPayload),
    }),
  });
  const evidence = managedCodexHarvestEvidence(error);
  expect(evidence.failure).toEqual({
    detail: "provider_execution_failed",
    landed: { completedTurns: 0, mcpCalls: 2, nativeCommands: 1 },
  });
  expect(evidence.turns).toEqual({
    unit: "provider-turn", count: 1, comparable: false,
  });
  expect(evidence.providerJoin?.sessionKey).toMatch(/^[a-f0-9]{64}$/);
  expect(evidence.providerJoin?.turnKeys).toEqual([expect.stringMatching(/^[a-f0-9]{64}$/)]);
  const encoded = JSON.stringify(evidence);
  expect(encoded).not.toContain("th_fixture");
  expect(encoded).not.toContain("turn_1");
  expect(encoded).not.toContain("stream disconnected");
});

test("a managed Codex deadline interrupt retains bounded interruption evidence", () => {
  const rawThread = "thread_private_canary_019f";
  const rawTurn = "turn_private_canary_019f";
  const rawModel = "gpt-private-model-canary";
  const privatePrompt = "PRIVATE_PROMPT_CANARY_do_not_publish";
  const jsonRpcCanary = JSON.stringify({
    jsonrpc: "2.0",
    method: "turn/start",
    params: {
      threadId: rawThread,
      turnId: rawTurn,
      model: rawModel,
      input: [{ type: "text", text: privatePrompt }],
    },
  });
  const error = new ManagedCodexHarvestError({
    threadId: rawThread, turnIds: [rawTurn], completedTurns: 0,
    text: privatePrompt, landedWork: true,
    mcp: {
      source: "codex-app-server:item-completed", coverage: "exact", totalCalls: 0,
      tools: [], operationReceipts: [], operationAggregates: [],
    },
    nativeCommands: {
      source: "codex-app-server:item-completed", coverage: "exact", totalCommands: 0,
      northBinaryProbe: "not_observed", completions: [],
    },
    unsupportedNotifications: {},
    // Provider diagnostics remain available only on the adapter-private error.
    stderrTail: [jsonRpcCanary],
    interrupt: {
      reason: "turn_deadline", deadlineMs: 1_500_000,
      inactivityThresholdMs: 300_000, lastActivityAgeMs: 300_012,
      openItemCount: 0, openItem: null,
      eventCount: 57,
      eventCounts: { "provider.codex.item.completed": 57 },
    },
  }, { cause: new Error("openai_codex_turn_interrupted") });
  const evidence = managedCodexHarvestEvidence(error);
  expect(evidence.failure).toEqual({
    detail: "north_turn_deadline",
    landed: { completedTurns: 0, mcpCalls: 0, nativeCommands: 0 },
  });
  expect(evidence.interrupt).toEqual({
    reason: "north_turn_deadline",
    deadlineMs: 1_500_000,
    inactivityThresholdMs: 300_000,
    lastActivityAgeMs: 300_012,
    openItemCount: 0,
    eventCount: 57,
  });

  const writer = new WireEventWriter({
    runId: wireRunId("run:managed-codex-privacy-canary"),
    eventId: (sequence) => wireEventId(`event:managed-codex-privacy-canary:${sequence}`),
  });
  const modelCallId = wireModelCallId("model-call:managed-codex-privacy-canary");
  writer.append({ kind: "run.started", lifecycle: "running" });
  writer.append({
    kind: "model-call.started",
    modelCallId,
    model: { provider: "openai", capabilityClass: "unknown" },
    attempt: 1,
  });
  writer.append({
    kind: "model-call.completed",
    modelCallId,
    status: "cancelled",
    origin: "north",
    usage: writer.snapshot()!.usage,
    usageCoverage: "unavailable",
    errorCode: "north_turn_deadline",
    evidence,
  });
  writer.terminate({ lifecycle: "cancelled", reason: { code: "cancelled" } });
  const publicTerminals = writer.events().filter((event) =>
    event.kind === "model-call.completed" || event.kind === "run.terminated");
  expect(publicTerminals.map((event) => event.kind)).toEqual([
    "model-call.completed", "run.terminated",
  ]);
  const encoded = publicTerminals.map((event) => JSON.stringify(event)).join("\n");
  for (const privateValue of [
    jsonRpcCanary, rawThread, rawTurn, rawModel, privatePrompt,
    "provider.codex", "stderrTail", "eventCounts",
  ]) expect(encoded).not.toContain(privateValue);
});

test("an empty spawn provider stream is a blocked provider error, never ran", async () => {
  const { spawn } = await import("./support/spawn");
  writeFileSync(log, "");

  const result = await spawn({
    prompt: "provider stream closes without a terminal", agentId: "test-empty-spawn",
    routingMetadata: presetRequest("integrator"),
    queryFn: () => ({
      close: async () => { writeFileSync(log, "QUERY_CLOSED spawn\n", { flag: "a" }); },
      async *[Symbol.asyncIterator]() {},
    }),
  });

  expect(result).toBe("");
  const lines = await settledRunLines("test-empty-spawn");
  expect(lines.some((line) => line.endsWith(" process_outcome provider_error"))).toBe(true);
  expect(lines.some((line) => line.endsWith(" delivery_outcome blocked"))).toBe(true);
  const replay = await readWireJsonl(join(dir, "agent-test-empty-spawn.stream.jsonl"));
  expect(replay.events.at(-1)).toMatchObject({
    kind: "run.terminated",
    lifecycle: "failed",
    reason: {
      code: "provider_error",
      detail: "provider_error",
    },
  });
  const publicationOrder = readFileSync(log, "utf8");
  expect(publicationOrder.indexOf("QUERY_CLOSED spawn")).toBeLessThan(
    publicationOrder.indexOf("tell agent:test-empty-spawn process_outcome provider_error"),
  );
});

test("an empty dispatch provider stream is a blocked provider error, never ran", async () => {
  const { dispatch } = await import("./support/dispatch");
  writeFileSync(log, "");
  const boundaryIds: string[] = [];

  const result = await dispatch("@test-empty-dispatch", {
    agentId: "test-empty-dispatch-agent",
    routingMetadata: presetRequest("integrator"),
    claimDriver: ((threadId: string) => {
      boundaryIds.push(`driver:${threadId}`);
      return { release() {} };
    }) as any,
    queryFn: () => ({
      close: async () => { writeFileSync(log, "QUERY_CLOSED dispatch\n", { flag: "a" }); },
      async *[Symbol.asyncIterator]() {},
    }) as any,
    loadThreadFacts: (threadId: string) => {
      boundaryIds.push(`facts:${threadId}`);
      return [
        { predicate: "title", value: "Empty provider dispatch" },
        { predicate: "planned", value: "true" },
        { predicate: "atomic", value: "true" },
        { predicate: "judgment_grade", value: "s" },
      ];
    },
    loadChildren: (threadId: string) => {
      boundaryIds.push(`children:${threadId}`);
      return [];
    },
  });

  expect(result.result).toBe("");
  expect(result.threadId).toBe("test-empty-dispatch");
  expect(boundaryIds).toEqual([
    "facts:test-empty-dispatch",
    "children:test-empty-dispatch",
    "driver:test-empty-dispatch",
  ]);
  const lines = await settledRunLines(
    "test-empty-dispatch-agent", "applied_domain_requirement_count 0",
  );
  expect(lines.some((line) => line.endsWith(" process_outcome provider_error"))).toBe(true);
  expect(lines.some((line) => line.endsWith(" delivery_outcome blocked"))).toBe(true);
  expect(lines.some((line) => line.endsWith(" judgment_grade s"))).toBe(true);
  expect(lines.some((line) => line.endsWith(" judgment_grade_status valid"))).toBe(true);
  expect(lines.some((line) => line.endsWith(" judgment_grade_source thread"))).toBe(true);
  expect(lines.some((line) => line.endsWith(" struggle_topology worker"))).toBe(true);
  expect(lines.some((line) => line.includes("@@test-empty-dispatch"))).toBe(false);
  const publicationOrder = readFileSync(log, "utf8");
  expect(publicationOrder.indexOf("QUERY_CLOSED dispatch")).toBeLessThan(
    publicationOrder.indexOf("tell agent:test-empty-dispatch-agent process_outcome provider_error"),
  );
});

test("dispatch snapshots estimate_hours onto its exact terminal run before comparison", async () => {
  const { dispatch } = await import("./support/dispatch");
  writeFileSync(log, "");
  const agentId = "test-dispatch-run-estimate";

  const priorStaffingSource = process.env.NORTH_STAFFING_SOURCE;
  const priorProvider = process.env.AGENT_PROVIDER;
  try {
    process.env.NORTH_STAFFING_SOURCE = "file";
    process.env.AGENT_PROVIDER = "anthropic";
    await dispatch("test-dispatch-run-estimate-thread", {
    agentId,
    routingMetadata: presetRequest("integrator"),
    pinEvidence: pinEvidence("anthropic"),
    claimDriver: (() => ({ release() {} })) as any,
    queryFn: (args: RoutedQueryArguments): WireQuery => ({
      [Symbol.asyncIterator](): AsyncIterator<WireEvent> {
        return (async function*(): AsyncGenerator<WireEvent> {
          const modelCallId = wireModelCallId("model-call:dispatch-estimate");
          const messageId = wireMessageId("message:dispatch-estimate");
          yield args.writer.append({
            kind: "model-call.started", modelCallId,
            model: { provider: "anthropic", tier: "senior", capabilityClass: "authoring" },
            effort: "high", attempt: 1,
          });
          yield args.writer.append({
            kind: "message.recorded", messageId, modelCallId,
            stage: "started", role: "assistant",
          });
          yield args.writer.append({
            kind: "message.recorded", messageId, modelCallId,
            stage: "completed", role: "assistant", content: "done",
          });
          yield args.writer.append({
            kind: "model-call.completed", modelCallId, status: "succeeded",
            origin: "provider",
            usage: {
              lifetime: {
                inputTokens: 1, outputTokens: 1, cacheReadTokens: 0,
                cacheWriteTokens: 0, reasoningTokens: 0, modelCalls: 1,
              },
              context: { tokens: 1, window: 200_000 },
            },
            usageCoverage: "exact",
            evidence: { turns: { unit: "assistant-turn", count: 1, comparable: true } },
          });
        })();
      },
    }),
    loadThreadFacts: () => [
      { predicate: "title", value: "Snapshot a dispatch estimate" },
      { predicate: "planned", value: "true" },
      { predicate: "atomic", value: "true" },
      { predicate: "estimate_hours", value: "1" },
      { predicate: "judgment_grade", value: "s" },
    ],
      loadChildren: () => [],
    });
  } finally {
    if (priorStaffingSource === undefined) delete process.env.NORTH_STAFFING_SOURCE;
    else process.env.NORTH_STAFFING_SOURCE = priorStaffingSource;
    if (priorProvider === undefined) delete process.env.AGENT_PROVIDER;
    else process.env.AGENT_PROVIDER = priorProvider;
  }

  const facts = await waitForRunFactProjection(agentId);
  const factValues = (predicate: string) => facts
    .filter(([candidate]) => candidate === predicate)
    .map(([, value]) => value);
  expect(factValues("estimate_hours")).toEqual(["1"]);
  expect(factValues("estimate_delta_ms")[0]).toMatch(/^-[0-9]+$/);
  expect(factValues("estimate_classification")).toEqual(["under"]);
  expect(factValues("estimate_ratio")[0]).toMatch(/^0(?:\.[0-9]+)?$/);
  expect(factValues("judgment_grade")).toEqual(["s"]);
  expect(factValues("judgment_grade_status")).toEqual(["valid"]);
  expect(factValues("judgment_grade_source")).toEqual(["thread"]);
});

test("dispatch rejects an invalid estimate before driver or provider side effects", async () => {
  const { dispatch } = await import("./support/dispatch");
  let claimed = false;
  let queried = false;

  const priorStaffingSource = process.env.NORTH_STAFFING_SOURCE;
  try {
    process.env.NORTH_STAFFING_SOURCE = "file";
    await expect(dispatch("test-invalid-run-estimate", {
      agentId: "test-invalid-run-estimate-agent",
      routingMetadata: presetRequest("integrator"),
      claimDriver: (() => {
        claimed = true;
        return { release() {} };
      }) as any,
      queryFn: (() => {
        queried = true;
        return (async function* () {})();
      }) as any,
      loadThreadFacts: () => [
        { predicate: "title", value: "Reject invalid timing input" },
        { predicate: "planned", value: "true" },
        { predicate: "atomic", value: "true" },
        { predicate: "estimate_hours", value: "-2" },
      ],
      loadChildren: () => [],
    })).rejects.toThrow("invalid thread estimate_hours");
  } finally {
    if (priorStaffingSource === undefined) delete process.env.NORTH_STAFFING_SOURCE;
    else process.env.NORTH_STAFFING_SOURCE = priorStaffingSource;
  }
  expect(claimed).toBe(false);
  expect(queried).toBe(false);
});

test("a spawn success terminal with an empty result is a LOUD ran_empty, never a clean ran (thread 019f8300)", async () => {
  const { spawn } = await import("./support/spawn");
  writeFileSync(log, "");

  // The opus-high death shape: a successful provider terminal whose completed
  // assistant output is empty — the final turn committed no deliverable text.
  // Recording this as process=ran read as a clean AGENT COMPLETE no-op.
  const emptyTerminalQuery = (args: RoutedQueryArguments): WireQuery => ({
    interrupt: async () => {},
    [Symbol.asyncIterator](): AsyncIterator<WireEvent> {
      return wireTurnEvents(args, { output: "", turns: 35, providerDurationMs: 1 });
    },
  });

  const result = await spawn({
    prompt: "runs long then returns nothing", agentId: "test-empty-result-spawn",
    routingMetadata: presetRequest("integrator"),
    coordinator: TEST_COORDINATOR, queryFn: emptyTerminalQuery,
  });
  expect(result).toBe("");

  const lines = await settledRunLines("test-empty-result-spawn");
  expect(lines.some((line) => line.endsWith(" process_outcome ran_empty"))).toBe(true);
  expect(lines.some((line) => line.endsWith(" process_outcome ran"))).toBe(false);
  expect(lines.some((line) => line.endsWith(" delivery_outcome blocked"))).toBe(true);
  expect(lines.some((line) => line.endsWith(" delivery_reason provider_terminal_empty_result"))).toBe(true);

  const logged = readFileSync(log, "utf8");
  expect(logged).toContain("tell agent:test-empty-result-spawn process_outcome ran_empty");
  // LOUD: a distinct subject, never the AGENT COMPLETE masquerade.
  expect(logged).toContain(`send test-empty-result-spawn ${TEST_COORDINATOR} AGENT EMPTY RESULT`);
  expect(logged.includes(`send test-empty-result-spawn ${TEST_COORDINATOR} AGENT COMPLETE`)).toBe(false);
});

test("a dispatch success terminal with an empty result is a LOUD ran_empty, never a clean ran", async () => {
  const { dispatch } = await import("./support/dispatch");
  writeFileSync(log, "");
  const agentId = "test-empty-result-dispatch";

  await dispatch(`thread-${agentId}`, {
    agentId,
    routingMetadata: presetRequest("integrator"),
    claimDriver: (() => ({ release() {} })) as any,
    queryFn: (args) => wireTurnQuery(args, {
      output: "", turns: 40, providerDurationMs: 1,
    }),
    loadThreadFacts: () => [
      { predicate: "title", value: "Empty result dispatch terminal" },
      { predicate: "planned", value: "true" },
      { predicate: "atomic", value: "true" },
    ],
    loadChildren: () => [],
  });

  const lines = await settledRunLines(agentId);
  expect(lines.some((line) => line.endsWith(" process_outcome ran_empty"))).toBe(true);
  expect(lines.some((line) => line.endsWith(" process_outcome ran"))).toBe(false);
  expect(lines.some((line) => line.endsWith(" delivery_outcome blocked"))).toBe(true);
  expect(lines.some((line) => line.endsWith(" delivery_reason provider_terminal_empty_result"))).toBe(true);

  const logged = readFileSync(log, "utf8");
  expect(logged).toContain(`send ${agentId} ${TEST_COORDINATOR} AGENT EMPTY RESULT`);
  expect(logged.includes(`send ${agentId} ${TEST_COORDINATOR} AGENT COMPLETE`)).toBe(false);
});

test("spawn repairs one empty Anthropic terminal on the same streaming query", async () => {
  writeFileSync(log, "");
  const agentId = "test-empty-repair-spawn";
  let queryConstructions = 0;
  let continuationCalls = 0;
  const inputs: string[] = [];
  const queryFn = (args: RoutedQueryArguments): WireQuery => {
    queryConstructions++;
    return wireTurnSequenceQuery(args, [
      { provider: "anthropic", output: "", turns: 1, providerDurationMs: 1 },
      { provider: "anthropic", output: "recovered result", turns: 1, providerDurationMs: 2 },
    ], {
      onContinue: () => { continuationCalls++; },
      onInput: (text) => inputs.push(text),
    });
  };

  const result = await spawnUnderTest({
    prompt: "finish after an empty first turn",
    agentId,
    routingMetadata: presetRequest("integrator"),
    provider: "anthropic",
    pinEvidence: pinEvidence("anthropic"),
    queryFn,
  });

  expect(result).toBe("recovered result");
  expect(queryConstructions).toBe(1);
  expect(continuationCalls).toBe(1);
  expect(inputs).toHaveLength(2);
  expect(inputs[1]).toContain("previous turn succeeded without any assistant text");
  const replay = await readWireJsonl(join(dir, `agent-${agentId}.stream.jsonl`));
  expect(replay.events.filter((event) => event.kind === "model-call.completed")).toHaveLength(2);
  expect(replay.events.filter((event) => event.kind === "model-call.started")
    .map((event) => event.model.provider)).toEqual(["anthropic", "anthropic"]);
  expect(replay.events.filter((event) => event.kind === "run.terminated")).toHaveLength(1);
  expect(replay.events.at(-1)).toMatchObject({
    kind: "run.terminated", lifecycle: "completed", reason: { code: "completed" },
  });
  const lines = await settledRunLines(agentId);
  expect(lines.some((line) => line.endsWith(" process_outcome ran"))).toBe(true);
});

test("private empty repair freezes live input and leaves between-turn mail replayable", async () => {
  writeFileSync(log, "");
  const agentId = "test-empty-repair-live-input-freeze";
  let deliver: ((message: string) => {
    consumed: Promise<boolean>;
    cancel: () => void;
  }) | undefined;
  let lateAdmission: {
    consumed: Promise<boolean>;
    cancel: () => void;
  } | undefined;
  let lateConsumed: boolean | undefined;
  let drainCalls = 0;
  let continuationCalls = 0;
  const inputs: string[] = [];
  const queryFn = (args: RoutedQueryArguments): WireQuery => {
    let turn = 0;
    let pendingInput: WireQueryInput | undefined = args.input;
    return {
      executionTransport: "sdk-stream",
      async continueTurn(input: WireQueryInput): Promise<void> {
        continuationCalls++;
        pendingInput = input;
      },
      [Symbol.asyncIterator](): AsyncIterator<WireEvent> {
        return (async function*(): AsyncGenerator<WireEvent> {
          const currentTurn = turn++;
          const input = pendingInput;
          if (input === undefined) throw new Error("repair turn opened without input");
          pendingInput = undefined;
          const message = await wireInputIterator(input).next();
          if (message.done) throw new Error("repair turn input was empty");
          inputs.push(message.value.text);
          try {
            yield* wireTurnEvents(args, {
              provider: "anthropic",
              output: currentTurn === 0 ? "" : "recovered after route freeze",
              turns: 1,
            });
          } finally {
            if (currentTurn === 0) {
              if (!deliver) throw new Error("live-input feed was not armed");
              lateAdmission = deliver("late between-turn message");
            }
          }
        })();
      },
    };
  };

  const result = await spawnUnderTest({
    prompt: "repair without orphaning live input",
    agentId,
    routingMetadata: presetRequest("integrator"),
    provider: "anthropic",
    pinEvidence: pinEvidence("anthropic"),
    sessionHardCapRuntime: {
      hardCapMs: 5_000,
      stateDirectory: join(dir, "repair-live-input-hard-cap"),
    },
    feedSubscriber: (_recipient: string, onMail: typeof deliver) => {
      if (!onMail) throw new Error("live-input feed has no admission callback");
      deliver = onMail;
      return Object.assign(async () => {}, {
        ready: Promise.resolve(),
        drain: async () => {
          drainCalls++;
          if (!lateAdmission) throw new Error("late admission was not observed before freeze");
          lateConsumed = await lateAdmission.consumed;
        },
        isArmed: () => true,
      });
    },
    queryFn,
  });

  expect(result).toBe("recovered after route freeze");
  expect(continuationCalls).toBe(1);
  expect(inputs).toHaveLength(2);
  expect(inputs[1]).toContain("previous turn succeeded without any assistant text");
  expect(inputs[1]).not.toContain("late between-turn message");
  expect(lateConsumed).toBe(false);
  expect(drainCalls).toBe(1);
  const lines = await settledRunLines(agentId);
  expect(lines.some((line) => line.endsWith(" process_outcome ran"))).toBe(true);
  expect(lines.some((line) => line.endsWith(" process_outcome session_hard_cap"))).toBe(false);
});

test("dispatch repairs one empty managed-Codex terminal in the retained iterator", async () => {
  writeFileSync(log, "");
  const agentId = "test-empty-repair-dispatch";
  let queryConstructions = 0;
  let continuationCalls = 0;
  const inputs: string[] = [];
  const queryFn = (args: RoutedQueryArguments): WireQuery => {
    queryConstructions++;
    return wireTurnSequenceQuery(args, [
      { provider: "openai", output: "", turns: 1, providerDurationMs: 1 },
      { provider: "openai", output: "dispatch recovered", turns: 1, providerDurationMs: 2 },
    ], {
      onContinue: () => { continuationCalls++; },
      onInput: (text) => inputs.push(text),
    });
  };

  const priorStaffingSource = process.env.NORTH_STAFFING_SOURCE;
  const priorProvider = process.env.AGENT_PROVIDER;
  const result = await (async () => {
    try {
    process.env.NORTH_STAFFING_SOURCE = "file";
    process.env.AGENT_PROVIDER = "openai";
    return await dispatchUnderTest("test-empty-repair-dispatch-thread", {
      agentId,
      routingMetadata: presetRequest("integrator"),
      pinEvidence: pinEvidence("openai"),
      queryFn,
      feedSubscriber: subscribeReadyFeed,
      claimDriver: () => ({ release: () => true }),
      loadThreadFacts: () => [
        { predicate: "title", value: "Finish after an empty first turn" },
        { predicate: "planned", value: "true" },
        { predicate: "atomic", value: "true" },
      ],
      loadChildren: () => [],
    });
    } finally {
      if (priorStaffingSource === undefined) delete process.env.NORTH_STAFFING_SOURCE;
      else process.env.NORTH_STAFFING_SOURCE = priorStaffingSource;
      if (priorProvider === undefined) delete process.env.AGENT_PROVIDER;
      else process.env.AGENT_PROVIDER = priorProvider;
    }
  })();

  expect(result.result).toBe("dispatch recovered");
  expect(queryConstructions).toBe(1);
  expect(continuationCalls).toBe(0);
  expect(inputs).toHaveLength(2);
  expect(inputs[1]).toContain("previous turn succeeded without any assistant text");
  const replay = await readWireJsonl(join(dir, `agent-${agentId}.stream.jsonl`));
  expect(replay.events.filter((event) => event.kind === "model-call.completed")).toHaveLength(2);
  expect(replay.events.filter((event) => event.kind === "model-call.started")
    .map((event) => event.model.provider)).toEqual(["openai", "openai"]);
  expect(replay.events.filter((event) => event.kind === "run.terminated")).toHaveLength(1);
  expect(replay.events.at(-1)).toMatchObject({
    kind: "run.terminated", lifecycle: "completed", reason: { code: "completed" },
  });
});

test("a second empty terminal exhausts the single corrective turn and remains ran_empty", async () => {
  writeFileSync(log, "");
  const agentId = "test-empty-repair-exhausted";
  let continuationCalls = 0;
  const result = await spawnUnderTest({
    prompt: "remain empty twice",
    agentId,
    routingMetadata: presetRequest("integrator"),
    provider: "anthropic",
    pinEvidence: pinEvidence("anthropic"),
    queryFn: (args) => wireTurnSequenceQuery(args, [
      { provider: "anthropic", output: "", turns: 1, providerDurationMs: 1 },
      { provider: "anthropic", output: "", turns: 1, providerDurationMs: 2 },
    ], { onContinue: () => { continuationCalls++; } }),
  });

  expect(result).toBe("");
  expect(continuationCalls).toBe(1);
  const replay = await readWireJsonl(join(dir, `agent-${agentId}.stream.jsonl`));
  expect(replay.events.filter((event) => event.kind === "model-call.completed")).toHaveLength(2);
  const lines = await settledRunLines(agentId);
  expect(lines.some((line) => line.endsWith(" process_outcome ran_empty"))).toBe(true);
  expect(lines.some((line) => line.endsWith(" process_outcome ran"))).toBe(false);
});

test("the exact token tripwire suppresses empty-result repair", async () => {
  writeFileSync(log, "");
  const agentId = "test-empty-repair-token-target";
  let continuationCalls = 0;
  const result = await spawnUnderTest({
    prompt: "stop at the token target",
    agentId,
    routingMetadata: presetRequest("integrator"),
    provider: "anthropic",
    pinEvidence: pinEvidence("anthropic"),
    tokenTarget: 15,
    queryFn: (args) => wireTurnSequenceQuery(args, [
      {
        provider: "anthropic", output: "", turns: 1,
        usage: {
          lifetime: {
            inputTokens: 10, outputTokens: 5, cacheReadTokens: 0,
            cacheWriteTokens: 0, reasoningTokens: 0, modelCalls: 1,
          },
          context: { tokens: 15, window: 200_000 },
        },
      },
      { provider: "anthropic", output: "must not run", turns: 1 },
    ], { onContinue: () => { continuationCalls++; } }),
  });

  expect(result).toBe("");
  expect(continuationCalls).toBe(0);
  const replay = await readWireJsonl(join(dir, `agent-${agentId}.stream.jsonl`));
  expect(replay.events.filter((event) => event.kind === "model-call.completed")).toHaveLength(1);
  const lines = await settledRunLines(agentId);
  expect(lines.some((line) => line.endsWith(" process_outcome token_budget_limited"))).toBe(true);
});

test("the absolute hard deadline suppresses repair even when its timer has not fired", async () => {
  writeFileSync(log, "");
  const agentId = "test-empty-repair-hard-deadline";
  let now = new Date("2026-08-12T00:00:00.000Z");
  let scheduledDelay: number | undefined;
  let continuationCalls = 0;
  const result = await spawnUnderTest({
    prompt: "finish at the absolute deadline",
    agentId,
    routingMetadata: presetRequest("integrator"),
    provider: "anthropic",
    pinEvidence: pinEvidence("anthropic"),
    sessionHardCapRuntime: {
      hardCapMs: 10,
      now: () => now,
      schedule: (_callback: () => void, delayMs: number) => {
        scheduledDelay = delayMs;
        return 1;
      },
      cancel: () => {},
    },
    queryFn: (args) => wireTurnSequenceQuery(args, [
      { provider: "anthropic", output: "", turns: 1 },
      { provider: "anthropic", output: "must not run", turns: 1 },
    ], {
      onContinue: () => { continuationCalls++; },
      onInput: (_text, turn) => {
        if (turn === 0) now = new Date("2026-08-12T00:00:00.010Z");
      },
    }),
  });

  expect(result).toBe("");
  expect(scheduledDelay).toBe(10);
  expect(continuationCalls).toBe(0);
  const replay = await readWireJsonl(join(dir, `agent-${agentId}.stream.jsonl`));
  expect(replay.events.filter((event) => event.kind === "model-call.completed")).toHaveLength(1);
  const lines = await settledRunLines(agentId);
  expect(lines.some((line) => line.endsWith(" process_outcome ran_empty"))).toBe(true);
});

test("SIGTERM during outer preflight waits for envelope and driver cleanup", async () => {
  const { dispatch } = await import("./support/dispatch");
  const host = fakeTerminationHost();
  const coordinator = new HostTerminationCoordinator(host.control as any);
  const order: string[] = [];
  let finishEnvelope!: () => void;
  const envelopeGate = new Promise<void>((resolve) => { finishEnvelope = resolve; });
  let finishDriver!: () => void;
  const driverGate = new Promise<void>((resolve) => { finishDriver = resolve; });
  const execution = dispatch("test-signal-preflight-cleanup", {
    agentId: "test-signal-preflight-cleanup-agent",
    routingMetadata: presetRequest("integrator"),
    registerTermination: (options: any) => coordinator.register(options),
    loadThreadFacts: () => [
      { predicate: "title", value: "Signal during outer preflight" },
      { predicate: "planned", value: "true" },
      { predicate: "atomic", value: "true" },
    ],
    loadChildren: () => [],
    claimDriver: (() => ({ release: () => true })) as any,
    admitResourceEnvelope: (async () => {
      order.push("preflight");
      host.emit("SIGTERM");
      return undefined;
    }) as any,
    completeResourceEnvelope: (async () => {
      order.push("envelope:start");
      await envelopeGate;
      order.push("envelope:end");
    }) as any,
    releaseDriver: async () => {
      order.push("driver:start");
      await driverGate;
      order.push("driver:error");
      throw new Error("driver cleanup failed");
    },
  }).catch((error) => error);

  await eventuallyTrue(() => order.includes("envelope:start"), "envelope cleanup start");
  expect(host.exitCodes).toEqual([]);
  finishEnvelope();
  await eventuallyTrue(() => order.includes("driver:start"), "driver cleanup start");
  expect(host.exitCodes).toEqual([]);
  finishDriver();
  const failure = await execution;
  expect(failure).toBeInstanceOf(AggregateError);
  expect((failure as AggregateError).errors.map((error: Error) => error.message)).toEqual([
    "host termination requested (SIGTERM)",
    "driver cleanup failed",
  ]);
  expect(order).toEqual([
    "preflight",
    "envelope:start", "envelope:end",
    "driver:start", "driver:error",
  ]);
  await eventuallyTrue(() => host.exitCodes.length === 1, "coordinated SIGTERM exit");
  expect(host.exitCodes).toEqual([143]);
});

test("an outer cleanup exception still closes query and releases termination ownership", async () => {
  const { spawn } = await import("./support/spawn");
  const host = fakeTerminationHost();
  const coordinator = new HostTerminationCoordinator(host.control as any);
  const order: string[] = [];
  await expect(spawn({
    prompt: "throw before terminal publication",
    agentId: "test-pre-publication-throw",
    routingMetadata: presetRequest("integrator"),
    registerTermination: (options: any) => coordinator.register(options),
    queryFn: () => ({
      close: async () => { order.push("query:closed"); },
      async *[Symbol.asyncIterator]() {},
    }),
    childSettlementReader: () => ({ kind: "settled", children: [] }),
    completeResourceEnvelope: (async () => {
      order.push("envelope:error");
      throw new Error("envelope cleanup failure");
    }) as any,
  })).rejects.toThrow("envelope cleanup failure");
  expect(order).toEqual([
    "query:closed",
    "envelope:error",
  ]);
  expect(host.listenerCount()).toBe(0);
});

test("dispatch wakes its coordinator once, after every terminal publication settles", async () => {
  const { dispatch } = await import("./support/dispatch");
  const scenarios = [
    {
      label: "ran",
      queryFn: (args: RoutedQueryArguments) => wireTurnQuery(args, {
        output: "done", turns: 1, providerDurationMs: 1,
      }),
      processOutcome: "ran",
      deliveryOutcome: "unverified",
      subject: "AGENT COMPLETE",
    },
    {
      label: "blocked-preflight",
      queryFn: () => {
        throw new ProviderRetrySafeError("north_coordination_log_missing");
      },
      processOutcome: "blocked_preflight",
      deliveryOutcome: "blocked",
      subject: "AGENT BLOCKED",
    },
    {
      label: "died",
      queryFn: () => (async function* () {
        throw new Error("provider subprocess died for ordering probe");
      })(),
      processOutcome: "died",
      deliveryOutcome: "blocked",
      subject: "AGENT DEATH",
    },
    {
      label: "turn-cap",
      queryFn: (args: RoutedQueryArguments) => wireTurnQuery(args, {
        output: "partial",
        turns: 2,
        providerDurationMs: 1,
        status: "failed",
        errorCode: "provider_max_turns",
      }),
      processOutcome: "max_turns",
      deliveryOutcome: "blocked",
      subject: "TURN CAP",
    },
    {
      label: "watchdog-aborted",
      queryFn: () => ({
        interrupt: async () => {},
        close: async () => { throw new Error("interrupted provider stream closing"); },
        [Symbol.asyncIterator]() {
          return {
            next: () => new Promise(() => {}),
          };
        },
      }),
      processOutcome: "watchdog_aborted",
      deliveryOutcome: "blocked",
      subject: "AGENT DEATH",
      stallMs: "10",
    },
  ];

  for (const scenario of scenarios) {
    writeFileSync(log, "");
    const agentId = `test-dispatch-notify-${scenario.label}`;
    if ("stallMs" in scenario) process.env.NORTH_STALL_MS = scenario.stallMs;
    try {
      await dispatch(`thread-${agentId}`, {
        agentId,
        routingMetadata: presetRequest("integrator"),
        claimDriver: (() => ({ release() {} })) as any,
        queryFn: scenario.queryFn as any,
        loadThreadFacts: () => [
          { predicate: "title", value: "Prove coordinator terminal notification" },
          { predicate: "planned", value: "true" },
          { predicate: "atomic", value: "true" },
        ],
        loadChildren: () => [],
      });
    } finally {
      delete process.env.NORTH_STALL_MS;
    }

    const output = await waitForLog(
      `${scenario.subject} ${scenario.processOutcome === "died"
        ? "provider subprocess died for ordering probe — "
        : scenario.processOutcome === "max_turns"
          ? "provider_max_turns — partial: partial — "
          : scenario.processOutcome === "watchdog_aborted"
            ? "north_watchdog_execution_inactivity silence_ms=20 last_outer=none last_provider=none — "
          : ""}process=${scenario.processOutcome}`,
    );
    const lines = output.split("\n").filter(Boolean);
    const pings = lines.filter((line) =>
      line.includes(`send ${agentId} ${TEST_COORDINATOR} ${scenario.subject}`)
    );
    expect(pings).toHaveLength(1);
    expect(pings[0]).toEndWith(
      `process=${scenario.processOutcome} — delivery=${scenario.deliveryOutcome} — terminal=recorded — run=recorded`,
    );
    expect(lines.some((line) =>
      line.includes(`send ${agentId} ${TEST_COORDINATOR} AGENT COMPLETE`)
      && scenario.subject !== "AGENT COMPLETE"
    )).toBe(false);
    const terminalIndex = lines.findIndex((line) =>
      line === `tell agent:${agentId} process_outcome ${scenario.processOutcome}`
    );
    const runIndex = lines.findIndex((line) =>
      line.startsWith("run-fact\t") && line.includes(agentId)
    );
    const projection = capturedRunProjections(output).find((candidate) =>
      candidate.agent === agentId);
    const pingIndex = lines.indexOf(pings[0]!);
    expect(terminalIndex).toBeGreaterThanOrEqual(0);
    expect(runIndex).toBeGreaterThanOrEqual(0);
    expect(projection?.facts).toContainEqual(["process_outcome", scenario.processOutcome]);
    expect(terminalIndex).toBeLessThan(pingIndex);
    expect(runIndex).toBeLessThan(pingIndex);
    if (scenario.processOutcome === "blocked_preflight") {
      const registerIndex = lines.findIndex((line) =>
        line.includes(`presence-cli.clj 59999 register ${agentId} `)
      );
      const forgetIndex = lines.findIndex((line) =>
        line.endsWith(
          `presence-cli.clj 59999 forget ${agentId} `
          + `{"resource":"session:${agentId}","holder":"${agentId}","epoch":1}`,
        )
      );
      const releaseIndex = lines.findIndex((line) =>
        line.endsWith(`acquire-cli.clj 59999 release thread-${agentId} ${agentId}`)
      );
      expect(registerIndex).toBeGreaterThanOrEqual(0);
      expect(forgetIndex).toBeGreaterThan(terminalIndex);
      expect(releaseIndex).toBeGreaterThan(terminalIndex);
      expect(forgetIndex).toBeLessThan(pingIndex);
      expect(releaseIndex).toBeLessThan(pingIndex);
    }
    if (scenario.processOutcome === "watchdog_aborted") {
      const diagnosticPings = lines.filter((line) =>
        line.includes(`send ${agentId} ${TEST_COORDINATOR} AGENT STALLED`)
      );
      expect(diagnosticPings).toHaveLength(1);
      expect(lines.indexOf(diagnosticPings[0]!)).toBeLessThan(terminalIndex);
      expect(lines.some((line) =>
        line.includes(`tell run:${agentId}-`)
        && line.endsWith(" watchdog_last_outer_activity none")
      )).toBe(true);
      expect(lines.some((line) =>
        line.includes(`tell run:${agentId}-`)
        && line.endsWith(" watchdog_last_provider_activity none")
      )).toBe(true);
      expect(lines.some((line) => line.includes(" provider_error_detail "))).toBe(false);
      expect(lines.some((line) =>
        line.endsWith(" delivery_reason north_watchdog_execution_inactivity")
      )).toBe(true);
    }
  }
}, 15_000);

test("spawn and dispatch keep a silent outer stream alive from provider-native activity", async () => {
  const { spawn } = await import("./support/spawn");
  const { dispatch } = await import("./support/dispatch");
  const activeNativeQuery = (args: RoutedQueryArguments): WireQuery => {
    const activity = createExecutionActivityEmitter();
    return {
      executionActivity: activity.source,
      close: async () => {},
      [Symbol.asyncIterator](): AsyncIterator<WireEvent> {
        return (async function*(): AsyncGenerator<WireEvent> {
          for (let pulse = 0; pulse < 25; pulse++) {
            await Bun.sleep(10);
            activity.record("provider", "provider.codex.command.interaction");
          }
          yield* wireTurnEvents(args, {
            output: "native activity completed", turns: 1, providerDurationMs: 1,
          });
        })();
      },
    };
  };
  process.env.NORTH_STALL_MS = "100";
  try {
    for (const surface of ["spawn", "dispatch"] as const) {
      writeFileSync(log, "");
      const agentId = `test-native-watchdog-${surface}`;
      if (surface === "spawn") {
        await spawn({
          prompt: "prove native liveness reaches spawn",
          agentId,
          routingMetadata: presetRequest("integrator"),
          queryFn: activeNativeQuery,
          childSettlementReader: () => ({ kind: "settled", children: [] }),
        });
      } else {
        await dispatch(`thread-${agentId}`, {
          agentId,
          routingMetadata: presetRequest("integrator"),
          claimDriver: (() => ({ release() {} })) as any,
          queryFn: activeNativeQuery,
          loadThreadFacts: () => [
            { predicate: "title", value: "Prove native dispatch liveness" },
            { predicate: "planned", value: "true" },
            { predicate: "atomic", value: "true" },
          ],
          loadChildren: () => [],
        });
      }
      const output = await waitForLog(`tell agent:${agentId} process_outcome ran`);
      expect(output).not.toContain(`tell agent:${agentId} stalled`);
      expect(output).not.toContain("watchdog_reason");
      expect(output).not.toContain("provider_process_stalled");
    }
  } finally {
    delete process.env.NORTH_STALL_MS;
  }
}, 15_000);

test("a failed dispatch completion wake-up never replaces the execution outcome", async () => {
  const { dispatch } = await import("./support/dispatch");
  writeFileSync(log, "");
  const agentId = "test-dispatch-notify-failure";
  const result = await dispatch(`thread-${agentId}`, {
    agentId,
    routingMetadata: presetRequest("integrator"),
    claimDriver: (() => ({ release() {} })) as any,
    queryFn: (args) => wireTurnQuery(args, {
      output: "done despite notification failure", turns: 1, providerDurationMs: 1,
    }),
    loadThreadFacts: () => [
      { predicate: "title", value: "Keep notification failure non-fatal" },
      { predicate: "planned", value: "true" },
      { predicate: "atomic", value: "true" },
    ],
    loadChildren: () => [],
  });
  expect(result.result).toBe("done despite notification failure");
  const output = await waitForLog(
    `send ${agentId} ${TEST_COORDINATOR} AGENT COMPLETE`,
  );
  expect(output.match(new RegExp(
    `send ${agentId} ${TEST_COORDINATOR} AGENT COMPLETE`,
    "g",
  ))).toHaveLength(1);
  const lines = await settledRunLines(agentId);
  expect(lines.some((line) => line.endsWith(" process_outcome ran"))).toBe(true);
});

test("a blocked auxiliary terminal writer cannot stack beyond the shared publication budget", async () => {
  const { dispatch } = await import("./support/dispatch");
  process.env.NORTH_TERMINAL_PUBLICATION_BUDGET_MS = "100";
  const runProbe = async (agentId: string) => {
    writeFileSync(log, "");
    const startedAt = performance.now();
    await dispatch(`thread-${agentId}`, {
      agentId,
      routingMetadata: presetRequest("integrator"),
      claimDriver: (() => ({ release() {} })) as any,
      queryFn: () => (async function* () {
        throw new Error("terminal auxiliary budget probe");
      })(),
      loadThreadFacts: () => [
        { predicate: "title", value: "Bound every terminal publication stage" },
        { predicate: "planned", value: "true" },
        { predicate: "atomic", value: "true" },
      ],
      loadChildren: () => [],
    });
    return {
      elapsedMs: performance.now() - startedAt,
      output: readFileSync(log, "utf8"),
    };
  };
  try {
    const control = await runProbe("test-terminal-aux-control");
    const blocked = await runProbe("test-terminal-aux-budget");
    expect(blocked.elapsedMs).toBeLessThan(1_500);
    expect(blocked.elapsedMs - control.elapsedMs).toBeLessThan(350);
    const lines = blocked.output.split("\n").filter(Boolean);
    const terminalIndex = lines.findIndex((line) => line.includes(
      "tell agent:test-terminal-aux-budget process_outcome died",
    ));
    const auxiliaryIndex = lines.findIndex((line) => line.includes(
      "tell @swarm agent_death test-terminal-aux-budget | terminal auxiliary budget probe",
    ));
    expect(terminalIndex).toBeGreaterThanOrEqual(0);
    // The 100ms test budget may expire before the best-effort auxiliary process
    // starts; if it does start, it must remain ordered after the terminal fact.
    if (auxiliaryIndex >= 0) expect(auxiliaryIndex).toBeGreaterThan(terminalIndex);
  } finally {
    delete process.env.NORTH_TERMINAL_PUBLICATION_BUDGET_MS;
  }
});


test("dispatch leaves a committed barless thread alone and still warns on missing judgment_grade", async () => {
  const { dispatch } = await import("./support/dispatch");
  writeFileSync(log, "");
  const captured: string[] = [];
  const originalLog = console.log;
  console.log = (...args: any[]) => { captured.push(args.join(" ")); };
  try {
    await dispatch("@test-warn-thread", {
      agentId: "test-warn-thread-agent",
      routingMetadata: presetRequest("integrator"),
      claimDriver: (() => ({ release() { return true; } })) as any,
      queryFn: () => (async function* () {})() as any,
      loadThreadFacts: () => [
        { predicate: "title", value: "Bar-less grade-less thread" },
        { predicate: "committed", value: "2026-07-20" },
        { predicate: "planned", value: "true" },
        { predicate: "atomic", value: "true" },
      ],
      loadChildren: () => [],
    });
  } finally {
    console.log = originalLog;
  }
  const doneWhenWarn = captured.find((l) => l.includes("has NO done_when"));
  const gradeWarn = captured.find((l) => l.includes("has NO judgment_grade"));
  expect(doneWhenWarn).toBeUndefined();
  expect(gradeWarn).toBeDefined();
  originalLog(`[bar-evidence] ${gradeWarn}`);
});

test("an MCP-preclaimed terminal thread verifies and safely releases before returning", async () => {
  const { dispatch } = await import("./support/dispatch");
  const events: string[] = [];
  const dependencies = {
    agentId: "test-preclaimed-terminal-agent",
    routingMetadata: presetRequest("integrator"),
    driverOptions: { preclaimed: true },
    loadThreadFacts: (threadId: string) => {
      events.push(`facts:${threadId}`);
      return [
        { predicate: "title", value: "Already terminal" },
        { predicate: "outcome", value: "done" },
      ];
    },
    loadChildren: (threadId: string) => {
      events.push(`children:${threadId}`);
      return [];
    },
    claimDriver: ((threadId: string, agentId: string) => {
      events.push(`verify:${threadId}:${agentId}`);
      return {
        release() {
          events.push(`release:${threadId}:${agentId}`);
          return true;
        },
      };
    }) as any,
  };
  const result = await dispatch("@test-preclaimed-terminal", dependencies);
  expect(result).toEqual({
    threadId: "test-preclaimed-terminal",
    posture: "atomic",
    result: "already done",
  });
  expect(events).toEqual([
    "facts:test-preclaimed-terminal",
    "children:test-preclaimed-terminal",
    "verify:test-preclaimed-terminal:test-preclaimed-terminal-agent",
    "release:test-preclaimed-terminal:test-preclaimed-terminal-agent",
  ]);

  let directClaims = 0;
  await dispatch("test-direct-terminal", {
    routingMetadata: presetRequest("integrator"),
    loadThreadFacts: () => [
      { predicate: "title", value: "Direct terminal" },
      { predicate: "outcome", value: "done" },
    ],
    loadChildren: () => [],
    claimDriver: (() => {
      directClaims++;
      return { release: () => true };
    }) as any,
  });
  expect(directClaims).toBe(0);

  await expect(dispatch("test-preclaimed-release-failure", {
    agentId: "test-preclaimed-release-failure-agent",
    routingMetadata: presetRequest("integrator"),
    driverOptions: { preclaimed: true },
    loadThreadFacts: () => [
      { predicate: "title", value: "Terminal with unavailable release" },
      { predicate: "outcome", value: "done" },
    ],
    loadChildren: () => [],
    claimDriver: (() => ({ release: () => false })) as any,
  })).rejects.toMatchObject({
    name: "DispatchDriverReleaseError",
    threadId: "test-preclaimed-release-failure",
    preSideEffect: false,
    retrySafe: false,
  });
});

test("dispatch rejects a worker composite before claims, envelopes, or query construction", async () => {
  const { dispatch } = await import("./support/dispatch");
  const sideEffects: string[] = [];

  await expect(dispatch("test-worker-composite-preflight", {
    agentId: "test-worker-composite-preflight-agent",
    routingMetadata: presetRequest("integrator"),
    loadThreadFacts: () => [
      { predicate: "title", value: "Worker cannot own a composite" },
      { predicate: "committed", value: "2026-07-23" },
      { predicate: "done_when", value: "dispatch rejects before execution" },
    ],
    loadChildren: () => ["test-worker-composite-child"],
    claimDriver: (() => {
      sideEffects.push("claim");
      return { release: () => true };
    }) as any,
    admitResourceEnvelope: (async () => {
      sideEffects.push("envelope");
      return undefined;
    }) as any,
    queryFn: (() => {
      sideEffects.push("query");
      return (async function* () {})();
    }) as any,
  })).rejects.toThrow("managed worker dispatch requires a leaf thread without children");

  expect(sideEffects).toEqual([]);
});

test("dispatch rejects malformed and injection-shaped ids before every read boundary", async () => {
  const { dispatch } = await import("./support/dispatch");
  for (const invalid of [
    "", "@", "@@test-thread", " test-thread", "test-thread;touch-owned",
    "test-thread$(touch-owned)", "test-thread\nother",
  ]) {
    let reads = 0;
    await expect(dispatch(invalid, {
      loadThreadFacts: () => {
        reads++;
        return [{ predicate: "title", value: "must not be read" }];
      },
      loadChildren: () => {
        reads++;
        return [];
      },
    })).rejects.toMatchObject({
      code: "NORTH_INVALID_ENTITY_ID",
      preSideEffect: true,
    });
    expect(reads).toBe(0);
  }
});

test("dispatch publishes newly observed done-bar evidence as reported, never self-verified", async () => {
  const { dispatch } = await import("./support/dispatch");
  writeFileSync(log, "");
  const baseline = [
    { predicate: "title", value: "Proof-carrying delivery" },
    { predicate: "planned", value: "true" },
    { predicate: "atomic", value: "true" },
    { predicate: "done_when", value: "focused tests pass" },
  ];
  let reads = 0;
  let reserved: DeliveryRunContext | undefined;
  await dispatch("test-reported-delivery", {
    agentId: "test-reported-delivery-agent",
    routingMetadata: presetRequest("integrator"),
    claimDriver: (() => ({ release() {} })) as any,
    loadChildren: () => [],
    loadThreadFacts: () => reads++ === 0
      ? baseline
      : [
          ...baseline,
          { predicate: "bar_evidence", value: "focused tests pass → 10/10" },
          { predicate: "outcome", value: "worker also closed the thread" },
        ],
    deliveryRuntime: {
      attemptRoute: attemptRoute("test-reported-delivery"),
      ...attemptTransitions(),
      reserve(context, route) {
        reserved = context;
        return attemptReservation(context, route, ["focused tests pass"]);
      },
      load(runId) {
        if (!reserved || runId !== reserved.runId) {
          return { reservationValid: false, evidence: [] };
        }
        return { reservationValid: true, evidence: [{
          version: RUN_BAR_EVIDENCE_VERSION,
          run: `@${runId}`,
          thread: "@test-reported-delivery",
          reporter: "@agent:test-reported-delivery-agent",
          bar: "focused tests pass",
          observed: "10/10",
          recordedAt: "2026-07-18T10:00:00Z",
        }] };
      },
    },
    queryFn: (args) => {
      expect(reserved?.runId.startsWith("run:test-reported-delivery-agent-")).toBe(true);
      return wireTurnQuery(args, { output: "done", turns: 1, providerDurationMs: 1 });
    },
  });
  const logged = readFileSync(log, "utf8");
  expect(logged).toContain(
    "tell agent:test-reported-delivery-agent delivery_outcome reported",
  );
  expect(logged).toContain(
    "tell agent:test-reported-delivery-agent delivery_reason complete_run_scoped_done_bar_evidence_self_reported",
  );
  expect(logged).not.toContain(
    "tell agent:test-reported-delivery-agent delivery_outcome verified",
  );
  const lines = await settledRunLines(
    "test-reported-delivery-agent",
    "applied_domain_requirement_count 0",
  );
  expect(lines.some((line) => line.endsWith(" delivery_outcome reported"))).toBe(true);
  expect(lines.some((line) => line.includes(" delivery_evidence "))).toBe(true);
});

test("spawn reserves before provider execution and binds evidence plus telemetry to its exact thread", async () => {
  const { spawn } = await import("./support/spawn");
  writeFileSync(log, "");
  const events: string[] = [];
  let reserved: DeliveryRunContext | undefined;
  const result = await spawn({
    prompt: "prove the bound task",
    agentId: "test-proof-bound-spawn",
    routingMetadata: presetRequest("integrator"),
    thread: "test-proof-bound-thread",
    loadThreadFacts: () => {
      events.push("thread-read");
      return [
        { predicate: "title", value: "Proof-bound task" },
        { predicate: "done_when", value: "focused tests pass" },
      ];
    },
    deliveryRuntime: {
      attemptRoute: attemptRoute("test-proof-bound-thread"),
      reserve(context) {
        events.push("reserve");
        reserved = context;
        return {
          contractOrigin: "accepted",
          baselineDoneWhen: ["focused tests pass"],
        };
      },
      load(runId) {
        events.push("evidence-load");
        if (!reserved || runId !== reserved.runId) {
          return { reservationValid: false, evidence: [] };
        }
        return {
          reservationValid: true,
          evidence: [{
            version: RUN_BAR_EVIDENCE_VERSION,
            run: `@${runId}`,
            thread: "@test-proof-bound-thread",
            reporter: "@agent:test-proof-bound-spawn",
            bar: "focused tests pass",
            observed: "28/28 pass",
            recordedAt: "2026-07-19T01:00:00Z",
          }],
        };
      },
    },
    queryFn: (args) => {
      events.push("provider");
      expect(reserved?.threadId).toBe("test-proof-bound-thread");
      return wireTurnQuery(args, {
        output: "evidence recorded", turns: 1, providerDurationMs: 1,
      });
    },
  });
  expect(result).toBe("evidence recorded");
  expect(events.slice(0, 3)).toEqual(["thread-read", "reserve", "provider"]);
  const logged = await waitForLog(
    "tell agent:test-proof-bound-spawn delivery_outcome reported",
  );
  expect(logged).toContain(
    "tell agent:test-proof-bound-spawn delivery_reason complete_run_scoped_done_bar_evidence_self_reported",
  );
  const lines = await settledRunLines("test-proof-bound-spawn");
  expect(lines.some((line) => line.endsWith(" thread @test-proof-bound-thread"))).toBe(true);
  expect(lines.some((line) => line.endsWith(" thread (ad-hoc)"))).toBe(false);
  expect(lines.some((line) => line.endsWith(" delivery_outcome reported"))).toBe(true);
});

test("a spawn orchestrator gets a provider reduction turn after child settlement", async () => {
  const { spawn } = await import("./support/spawn");
  writeFileSync(log, "");
  const settlements = [
    {
      kind: "live",
      children: ["@agent:child-a", "@agent:child-b"],
      live: ["@agent:child-a"],
    },
    { kind: "settled", children: ["@agent:child-a", "@agent:child-b"] },
    { kind: "settled", children: ["@agent:child-a", "@agent:child-b"] },
    { kind: "settled", children: ["@agent:child-a", "@agent:child-b"] },
  ] as const;
  let settlementIndex = 0;
  const seenInputs: string[] = [];
  const queryFn = (args: RoutedQueryArguments): WireQuery => wireTurnSequenceQuery(args, [
    { output: "premature", providerDurationMs: 1, turns: 1 },
    { output: "child terminal observed", providerDurationMs: 2, turns: 2 },
    { output: "reduced", providerDurationMs: 3, turns: 3 },
  ], { onInput: (text) => seenInputs.push(text) });

  const result = await spawn({
    prompt: "coordinate the child",
    agentId: "test-spawn-child-resolves",
          routingMetadata: presetRequest("director"),
    queryFn,
    feedSubscriber: subscribeReadyFeed,
    childSettlementReader: () =>
      settlements[Math.min(settlementIndex++, settlements.length - 1)]!,
  });
  expect(result).toBe("reduced");
  expect(seenInputs[1]).toContain("North refuses orchestrator turn-end");
  expect(seenInputs[2]).toContain("post-settlement reduction turn");
  expect(seenInputs[2]).toContain("@agent:child-a");
  expect(settlementIndex).toBe(4);
  const lines = await settledRunLines("test-spawn-child-resolves");
  expect(lines.some((line) => line.endsWith(" process_outcome ran"))).toBe(true);
});

// Orchestrator continuation race (thread 019f8ec5): a reduction continuation is
// injected at turn-end, but the Anthropic session tears down after its final
// message — the continuation lands on a closing stream and the provider answers
// with a degenerate empty-success terminal. Before the fix decideChildTurnEnd
// read that empty result as a COMPLETED reduction and finalized ran_empty (a
// 0-byte success masquerade); the child results were never reduced. Assert the
// lane now records the explicit obligation-specific blocked outcome instead,
// and NEVER ran_empty / ran.
test("an orchestrator reduction continuation racing a closing provider stream blocks, never ran_empty", async () => {
  const { spawn } = await import("./support/spawn");
  writeFileSync(log, "");
  const settlement = {
    kind: "settled" as const,
    children: ["@agent:child-a", "@agent:child-b"],
  };
  const seenInputs: string[] = [];
  const queryFn = (args: RoutedQueryArguments): WireQuery => wireTurnSequenceQuery(args, [
    { output: "children coordinated", providerDurationMs: 1, turns: 1 },
    { output: "", providerDurationMs: 2, turns: 2 },
  ], { onInput: (text) => seenInputs.push(text) });

  await spawn({
    prompt: "coordinate two children then reduce",
    agentId: "test-spawn-reduction-race",
    routingMetadata: presetRequest("director"),
    coordinator: TEST_COORDINATOR,
    queryFn,
    feedSubscriber: subscribeReadyFeed,
    childSettlementReader: () => settlement,
  });

  expect(seenInputs[1]).toContain("post-settlement reduction turn");
  const lines = await settledRunLines("test-spawn-reduction-race");
  expect(lines.some((line) =>
    line.endsWith(" process_outcome orchestrator_reduction_incomplete"),
  )).toBe(true);
  expect(lines.some((line) => line.endsWith(" process_outcome ran"))).toBe(false);
  expect(lines.some((line) => line.endsWith(" process_outcome ran_empty"))).toBe(false);
  const logged = readFileSync(log, "utf8").split("\n").filter(Boolean);
  expect(logged.some((line) =>
    line === "tell agent:test-spawn-reduction-race process_outcome orchestrator_reduction_incomplete",
  )).toBe(true);
  expect(logged.some((line) =>
    line.includes(`send test-spawn-reduction-race ${TEST_COORDINATOR} AGENT COMPLETE`),
  )).toBe(false);
});

// A streaming provider owns private continuation identity. The outer runtime
// consumes each per-turn iterator completely, then asks the same query for the
// next semantic turn and creates a fresh iterator. Both model terminals remain
// in one canonical wire run; no provider session identifier crosses the seam.
test("a spawn orchestrator consumes two model terminals from one streaming query across continuation", async () => {
  const { spawn } = await import("./support/spawn");
  writeFileSync(log, "");
  const settlement = {
    kind: "settled" as const,
    children: ["@agent:child-a", "@agent:child-b"],
  };
  const seenInputs: string[] = [];
  let queryConstructions = 0;
  let iteratorCount = 0;
  let continuationCalls = 0;
  let streaming = false;
  let continuedInput: WireQueryInput | undefined;
  const inputText = async (input: WireQueryInput): Promise<string> => {
    if (typeof input === "string") return input;
    const message = await input[Symbol.asyncIterator]().next();
    if (message.done) throw new Error("streaming query input ended before a user message");
    return message.value.text;
  };
  const queryFn = (args: RoutedQueryArguments): WireQuery => {
    queryConstructions++;
    return {
      async continueTurn(input: WireQueryInput): Promise<void> {
        if (streaming) throw new Error("continueTurn called before the current iterator completed");
        if (continuedInput !== undefined) throw new Error("continuation input was not consumed");
        continuationCalls++;
        continuedInput = input;
      },
      async close(): Promise<void> {},
      [Symbol.asyncIterator](): AsyncIterator<WireEvent> {
        return (async function*(): AsyncGenerator<WireEvent> {
          if (streaming) throw new Error("streaming query iterators overlapped");
          streaming = true;
          try {
            iteratorCount++;
            const turn = iteratorCount;
            const input = turn === 1 ? args.input : continuedInput;
            if (input === undefined) throw new Error("continuation iterator opened without input");
            continuedInput = undefined;
            seenInputs.push(await inputText(input));

            const modelCallId = wireModelCallId(`model-call:spawn-continuation:${turn}`);
            const messageId = wireMessageId(`message:spawn-continuation:${turn}`);
            yield args.writer.append({
              kind: "model-call.started",
              modelCallId,
              model: { provider: "anthropic", tier: "frontier", capabilityClass: "orchestrator" },
              effort: "high",
              attempt: turn,
            });
            yield args.writer.append({
              kind: "message.recorded",
              messageId,
              modelCallId,
              stage: "started",
              role: "assistant",
            });
            yield args.writer.append({
              kind: "message.recorded",
              messageId,
              modelCallId,
              stage: "completed",
              role: "assistant",
              content: turn === 1 ? "children coordinated" : "reduced",
            });
            yield args.writer.append({
              kind: "model-call.completed",
              modelCallId,
              status: "succeeded",
              origin: "provider",
              usage: {
                lifetime: {
                  inputTokens: 10 * turn,
                  outputTokens: 5 * turn,
                  cacheReadTokens: 0,
                  cacheWriteTokens: 0,
                  reasoningTokens: 0,
                  modelCalls: turn,
                },
                context: { tokens: 10 * turn, window: 200_000 },
              },
              usageCoverage: "exact",
              evidence: {
                turns: { unit: "assistant-turn", count: 1, comparable: true },
              },
            });
          } finally {
            streaming = false;
          }
        })();
      },
    };
  };

  const result = await spawn({
    prompt: "coordinate two children then reduce",
    agentId: "test-spawn-resume-reduction",
    routingMetadata: presetRequest("director"),
    provider: "anthropic",
    pinEvidence: pinEvidence("anthropic"),
    coordinator: TEST_COORDINATOR,
    queryFn,
    childSettlementReader: () => settlement,
  });

  expect(result).toBe("reduced");
  expect(queryConstructions).toBe(1);
  expect(iteratorCount).toBe(2);
  expect(continuationCalls).toBe(1);
  expect(seenInputs[0]).toBe("coordinate two children then reduce");
  expect(seenInputs[1]).toContain("post-settlement reduction turn");

  const replay = await readWireJsonl(
    join(dir, "agent-test-spawn-resume-reduction.stream.jsonl"),
  );
  expect(replay.events.filter((event) => event.kind === "run.started")).toHaveLength(1);
  expect(replay.events.filter((event) => event.kind === "model-call.completed")).toHaveLength(2);
  expect(replay.events.filter((event) => event.kind === "run.terminated")).toHaveLength(1);
  expect(replay.events.at(-1)).toMatchObject({
    kind: "run.terminated",
    lifecycle: "completed",
    reason: { code: "completed" },
  });
});

test("the managed token tripwire publishes once before an orchestrator continuation", async () => {
  writeFileSync(log, "");
  let queryConstructions = 0;
  let iteratorCount = 0;
  let continuationCalls = 0;
  const queryFn = (args: RoutedQueryArguments): WireQuery => {
    queryConstructions++;
    return {
      async continueTurn(): Promise<void> { continuationCalls++; },
      async close(): Promise<void> {},
      [Symbol.asyncIterator](): AsyncIterator<WireEvent> {
        iteratorCount++;
        return wireTurnEvents(args, {
          output: "children coordinated",
          provider: "anthropic",
          turns: 1,
          usage: {
            lifetime: {
              inputTokens: 10,
              outputTokens: 5,
              cacheReadTokens: 0,
              cacheWriteTokens: 0,
              reasoningTokens: 0,
              modelCalls: 1,
            },
            context: { tokens: 15, window: 200_000 },
          },
        });
      },
    };
  };

  const result = await spawnUnderTest({
    prompt: "coordinate then reduce",
    agentId: "test-spawn-token-budget",
    routingMetadata: presetRequest("director"),
    provider: "anthropic",
    pinEvidence: pinEvidence("anthropic"),
    coordinator: TEST_COORDINATOR,
    tokenTarget: 15,
    queryFn,
    childSettlementReader: () => ({
      kind: "settled" as const,
      children: ["@agent:child-a", "@agent:child-b"],
    }),
  });

  expect(result).toBe("children coordinated");
  expect(queryConstructions).toBe(1);
  expect(iteratorCount).toBe(1);
  expect(continuationCalls).toBe(0);

  const replay = await readWireJsonl(
    join(dir, "agent-test-spawn-token-budget.stream.jsonl"),
  );
  expect(replay.events.filter((event) => event.kind === "model-call.completed")).toHaveLength(1);
  expect(replay.events.filter((event) => event.kind === "run.terminated")).toHaveLength(1);
  expect(replay.events.at(-1)).toMatchObject({
    kind: "run.terminated",
    lifecycle: "blocked",
    reason: { code: "blocked", detail: "blocked" },
  });

  const lines = await settledRunLines("test-spawn-token-budget");
  for (const fact of [
    "process_outcome token_budget_limited",
    "run_token_target 15",
    "run_token_budget_status budget_limited",
    "run_token_budget_coverage exact",
    "run_token_observed 15",
    "run_token_overshoot 0",
    'run_token_budget_handoff {"reason":"managed_run_token_budget_limited","target":15,"observed":15,"overshoot":0,"coverage":"exact"}',
  ]) {
    expect(lines.some((line) => line.endsWith(` ${fact}`)), fact).toBe(true);
  }
  const notifications = readFileSync(log, "utf8").split("\n")
    .filter((line) => line.includes("TOKEN TARGET"));
  expect(notifications).toHaveLength(1);
  expect(notifications[0]).toContain(
    '{"reason":"managed_run_token_budget_limited","target":15,"observed":15,"overshoot":0,"coverage":"exact"}',
  );
});

test("one late turn-messages follow-up runs exactly once on the retained provider query", async () => {
  const priorStaffingSource = process.env.NORTH_STAFFING_SOURCE;
  process.env.NORTH_STAFFING_SOURCE = "file";
  try {
    writeFileSync(log, "");
    const seenInputs: string[] = [];
    const ownership: string[] = [];
    let queryConstructions = 0;
    let feedConstructions = 0;
    let replayCalls = 0;
    let providerAcks = 0;
    let rejectedExtras = 0;

    const queryFn = (args: RoutedQueryArguments): WireQuery => {
      queryConstructions++;
      const query = wireTurnSequenceQuery(args, [
        { output: "first terminal", provider: "openai", turns: 1 },
        { output: "follow-up answered", provider: "openai", turns: 2 },
      ], {
        onInput: (text, turn) => {
          if (turn === 1) ownership.push(`provider-dequeue:${providerAcks}`);
          seenInputs.push(text);
        },
      });
      return Object.assign(query, {
        executionTransport: "managed-app-server" as const,
        close: async () => {},
      });
    };

    const feedSubscriber = (
      _agentId: string,
      onMail: (message: string) => InputAdmission,
      runtime?: { deferredStart?: boolean },
    ): FeedSubscription => {
      feedConstructions++;
      expect(runtime).toMatchObject({ deferredStart: true });
      const caughtUp = Promise.withResolvers<void>();
      let first: InputAdmission | undefined;
      const stop = async () => {
        first?.cancel();
        caughtUp.resolve();
      };
      return Object.assign(stop, {
        ready: Promise.resolve(),
        caughtUp: caughtUp.promise,
        replay: () => {
          replayCalls++;
          ownership.push("terminal-replay");
          first = onMail("late follow-up from coordinator");
          void first.consumed.then((consumed) => {
            if (consumed) {
              providerAcks++;
              ownership.push("provider-ack");
              caughtUp.resolve();
            }
          });
          const extra = onMail("second follow-up must remain replayable");
          void extra.consumed.then((consumed) => {
            if (!consumed) rejectedExtras++;
          });
          return caughtUp.promise;
        },
        drain: async () => {},
        isArmed: () => true,
      });
    };

    const result = await spawnUnderTest({
      prompt: "answer, then accept one late follow-up",
      agentId: "test-turn-messages-follow-up",
      routingMetadata: presetRequest("integrator"),
      provider: "openai",
      pinEvidence: pinEvidence("openai"),
      coordinator: TEST_COORDINATOR,
      queryFn,
      feedSubscriber,
    });

    expect(result).toBe("follow-up answered");
    expect(queryConstructions).toBe(1);
    expect(feedConstructions).toBe(1);
    expect(replayCalls).toBe(1);
    expect(providerAcks).toBe(1);
    expect(rejectedExtras).toBe(1);
    expect(seenInputs).toEqual([
      "answer, then accept one late follow-up",
      "late follow-up from coordinator",
    ]);
    expect(ownership).toEqual([
      "terminal-replay",
      "provider-ack",
      "provider-dequeue:1",
    ]);

    const replay = await readWireJsonl(
      join(dir, "agent-test-turn-messages-follow-up.stream.jsonl"),
    );
    expect(replay.events.filter((event) => event.kind === "model-call.completed"))
      .toHaveLength(2);
  } finally {
    if (priorStaffingSource === undefined) delete process.env.NORTH_STAFFING_SOURCE;
    else process.env.NORTH_STAFFING_SOURCE = priorStaffingSource;
  }
});

test("dispatch uses the same token tripwire before its orchestrator continuation", async () => {
  writeFileSync(log, "");
  let queryConstructions = 0;
  let iteratorCount = 0;
  let continuationCalls = 0;
  const queryFn = (args: RoutedQueryArguments): WireQuery => {
    queryConstructions++;
    return {
      async continueTurn(): Promise<void> { continuationCalls++; },
      async close(): Promise<void> {},
      [Symbol.asyncIterator](): AsyncIterator<WireEvent> {
        iteratorCount++;
        return wireTurnEvents(args, {
          output: "children coordinated",
          turns: 1,
          usage: {
            lifetime: {
              inputTokens: 10,
              outputTokens: 5,
              cacheReadTokens: 0,
              cacheWriteTokens: 0,
              reasoningTokens: 0,
              modelCalls: 1,
            },
            context: { tokens: 15, window: 200_000 },
          },
        });
      },
    };
  };

  const result = await dispatchUnderTest("test-dispatch-token-budget-thread", {
    agentId: "test-dispatch-token-budget",
    routingMetadata: presetRequest("director"),
    tokenTarget: 15,
    queryFn,
    claimDriver: () => ({ release: () => true }),
    loadThreadFacts: () => [
      { predicate: "title", value: "Coordinate then reduce" },
      { predicate: "planned", value: "true" },
    ],
    loadChildren: () => ["@agent:child-a", "@agent:child-b"],
    childSettlementReader: () => ({
      kind: "settled" as const,
      children: ["@agent:child-a", "@agent:child-b"],
    }),
  });

  expect(result.result).toBe("children coordinated");
  expect(queryConstructions).toBe(1);
  expect(iteratorCount).toBe(1);
  expect(continuationCalls).toBe(0);

  const replay = await readWireJsonl(
    join(dir, "agent-test-dispatch-token-budget.stream.jsonl"),
  );
  expect(replay.events.filter((event) => event.kind === "model-call.completed")).toHaveLength(1);
  expect(replay.events.filter((event) => event.kind === "run.terminated")).toHaveLength(1);
  expect(replay.events.at(-1)).toMatchObject({
    kind: "run.terminated",
    lifecycle: "blocked",
    reason: { code: "blocked", detail: "blocked" },
  });

  const lines = await settledRunLines("test-dispatch-token-budget");
  expect(lines.some((line) => line.endsWith(" process_outcome token_budget_limited"))).toBe(true);
  expect(lines.some((line) => line.endsWith(" run_token_budget_coverage exact"))).toBe(true);
  const notifications = readFileSync(log, "utf8").split("\n")
    .filter((line) => line.includes("TOKEN TARGET"));
  expect(notifications).toHaveLength(1);
  expect(notifications[0]).toContain(
    '{"reason":"managed_run_token_budget_limited","target":15,"observed":15,"overshoot":0,"coverage":"exact"}',
  );
});

test("provider preaccept causes stay out of public spawn and dispatch wire terminals", async () => {
  writeFileSync(log, "");
  const canary = "RAW_PROVIDER_CAUSE_CANARY_64bde4b8";
  const failure = () => new ProviderRetrySafeError(
    "provider preaccept refused",
    { cause: new Error(canary) },
  );
  const spawnAgent = "test-spawn-preflight-redaction";
  const dispatchAgent = "test-dispatch-preflight-redaction";

  await spawnUnderTest({
    prompt: "prove spawn public terminal redaction",
    agentId: spawnAgent,
    routingMetadata: presetRequest("integrator"),
    queryFn: () => { throw failure(); },
  });
  await dispatchUnderTest("test-dispatch-preflight-redaction-thread", {
    agentId: dispatchAgent,
    routingMetadata: presetRequest("integrator"),
    claimDriver: () => ({ release: () => true }),
    queryFn: () => { throw failure(); },
    loadThreadFacts: () => [
      { predicate: "title", value: "Prove dispatch public terminal redaction" },
      { predicate: "planned", value: "true" },
      { predicate: "atomic", value: "true" },
    ],
    loadChildren: () => [],
  });

  for (const agentId of [spawnAgent, dispatchAgent]) {
    const replay = await readWireJsonl(join(dir, `agent-${agentId}.stream.jsonl`));
    expect(JSON.stringify(replay.events)).not.toContain(canary);
    expect(replay.events.at(-1)).toMatchObject({
      kind: "run.terminated",
      lifecycle: "blocked",
      reason: {
        code: "blocked",
        detail: "blocked",
      },
    });
  }
});

test("spawn and dispatch reject altered clones and replay only writer-owned events", async () => {
  writeFileSync(log, "");
  const forgedContent = "FORGED_PROVIDER_CONTENT_129f442d";
  const canonicalEvents = new Map<string, WireEvent>();
  const alteredCloneQuery = (label: string) => (args: RoutedQueryArguments): WireQuery => ({
    async *[Symbol.asyncIterator](): AsyncGenerator<WireEvent> {
      const modelCallId = wireModelCallId(`model-call:canonical-auth:${label}`);
      const messageId = wireMessageId(`message:canonical-auth:${label}`);
      yield args.writer.append({
        kind: "model-call.started",
        modelCallId,
        model: { provider: "anthropic", tier: "senior", capabilityClass: "authoring" },
        effort: "high",
        attempt: 1,
      });
      yield args.writer.append({
        kind: "message.recorded",
        messageId,
        modelCallId,
        stage: "started",
        role: "assistant",
      });
      const canonical = args.writer.append({
        kind: "message.recorded",
        messageId,
        modelCallId,
        stage: "completed",
        role: "assistant",
        content: `writer-owned-${label}`,
      });
      canonicalEvents.set(label, canonical);
      yield decodeWireEvent({ ...canonical, content: forgedContent });
    },
  });
  const spawnAgent = "test-spawn-canonical-event-auth";
  const dispatchAgent = "test-dispatch-canonical-event-auth";

  const spawnResult = await spawnUnderTest({
    prompt: "reject a forged spawn event",
    agentId: spawnAgent,
    routingMetadata: presetRequest("integrator"),
    provider: "anthropic",
    pinEvidence: pinEvidence("anthropic"),
    queryFn: alteredCloneQuery("spawn"),
  });
  expect(spawnResult).toBe("");
  await dispatchUnderTest("test-dispatch-canonical-event-auth-thread", {
    agentId: dispatchAgent,
    routingMetadata: presetRequest("integrator"),
    claimDriver: () => ({ release: () => true }),
    queryFn: alteredCloneQuery("dispatch"),
    loadThreadFacts: () => [
      { predicate: "title", value: "Reject a forged dispatch event" },
      { predicate: "planned", value: "true" },
      { predicate: "atomic", value: "true" },
    ],
    loadChildren: () => [],
  });

  for (const [label, agentId] of [
    ["spawn", spawnAgent],
    ["dispatch", dispatchAgent],
  ] as const) {
    const replay = await readWireJsonl(join(dir, `agent-${agentId}.stream.jsonl`));
    const canonical = canonicalEvents.get(label)!;
    expect(replay.events.find((event) => event.id === canonical.id)).toEqual(canonical);
    expect(JSON.stringify(replay.events)).not.toContain(forgedContent);
    expect(replay.events.at(-1)).toMatchObject({
      kind: "run.terminated",
      lifecycle: "failed",
      reason: {
        code: "provider_process_died",
        detail: "provider_process_died",
      },
    });
  }
});

// Resume is not a success guarantee (thread 019f8ec5): if the resumed reduction
// turn itself comes back with a degenerate empty terminal, the obligation is
// still unmet. The lane must record the obligation-specific blocked outcome,
// never a ran_empty masquerade and never a false AGENT COMPLETE.
test("a spawn orchestrator whose resumed reduction turn returns empty records the blocked outcome, never ran_empty", async () => {
  const { spawn } = await import("./support/spawn");
  writeFileSync(log, "");
  const settlement = {
    kind: "settled" as const,
    children: ["@agent:child-a", "@agent:child-b"],
  };
  let queryConstructions = 0;
  let continuationCalls = 0;
  const seenInputs: string[] = [];
  const queryFn = (args: RoutedQueryArguments): WireQuery => {
    queryConstructions++;
    return wireTurnSequenceQuery(args, [
      { output: "children coordinated", providerDurationMs: 1, turns: 1 },
      { output: "", providerDurationMs: 2, turns: 1 },
    ], {
      onContinue: () => { continuationCalls++; },
      onInput: (text) => seenInputs.push(text),
    });
  };

  await spawn({
    prompt: "coordinate two children then reduce",
    agentId: "test-spawn-resume-reduction-empty",
    routingMetadata: presetRequest("director"),
    provider: "anthropic",
    pinEvidence: pinEvidence("anthropic"),
    coordinator: TEST_COORDINATOR,
    queryFn,
    childSettlementReader: () => settlement,
  });

  expect(queryConstructions).toBe(1);
  expect(continuationCalls).toBe(1);
  expect(seenInputs[1]).toContain("post-settlement reduction turn");
  const lines = await settledRunLines("test-spawn-resume-reduction-empty");
  expect(lines.some((line) =>
    line.endsWith(" process_outcome orchestrator_reduction_incomplete"),
  )).toBe(true);
  expect(lines.some((line) => line.endsWith(" process_outcome ran"))).toBe(false);
  expect(lines.some((line) => line.endsWith(" process_outcome ran_empty"))).toBe(false);
  const logged = readFileSync(log, "utf8").split("\n").filter(Boolean);
  expect(logged.some((line) =>
    line.includes(`send test-spawn-resume-reduction-empty ${TEST_COORDINATOR} AGENT COMPLETE`),
  )).toBe(false);
});

// Recursive child admission across a resumed continuation turn (thread
// 019f8ec5): mcp__north__spawn's bind-child-thread validates the parent context
// from NORTH_RUN_ID/NORTH_THREAD_ID/NORTH_RUN_CAPABILITY on the orchestrator's
// provider process. Those env vars are set ONCE on the harness options
// (deliveryRun) and the resumed turn re-invokes the SAME options object — so a
// resume can never strip the parent context. A threaded orchestrator therefore
// keeps a valid parent reservation on turn 1 AND on its resumed reduction turn.
test("a resumed continuation turn carries the same parent-run context for recursive child admission", async () => {
  const { spawn } = await import("./support/spawn");
  writeFileSync(log, "");
  const settlement = {
    kind: "settled" as const,
    children: ["@agent:child-a", "@agent:child-b"],
  };
  const optionsPerTurn: any[] = [];
  let queryConstructions = 0;
  const queryFn = (args: RoutedQueryArguments): WireQuery => {
    queryConstructions++;
    return wireTurnSequenceQuery(args, [
      { output: "children coordinated", providerDurationMs: 1, turns: 1 },
      { output: "reduced", providerDurationMs: 2, turns: 1 },
    ], { onInput: () => optionsPerTurn.push(args.options) });
  };

  await spawn({
    prompt: "coordinate two children then reduce",
    agentId: "test-spawn-resume-parent-context",
    thread: "2026-07-23-101500",
    routingMetadata: presetRequest("director"),
    provider: "anthropic",
    pinEvidence: pinEvidence("anthropic"),
    coordinator: TEST_COORDINATOR,
    queryFn,
    deliveryRuntime: {
      attemptRoute: attemptRoute("2026-07-23-101500", "anthropic"),
      reserve: () => ({}), load: () => ({}),
    },
    childSettlementReader: () => settlement,
  });

  expect(queryConstructions).toBe(1);
  expect(optionsPerTurn).toHaveLength(2);
  const turn1Env = optionsPerTurn[0].env;
  const resumedEnv = optionsPerTurn[1].env;
  // Parent context present on turn 1 (bind-child-thread reads exactly these).
  expect(turn1Env.NORTH_THREAD_ID).toBe("2026-07-23-101500");
  expect(typeof turn1Env.NORTH_RUN_ID).toBe("string");
  expect(turn1Env.NORTH_RUN_ID.length).toBeGreaterThan(0);
  expect(turn1Env.NORTH_RUN_CAPABILITY).toMatch(/^[0-9a-f]{64}$/);
  // The resumed turn reuses the identical options+env — parent context is byte-
  // identical, so recursive mcp__north__spawn admits children from the resume.
  expect(optionsPerTurn[1]).toBe(optionsPerTurn[0]);
  expect(resumedEnv.NORTH_RUN_ID).toBe(turn1Env.NORTH_RUN_ID);
  expect(resumedEnv.NORTH_THREAD_ID).toBe(turn1Env.NORTH_THREAD_ID);
  expect(resumedEnv.NORTH_RUN_CAPABILITY).toBe(turn1Env.NORTH_RUN_CAPABILITY);
});

test("spawn and dispatch force a zero-child director to dispatch two children before reduction", async () => {
  const { spawn } = await import("./support/spawn");
  const { dispatch } = await import("./support/dispatch");

  for (const surface of ["spawn", "dispatch"] as const) {
    writeFileSync(log, "");
    const agentId = `test-${surface}-director-child-obligation`;
    const settlements = [
      { kind: "settled" as const, children: [] },
      {
        kind: "settled" as const,
        children: ["@agent:child-a", "@agent:child-b"],
      },
      {
        kind: "settled" as const,
        children: ["@agent:child-a", "@agent:child-b"],
      },
      {
        kind: "settled" as const,
        children: ["@agent:child-a", "@agent:child-b"],
      },
    ];
    let reads = 0;
    const seenInputs: string[] = [];
    const queryFn = (args: RoutedQueryArguments): WireQuery => wireTurnSequenceQuery(
      args,
      Array.from({ length: 3 }, (_, index) => ({
        output: `provider-result-${index + 1}`,
        providerDurationMs: index + 1,
        turns: index + 1,
      })),
      { onInput: (text) => seenInputs.push(text) },
    );
    const childSettlementReader = () =>
      settlements[Math.min(reads++, settlements.length - 1)]!;

    if (surface === "spawn") {
      await spawn({
        prompt: "coordinate two independent children",
        agentId,
        routingMetadata: presetRequest("director"),
        queryFn,
        feedSubscriber: subscribeReadyFeed,
        childSettlementReader,
      });
    } else {
      await dispatch(`thread-${agentId}`, {
        agentId,
        routingMetadata: presetRequest("director"),
        claimDriver: (() => ({ release() {} })) as any,
        queryFn,
        feedSubscriber: subscribeReadyFeed,
        loadThreadFacts: () => [
          { predicate: "title", value: "Coordinate two independent children" },
          { predicate: "planned", value: "true" },
          { predicate: "atomic", value: "true" },
        ],
        loadChildren: () => [],
        childSettlementReader,
      });
    }

    expect(reads).toBe(4);
    expect(seenInputs[1]).toContain("direct-child obligation is unmet (0/2 observed)");
    expect(seenInputs[2]).toContain("post-settlement reduction turn");
    const lines = await settledRunLines(
      agentId,
      surface === "dispatch" ? "applied_domain_requirement_count 0" : "error_count 0",
    );
    expect(lines.some((line) => line.endsWith(" process_outcome ran"))).toBe(true);
  }
}, 15_000);

test("a spawn orchestrator hits a bounded no-progress cap as incomplete, never ran", async () => {
  const { spawn } = await import("./support/spawn");
  writeFileSync(log, "");
  const previousCap = process.env.NORTH_BG_MAX_CONTINUATIONS;
  process.env.NORTH_BG_MAX_CONTINUATIONS = "1";
  try {
    const queryFn = (args: RoutedQueryArguments): WireQuery => wireTurnSequenceQuery(args, [
      { output: "first early exit", providerDurationMs: 1, turns: 1 },
      { output: "second early exit", providerDurationMs: 2, turns: 2 },
    ]);
    await spawn({
      prompt: "coordinate a stuck child",
      agentId: "test-spawn-child-cap",
          routingMetadata: presetRequest("director"),
      coordinator: TEST_COORDINATOR,
      queryFn,
      feedSubscriber: subscribeReadyFeed,
      childSettlementReader: () => ({
        kind: "live", children: ["@agent:stuck-child"], live: ["@agent:stuck-child"],
      }),
    });
    const lines = await settledRunLines("test-spawn-child-cap");
    expect(lines.some((line) =>
      line.endsWith(" process_outcome orchestrator_children_incomplete"),
    )).toBe(true);
    expect(lines.some((line) => line.endsWith(" process_outcome ran"))).toBe(false);
    const logged = readFileSync(log, "utf8").split("\n").filter(Boolean);
    const pings = logged.filter((line) =>
      line.includes(
        `send test-spawn-child-cap ${TEST_COORDINATOR} EARLY EXIT WITH LIVE CHILDREN`,
      )
    );
    expect(pings).toHaveLength(1);
    expect(logged.some((line) =>
      line.includes(`send test-spawn-child-cap ${TEST_COORDINATOR} AGENT COMPLETE`)
    )).toBe(false);
    const terminalIndex = logged.indexOf(
      "tell agent:test-spawn-child-cap process_outcome orchestrator_children_incomplete",
    );
    const runIndex = logged.findIndex((line) =>
      line.includes("tell run:test-spawn-child-cap-")
      && line.endsWith(" process_outcome orchestrator_children_incomplete")
    );
    const pingIndex = logged.indexOf(pings[0]!);
    expect(terminalIndex).toBeLessThan(pingIndex);
    expect(runIndex).toBeLessThan(pingIndex);
  } finally {
    if (previousCap === undefined) delete process.env.NORTH_BG_MAX_CONTINUATIONS;
    else process.env.NORTH_BG_MAX_CONTINUATIONS = previousCap;
  }
});

test("spawn and dispatch require reduction for first-seen and changed settled child sets", async () => {
  const { spawn } = await import("./support/spawn");
  const { dispatch } = await import("./support/dispatch");
  const scenarios = [
    {
      label: "already-settled",
      settlements: [
        {
          kind: "settled" as const,
          children: ["@agent:child-a", "@agent:child-b"],
        },
        {
          kind: "settled" as const,
          children: ["@agent:child-a", "@agent:child-b"],
        },
        {
          kind: "settled" as const,
          children: ["@agent:child-a", "@agent:child-b"],
        },
      ],
      providerResults: 2,
      reductionTurns: 1,
    },
    {
      label: "settled-set-changed",
      settlements: [
        { kind: "settled" as const, children: ["@agent:child-a", "@agent:child-b"] },
        {
          kind: "settled" as const,
          children: ["@agent:child-a", "@agent:child-b", "@agent:child-c"],
        },
        {
          kind: "settled" as const,
          children: ["@agent:child-a", "@agent:child-b", "@agent:child-c"],
        },
        {
          kind: "settled" as const,
          children: ["@agent:child-a", "@agent:child-b", "@agent:child-c"],
        },
      ],
      providerResults: 3,
      reductionTurns: 2,
    },
  ];

  for (const surface of ["spawn", "dispatch"] as const) {
    for (const scenario of scenarios) {
      writeFileSync(log, "");
      const agentId = `test-${surface}-${scenario.label}`;
      let reads = 0;
      const seenInputs: string[] = [];
      const queryFn = (args: RoutedQueryArguments): WireQuery => wireTurnSequenceQuery(
        args,
        Array.from({ length: scenario.providerResults }, (_, index) => ({
          output: `provider-result-${index + 1}`,
          providerDurationMs: index + 1,
          turns: index + 1,
        })),
        { onInput: (text) => seenInputs.push(text) },
      );
      const childSettlementReader = () =>
        scenario.settlements[Math.min(reads++, scenario.settlements.length - 1)]!;

      if (surface === "spawn") {
        await spawn({
          prompt: "reduce settled child results",
          agentId,
          routingMetadata: presetRequest("director"),
          queryFn,
          feedSubscriber: subscribeReadyFeed,
          childSettlementReader,
        });
      } else {
        await dispatch(`thread-${agentId}`, {
          agentId,
          routingMetadata: presetRequest("director"),
          claimDriver: (() => ({ release() {} })) as any,
          queryFn,
          feedSubscriber: subscribeReadyFeed,
          loadThreadFacts: () => [
            { predicate: "title", value: "Reduce settled child results" },
            { predicate: "planned", value: "true" },
            { predicate: "atomic", value: "true" },
          ],
          loadChildren: () => [],
          childSettlementReader,
        });
      }

      expect(reads).toBe(scenario.settlements.length);
      const reductionInputs = seenInputs.filter((input) =>
        input.includes("post-settlement reduction turn")
      );
      expect(reductionInputs).toHaveLength(scenario.reductionTurns);
      expect(reductionInputs[0]).toContain("@agent:child-a");
      if (scenario.label === "settled-set-changed") {
        expect(reductionInputs[1]).toContain("@agent:child-c");
      }
      const lines = await settledRunLines(
        agentId,
        surface === "dispatch" ? "applied_domain_requirement_count 0" : "error_count 0",
      );
      expect(lines.some((line) => line.endsWith(" process_outcome ran"))).toBe(true);
    }
  }
}, 15_000);

test("spawn and dispatch block a previously live child disappearing from the graph", async () => {
  const { spawn } = await import("./support/spawn");
  const { dispatch } = await import("./support/dispatch");

  for (const surface of ["spawn", "dispatch"] as const) {
    writeFileSync(log, "");
    const agentId = `test-${surface}-child-set-shrink`;
    const settlements = [
      {
        kind: "live" as const,
        children: ["@agent:child-a"],
        live: ["@agent:child-a"],
      },
      { kind: "settled" as const, children: [] },
      { kind: "settled" as const, children: [] },
    ];
    let reads = 0;
    const queryFn = (args: RoutedQueryArguments): WireQuery => wireTurnSequenceQuery(args, [
      { output: "provider-result-1", providerDurationMs: 1, turns: 1 },
      { output: "provider-result-2", providerDurationMs: 2, turns: 2 },
    ]);
    const childSettlementReader = () =>
      settlements[Math.min(reads++, settlements.length - 1)]!;

    if (surface === "spawn") {
      await spawn({
        prompt: "observe a live child before its graph edge disappears",
        agentId,
          routingMetadata: presetRequest("director"),
        queryFn,
        feedSubscriber: subscribeReadyFeed,
        childSettlementReader,
      });
    } else {
      await dispatch(`thread-${agentId}`, {
        agentId,
        routingMetadata: presetRequest("director"),
        claimDriver: (() => ({ release() {} })) as any,
        queryFn,
        feedSubscriber: subscribeReadyFeed,
        loadThreadFacts: () => [
          { predicate: "title", value: "Observe a disappearing child" },
          { predicate: "planned", value: "true" },
          { predicate: "atomic", value: "true" },
        ],
        loadChildren: () => [],
        childSettlementReader,
      });
    }

    expect(reads).toBe(3);
    const lines = await settledRunLines(
      agentId,
      surface === "dispatch" ? "applied_domain_requirement_count 0" : "error_count 0",
    );
    expect(lines.some((line) =>
      line.endsWith(" process_outcome orchestrator_child_set_inconsistent"),
    )).toBe(true);
    expect(lines.some((line) =>
      line.endsWith(" delivery_reason orchestrator_child_relation_regressed"),
    )).toBe(true);
    expect(lines.some((line) => line.endsWith(" process_outcome ran"))).toBe(false);
  }
});

test("spawn and dispatch final gates reject a child disappearing after reduction", async () => {
  const { spawn } = await import("./support/spawn");
  const { dispatch } = await import("./support/dispatch");

  for (const surface of ["spawn", "dispatch"] as const) {
    writeFileSync(log, "");
    const agentId = `test-${surface}-child-set-final-race`;
    const settlements = [
      {
        kind: "settled" as const,
        children: ["@agent:child-a", "@agent:child-b"],
      },
      {
        kind: "settled" as const,
        children: ["@agent:child-a", "@agent:child-b"],
      },
      { kind: "settled" as const, children: [] },
    ];
    let reads = 0;
    const seenInputs: string[] = [];
    const queryFn = (args: RoutedQueryArguments): WireQuery => wireTurnSequenceQuery(args, [
      { output: "provider-result-1", providerDurationMs: 1, turns: 1 },
      { output: "provider-result-2", providerDurationMs: 2, turns: 2 },
    ], { onInput: (text) => seenInputs.push(text) });
    const childSettlementReader = () =>
      settlements[Math.min(reads++, settlements.length - 1)]!;

    if (surface === "spawn") {
      await spawn({
        prompt: "reduce a child before its graph edge disappears",
        agentId,
          routingMetadata: presetRequest("director"),
        queryFn,
        feedSubscriber: subscribeReadyFeed,
        childSettlementReader,
      });
    } else {
      await dispatch(`thread-${agentId}`, {
        agentId,
        routingMetadata: presetRequest("director"),
        claimDriver: (() => ({ release() {} })) as any,
        queryFn,
        feedSubscriber: subscribeReadyFeed,
        loadThreadFacts: () => [
          { predicate: "title", value: "Exercise the post-reduction final race" },
          { predicate: "planned", value: "true" },
          { predicate: "atomic", value: "true" },
        ],
        loadChildren: () => [],
        childSettlementReader,
      });
    }

    expect(reads).toBe(3);
    expect(seenInputs[1]).toContain("post-settlement reduction turn");
    const lines = await settledRunLines(
      agentId,
      surface === "dispatch" ? "applied_domain_requirement_count 0" : "error_count 0",
    );
    expect(lines.some((line) =>
      line.endsWith(" process_outcome orchestrator_child_set_inconsistent"),
    )).toBe(true);
    expect(lines.some((line) =>
      line.endsWith(" delivery_reason orchestrator_child_relation_regressed"),
    )).toBe(true);
    expect(lines.some((line) => line.endsWith(" process_outcome ran"))).toBe(false);
  }
});

test("spawn and dispatch final gates reject late live, unavailable, or unreduced settled state", async () => {
  const { spawn } = await import("./support/spawn");
  const { dispatch } = await import("./support/dispatch");
  const terminalStates = [
    {
      label: "live",
      state: {
        kind: "live" as const,
        children: ["@agent:child-a", "@agent:child-b", "@agent:late-child"],
        live: ["@agent:late-child"],
      },
      outcome: "orchestrator_children_incomplete",
    },
    {
      label: "unavailable",
      state: {
        kind: "unavailable" as const,
        reason: "injected graph outage",
      },
      outcome: "child_reconciliation_unavailable",
    },
    {
      label: "settled",
      state: {
        kind: "settled" as const,
        children: ["@agent:child-a", "@agent:child-b", "@agent:late-terminal-child"],
      },
      outcome: "orchestrator_reduction_incomplete",
    },
  ];
  const reducedTerminalQuery = (args: RoutedQueryArguments): WireQuery => wireTurnSequenceQuery(
    args,
    [
      { output: "provider said done", providerDurationMs: 1, turns: 1 },
      { output: "provider said done", providerDurationMs: 2, turns: 2 },
    ],
  );

  for (const surface of ["spawn", "dispatch"] as const) {
    for (const terminalState of terminalStates) {
      writeFileSync(log, "");
      const agentId = `test-${surface}-late-${terminalState.label}`;
      let calls = 0;
      const childSettlementReader = () => {
        calls++;
        return calls <= 2
          ? {
              kind: "settled" as const,
              children: ["@agent:child-a", "@agent:child-b"],
            }
          : terminalState.state;
      };
      if (surface === "spawn") {
        await spawn({
          prompt: "exercise the final child gate",
          agentId,
          routingMetadata: presetRequest("director"),
          queryFn: reducedTerminalQuery,
          feedSubscriber: subscribeReadyFeed,
          childSettlementReader,
        });
      } else {
        await dispatch(`thread-${agentId}`, {
          agentId,
          routingMetadata: presetRequest("director"),
          claimDriver: (() => ({ release() {} })) as any,
          queryFn: reducedTerminalQuery,
          feedSubscriber: subscribeReadyFeed,
          loadThreadFacts: () => [
            { predicate: "title", value: "Exercise dispatch child gate" },
            { predicate: "planned", value: "true" },
            { predicate: "atomic", value: "true" },
          ],
          loadChildren: () => [],
          childSettlementReader,
        });
      }
      expect(calls).toBe(3);
      const lines = await settledRunLines(
        agentId,
        surface === "dispatch" ? "applied_domain_requirement_count 0" : "error_count 0",
      );
      expect(lines.some((line) =>
        line.endsWith(` process_outcome ${terminalState.outcome}`),
      )).toBe(true);
      expect(lines.some((line) => line.endsWith(" process_outcome ran"))).toBe(false);
      const logged = readFileSync(log, "utf8");
      expect(logged).toContain(
        `tell agent:${agentId} delivery_outcome blocked`,
      );
      if (terminalState.label === "live") {
        expect(logged).toContain(`tell agent:${agentId} early_exit_children`);
      }
    }
  }
}, 15_000);

test("spawn replays one transport-ambiguous reservation with the exact context before provider construction", async () => {
  const { spawn } = await import("./support/spawn");
  writeFileSync(log, "");
  const reservations: DeliveryRunContext[] = [];
  let constructions = 0;
  let providerEnv: Record<string, string> | undefined;
  await spawn({
    prompt: "reserve before provider construction",
    agentId: "test-spawn-reservation-retry",
    routingMetadata: presetRequest("integrator"),
    thread: "thread-reservation-retry",
    deliveryRuntime: {
      attemptRoute: attemptRoute("thread-reservation-retry"),
      reserve(context) {
        reservations.push(context);
        if (reservations.length === 1)
          throw new DeliveryReservationWriterProcessFailure(
            "delivery evidence reserve rejected: run reservation refused:"
            + " receipt=unavailable reason=writer-process-failure",
          );
        return { contractOrigin: "accepted", baselineDoneWhen: [] };
      },
      load: () => ({ reservationValid: true, evidence: [] }),
    },
    queryFn: (args) => {
      constructions++;
      providerEnv = args.options.env;
      return wireTurnQuery(args, { output: "done", turns: 1, providerDurationMs: 1 });
    },
  });
  expect(reservations).toHaveLength(2);
  expect(reservations[1]).toBe(reservations[0]);
  expect(reservations[1]).toEqual(reservations[0]);
  expect(constructions).toBe(1);
  expect(providerEnv).toMatchObject({
    NORTH_RUN_ID: reservations[0]!.runId,
    NORTH_THREAD_ID: "thread-reservation-retry",
    NORTH_RUN_CAPABILITY: reservations[0]!.capability,
  });
});

test("reservation refusal or repeated transport failure constructs no provider", async () => {
  const { spawn } = await import("./support/spawn");
  writeFileSync(log, "");
  for (const scenario of [
    {
      label: "refusal",
      failure: () => new Error("run reservation refused: reason=existing-reservation"),
      expectedReserveCalls: 1,
    },
    {
      label: "publication-deadline",
      failure: () => new DeliveryEvidenceRetryableError(
        "delivery evidence reserve rejected: publication deadline exceeded",
      ),
      expectedReserveCalls: 1,
    },
  ]) {
    let reserveCalls = 0;
    let constructions = 0;
    const reservations: DeliveryRunContext[] = [];
    const assignmentRuns: string[] = [];
    await spawn({
      prompt: "must not reach provider without a reservation",
      agentId: `test-spawn-reservation-${scenario.label}`,
      routingMetadata: presetRequest("integrator"),
      thread: "thread-spawn-reservation-refusal",
      publishLearningAssignment: async (runId: string) => {
        assignmentRuns.push(runId);
        return "recorded" as const;
      },
      deliveryRuntime: {
        attemptRoute: attemptRoute("thread-spawn-reservation-refusal"),
        reserve(context) {
          reserveCalls++;
          reservations.push(context);
          throw scenario.failure();
        },
        load: () => ({ reservationValid: false, evidence: [] }),
      },
      queryFn: () => { constructions++; return (async function* () {})(); },
    });
    expect(constructions).toBe(0);
    expect(reserveCalls).toBe(scenario.expectedReserveCalls);
    if (scenario.expectedReserveCalls === 2) {
      expect(reservations[1]).toBe(reservations[0]);
    }
    const lines = await settledRunLines(
      `test-spawn-reservation-${scenario.label}`,
      "error_count 0",
    );
    const subjects = new Set(lines.map((line) => line.split(/\s+/)[1]));
    expect(subjects.size).toBe(1);
    for (const attempted of reservations) {
      expect(subjects.has(attempted.runId)).toBe(false);
    }
    expect(assignmentRuns).toHaveLength(2);
    expect(assignmentRuns[0]).toBe(reservations[0]!.runId);
    expect(subjects).toEqual(new Set([assignmentRuns[1]!]));
    expect(lines.some((line) =>
      line.endsWith(" process_outcome blocked_preflight"),
    )).toBe(true);
  }
});

test("dispatch fails closed before provider construction when reservation is unavailable", async () => {
  const { dispatch } = await import("./support/dispatch");
  writeFileSync(log, "");
  let failedRunId: string | undefined;
  let reserveCalls = 0;
  let constructions = 0;
  await expect(dispatch("test-dispatch-reservation-rotation", {
    agentId: "test-dispatch-reservation-rotation-agent",
    routingMetadata: presetRequest("integrator"),
    claimDriver: (() => ({ release() {} })) as any,
    loadChildren: () => [],
    loadThreadFacts: () => [
      { predicate: "title", value: "Rotate failed reservation telemetry" },
      { predicate: "planned", value: "true" },
      { predicate: "atomic", value: "true" },
    ],
    deliveryRuntime: {
      attemptRoute: attemptRoute("test-dispatch-reservation-rotation"),
      reserve(context) {
        reserveCalls++;
        failedRunId = context.runId;
        throw new DeliveryEvidenceRetryableError(
          "delivery evidence reserve rejected: publication deadline exceeded",
        );
      },
      load: () => ({ reservationValid: false, evidence: [] }),
      reserveOptions: { sleep: () => {} },
    },
    queryFn: (args) => {
      constructions++;
      return wireTurnQuery(args, { output: "done", turns: 1, providerDurationMs: 1 });
    },
  })).rejects.toThrow("delivery evidence reserve rejected: publication deadline exceeded");
  expect(reserveCalls).toBe(1);
  expect(constructions).toBe(0);
  expect(failedRunId).toBeDefined();
});

test("dispatch retains its wire run when its reservation is invalid at finalization", async () => {
  const { dispatch } = await import("./support/dispatch");
  writeFileSync(log, "");
  let reservedRunId: string | undefined;
  await dispatch("test-dispatch-finalize-rotation", {
    agentId: "test-dispatch-finalize-rotation-agent",
    routingMetadata: presetRequest("integrator"),
    claimDriver: (() => ({ release() {} })) as any,
    loadChildren: () => [],
    loadThreadFacts: () => [
      { predicate: "title", value: "Finalize reservation recovery" },
      { predicate: "planned", value: "true" },
      { predicate: "atomic", value: "true" },
      { predicate: "done_when", value: "tests pass" },
    ],
    deliveryRuntime: {
      attemptRoute: attemptRoute("test-dispatch-finalize-rotation"),
      ...attemptTransitions(),
      reserve(context, route) {
        reservedRunId = context.runId;
        return attemptReservation(context, route, ["tests pass"]);
      },
      load() {
        return { reservationValid: false, evidence: [] };
      },
    },
    queryFn: (args) => wireTurnQuery(args, {
      output: "done", turns: 1, providerDurationMs: 1,
    }),
  });
  const lines = await settledRunLines(
    "test-dispatch-finalize-rotation-agent",
    "applied_domain_requirement_count 0",
  );
  const subjects = new Set(lines.map((line) => line.split(/\s+/)[1]));
  expect(reservedRunId).toBeDefined();
  expect(lines.some((line) => line.startsWith(`tell ${reservedRunId} `))).toBe(true);
  expect(subjects.size).toBe(1);
  expect(subjects).toEqual(new Set([reservedRunId!]));
  expect(lines.some((line) =>
    line.endsWith(" delivery_reason delivery_reservation_unavailable_at_finalize"),
  )).toBe(true);
});

// Thread 019f9e0d: same defect mirrored on the dispatch path. `loadThreadFacts`
// is also the preflight hydration read, so the first call must succeed; only
// the FINALIZE read (via loadTerminalFacts) is fault-injected here.
test("dispatch's finalize names an exhausted thread load apart from a genuinely absent thread", async () => {
  const { dispatch } = await import("./support/dispatch");
  writeFileSync(log, "");
  const errorSpy = spyOn(console, "error");
  let calls = 0;
  try {
    await dispatch("test-dispatch-thread-load-failed", {
      agentId: "test-dispatch-thread-load-failed-agent",
      routingMetadata: presetRequest("integrator"),
      claimDriver: (() => ({ release() {} })) as any,
      loadChildren: () => [],
      loadThreadFacts: () => {
        calls++;
        if (calls === 1) {
          return [
            { predicate: "title", value: "Contended thread finalize" },
            { predicate: "planned", value: "true" },
            { predicate: "atomic", value: "true" },
            { predicate: "done_when", value: "tests pass" },
          ];
        }
        throw new Error("torn thread fact row");
      },
      deliveryRuntime: {
        attemptRoute: attemptRoute("test-dispatch-thread-load-failed"),
        ...attemptTransitions(),
        reserve(context, route) {
          return attemptReservation(context, route, ["tests pass"]);
        },
        load() {
          return { reservationValid: true, evidence: [] };
        },
      },
      threadFactsLoadOptions: { attempts: 3, backoffMs: 0, sleep: () => {} },
      queryFn: (args) => wireTurnQuery(args, {
        output: "done", turns: 1, providerDurationMs: 1,
      }),
    });
    expect(calls).toBe(1 + 3);
    const messages = errorSpy.mock.calls.map((call) => String(call[0]));
    expect(messages.some((message) =>
      message.includes("thread unreadable at finalize after 3 attempt(s)")
      && message.includes("torn thread fact row"),
    )).toBe(true);
  } finally {
    errorSpy.mockRestore();
  }
  const lines = await settledRunLines(
    "test-dispatch-thread-load-failed-agent",
    "applied_domain_requirement_count 0",
  );
  expect(lines.some((line) =>
    line.endsWith(" delivery_reason delivery_thread_load_failed_at_finalize"),
  )).toBe(true);
});

test("dispatch still reports delivery when the thread read only fails transiently", async () => {
  const { dispatch } = await import("./support/dispatch");
  writeFileSync(log, "");
  let reservedRunId: string | undefined;
  let calls = 0;
  const slept: number[] = [];
  await dispatch("test-dispatch-contended-thread-load", {
    agentId: "test-dispatch-contended-thread-load-agent",
    routingMetadata: presetRequest("integrator"),
    claimDriver: (() => ({ release() {} })) as any,
    loadChildren: () => [],
    loadThreadFacts: () => {
      calls++;
      const baseline = [
        { predicate: "title", value: "Contended thread finalize recovers" },
        { predicate: "planned", value: "true" },
        { predicate: "atomic", value: "true" },
        { predicate: "done_when", value: "focused tests pass" },
      ];
      if (calls === 1) return baseline;
      // Two contended finalize reads, then the coordinator answers.
      if (calls < 4) throw new Error("reader timed out");
      return baseline;
    },
    deliveryRuntime: {
      attemptRoute: attemptRoute("test-dispatch-contended-thread-load"),
      ...attemptTransitions(),
      reserve(context, route) {
        reservedRunId = context.runId;
        return attemptReservation(context, route, ["focused tests pass"]);
      },
      load(runId) {
        return { reservationValid: true, evidence: [{
          version: RUN_BAR_EVIDENCE_VERSION,
          run: `@${runId}`,
          thread: "@test-dispatch-contended-thread-load",
          reporter: "@agent:test-dispatch-contended-thread-load-agent",
          bar: "focused tests pass",
          observed: "10/10",
          recordedAt: "2026-07-18T10:00:00Z",
        }] };
      },
    },
    threadFactsLoadOptions: { attempts: 3, backoffMs: 5, sleep: (ms) => slept.push(ms) },
    queryFn: (args) => wireTurnQuery(args, {
      output: "done", turns: 1, providerDurationMs: 1,
    }),
  });
  expect([calls, slept]).toEqual([4, [5, 10]]);
  const lines = await settledRunLines(
    "test-dispatch-contended-thread-load-agent",
    "applied_domain_requirement_count 0",
  );
  expect(new Set(lines.map((line) => line.split(/\s+/)[1])))
    .toEqual(new Set([reservedRunId!]));
  expect(lines.some((line) => line.endsWith(" delivery_outcome reported"))).toBe(true);
  expect(lines.some((line) => line.includes(" delivery_outcome unverified"))).toBe(false);
});

// A load that ANSWERS with no facts at finalize is a content verdict, not a
// load failure: it stays fail-closed on the first finalize attempt.
test("dispatch's finalize leaves a genuinely absent thread fail-closed without retry", async () => {
  const { dispatch } = await import("./support/dispatch");
  writeFileSync(log, "");
  let calls = 0;
  await dispatch("test-dispatch-thread-absent", {
    agentId: "test-dispatch-thread-absent-agent",
    routingMetadata: presetRequest("integrator"),
    claimDriver: (() => ({ release() {} })) as any,
    loadChildren: () => [],
    loadThreadFacts: () => {
      calls++;
      if (calls === 1) {
        return [
          { predicate: "title", value: "Absent at finalize" },
          { predicate: "planned", value: "true" },
          { predicate: "atomic", value: "true" },
          { predicate: "done_when", value: "tests pass" },
        ];
      }
      return [];
    },
    deliveryRuntime: {
      attemptRoute: attemptRoute("test-dispatch-thread-absent"),
      ...attemptTransitions(),
      reserve(context, route) {
        return attemptReservation(context, route, ["tests pass"]);
      },
      load() {
        return { reservationValid: true, evidence: [] };
      },
    },
    threadFactsLoadOptions: { attempts: 3, backoffMs: 0, sleep: () => {} },
    queryFn: (args) => wireTurnQuery(args, {
      output: "done", turns: 1, providerDurationMs: 1,
    }),
  });
  expect(calls).toBe(2);
  const lines = await settledRunLines(
    "test-dispatch-thread-absent-agent",
    "applied_domain_requirement_count 0",
  );
  expect(lines.some((line) =>
    line.endsWith(" delivery_reason delivery_thread_unavailable_at_finalize"),
  )).toBe(true);
});

// Thread 019f9cc1: a load that never produced facts is now reported as
// UNREADABLE (with its attempt count and cause) and carries its own
// delivery_reason, so downstream analysis can split a contended coordinator from
// a genuinely invalid reservation. The old assertion here conflated them.
test("spawn's finalize-rotation names an exhausted load apart from an invalid reservation", async () => {
  const { spawn } = await import("./support/spawn");
  writeFileSync(log, "");
  const errorSpy = spyOn(console, "error");
  let loads = 0;
  try {
    await spawn({
      prompt: "recover at finalization, loudly",
      agentId: "test-spawn-finalize-rotation-loud",
      routingMetadata: presetRequest("integrator"),
      thread: "thread-spawn-finalize-rotation-loud",
      deliveryRuntime: {
        attemptRoute: attemptRoute("thread-spawn-finalize-rotation-loud"),
        reserve(context) {
          return { contractOrigin: "accepted", baselineDoneWhen: ["tests pass"] };
        },
        load() {
          loads++;
          throw new Error("torn rotated-run predicate row");
        },
        loadOptions: { attempts: 3, backoffMs: 0, sleep: () => {} },
      },
      queryFn: (args) => wireTurnQuery(args, {
        output: "done", turns: 1, providerDurationMs: 1,
      }),
    });
    expect(loads).toBe(3);
    const messages = errorSpy.mock.calls.map((call) => String(call[0]));
    expect(messages.some((message) =>
      message.includes("reservation unreadable at finalize after 3 attempt(s)")
      && message.includes("torn rotated-run predicate row"),
    )).toBe(true);
  } finally {
    errorSpy.mockRestore();
  }
  const lines = await settledRunLines("test-spawn-finalize-rotation-loud");
  expect(lines.some((line) =>
    line.endsWith(" delivery_reason delivery_reservation_load_failed_at_finalize"),
  )).toBe(true);
});

// The defect this thread exists for: lanes ms1awg94/ms1b7syb recorded their bar
// evidence, committed, were harvested — and finalized unverified because ONE
// reservation read timed out against a busy coordinator. A delivered lane must
// survive a contended read.
test("spawn still reports delivery when the reservation read only fails transiently", async () => {
  const { spawn } = await import("./support/spawn");
  writeFileSync(log, "");
  let reservedRunId: string | undefined;
  let loads = 0;
  const slept: number[] = [];
  await spawn({
    prompt: "deliver against a busy coordinator",
    agentId: "test-spawn-contended-load",
    routingMetadata: presetRequest("integrator"),
    thread: "thread-spawn-contended-load",
    loadThreadFacts: () => [
      { predicate: "title", value: "Contended reservation read" },
      { predicate: "planned", value: "true" },
      { predicate: "atomic", value: "true" },
      { predicate: "done_when", value: "focused tests pass" },
    ],
    deliveryRuntime: {
      attemptRoute: attemptRoute("thread-spawn-contended-load"),
      reserve(context) {
        reservedRunId = context.runId;
        return { contractOrigin: "accepted", baselineDoneWhen: ["focused tests pass"] };
      },
      load(runId) {
        loads++;
        // Two contended reads, then the coordinator answers.
        if (loads < 3) throw new Error("reader timed out");
        return { reservationValid: true, evidence: [{
          version: RUN_BAR_EVIDENCE_VERSION,
          run: `@${runId}`,
          thread: "@thread-spawn-contended-load",
          reporter: "@agent:test-spawn-contended-load",
          bar: "focused tests pass",
          observed: "10/10",
          recordedAt: "2026-07-18T10:00:00Z",
        }] };
      },
      loadOptions: { attempts: 3, backoffMs: 5, sleep: (ms) => slept.push(ms) },
    },
    queryFn: (args) => wireTurnQuery(args, {
      output: "done", turns: 1, providerDurationMs: 1,
    }),
  });
  expect([loads, slept]).toEqual([3, [5, 10]]);
  const lines = await settledRunLines("test-spawn-contended-load");
  // Telemetry stays on the RESERVED run: no rotation, no unverified stamp.
  expect(new Set(lines.map((line) => line.split(/\s+/)[1])))
    .toEqual(new Set([reservedRunId!]));
  expect(lines.some((line) => line.endsWith(" delivery_outcome reported"))).toBe(true);
  expect(lines.some((line) => line.includes(" delivery_outcome unverified"))).toBe(false);
});

test("spawn retains its wire run when its reservation is invalid at finalization", async () => {
  const { spawn } = await import("./support/spawn");
  writeFileSync(log, "");
  let reservedRunId: string | undefined;
  await spawn({
    prompt: "recover at finalization",
    agentId: "test-spawn-finalize-rotation",
    routingMetadata: presetRequest("integrator"),
    thread: "thread-spawn-finalize-rotation",
    deliveryRuntime: {
      attemptRoute: attemptRoute("thread-spawn-finalize-rotation"),
      reserve(context) {
        reservedRunId = context.runId;
        return { contractOrigin: "accepted", baselineDoneWhen: ["tests pass"] };
      },
      load() {
        return { reservationValid: false, evidence: [] };
      },
    },
    queryFn: (args) => wireTurnQuery(args, {
      output: "done", turns: 1, providerDurationMs: 1,
    }),
  });
  const lines = await settledRunLines("test-spawn-finalize-rotation");
  const subjects = new Set(lines.map((line) => line.split(/\s+/)[1]));
  expect(reservedRunId).toBeDefined();
  expect(lines.some((line) => line.startsWith(`tell ${reservedRunId} `))).toBe(true);
  expect(subjects.size).toBe(1);
  expect(subjects).toEqual(new Set([reservedRunId!]));
  expect(lines.some((line) =>
    line.endsWith(" delivery_reason delivery_reservation_unavailable_at_finalize"),
  )).toBe(true);
});

// Thread 019f9e0d: delivery_thread_unavailable_at_finalize is the deferred
// sibling of the reservation-load defect (019f9cc1) — lane ms1o5ipp ran,
// completed, recorded evidence, and finalized unverified with this reason
// under matrix load because the thread read at finalize was never retried.
test("spawn's finalize names an exhausted thread load apart from a genuinely absent thread", async () => {
  const { spawn } = await import("./support/spawn");
  writeFileSync(log, "");
  const errorSpy = spyOn(console, "error");
  // Spawn also reads thread facts once, early, for judgment-grade admission
  // (unrelated to finalize and already exercised elsewhere) — a fake that
  // always throws hits that call too, so the exhausted finalize count is
  // total calls minus that one leading admission read.
  let loads = 0;
  try {
    await spawn({
      prompt: "recover a contended thread read at finalization, loudly",
      agentId: "test-spawn-thread-load-failed-loud",
      routingMetadata: presetRequest("integrator"),
      thread: "thread-spawn-thread-load-failed-loud",
      loadThreadFacts: () => {
        loads++;
        throw new Error("torn thread fact row");
      },
      deliveryRuntime: {
        attemptRoute: attemptRoute("thread-spawn-thread-load-failed-loud"),
        reserve(context) {
          return { contractOrigin: "accepted", baselineDoneWhen: ["tests pass"] };
        },
        load(runId) {
          return { reservationValid: true, evidence: [] };
        },
      },
      threadFactsLoadOptions: { attempts: 3, backoffMs: 0, sleep: () => {} },
      queryFn: (args) => wireTurnQuery(args, {
        output: "done", turns: 1, providerDurationMs: 1,
      }),
    });
    expect(loads).toBe(1 + 3);
    const messages = errorSpy.mock.calls.map((call) => String(call[0]));
    expect(messages.some((message) =>
      message.includes("thread unreadable at finalize after 3 attempt(s)")
      && message.includes("torn thread fact row"),
    )).toBe(true);
  } finally {
    errorSpy.mockRestore();
  }
  const lines = await settledRunLines("test-spawn-thread-load-failed-loud");
  expect(lines.some((line) =>
    line.endsWith(" delivery_reason delivery_thread_load_failed_at_finalize"),
  )).toBe(true);
});

// The evidence exists on the graph; only the read was contended. A delivered
// lane must survive a thread read that fails transiently, exactly like the
// reservation read.
test("spawn still reports delivery when the thread read only fails transiently", async () => {
  const { spawn } = await import("./support/spawn");
  writeFileSync(log, "");
  let reservedRunId: string | undefined;
  // Call 1 is spawn's early judgment-grade admission read (must succeed);
  // calls 2+ are the finalize load under test.
  let loads = 0;
  const slept: number[] = [];
  const contendedFacts = [
    { predicate: "title", value: "Contended thread read" },
    { predicate: "planned", value: "true" },
    { predicate: "atomic", value: "true" },
    { predicate: "done_when", value: "focused tests pass" },
  ];
  await spawn({
    prompt: "deliver against a busy coordinator's thread read",
    agentId: "test-spawn-contended-thread-load",
    routingMetadata: presetRequest("integrator"),
    thread: "thread-spawn-contended-thread-load",
    loadThreadFacts: () => {
      loads++;
      if (loads === 1) return contendedFacts;
      // Two contended finalize reads, then the coordinator answers.
      if (loads < 4) throw new Error("reader timed out");
      return contendedFacts;
    },
    deliveryRuntime: {
      attemptRoute: attemptRoute("thread-spawn-contended-thread-load"),
      reserve(context) {
        reservedRunId = context.runId;
        return { contractOrigin: "accepted", baselineDoneWhen: ["focused tests pass"] };
      },
      load(runId) {
        return { reservationValid: true, evidence: [{
          version: RUN_BAR_EVIDENCE_VERSION,
          run: `@${runId}`,
          thread: "@thread-spawn-contended-thread-load",
          reporter: "@agent:test-spawn-contended-thread-load",
          bar: "focused tests pass",
          observed: "10/10",
          recordedAt: "2026-07-18T10:00:00Z",
        }] };
      },
    },
    threadFactsLoadOptions: { attempts: 3, backoffMs: 5, sleep: (ms) => slept.push(ms) },
    queryFn: (args) => wireTurnQuery(args, {
      output: "done", turns: 1, providerDurationMs: 1,
    }),
  });
  expect([loads, slept]).toEqual([4, [5, 10]]);
  const lines = await settledRunLines("test-spawn-contended-thread-load");
  // Telemetry stays on the RESERVED run: no rotation, no unverified stamp.
  expect(new Set(lines.map((line) => line.split(/\s+/)[1])))
    .toEqual(new Set([reservedRunId!]));
  expect(lines.some((line) => line.endsWith(" delivery_outcome reported"))).toBe(true);
  expect(lines.some((line) => line.includes(" delivery_outcome unverified"))).toBe(false);
});

// A load that ANSWERS with no facts is a content verdict (genuinely absent
// thread), not a load failure — it must stay fail-closed on the first
// attempt with its own named reason, never confused with an unreadable load.
test("spawn's finalize leaves a genuinely absent thread fail-closed without retry", async () => {
  const { spawn } = await import("./support/spawn");
  writeFileSync(log, "");
  // Call 1 is spawn's early judgment-grade admission read (must succeed so it
  // does not mask this test's own reason); call 2 is the finalize load,
  // which finds the thread absent and must not retry.
  let loads = 0;
  await spawn({
    prompt: "finalize against a thread that no longer exists",
    agentId: "test-spawn-thread-absent",
    routingMetadata: presetRequest("integrator"),
    thread: "thread-spawn-thread-absent",
    loadThreadFacts: () => {
      loads++;
      return loads === 1
        ? [{ predicate: "title", value: "Absent at finalize" }]
        : [];
    },
    deliveryRuntime: {
      attemptRoute: attemptRoute("thread-spawn-thread-absent"),
      reserve(context) {
        return { contractOrigin: "accepted", baselineDoneWhen: ["tests pass"] };
      },
      load(runId) {
        return { reservationValid: true, evidence: [] };
      },
    },
    threadFactsLoadOptions: { attempts: 3, backoffMs: 0, sleep: () => {} },
    queryFn: (args) => wireTurnQuery(args, {
      output: "done", turns: 1, providerDurationMs: 1,
    }),
  });
  expect(loads).toBe(2);
  const lines = await settledRunLines("test-spawn-thread-absent");
  expect(lines.some((line) =>
    line.endsWith(" delivery_reason delivery_thread_unavailable_at_finalize"),
  )).toBe(true);
});

test("spawn keeps omitted, reported-zero, and preflight-zero turn evidence distinct", async () => {
  const { spawn } = await import("./support/spawn");
  writeFileSync(log, "");

  const terminal = (numTurns?: number) => (args: RoutedQueryArguments): WireQuery =>
    wireTurnQuery(args, {
      output: "done", providerDurationMs: 1,
      ...(numTurns === undefined ? {} : { turns: numTurns }),
    });

  await spawn({
    prompt: "provider omits turn count", agentId: "test-turns-omitted",
    routingMetadata: presetRequest("integrator"), queryFn: terminal(),
  });
  const omitted = await readWireJsonl(join(dir, "agent-test-turns-omitted.stream.jsonl"));
  const omittedTerminal = omitted.events.find((event) => event.kind === "model-call.completed");
  expect(omittedTerminal?.kind === "model-call.completed"
    ? omittedTerminal.evidence?.turns : undefined).toBeUndefined();

  await spawn({
    prompt: "provider reports zero turns", agentId: "test-turns-reported-zero",
    routingMetadata: presetRequest("integrator"), queryFn: terminal(0),
  });
  const reportedZero = await readWireJsonl(
    join(dir, "agent-test-turns-reported-zero.stream.jsonl"),
  );
  expect(reportedZero.events.find((event) => event.kind === "model-call.completed"))
    .toMatchObject({ evidence: { turns: { unit: "assistant-turn", count: 0 } } });

  await spawn({
    prompt: "preflight blocks before provider acceptance", agentId: "test-turns-preflight-zero",
    routingMetadata: presetRequest("integrator"),
    queryFn: () => { throw new ProviderRetrySafeError("test_retry_safe_preflight"); },
  });
  const preflightZero = await readWireJsonl(
    join(dir, "agent-test-turns-preflight-zero.stream.jsonl"),
  );
  expect(preflightZero.events.filter((event) => event.kind === "model-call.started"))
    .toHaveLength(0);
  expect(preflightZero.events.at(-1)).toMatchObject({
    kind: "run.terminated", lifecycle: "blocked",
  });
});

test("dispatch keeps omitted, reported-zero, and preflight-zero turn evidence distinct", async () => {
  const { dispatch } = await import("./support/dispatch");
  writeFileSync(log, "");

  const terminal = (numTurns?: number) => (args: RoutedQueryArguments): WireQuery =>
    wireTurnQuery(args, {
      output: "done", providerDurationMs: 1,
      ...(numTurns === undefined ? {} : { turns: numTurns }),
    });
  const dependencies = (
    agentId: string,
    queryFn: (args: RoutedQueryArguments) => WireQuery,
  ) => ({
    agentId,
    routingMetadata: presetRequest("integrator"),
    claimDriver: (() => ({ release() {} })) as any,
    queryFn,
    loadThreadFacts: () => [
      { predicate: "title", value: `Turn evidence for ${agentId}` },
      { predicate: "planned", value: "true" },
      { predicate: "atomic", value: "true" },
    ],
    loadChildren: () => [],
  });

  await dispatch(
    "test-dispatch-turns-omitted",
    dependencies("test-dispatch-turns-omitted-agent", terminal()),
  );
  const omitted = await readWireJsonl(
    join(dir, "agent-test-dispatch-turns-omitted-agent.stream.jsonl"),
  );
  const omittedTerminal = omitted.events.find((event) => event.kind === "model-call.completed");
  expect(omittedTerminal?.kind === "model-call.completed"
    ? omittedTerminal.evidence?.turns : undefined).toBeUndefined();

  await dispatch(
    "test-dispatch-turns-reported-zero",
    dependencies("test-dispatch-turns-reported-zero-agent", terminal(0)),
  );
  const reportedZero = await readWireJsonl(
    join(dir, "agent-test-dispatch-turns-reported-zero-agent.stream.jsonl"),
  );
  expect(reportedZero.events.find((event) => event.kind === "model-call.completed"))
    .toMatchObject({ evidence: { turns: { unit: "assistant-turn", count: 0 } } });

  await dispatch(
    "test-dispatch-turns-preflight-zero",
    dependencies(
      "test-dispatch-turns-preflight-zero-agent",
      () => { throw new ProviderRetrySafeError("test_retry_safe_preflight"); },
    ),
  );
  const preflightZero = await readWireJsonl(
    join(dir, "agent-test-dispatch-turns-preflight-zero-agent.stream.jsonl"),
  );
  expect(preflightZero.events.filter((event) => event.kind === "model-call.started"))
    .toHaveLength(0);
  expect(preflightZero.events.at(-1)).toMatchObject({
    kind: "run.terminated", lifecycle: "blocked",
  });
});

interface CapturedRunProjection {
  agent: string;
  facts: Array<[string, string]>;
  lines: string[];
}

function capturedRunProjections(value: string): CapturedRunProjection[] {
  const projections: CapturedRunProjection[] = [];
  for (const line of value.split("\n")) {
    if (!line.startsWith("run-fact\t")) continue;
    const payload = line.split("\t").at(-1);
    if (!payload) continue;
    try {
      const parsed: unknown = JSON.parse(payload);
      if (!Array.isArray(parsed)) continue;
      const facts = parsed as Array<[string, string]>;
      const agent = facts.find(([predicate]) => predicate === "agent")?.[1];
      const runId = facts.find(([predicate]) => predicate === "wire_run_id")?.[1];
      if (!agent || !runId) continue;
      projections.push({
        agent,
        facts,
        lines: facts.map(([predicate, factValue]) =>
          `tell ${runId} ${predicate} ${factValue}`),
      });
    } catch {
      // The fake writer may still be appending its single capture line.
    }
  }
  return projections;
}

function capturedLogWithRunProjections(value: string): string {
  const projected = capturedRunProjections(value).flatMap(({ lines }) => lines);
  return projected.length === 0 ? value : `${value}\n${projected.join("\n")}`;
}

async function waitForLog(needle: string): Promise<string> {
  for (let i = 0; i < 100; i++) {
    const value = existsSync(log) ? readFileSync(log, "utf8") : "";
    const observable = capturedLogWithRunProjections(value);
    if (observable.includes(needle)) return observable;
    await Bun.sleep(10);
  }
  throw new Error(`timed out waiting for telemetry fact: ${needle}`);
}

async function waitForRunFactProjection(agent: string): Promise<Array<[string, string]>> {
  for (let i = 0; i < 100; i++) {
    const value = existsSync(log) ? readFileSync(log, "utf8") : "";
    const projection = capturedRunProjections(value).find((candidate) =>
      candidate.agent === agent);
    if (projection) return projection.facts;
    await Bun.sleep(10);
  }
  throw new Error(`timed out waiting for run-fact projection: ${agent}`);
}

async function settledRunLines(agent: string, requiredSuffix = "error_count 0"): Promise<string[]> {
  for (let i = 0, stable = 0, previous = ""; i < 100; i++) {
    const value = existsSync(log) ? readFileSync(log, "utf8") : "";
    const lines = capturedRunProjections(value)
      .filter((projection) => projection.agent === agent)
      .flatMap((projection) => projection.lines);
    const snapshot = lines.slice().sort().join("\n");
    const hasTailEvidence = lines.some((line) => line.endsWith(` ${requiredSuffix}`));
    if (hasTailEvidence && snapshot === previous) stable++;
    else stable = 0;
    if (stable >= 5) return lines;
    previous = snapshot;
    await Bun.sleep(10);
  }
  throw new Error(`timed out waiting for settled run telemetry: ${agent}`);
}

test("public spawn composes justified explicit axes before Orchestration hydration", async () => {
  const { spawn } = await import("./support/spawn");
  writeFileSync(log, "");
  let queryOptions: any;
  const queryFn = (args: RoutedQueryArguments): WireQuery => {
    queryOptions = args.options;
    return wireTurnQuery(args, {
      output: "composed", turns: 1, providerDurationMs: 1,
    });
  };

  await spawn({
    prompt: "exercise the real composition boundary", agentId: "test-composed-director",
    routingMetadata: applyOrchestrationStaffing({
      role: "director", tier: "economy", reasoning: "low", posture: "preserve",
      composition: { kind: "template", id: "director",
        overrides: ["tier", "reasoning", "posture"],
        overrideReason: "exercise the explicit public-dial composition boundary" },
    }), provider: "anthropic", pinEvidence: pinEvidence("anthropic"), queryFn,
  });

  expect(queryOptions.model).toBe("claude-sonnet-5");
  expect(queryOptions.effort).toBe("low");
  const logged = await waitForLog("topology orchestrator");
  for (const fact of [
    "requested_role director", "task_grade staff", "topology orchestrator",
    "routing_tier economy", "requested_reasoning low", "routing_posture preserve",
  ]) expect(logged).toContain(fact);
});

test("public role-only integrator spawn hydrates the complete Orchestration preset", async () => {
  const { spawn } = await import("./support/spawn");
  writeFileSync(log, "");
  let queryOptions: any;
  const queryFn = (args: RoutedQueryArguments): WireQuery => {
    queryOptions = args.options;
    return {
      mcpActivity: () => ({
        source: "production-path-fixture", coverage: "exact", totalCalls: 1,
        tools: [{ server: "north", tool: "show", count: 1 }],
        operationReceipts: [{
          tool: "north/show", operation: "reasoning.inspect",
          durationMs: 2, resultSize: 3, outcome: "ok",
        }],
        operationAggregates: [{
          operation: "reasoning.inspect", count: 1,
          totalDurationMs: 2, meanDurationMs: 2, failureCount: 0,
        }],
      }),
      nativeCommandActivity: () => ({
        source: "production-path-fixture", coverage: "exact", totalCommands: 1,
        successfulCommands: 1, readCommands: 1, northBinaryProbe: "passed",
        completions: [{
          commandSha256: "a".repeat(64), outputSha256: "b".repeat(64),
          status: "completed", exitCode: 0, shape: "read", durationMs: 3,
        }],
      }),
      [Symbol.asyncIterator](): AsyncIterator<WireEvent> {
        return (async function*(): AsyncGenerator<WireEvent> {
          const modelCallId = wireModelCallId("model-call:production-provenance");
          const messageId = wireMessageId("message:production-provenance");
          yield args.writer.append({
            kind: "model-call.started", modelCallId,
            model: { provider: "anthropic", tier: "senior", capabilityClass: "authoring" },
            effort: "high", attempt: 1,
          });
          yield args.writer.append({
            kind: "message.recorded", messageId, modelCallId,
            stage: "started", role: "assistant",
          });
          yield args.writer.append({
            kind: "message.recorded", messageId, modelCallId,
            stage: "completed", role: "assistant", content: "integrated",
          });
          yield args.writer.append({
            kind: "model-call.completed", modelCallId, status: "succeeded",
            origin: "provider",
            usage: {
              lifetime: {
                inputTokens: 10, outputTokens: 5, cacheReadTokens: 0,
                cacheWriteTokens: 0, reasoningTokens: 0, modelCalls: 1,
              },
              context: { tokens: 10, window: 200_000 },
            },
            usageCoverage: "exact",
            evidence: { turns: { unit: "assistant-turn", count: 1, comparable: true } },
          });
        })();
      },
    };
  };

  const priorStaffingSource = process.env.NORTH_STAFFING_SOURCE;
  try {
    process.env.NORTH_STAFFING_SOURCE = "file";
    await spawn({
      prompt: "hydrate a role-only request", agentId: "test-role-only-integrator",
      routingMetadata: presetRequest("integrator"), provider: "anthropic",
      pinEvidence: pinEvidence("anthropic"), queryFn,
    });
  } finally {
    if (priorStaffingSource === undefined) delete process.env.NORTH_STAFFING_SOURCE;
    else process.env.NORTH_STAFFING_SOURCE = priorStaffingSource;
  }

  expect(queryOptions.model).toBe("claude-opus-5");
  expect(queryOptions.effort).toBe("high");
  const projectionFacts = await waitForRunFactProjection("test-role-only-integrator");
  const logged = readFileSync(log, "utf8");
  for (const fact of [
    "tell agent:test-role-only-integrator provider anthropic",
    "tell agent:test-role-only-integrator provider_target anthropic",
    "tell agent:test-role-only-integrator model claude-opus-5",
    "tell agent:test-role-only-integrator effort high",
    "tell agent:test-role-only-integrator composition_kind template",
    "tell agent:test-role-only-integrator composition_id integrator",
    "tell agent:test-role-only-integrator display_handle anthropic-ambient-opus-high-orchestration-integrator-integrator",
  ]) expect(logged).toContain(fact);
  const factValues = (predicate: string) => projectionFacts
    .filter(([candidate]) => candidate === predicate)
    .map(([, value]) => value);
  for (const [predicate, value] of [
    ["task_grade", "senior"],
    ["topology", "worker"],
    ["routing_tier", "senior"],
    ["requested_reasoning", "high"],
    ["routing_posture", "deliver"],
  ] as const) expect(factValues(predicate)).toEqual([value]);
  expect(factValues("provider")).toEqual(["anthropic"]);
  expect(factValues("provider_target")).toEqual(["anthropic"]);
  expect(factValues("requested_role")).toEqual(["integrator"]);
  expect(factValues("routing_admission_receipt_version")).toEqual(["1"]);
  expect(factValues("provider_catalogs_sha256")).toHaveLength(1);
  expect(factValues("provider_catalogs_sha256")[0]).toMatch(/^[a-f0-9]{64}$/);
  expect(factValues("learning_assignment_sha256")[0]).toMatch(/^[a-f0-9]{64}$/);
  expect(factValues("prompt_receipt_sha256")[0]).toMatch(/^[a-f0-9]{64}$/);
  expect(factValues("environment_receipt_sha256")[0]).toMatch(/^[a-f0-9]{64}$/);
  expect(factValues("run_envelope_sha256")[0]).toMatch(/^[a-f0-9]{64}$/);
  expect(factValues("prompt_composition_applied")).toEqual(["true"]);
  expect(factValues("mcp_actual_calls")).toEqual(["1"]);
  expect(factValues("native_command_total")).toEqual(["1"]);
  expect(factValues("process_outcome")).toEqual(["ran"]);
  expect(factValues("delivery_outcome")).toEqual(["unverified"]);
  expect(factValues("error_count")).toEqual(["0"]);
  expect(factValues("struggle_topology")).toEqual(["worker"]);
  expect(factValues("judgment_grade_status")).toEqual(["unavailable"]);
  expect(factValues("judgment_grade_source")).toEqual(["ad-hoc"]);
  const encodedProjection = JSON.stringify(projectionFacts);
  expect(encodedProjection).not.toContain(queryOptions.model);
  expect(encodedProjection).not.toContain("completion outcome fixture");
});

test("tier-routed OpenAI identity records the resolved Sol route, not requested blanks", async () => {
  const { spawn } = await import("./support/spawn");
  writeFileSync(log, "");
  let queryOptions: any;
  const queryFn = (args: RoutedQueryArguments): WireQuery => {
    queryOptions = args.options;
    return wireTurnQuery(args, {
      output: "routed", turns: 1, providerDurationMs: 1,
    });
  };

  await spawn({ prompt: "route with OpenAI", agentId: "test-openai-designer",
    routingMetadata: presetRequest("designer"), provider: "openai",
    pinEvidence: pinEvidence("openai"), queryFn });

  expect(queryOptions.model).toBe("gpt-5.6-sol");
  expect(queryOptions.effort).toBe("xhigh");
  const logged = readFileSync(log, "utf8");
  for (const fact of [
    "tell agent:test-openai-designer provider openai",
    "tell agent:test-openai-designer provider_target openai",
    "tell agent:test-openai-designer model gpt-5.6-sol",
    "tell agent:test-openai-designer effort xhigh",
    "tell agent:test-openai-designer display_handle openai-ambient-sol-xhigh-orchestration-designer-designer",
  ]) expect(logged).toContain(fact);
});

test("public SpawnOptions target and Orchestration role land on exact account identity", async () => {
  const { spawn } = await import("./support/spawn");
  writeFileSync(log, "");
  const policyPath = process.env.NORTH_ROUTING_POLICY!;
  writeFileSync(policyPath, JSON.stringify({
    version: 1,
    mode: "preferential",
    targets: [
      { id: "claude-work", provider: "anthropic", authMode: "ambient" },
      { id: "openai", provider: "openai", authMode: "ambient" },
    ],
    targetOrder: ["claude-work", "openai"],
  }));
  try {
    await spawn({
      prompt: "design on the work account", agentId: "test-target-designer",
      routingMetadata: presetRequest("designer"), provider: "anthropic", target: "claude-work",
      pinEvidence: pinEvidence("anthropic", "claude-work"),
      queryFn: (args) => wireTurnQuery(args, {
        output: "designed", turns: 1, providerDurationMs: 1,
      }),
    });
    const logged = await waitForLog("requested_target claude-work");
    expect(logged).toContain("tell agent:test-target-designer provider_target claude-work");
    expect(logged).toContain("tell agent:test-target-designer composition_id designer");
    expect(logged).toContain("tell agent:test-target-designer display_name anthropic:claude-work");
    expect(logged).toContain("provider_target claude-work");
    expect(logged).toContain("requested_target claude-work");
  } finally {
    rmSync(policyPath, { force: true });
  }
});

test("a struggle sensor firing records a struggle run fact without any in-flight route change", async () => {
  // In-flight escalation is retired (escalation-arch D5). The struggle sensors now run on
  // EVERY spawn as harness-observed execution-axis evidence: a fired sensor writes a
  // `struggle <reason>` run fact at terminal and never changes model/effort. Three
  // consecutive tool errors trip the consecutive_errors sensor (STRUGGLE_ERROR_STREAK=3).
  const { spawn } = await import("./support/spawn");
  writeFileSync(log, "");
  const queryFn = (args: RoutedQueryArguments): WireQuery => ({
    [Symbol.asyncIterator](): AsyncIterator<WireEvent> {
      return (async function*(): AsyncGenerator<WireEvent> {
        const modelCallId = wireModelCallId("model-call:struggle-contract");
        const messageId = wireMessageId("message:struggle-contract");
        yield args.writer.append({
          kind: "model-call.started", modelCallId,
          model: { provider: "anthropic", tier: "senior", capabilityClass: "authoring" },
          effort: "high", attempt: 1,
        });
        yield args.writer.append({
          kind: "message.recorded", messageId, modelCallId,
          stage: "started", role: "assistant",
        });
        for (let index = 0; index < 3; index++) {
          const toolCallId = wireToolCallId(`tool:struggle-contract:${index}`);
          yield args.writer.append({
            kind: "tool.admitted", toolCallId, modelCallId, messageId,
            name: "Bash", argumentPreview: JSON.stringify({ index }),
            schema: { status: "unavailable", reason: "fixture" },
          });
          yield args.writer.append({
            kind: "tool.terminal", toolCallId,
            status: "failed", origin: "provider", errorCode: "fixture_failure",
          });
        }
        yield args.writer.append({
          kind: "message.recorded", messageId, modelCallId,
          stage: "completed", role: "assistant", content: "done anyway",
        });
        yield args.writer.append({
          kind: "model-call.completed", modelCallId, status: "succeeded",
          origin: "provider",
          usage: {
            lifetime: {
              inputTokens: 3, outputTokens: 1, cacheReadTokens: 0,
              cacheWriteTokens: 0, reasoningTokens: 0, modelCalls: 1,
            },
            context: { tokens: 3, window: 200_000 },
          },
          usageCoverage: "exact",
          evidence: { turns: { unit: "assistant-turn", count: 1, comparable: true } },
        });
      })();
    },
  });

  const priorStaffingSource = process.env.NORTH_STAFFING_SOURCE;
  try {
    process.env.NORTH_STAFFING_SOURCE = "file";
    await spawn({ prompt: "hit repeated errors", agentId: "test-struggle-lane",
      routingMetadata: presetRequest("integrator"),
      provider: "anthropic", pinEvidence: pinEvidence("anthropic"), queryFn });
  } finally {
    if (priorStaffingSource === undefined) delete process.env.NORTH_STAFFING_SOURCE;
    else process.env.NORTH_STAFFING_SOURCE = priorStaffingSource;
  }

  const facts = await waitForRunFactProjection("test-struggle-lane");
  const factValues = (predicate: string) => facts
    .filter(([candidate]) => candidate === predicate)
    .map(([, value]) => value);
  expect(factValues("struggle")).toEqual(["consecutive_errors"]);
  expect(factValues("error_count")).toEqual(["3"]);
  expect(factValues("struggle_detector_policy_version"))
    .toEqual(["north:struggle-observer:v2"]);
  expect(factValues("struggle_error_streak_threshold")).toEqual(["3"]);
  // The run still finished normally at its immutable admitted route.
  const logged = readFileSync(log, "utf8");
  expect(logged).toContain("tell agent:test-struggle-lane model claude-opus-5");
  expect(logged).not.toContain("outcome provider_escalation_unsupported");
});
