import { expect, test } from "bun:test";
import {
  classifyTurnProvenance, codexTurnActivityFromResult, newRunId,
  authoringAuthoritySurfaceEvidence, applyTerminalCoordinatorReadTimeout,
  runEstimateFromThreadFacts, runFacts,
} from "../src/telemetry";
import {
  assessThreadDelivery, RUN_BAR_EVIDENCE_VERSION, validRunEntity,
} from "../src/delivery-verification";
import { makeStruggleObserver, resolveStrugglePolicy } from "../src/struggle";
import {
  providerJoinEvidence, providerSessionKey, providerTurnKey,
} from "../src/providers/provider-join";

// Mirror of the fram coord_daemon log-split contract (coord_daemon.clj
// subject-token + default-telemetry-kinds). A subject routes to telemetry.log
// iff its stored `kind` OR — kind-less — the token before its first colon is in
// this allow-list. A run's body facts are written BEFORE its `kind run` commit
// marker, so during that window the run subject is kind-less and MUST carry a
// colon token to route correctly. A dash-form `@run-…` id has no colon → token
// undefined → its body facts misroute to coordination.log (the 2026-07-17
// regression). This guards the id format so that never recurs.
const TELEMETRY_KINDS = new Set(["run", "session", "mine", "guard_denial"]);

test("terminal publication derives the coordinator read timeout without overriding callers", () => {
  const derived: NodeJS.ProcessEnv = {};
  applyTerminalCoordinatorReadTimeout(derived);
  expect(derived.NORTH_COORD_READ_TIMEOUT_MS).toBe("70000");

  const explicit: NodeJS.ProcessEnv = { NORTH_COORD_READ_TIMEOUT_MS: "45000" };
  applyTerminalCoordinatorReadTimeout(explicit);
  expect(explicit.NORTH_COORD_READ_TIMEOUT_MS).toBe("45000");
});

function subjectToken(subject: string): string | undefined {
  const s = subject.startsWith("@") ? subject : `@${subject}`;
  const colon = s.indexOf(":");
  return colon > 0 ? s.slice(1, colon) : undefined;
}

test("a minted run subject routes to telemetry.log before its kind marker lands", () => {
  for (const agent of ["lane-abc123", "sdk-spawn-mrok0z6m-165cef51", "codex-work"]) {
    const runId = newRunId(agent);
    // kind-less window: routing falls back to the first-colon token, which must
    // be an allow-listed telemetry kind or the body facts land in coordination.log.
    const token = subjectToken(runId);
    expect(token).toBe("run");
    expect(TELEMETRY_KINDS.has(token as string)).toBe(true);
    // and the id must still validate as a run entity (both `@run-`/`@run:` forms).
    expect(validRunEntity(`@${runId}`)).toBe(true);
  }
});

test("a completed run carries every mandatory terminal predicate", () => {
  // The dark-telemetry symptom (2026-07-17..20) was @run subjects reduced to a
  // lone `kind run`. A completed run MUST carry its terminal facts.
  const facts = runFacts({
    thread: "@2026-07-20-000000", agent: "lane-complete",
    tokenUsage: {
      inputTokens: 8794, outputTokens: 86323,
      cacheCreateTokens: 165477, cacheReadTokens: 10047431,
      total: 10308025, terminalCount: 1,
      terminalScope: "anthropic_result_terminal", totalStatus: "exact",
    },
    durationMs: 2171896, posture: "spawn", outcome: "ran", processOutcome: "ran",
  });
  const predicates = new Set(facts.map(([predicate]) => predicate));
  for (const mandatory of ["kind", "thread", "agent", "tokens", "duration_ms", "posture", "outcome", "at"]) {
    expect(predicates.has(mandatory)).toBe(true);
  }
  expect(facts).toContainEqual(["tokens", "10308025"]);
  expect(facts).toContainEqual(["outcome", "ran"]);
});

test("managed runs record the exact effective authoring surface and native runs stay unknown", () => {
  const authority = (capabilities: any[]) => ({
    provider: "openai" as const,
    capabilities,
    nativeMultiAgent: "disabled" as const,
    liveInput: "unsupported" as const,
    northEnabledTools: [],
    authoringHooks: "managed-only" as const,
    sandbox: capabilities.includes("filesystem.write")
      ? "workspace-write" as const : "read-only" as const,
    web: "disabled" as const,
    managedTools: [],
  });
  expect(authoringAuthoritySurfaceEvidence({
    executionSource: "north-managed",
    effectiveAuthority: authority(["filesystem.read", "filesystem.search",
      "filesystem.write", "shell", "graph-authoring.fram"]),
  })).toEqual({ surface: "graph", coverage: "exact" });
  expect(authoringAuthoritySurfaceEvidence({
    executionSource: "north-managed",
    effectiveAuthority: authority(["filesystem.read", "filesystem.search",
      "filesystem.write", "shell"]),
  })).toEqual({ surface: "text", coverage: "exact" });
  expect(authoringAuthoritySurfaceEvidence({
    executionSource: "north-managed",
    effectiveAuthority: authority(["filesystem.read", "filesystem.search", "shell.readonly"]),
  })).toEqual({ surface: "none", coverage: "exact" });
  expect(authoringAuthoritySurfaceEvidence({ executionSource: "provider-native" }))
    .toEqual({ surface: "unknown", coverage: "unknown" });

  const facts = runFacts({
    thread: "thread-authoring", agent: "lane-authoring", durationMs: 1,
    posture: "spawn", outcome: "ran", executionSource: "north-managed",
    effectiveAuthority: authority(["filesystem.read", "filesystem.search",
      "filesystem.write", "shell", "graph-authoring.fram"]),
  });
  expect(facts).toContainEqual(["authoring_authority_surface", "graph"]);
  expect(facts).toContainEqual(["authoring_authority_surface_coverage", "exact"]);
});

test("run wall-time comparison varies for under/on/over and preserves no-estimate runs", () => {
  const timing = (durationMs: number) => Object.fromEntries(runFacts({
    thread: "@timed-thread", agent: `lane-${durationMs}`,
    durationMs, estimateHours: "0.001", posture: "atomic", outcome: "ran",
  }, "2026-07-29T00:00:00.000Z"));

  expect(timing(1_800)).toMatchObject({
    duration_ms: "1800", estimate_hours: "0.001", estimate_delta_ms: "-1800",
    estimate_ratio: "0.5", estimate_classification: "under",
  });
  expect(timing(0)).toMatchObject({
    duration_ms: "0", estimate_delta_ms: "-3600",
    estimate_ratio: "0", estimate_classification: "under",
  });
  expect(timing(3_600)).toMatchObject({
    duration_ms: "3600", estimate_hours: "0.001", estimate_delta_ms: "0",
    estimate_ratio: "1", estimate_classification: "on",
  });
  expect(timing(7_200)).toMatchObject({
    duration_ms: "7200", estimate_hours: "0.001", estimate_delta_ms: "3600",
    estimate_ratio: "2", estimate_classification: "over",
  });

  const legacy = Object.fromEntries(runFacts({
    thread: "@legacy-thread", agent: "lane-legacy",
    durationMs: 3_600, posture: "atomic", outcome: "ran",
  }));
  expect(Object.keys(legacy).some((predicate) => predicate.startsWith("estimate_"))).toBe(false);
});

test("dispatch estimate capture accepts one positive estimate and rejects malformed snapshots", () => {
  expect(runEstimateFromThreadFacts([
    { predicate: "title", value: "Timed work" },
    { predicate: "estimate_hours", value: "1.25" },
  ])).toEqual({ hours: "1.25", durationMs: 4_500_000 });
  expect(runEstimateFromThreadFacts([{ predicate: "title", value: "Legacy work" }]))
    .toBeUndefined();
  expect(() => runEstimateFromThreadFacts([
    { predicate: "estimate_hours", value: "1" },
    { predicate: "estimate_hours", value: "2" },
  ])).toThrow("duplicate");
  expect(() => runEstimateFromThreadFacts([
    { predicate: "estimate_hours", value: "0" },
  ])).toThrow("not-positive-finite-hours");
});

test("recurring canaries retain only their reliability roll-up projection", () => {
  const facts = runFacts({
    thread: "@canary-thread", agent: "lane-canary", durationMs: 250,
    posture: "spawn", outcome: "ran", processOutcome: "ran",
    provider: "openai", providerTarget: "codex-personal",
    deliveryOutcome: "reported", deliveryReason: "complete_run_scoped_done_bar_evidence_self_reported",
    deliveryProof: {
      deliveryEvidence: "{\"run\":\"@run:canary\"}",
      deliveryEvidenceSha256: "a".repeat(64),
    },
    routingPinEvidence: {
      policyVersion: "north-routing-pin-v1",
      issuedAt: "2026-07-26T00:00:00.000Z",
      expiresAt: "2026-07-26T00:15:00.000Z",
      reasonCode: "calibration-experiment",
      detail: "recurring-cross-provider-canary:@canary-thread",
      pins: [{ kind: "provider", value: "openai" }],
    },
  } as any);

  const predicates = new Set(facts.map(([predicate]) => predicate));
  expect(facts).toHaveLength(18);
  expect(predicates).toEqual(new Set([
    "kind", "thread", "agent", "agent_run_ledger_version", "run_event_status",
    "duration_ms", "posture", "outcome", "at", "process_outcome",
    "provider", "provider_target", "delivery_outcome", "delivery_reason",
    "delivery_evidence", "delivery_evidence_sha256",
    "routing_pin_reason_code", "routing_pin_detail",
  ]));
  expect(predicates.has("routing_assessment_policy")).toBe(false);
  expect(predicates.has("prompt_composition_applied")).toBe(false);
});

test("a @run model fact is canonicalized at write, never a bare family alias", () => {
  const facts = runFacts({
    thread: "@run-alias", agent: "lane-alias", durationMs: 1,
    posture: "spawn", outcome: "ran", provider: "anthropic", model: "opus",
  });
  expect(facts).toContainEqual(["model", "claude-opus-5"]);
  expect(facts.some(([, v]) => v === "opus")).toBe(false);
});

test("a fallback-death @run drops the routed-intent model rather than write a cross-provider phantom", () => {
  // lane-mrtcfwgj shape: executed provider=anthropic after openai->anthropic
  // fallback, but the routed-intent model gpt-5.6-sol lagged. Write no model.
  const facts = runFacts({
    thread: "@run-phantom", agent: "lane-phantom", durationMs: 1,
    posture: "spawn", outcome: "died", provider: "anthropic", model: "gpt-5.6-sol",
  });
  expect(facts.some(([predicate]) => predicate === "model")).toBe(false);
});

test("a blocked_preflight @run carries the full nested cause chain as preflight_cause", () => {
  const facts = runFacts({
    thread: "@run-preflight", agent: "lane-preflight", durationMs: 1,
    posture: "spawn", outcome: "blocked_preflight", processOutcome: "blocked_preflight",
    preflightCause: "openai_codex_authority_preflight_failed <- cause: rpc handshake refused",
  });
  expect(facts).toContainEqual([
    "preflight_cause",
    "openai_codex_authority_preflight_failed <- cause: rpc handshake refused",
  ]);
});

test("a provider_error @run carries the provider payload as provider_error_detail", () => {
  // thread 019f9cec: without this fact a dead managed lane leaves NOTHING in the
  // graph naming why the provider failed — the frame is dropped, the throw is
  // discarded by the message loop's break, and the managed home is disposed.
  const detail = "provider error terminal: subtype=error_during_execution is_error=true "
    + "failure=openai_provider_execution_failed <- cause: provider turn error";
  const facts = runFacts({
    thread: "@run-provider-error", agent: "lane-provider-error", durationMs: 1,
    posture: "spawn", outcome: "provider_error", processOutcome: "provider_error",
    providerErrorDetail: `${detail}\n  padded`,
  });
  expect(facts).toContainEqual(["provider_error_detail", `${detail} padded`]);
  const [, value] = facts.find(([predicate]) => predicate === "provider_error_detail")!;
  expect(value.length).toBeLessThanOrEqual(1200);
});

test("a North watchdog abort carries its initiating reason and both last-activity observations", () => {
  const facts = runFacts({
    thread: "@run-watchdog", agent: "lane-watchdog", durationMs: 1,
    posture: "spawn", outcome: "watchdog_aborted", processOutcome: "watchdog_aborted",
    watchdogAbort: {
      reason: "north_watchdog_execution_inactivity",
      silenceMs: 1_200_000,
      lastOuter: {
        origin: "outer",
        kind: "outer.assistant.text",
        observedAt: "2026-07-28T01:40:00.000Z",
      },
      lastProvider: {
        origin: "provider",
        kind: "provider.codex.mcp.progress",
        observedAt: "2026-07-28T01:48:48.000Z",
      },
    },
  });
  expect(facts).toContainEqual([
    "watchdog_reason", "north_watchdog_execution_inactivity",
  ]);
  expect(facts).toContainEqual(["watchdog_silence_ms", "1200000"]);
  expect(facts).toContainEqual([
    "watchdog_last_outer_activity",
    JSON.stringify({
      origin: "outer",
      kind: "outer.assistant.text",
      observedAt: "2026-07-28T01:40:00.000Z",
    }),
  ]);
  expect(facts).toContainEqual([
    "watchdog_last_provider_activity",
    JSON.stringify({
      origin: "provider",
      kind: "provider.codex.mcp.progress",
      observedAt: "2026-07-28T01:48:48.000Z",
    }),
  ]);
  expect(facts.some(([predicate]) => predicate === "provider_error_detail")).toBe(false);
});

test("current run telemetry freezes judgment and the full effective detector policy", () => {
  const struggle = makeStruggleObserver(resolveStrugglePolicy("orchestrator", {
    STRUGGLE_ERROR_STREAK: "4",
    STRUGGLE_LOOP_REPEAT: "3",
    STRUGGLE_LOOP_WINDOW: "24",
    STRUGGLE_STALL_TURNS: "8",
    STRUGGLE_STALL_TURNS_ORCHESTRATOR: "16",
  }));
  const facts = runFacts({
    thread: "thread-grade", agent: "lane-grade", durationMs: 1,
    posture: "composite", outcome: "ran",
    judgmentGrade: { grade: "l", status: "valid", source: "thread" },
    struggleObservation: struggle.snapshot(),
  });
  for (const expected of [
    ["judgment_grade", "l"],
    ["judgment_grade_status", "valid"],
    ["judgment_grade_source", "thread"],
    ["struggle_detector_policy_version", "north:struggle-observer:v1"],
    ["struggle_topology", "orchestrator"],
    ["struggle_error_streak_threshold", "4"],
    ["struggle_loop_repeat_threshold", "3"],
    ["struggle_loop_window", "24"],
    ["struggle_no_progress_turn_threshold", "16"],
    ["error_count", "0"],
  ]) expect(facts).toContainEqual(expected);

  const adHoc = runFacts({
    thread: "(ad-hoc)", agent: "lane-ad-hoc", durationMs: 1,
    posture: "spawn", outcome: "ran",
    judgmentGrade: { status: "unavailable", source: "ad-hoc" },
    struggleObservation: makeStruggleObserver(resolveStrugglePolicy("worker", {})).snapshot(),
  });
  expect(adHoc).toContainEqual(["judgment_grade_status", "unavailable"]);
  expect(adHoc).toContainEqual(["judgment_grade_source", "ad-hoc"]);
  expect(adHoc.some(([predicate]) => predicate === "judgment_grade")).toBe(false);
});

test("telemetry rejects internally inconsistent observation snapshots", () => {
  const base = {
    thread: "thread", agent: "lane", durationMs: 1, posture: "atomic", outcome: "ran",
    struggleObservation: makeStruggleObserver(resolveStrugglePolicy("worker", {})).snapshot(),
  };
  expect(() => runFacts({
    ...base,
    judgmentGrade: { grade: "s", status: "unavailable", source: "thread" } as any,
  })).toThrow("invalid run-local judgment_grade snapshot");
  expect(() => runFacts({
    ...base,
    judgmentGrade: { status: "unavailable", source: "ad-hoc" },
    struggleObservation: { ...base.struggleObservation, loopWindow: 2, loopRepeatThreshold: 3 },
  })).toThrow("exceeds loop window");
});

test("run telemetry is token- and routing-based with no price-derived fields", () => {
  expect(runFacts({
    thread: "thread-1",
    agent: "lane-1",
    tokens: 321,
    durationMs: 45,
    posture: "spawn",
    outcome: "ran",
    provider: "openai",
  }, "2026-07-16T00:00:00.000Z")).toEqual([
    ["kind", "run"],
    ["thread", "thread-1"],
    ["agent", "lane-1"],
    ["agent_run_ledger_version", "north-agent-run-ledger:v1"],
    ["run_event_status", "unavailable"],
    ["tokens", "321"],
    ["duration_ms", "45"],
    ["posture", "spawn"],
    ["outcome", "ran"],
    ["at", "2026-07-16T00:00:00.000Z"],
    ["provider", "openai"],
  ]);
});

test("run telemetry carries admission receipt and overlap-safe execution provenance", () => {
  const providerJoin = providerJoinEvidence("openai", {
    sessionId: "session-provenance",
    turnIds: ["turn-provenance-a", "turn-provenance-b"],
    sessionPersistence: "persisted",
  });
  const facts = runFacts({
    thread: "thread-provenance", agent: "lane-provenance", durationMs: 1,
    posture: "spawn", outcome: "ran", provider: "openai",
    executionSource: "north-managed", executionTransport: "codex-cli",
    providerSessionPersistence: "persisted", providerJoin, northSessionId: "north-session",
    threadProvenance: "exact", turnProvenance: "provider-terminal",
    routingAdmissionReceipt: {
      version: 1,
      routingRequestSha256: "a".repeat(64),
      staffingCatalogSha256: "b".repeat(64),
      providerCatalogsSha256: "c".repeat(64),
      routingPolicySha256: "unavailable",
      appliedAxes: { taskGrade: "mid", topology: "worker", tier: "standard", reasoning: "medium", posture: "deliver" },
      overrideEvidence: { changedAxes: [], status: "none" },
      pinEvidenceStatus: "none",
    },
  });
  for (const expected of [
    ["execution_source", "north-managed"],
    ["execution_transport", "codex-cli"],
    ["provider_session_persistence", "persisted"],
    ["provider_join_key_version", "north-provider-join:v1"],
    ["provider_join_coverage", "exact"],
    ["provider_session_key", providerSessionKey("session-provenance")],
    ["provider_turn_key", providerTurnKey("openai", "turn-provenance-a")],
    ["provider_turn_key", providerTurnKey("openai", "turn-provenance-b")],
    ["north_session_id", "north-session"],
    ["thread_provenance", "exact"],
    ["turn_provenance", "provider-terminal"],
    ["routing_assessment_status", "unavailable"],
    ["routing_pin_evidence_status", "none"],
  ]) expect(facts).toContainEqual(expected);
});

test("turn provenance follows terminal phase, not a zero-turn counter", () => {
  expect(classifyTurnProvenance({ type: "result", num_turns: 0 }, "ran"))
    .toBe("provider-terminal");
  expect(classifyTurnProvenance(undefined, "blocked_preflight")).toBe("pre-provider");
  expect(classifyTurnProvenance(undefined, "blocked_spend_guard")).toBe("pre-provider");
  expect(classifyTurnProvenance(undefined, "provider_error")).toBe("unknown");
});

test("telemetry accepts the managed Codex app-server transport distinctly from CLI fallback", () => {
  const facts = runFacts({
    thread: "thread-app-server", agent: "lane-app-server", durationMs: 1,
    posture: "spawn", outcome: "ran", provider: "openai",
    executionSource: "north-managed", executionTransport: "codex-app-server",
  });
  expect(facts).toContainEqual(["execution_transport", "codex-app-server"]);
});

test("a managed Codex app-server terminal records its observed tool-item count, not just turn units", () => {
  // Thread 019f9cc2: every managed lane wrote codex_turn_units=1 and NO
  // codex_tool_items, because the app-server terminal carried no toolItems at
  // all — lanes that provably ran tools (file written, commit harvested) read
  // as "one turn, activity unknown". The app-server terminal now carries the
  // count summed from observed item/completed events, and it must survive the
  // terminal -> RunRecord -> facts translation.
  const codexTurnActivity = codexTurnActivityFromResult({
    type: "result", subtype: "success",
    _north_codex_turn_activity: { turnUnits: 1, toolItems: 7, comparable: false },
  });
  expect(codexTurnActivity).toEqual({ turnUnits: 1, toolItems: 7, comparable: false });
  const facts = runFacts({
    thread: "thread-app-server-items", agent: "lane-app-server-items", durationMs: 1,
    posture: "spawn", outcome: "ran", provider: "openai",
    executionSource: "north-managed", executionTransport: "codex-app-server",
    codexTurnActivity,
  });
  expect(facts).toContainEqual(["codex_turn_units", "1"]);
  expect(facts).toContainEqual(["codex_tool_items", "7"]);
  // The disclaimer travels with the count; num_turns never appears.
  expect(facts).toContainEqual(["codex_turn_metric_comparable", "false"]);
  expect(facts.map(([predicate]) => predicate)).not.toContain("num_turns");
});

test("run telemetry carries bounded native command evidence without raw command or output", () => {
  const facts = runFacts({
    thread: "thread-native-command", agent: "lane-native-command", durationMs: 1,
    posture: "spawn", outcome: "ran", provider: "openai",
    nativeCommandActivity: {
      source: "codex-app-server:item-completed", coverage: "exact",
      totalCommands: 1, successfulCommands: 1, failedCommands: 0, declinedCommands: 0,
      northBinaryProbe: "passed",
      completions: [{
        commandSha256: "a".repeat(64), outputSha256: "b".repeat(64),
        status: "completed", exitCode: 0,
      }],
    },
  });
  for (const expected of [
    ["native_command_activity_source", "codex-app-server:item-completed"],
    ["native_command_activity_coverage", "exact"],
    ["native_north_binary_probe", "passed"],
    ["native_command_total", "1"],
    ["native_command_successful", "1"],
  ]) expect(facts).toContainEqual(expected);
  const serialized = JSON.stringify(facts);
  expect(serialized).not.toContain("command -v north");
  expect(serialized).not.toContain("/nix/store/");
  expect(facts.filter(([predicate]) => predicate === "native_command_completion"))
    .toHaveLength(1);
});

test("run telemetry rejects incomplete or unbounded native command evidence", () => {
  const base = {
    thread: "thread-native-command", agent: "lane-native-command", durationMs: 1,
    posture: "spawn", outcome: "ran",
  };
  expect(() => runFacts({
    ...base,
    nativeCommandActivity: {
      source: "codex-app-server:unsettled", coverage: "unknown",
      totalCommands: 0, northBinaryProbe: "passed", completions: [],
    },
  } as any)).toThrow("unknown native command activity carries terminal evidence");
  expect(() => runFacts({
    ...base,
    nativeCommandActivity: {
      source: "codex-app-server:item-completed", coverage: "exact",
      totalCommands: 33, successfulCommands: 33, failedCommands: 0, declinedCommands: 0,
      northBinaryProbe: "passed",
      completions: Array.from({ length: 33 }, () => ({
        commandSha256: "a".repeat(64), outputSha256: "b".repeat(64),
        status: "completed", exitCode: 0,
      })),
    },
  } as any)).toThrow("invalid native command activity observation");
});

test("run telemetry preserves requested, active, and fallback account targets", () => {
  const facts = runFacts({
    thread: "thread-target", agent: "lane-target", durationMs: 2, posture: "spawn", outcome: "ran",
    provider: "openai", providerTarget: "codex-work", requestedProvider: "auto",
    requestedTarget: "claude-personal", fallbackPath: ["anthropic", "openai"],
    fallbackTargetPath: ["claude-personal", "codex-work"],
    providerReason: "mode=preferential; target=claude-personal; pressure=normal; order=claude-personal -> codex-work",
    allocationMode: "preferential", entitlementPressure: "low",
    fallbackReasons: [{ sequence: 1, reason: "provider_retry_safe_before_acceptance",
      fromTarget: "claude-personal", fromProvider: "anthropic",
      toTarget: "codex-work", toProvider: "openai" }],
  });
  expect(facts).toContainEqual(["provider_target", "codex-work"]);
  expect(facts).toContainEqual(["requested_provider", "auto"]);
  expect(facts).toContainEqual(["requested_target", "claude-personal"]);
  expect(facts).toContainEqual(["fallback_target_path", "claude-personal -> codex-work"]);
  expect(facts).toContainEqual(["provider_reason", "mode=preferential; target=claude-personal; pressure=normal; order=claude-personal -> codex-work"]);
  expect(facts).toContainEqual(["allocation_mode", "preferential"]);
  expect(facts).toContainEqual(["entitlement_pressure", "low"]);
  expect(facts.filter(([predicate]) => predicate === "fallback_reason")).toEqual([["fallback_reason", JSON.stringify({
    sequence: 1, reason: "provider_retry_safe_before_acceptance",
    fromTarget: "claude-personal", fromProvider: "anthropic",
    toTarget: "codex-work", toProvider: "openai",
  })]]);
});

test("run telemetry persists structured exact-model availability evidence", () => {
  const facts = runFacts({
    thread: "thread-model", agent: "lane-model", durationMs: 2,
    posture: "spawn", outcome: "ran", provider: "anthropic",
    providerTarget: "claude-personal", model: "claude-fable-5",
    modelAvailability: {
      provider: "anthropic", targetId: "claude-personal", authMode: "ambient",
      model: "claude-fable-5", observedAt: "2026-07-20T10:00:00.000Z",
      source: "claude-agent-sdk:Query.supportedModels",
      observationDigest: "a".repeat(64),
    },
  });
  expect(facts).toContainEqual(["provider_target", "claude-personal"]);
  expect(facts).toContainEqual(["model", "claude-fable-5"]);
  expect(facts).toContainEqual(["model_availability_target", "claude-personal"]);
  expect(facts).toContainEqual(["model_availability_source", "claude-agent-sdk:Query.supportedModels"]);
  expect(facts).toContainEqual(["model_availability_observed_at", "2026-07-20T10:00:00.000Z"]);
  expect(facts).toContainEqual(["model_availability_model", "claude-fable-5"]);
  expect(facts).toContainEqual(["model_availability_digest", "a".repeat(64)]);
  expect(() => runFacts({
    thread: "thread-model", agent: "lane-model", durationMs: 2,
    posture: "spawn", outcome: "ran", provider: "anthropic",
    providerTarget: "claude-personal", model: "claude-opus-4-8",
    modelAvailability: {
      provider: "anthropic", targetId: "claude-personal", authMode: "ambient",
      model: "claude-fable-5", observedAt: "2026-07-20T10:00:00.000Z",
      source: "claude-agent-sdk:Query.supportedModels",
      observationDigest: "a".repeat(64),
    },
  })).toThrow("does not match the final provider route");
});

test("run telemetry separates wall time, provider time, process terminal, and delivery truth", () => {
  const facts = runFacts({
    thread: "thread-terminal", agent: "lane-terminal",
    durationMs: 1250, providerDurationMs: 900,
    posture: "spawn", outcome: "ran", processOutcome: "ran",
    deliveryOutcome: "unverified",
    deliveryReason: "provider_terminal_success_without_external_verification",
  });
  expect(facts).toContainEqual(["duration_ms", "1250"]);
  expect(facts).toContainEqual(["provider_duration_ms", "900"]);
  expect(facts).toContainEqual(["process_outcome", "ran"]);
  expect(facts).toContainEqual(["delivery_outcome", "unverified"]);
  expect(facts).toContainEqual([
    "delivery_reason", "provider_terminal_success_without_external_verification",
  ]);
});

test("reported run telemetry carries the exact evidence snapshot and digest", () => {
  const assessment = assessThreadDelivery("thread", "agent", [
    { predicate: "done_when", value: "tests pass" },
  ], [
    { predicate: "done_when", value: "tests pass" },
  ], "run-agent", [{
    version: RUN_BAR_EVIDENCE_VERSION,
    run: "@run-agent",
    thread: "@thread",
    reporter: "@agent:agent",
    bar: "tests pass",
    observed: "exit 0",
    recordedAt: "2026-07-18T10:00:00.000Z",
  }]);
  if (assessment.deliveryOutcome !== "reported") throw new Error("expected reported");
  const facts = runFacts({
    thread: "thread", agent: "agent", durationMs: 1, posture: "atomic",
    outcome: "ran", processOutcome: "ran",
    deliveryOutcome: assessment.deliveryOutcome,
    deliveryReason: assessment.deliveryReason,
    deliveryProof: assessment.proof,
  }, "2026-07-18T10:00:01.000Z");
  expect(facts).toContainEqual(["delivery_outcome", "reported"]);
  expect(facts).toContainEqual(["delivery_evidence", assessment.proof.deliveryEvidence]);
  expect(facts).toContainEqual([
    "delivery_evidence_sha256",
    assessment.proof.deliveryEvidenceSha256,
  ]);
  expect(facts.some(([predicate]) => predicate === "delivery_attestation")).toBe(false);
});

test("run telemetry preserves each exact observed token component once", () => {
  const facts = runFacts({
    thread: "thread-2",
    agent: "lane-2",
    tokens: 200,
    tokenUsage: {
      inputTokens: 101,
      outputTokens: 23,
      cacheCreateTokens: 17,
      cacheReadTokens: 59,
      total: 200,
      terminalCount: 1,
      terminalScope: "anthropic_result_terminal",
      totalStatus: "exact",
    },
    durationMs: 45,
    posture: "spawn",
    outcome: "ran",
  }, "2026-07-16T00:00:00.000Z");

  expect(facts.filter(([predicate]) => predicate === "tokens")).toEqual([["tokens", "200"]]);
  expect(facts.filter(([predicate]) => predicate.endsWith("_tokens"))).toEqual([
    ["input_tokens", "101"],
    ["output_tokens", "23"],
    ["cache_create_tokens", "17"],
    ["cache_read_tokens", "59"],
  ]);
  expect(facts).toContainEqual(["usage_terminal_count", "1"]);
  expect(facts).toContainEqual(["usage_scope", "anthropic_result_terminal"]);
  expect(facts).toContainEqual(["usage_total_status", "exact"]);
});

test("run telemetry omits terminal components that were not observed", () => {
  const facts = runFacts({
    thread: "thread-3",
    agent: "lane-3",
    tokenUsage: { inputTokens: 7, terminalCount: 1,
      terminalScope: "anthropic_result_terminal", totalStatus: "unknown_incomplete_terminal" },
    durationMs: 0,
    posture: "atomic",
    outcome: "ran",
  });

  expect(facts).toContainEqual(["input_tokens", "7"]);
  expect(facts.some(([predicate]) => predicate === "tokens")).toBe(false);
  expect(facts.some(([predicate]) => predicate === "output_tokens")).toBe(false);
  expect(facts.some(([predicate]) => predicate.startsWith("cache_") && predicate.endsWith("_tokens"))).toBe(false);
});

test("Codex subset counters are retained without changing its adapter-owned total", () => {
  const facts = runFacts({
    thread: "thread-4", agent: "lane-4", tokens: 999,
    tokenUsage: {
      inputTokens: 100, cachedInputTokens: 60,
      outputTokens: 20, reasoningOutputTokens: 7,
      total: 120, terminalCount: 1,
      terminalScope: "codex_fresh_invocation_thread_cumulative", totalStatus: "exact",
    },
    durationMs: 1, posture: "spawn", outcome: "ran",
  });
  expect(facts).toContainEqual(["tokens", "120"]);
  expect(facts).toContainEqual(["cached_input_tokens", "60"]);
  expect(facts).toContainEqual(["reasoning_output_tokens", "7"]);
  expect(facts).not.toContainEqual(["tokens", "999"]);
});

test("zero and repeated terminals remain queryable without a fabricated token total", () => {
  for (const tokenUsage of [
    { terminalCount: 0, totalStatus: "unknown_no_terminal" as const },
    { terminalCount: 2, terminalScope: "anthropic_result_terminal" as const,
      totalStatus: "unknown_repeated_terminal" as const },
  ]) {
    const facts = runFacts({ thread: "thread-u", agent: "lane-u", tokenUsage,
      tokens: 0,
      durationMs: 0, posture: "spawn", outcome: "died" });
    expect(facts.some(([predicate]) => predicate === "tokens")).toBe(false);
    expect(facts).toContainEqual(["usage_terminal_count", String(tokenUsage.terminalCount)]);
    expect(facts).toContainEqual(["usage_total_status", tokenUsage.totalStatus]);
  }
});

test("prompt economics persists only content-free measurements and exact zero compactions", () => {
  const facts = runFacts({
    thread: "thread-economics", agent: "lane-economics", durationMs: 1,
    posture: "spawn", outcome: "ran", compactions: 0,
    promptComposition: { promptEconomics: {
      compositionVersion: "north-harness-prompt:v1",
      compositionDigest: "a".repeat(64),
      capabilityClass: "authoring", capabilityCount: 4,
      stablePrefixBytes: 1200, uniqueTailBytes: 300, totalBytes: 1500,
      byteMeasurementSource: "node-buffer-byte-length:utf8",
      tokenMeasurementStatus: "unknown",
      tokenMeasurementSource: "authoritative-tokenizer-unavailable",
      providerContextWindowTokens: 400000,
      contextWindowEffectiveFrom: "2026-01-01",
      contextWindowStatus: "observed", contextWindowSource: "orchestration-provider-catalog",
      contextBudgetStatus: "unknown", contextBudgetSource: "north-harness-unconfigured",
      compactionPolicy: "native-auto-compact-enabled",
      compactionPolicyVersion: "north-native-auto-compact:v1",
    } },
  });
  for (const expected of [
    ["prompt_composition_version", "north-harness-prompt:v1"],
    ["capability_class", "authoring"],
    ["prompt_stable_prefix_bytes", "1200"],
    ["prompt_unique_tail_bytes", "300"],
    ["prompt_total_bytes", "1500"],
    ["provider_context_window_tokens", "400000"],
    ["context_budget_status", "unknown"],
    ["compaction_count", "0"],
  ]) expect(facts).toContainEqual(expected);
  expect(JSON.stringify(facts)).not.toContain("CANARY-private-prompt-content");
});
