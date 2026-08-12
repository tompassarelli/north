import { describe, expect, test } from "bun:test";
import { wireTerminalDecision } from "../src/execution-outcome";
import {
  WIRE_MAX_ENTITIES_PER_KIND,
  WIRE_REQUIRED_SEMANTICS,
	WIRE_JSON_MAX_BYTES,
  WIRE_JSON_MAX_STRING_BYTES,
  WIRE_VERSION,
  WireContractError,
  WireEventWriter,
  decodeWireEvent,
  reduceWireEvent,
  reduceWireEvents,
  wireArtifactId,
  wireEventId,
  wireMessageId,
  wireModelCallId,
  wireParentId,
	wireResourceId,
  wireRunId,
  wireToolCallId,
	wireToolArgumentDigest,
  type WireContractErrorCode,
  type WireEvent,
	type WireEventDraft,
  type WireKnownEvent,
} from "../src/wire";

const RUN_ID = wireRunId("run:test-wire-v2");
const TOOL_ID = wireToolCallId("tool:test-wire-v2");
const TIMES = [
  "2026-08-10T00:00:00.000Z",
  "2026-08-10T00:00:01.000Z",
  "2026-08-10T00:00:02.000Z",
  "2026-08-10T00:00:03.000Z",
  "2026-08-10T00:00:04.000Z",
  "2026-08-10T00:00:05.000Z",
] as const;

function rawEvent(
  sequence: number,
  kind: string,
  payload: Readonly<Record<string, unknown>> = {},
): Record<string, unknown> {
	let parentId: string | undefined;
	switch (kind) {
		case "run.started":
			parentId = typeof payload.parentRunId === "string" ? payload.parentRunId : undefined;
			break;
		case "run.progress":
		case "model-call.started":
		case "run.terminated":
			parentId = RUN_ID;
			break;
		case "artifact.published":
		case "resource.pressure":
			parentId = typeof payload.resourceId === "string" ? payload.resourceId : RUN_ID;
			break;
		case "model-call.completed":
			parentId = typeof payload.modelCallId === "string" ? payload.modelCallId : undefined;
			break;
		case "message.recorded":
			parentId = payload.stage === "started"
				? typeof payload.parentToolCallId === "string" ? payload.parentToolCallId
					: typeof payload.modelCallId === "string" ? payload.modelCallId : RUN_ID
				: typeof payload.messageId === "string" ? payload.messageId : undefined;
			break;
		case "tool.admitted":
			parentId = typeof payload.messageId === "string" ? payload.messageId
				: typeof payload.parentToolCallId === "string" ? payload.parentToolCallId
					: typeof payload.modelCallId === "string" ? payload.modelCallId : RUN_ID;
			break;
		case "tool.progress":
		case "tool.terminal":
			parentId = typeof payload.toolCallId === "string" ? payload.toolCallId : undefined;
			break;
	}
  return {
    version: WIRE_VERSION,
    id: `event:${sequence}`,
    runId: RUN_ID,
    sequence,
    at: TIMES[sequence] ?? "2026-08-10T00:01:00.000Z",
    kind,
    essential: true,
    requiredSemantics: WIRE_REQUIRED_SEMANTICS,
		...(parentId === undefined ? {} : { parentId }),
    ...payload,
  };
}

function started(sequence = 0): WireKnownEvent {
  return decodeWireEvent(rawEvent(sequence, "run.started", { lifecycle: "running" })) as WireKnownEvent;
}

function expectContractError(action: () => unknown, code: WireContractErrorCode): void {
  try {
    action();
  } catch (error) {
    expect(error).toBeInstanceOf(WireContractError);
    if (!(error instanceof WireContractError)) throw error;
    expect(error.code).toBe(code);
    return;
  }
  throw new Error(`expected wire contract error ${code}`);
}

describe("wire-v2 decoding", () => {
  test("rejects non-JSON values with a stable typed error", () => {
    expectContractError(() => decodeWireEvent({
      ...rawEvent(0, "run.started", { lifecycle: "running" }),
      extensionValue: Number.NaN,
    }), "malformed_event");
    expectContractError(() => decodeWireEvent({
      ...rawEvent(0, "run.started", { lifecycle: "running" }),
      sparse: new Array(1),
    }), "malformed_event");

    const proxy = new Proxy(rawEvent(0, "run.started", { lifecycle: "running" }), {
      getPrototypeOf: () => { throw new Error("hostile proxy"); },
    });
    expectContractError(() => decodeWireEvent(proxy), "malformed_event");
    expectContractError(() => decodeWireEvent({
      ...rawEvent(0, "run.started", { lifecycle: "running" }),
      oversized: "x".repeat(WIRE_JSON_MAX_STRING_BYTES + 1),
    }), "malformed_event");
  });

  test("normalizes negative zero before persistence", () => {
    const decoded = decodeWireEvent(rawEvent(0, "run.started", {
      lifecycle: "running",
      observed: -0,
    }));
    expect(decoded.extensions?.observed).toBe(0);
    expect(Object.is(decoded.extensions?.observed, -0)).toBe(false);
  });

  test("rejects essential future events and preserves nonessential future events opaquely", () => {
    expectContractError(() => decodeWireEvent({
      ...rawEvent(1, "provider.future"),
      version: "north:wire:v3",
    }), "unsupported_version");

    const decoded = decodeWireEvent({
      ...rawEvent(1, "provider.future", { futurePayload: { answer: 42 } }),
      version: "north:wire:v3",
      essential: false,
    });
    expect(decoded.kind).toBe("provider.future");
    expect(decoded.essential).toBe(false);
    expect(JSON.parse(JSON.stringify(decoded)).futurePayload).toEqual({ answer: 42 });
    expect(decodeWireEvent(JSON.parse(JSON.stringify(decoded)))).toEqual(decoded);
  });

  test("fails closed when any event requires unknown semantics", () => {
    expectContractError(() => decodeWireEvent({
      ...rawEvent(1, "provider.future"),
      version: "north:wire:v3",
      essential: false,
      requiredSemantics: [...WIRE_REQUIRED_SEMANTICS, "north.future-required.v1"],
    }), "unsupported_required_semantics");
  });

  test("requires all known events to carry non-droppable wire semantics", () => {
    expectContractError(() => decodeWireEvent({
      ...rawEvent(0, "run.started", { lifecycle: "running" }),
      essential: false,
    }), "malformed_event");
    expectContractError(() => decodeWireEvent({
      ...rawEvent(0, "run.started", { lifecycle: "running" }),
      requiredSemantics: WIRE_REQUIRED_SEMANTICS.slice(1),
    }), "malformed_event");
    expectContractError(() => decodeWireEvent(rawEvent(1, "run.progress", {
      lifecycle: "running",
      progress: { recentTools: [] },
    })), "malformed_event");
  });

  test("preserves unknown known-version fields in extensions", () => {
    const decoded = decodeWireEvent({
      ...rawEvent(0, "run.started", { lifecycle: "running", futureHint: "keep-me" }),
      extensions: { sourceHint: "adapter" },
    });
    expect(decoded.extensions).toEqual({ sourceHint: "adapter", futureHint: "keep-me" });
  });

  test("keeps provider model identifiers outside the semantic model boundary", () => {
    expectContractError(() => decodeWireEvent(rawEvent(1, "model-call.started", {
      modelCallId: "model-call:1",
      model: { provider: "openai", tier: "senior", modelId: "provider-private-id" },
      attempt: 1,
    })), "malformed_event");
    expectContractError(() => decodeWireEvent(rawEvent(1, "model-call.started", {
      modelCallId: "model-call:1",
      model: { provider: "openai", tier: "provider-private-id" },
      attempt: 1,
    })), "malformed_event");
  });

  test("rejects impossible provider-originated synthetic tool failures", () => {
    expectContractError(() => decodeWireEvent(rawEvent(1, "tool.terminal", {
      toolCallId: TOOL_ID,
      status: "synthetic_failure",
      origin: "provider",
    })), "malformed_event");
  });

	test("requires an exact SHA-256 digest alongside a terminal result artifact id", () => {
		const artifactId = wireArtifactId("artifact:terminal-digest-decode");
		const digest = "a".repeat(64);
		const decoded = decodeWireEvent(rawEvent(1, "tool.terminal", {
			toolCallId: TOOL_ID,
			status: "succeeded",
			origin: "provider",
			resultArtifactId: artifactId,
			resultArtifactDigest: digest,
		}));
		expect(decoded).toMatchObject({ resultArtifactId: artifactId, resultArtifactDigest: digest });
		expect(decoded.extensions).toBeUndefined();

		for (const payload of [
			{ resultArtifactId: artifactId },
			{ resultArtifactDigest: digest },
			{ resultArtifactId: artifactId, resultArtifactDigest: "A".repeat(64) },
			{ resultArtifactId: artifactId, resultArtifactDigest: "a".repeat(63) },
		] as const) {
			expectContractError(() => decodeWireEvent(rawEvent(1, "tool.terminal", {
				toolCallId: TOOL_ID,
				status: "succeeded",
				origin: "provider",
				...payload,
			})), "malformed_event");
		}
	});

	test("tool argument digests are canonical, privacy-bounded, and optional on replay", () => {
		const first = wireToolArgumentDigest({
			z: [{ b: 2, i: "private intent", a: 1 }],
			a: { __intent: "legacy intent", value: true },
		});
		const reordered = wireToolArgumentDigest({
			a: { value: true, __intent: "changed legacy intent" },
			z: [{ a: 1, i: "changed intent", b: 2 }],
		});
		expect(first).toMatch(/^[a-f0-9]{64}$/);
		expect(first).toBe(reordered);
		expect(first).not.toContain("private intent");
		expect(wireToolArgumentDigest({ items: [1, 2] }))
			.not.toBe(wireToolArgumentDigest({ items: [2, 1] }));

		const admitted = decodeWireEvent(rawEvent(1, "tool.admitted", {
			toolCallId: TOOL_ID,
			name: "read",
			schema: { status: "unavailable", reason: "provider omitted schema" },
			argumentDigest: first,
		}));
		expect(admitted.kind === "tool.admitted" ? admitted.argumentDigest : undefined).toBe(first);
		expect(reduceWireEvent(reduceWireEvent(undefined, started()), admitted)
			.toolCalls[TOOL_ID]?.argumentDigest).toBe(first);

		const legacy = decodeWireEvent(rawEvent(1, "tool.admitted", {
			toolCallId: TOOL_ID,
			name: "read",
			schema: { status: "unavailable", reason: "legacy provider" },
		}));
		expect(legacy.kind === "tool.admitted" ? legacy.argumentDigest : undefined).toBeUndefined();
		expectContractError(() => decodeWireEvent(rawEvent(1, "tool.admitted", {
			toolCallId: TOOL_ID,
			name: "read",
			schema: { status: "unavailable", reason: "provider omitted schema" },
			argumentDigest: "A".repeat(64),
		})), "malformed_event");
	});

  test("decodes bounded completion provenance and rejects malformed evidence", () => {
    const modelCallId = wireModelCallId("model-call:evidence");
    const evidence = {
      providerJoin: {
        version: "north-provider-join:v1",
        sessionKey: "a".repeat(64),
        turnKeys: ["b".repeat(64), "c".repeat(64)],
        sessionPersistence: "persisted",
        coverage: "exact",
      },
      turns: { unit: "provider-turn", count: 2, toolItems: 3, comparable: false },
      providerDurationMs: 1_250,
      failure: {
        detail: "provider_timeout",
        landed: { completedTurns: 1, toolItems: 3, mcpCalls: 2, nativeCommands: 1 },
      },
      interrupt: {
        reason: "north_turn_deadline",
        deadlineMs: 60_000,
        inactivityThresholdMs: 10_000,
        lastActivityAgeMs: 10_001,
        openItemCount: 1,
        openItem: { kind: "commandExecution", ageMs: 9_000 },
        eventCount: 3,
      },
    };
    const decoded = decodeWireEvent(rawEvent(2, "model-call.completed", {
      modelCallId,
      status: "failed",
      origin: "provider",
      usage: usage(10, 10),
		usageCoverage: "exact",
      errorCode: "provider_timeout",
      evidence,
    }));
    expect(decoded.kind).toBe("model-call.completed");
    if (decoded.kind !== "model-call.completed") throw new Error("expected model completion");
    expect(decoded.origin).toBe("provider");
    expect(decoded.evidence?.turns).toEqual({
      unit: "provider-turn", count: 2, toolItems: 3, comparable: false,
    });
    expect(decodeWireEvent(JSON.parse(JSON.stringify(decoded)))).toEqual(decoded);

    const anthropic = decodeWireEvent(rawEvent(2, "model-call.completed", {
      modelCallId,
      status: "succeeded",
      origin: "provider",
      usage: usage(10, 10),
		usageCoverage: "exact",
      evidence: { turns: { unit: "assistant-turn", count: 4, comparable: true } },
    }));
    if (anthropic.kind !== "model-call.completed") throw new Error("expected model completion");
    expect(anthropic.evidence?.turns).toEqual({
      unit: "assistant-turn", count: 4, comparable: true,
    });

    expectContractError(() => decodeWireEvent(rawEvent(2, "model-call.completed", {
      modelCallId,
      status: "succeeded",
      usage: usage(10, 10),
		usageCoverage: "exact",
    })), "malformed_event");

		expectContractError(() => decodeWireEvent(rawEvent(2, "model-call.completed", {
			modelCallId,
			status: "succeeded",
			origin: "provider",
			usage: usage(10, 10),
		})), "malformed_event");
		expectContractError(() => decodeWireEvent(rawEvent(2, "model-call.completed", {
			modelCallId,
			status: "cancelled",
			origin: "north",
			usage: usage(10, 10),
			usageCoverage: "exact",
		})), "malformed_event");

    expectContractError(() => decodeWireEvent(rawEvent(2, "model-call.completed", {
      modelCallId,
      status: "failed",
      origin: "provider",
      usage: usage(10, 10),
		usageCoverage: "exact",
      evidence: {
        ...evidence,
        providerJoin: { ...evidence.providerJoin, turnKeys: ["c".repeat(64), "b".repeat(64)] },
      },
    })), "malformed_event");
		for (const providerJoin of [
			{
				version: "north-provider-join:v1",
				turnKeys: ["b".repeat(64)],
				sessionPersistence: "persisted",
				coverage: "exact",
			},
			{
				version: "north-provider-join:v1",
				sessionKey: "a".repeat(64),
				turnKeys: [],
				sessionPersistence: "persisted",
				coverage: "exact",
			},
			{
				version: "north-provider-join:v1",
				turnKeys: [],
				sessionPersistence: "unknown",
				coverage: "partial",
			},
		]) {
			expectContractError(() => decodeWireEvent(rawEvent(2, "model-call.completed", {
				modelCallId,
				status: "failed",
				origin: "provider",
				usage: usage(10, 10),
				usageCoverage: "exact",
				evidence: { providerJoin },
			})), "malformed_event");
		}
    expectContractError(() => decodeWireEvent(rawEvent(2, "model-call.completed", {
      modelCallId,
      status: "failed",
      origin: "provider",
      usage: usage(10, 10),
		usageCoverage: "exact",
      evidence: {
        providerJoin: { ...evidence.providerJoin, sessionId: "raw-provider-session" },
      },
    })), "malformed_event");
    expectContractError(() => decodeWireEvent(rawEvent(2, "model-call.completed", {
      modelCallId,
      status: "failed",
      origin: "provider",
      usage: usage(10, 10),
		usageCoverage: "exact",
      errorCode: "provider_error",
      evidence: { failure: { detail: "not\nnormalized" } },
    })), "malformed_event");
    expectContractError(() => decodeWireEvent(rawEvent(2, "model-call.completed", {
      modelCallId,
      status: "cancelled",
      origin: "north",
      usage: usage(10, 10),
		usageCoverage: "partial",
      evidence: {
        interrupt: { ...evidence.interrupt, stderrTail: ["private provider prose"] },
      },
    })), "malformed_event");
    expectContractError(() => decodeWireEvent(rawEvent(2, "model-call.completed", {
      modelCallId,
      status: "cancelled",
      origin: "north",
      usage: usage(10, 10),
		usageCoverage: "partial",
      evidence: {
        interrupt: { ...evidence.interrupt, eventCounts: { "provider.codex.turn.started": 3 } },
      },
    })), "malformed_event");
  });

	test("rejects provider-named public error codes and non-code failure evidence", () => {
		const modelCallId = wireModelCallId("model-call:public-error-boundary");
		const invalidPublicCodes = [
			"codex_turn_failed",
			"openai_provider_execution_failed",
			"anthropic_transport_failed",
			"claude_process_failed",
			"managed_codex_execution_failed",
			"OpenAI failed",
			"provider failed on gpt-5.6-private",
		] as const;
		for (const errorCode of invalidPublicCodes) {
			expectContractError(() => decodeWireEvent(rawEvent(2, "model-call.completed", {
				modelCallId,
				status: "failed",
				origin: "provider",
				usage: usage(10, 10),
				usageCoverage: "exact",
				errorCode,
				evidence: { failure: { detail: errorCode } },
			})), "malformed_event");
			expectContractError(() => decodeWireEvent(rawEvent(2, "tool.terminal", {
				toolCallId: TOOL_ID,
				status: "failed",
				origin: "provider",
				errorCode,
			})), "malformed_event");
		}

		expectContractError(() => decodeWireEvent(rawEvent(2, "model-call.completed", {
			modelCallId,
			status: "failed",
			origin: "provider",
			usage: usage(10, 10),
			usageCoverage: "exact",
			errorCode: "provider_execution_failed",
			evidence: { failure: { detail: "provider returned private prose" } },
		})), "malformed_event");

		const writer = new WireEventWriter({ runId: wireRunId("run:public-error-boundary") });
		writer.append({ kind: "run.started", lifecycle: "running" });
		writer.append({
			kind: "model-call.started",
			modelCallId,
			model: { provider: "openai", tier: "senior" },
			attempt: 1,
		});
		writer.append({
			kind: "tool.admitted",
			toolCallId: TOOL_ID,
			modelCallId,
			name: "provider-item",
			schema: { status: "unavailable", reason: "test" },
		});
		for (const errorCode of invalidPublicCodes) {
			expectContractError(() => writer.append({
				kind: "model-call.completed",
				modelCallId,
				status: "failed",
				origin: "north",
				usage: writer.snapshot()!.usage,
				usageCoverage: "unavailable",
				errorCode,
				evidence: { failure: { detail: errorCode } },
			}), "malformed_event");
			expectContractError(() => writer.append({
				kind: "tool.terminal",
				toolCallId: TOOL_ID,
				status: "failed",
				origin: "provider",
				errorCode,
			}), "malformed_event");
		}
		expectContractError(() => writer.append({
			kind: "model-call.completed",
			modelCallId,
			status: "failed",
			origin: "north",
			usage: writer.snapshot()!.usage,
			usageCoverage: "unavailable",
			errorCode: "provider_execution_failed",
			evidence: { failure: { detail: "private provider prose" } },
		}), "malformed_event");
		expect(writer.snapshot()?.modelCalls[modelCallId]?.status).toBe("running");
		expect(writer.snapshot()?.toolCalls[TOOL_ID]?.status).toBe("pending");
	});

	test("keeps outer termination details code-owned and rejects arbitrary evidence", () => {
		const privateDetail = "OpenAI process exited: private provider diagnostics";
		const decision = wireTerminalDecision("died", privateDetail, undefined);
		expect(decision).toEqual({
			lifecycle: "failed",
			reason: { code: "provider_process_died", detail: "provider_process_died" },
		});
		expect(JSON.stringify(decision)).not.toContain(privateDetail);
		expect(JSON.stringify(decision)).not.toContain("OpenAI");

		expectContractError(() => decodeWireEvent(rawEvent(1, "run.terminated", {
			lifecycle: "failed",
			reason: { code: "provider_error", detail: privateDetail },
		})), "malformed_event");
		expectContractError(() => decodeWireEvent(rawEvent(1, "run.terminated", {
			lifecycle: "failed",
			reason: {
				code: "provider_error",
				detail: "provider_error",
				evidence: { diagnostics: privateDetail },
			},
		})), "malformed_event");
	});
});

describe("wire-v2 reduction", () => {
  test("incremental and replay reduction produce the same snapshot", () => {
    let tick = 0;
    const writer = new WireEventWriter({
      runId: RUN_ID,
      eventId: (sequence) => wireEventId(`writer-event:${sequence}`),
      now: () => TIMES[tick++] ?? "2026-08-10T00:01:00.000Z",
    });
    writer.append({ kind: "run.started", lifecycle: "running", owner: "test" });
    writer.append({
      kind: "tool.admitted",
      toolCallId: TOOL_ID,
      name: "read",
      schema: { status: "unavailable", reason: "provider omitted schema" },
      argumentPreview: "README.md",
    });
    writer.append({
      kind: "tool.progress",
      toolCallId: TOOL_ID,
      progress: { bytesRead: 128 },
    });
    writer.append({
      kind: "tool.terminal",
      toolCallId: TOOL_ID,
      status: "succeeded",
      origin: "provider",
      resultPreview: "contents",
    });
    writer.append({
      kind: "run.terminated",
      lifecycle: "completed",
      reason: { code: "completed" },
    });

    expect(writer.snapshot()).toEqual(reduceWireEvents(writer.events()));
  });

  test("retains exact model completion evidence across replay", () => {
    const modelCallId = wireModelCallId("model-call:replay-evidence");
    const events = [
      started(),
      decodeWireEvent(rawEvent(1, "model-call.started", {
        modelCallId,
        model: { provider: "anthropic", tier: "senior" },
        effort: "high",
        attempt: 1,
      })),
      decodeWireEvent(rawEvent(2, "model-call.completed", {
        modelCallId,
        status: "succeeded",
        origin: "provider",
        usage: usage(20, 15),
		usageCoverage: "exact",
        evidence: {
          turns: { unit: "assistant-turn", count: 3, comparable: true },
          providerDurationMs: 900,
        },
      })),
    ];
    const snapshot = reduceWireEvents(events);
    expect(snapshot.modelCalls[modelCallId]?.origin).toBe("provider");
    expect(snapshot.modelCalls[modelCallId]?.evidence).toEqual({
      turns: { unit: "assistant-turn", count: 3, comparable: true },
      providerDurationMs: 900,
    });
    expect(reduceWireEvents(events)).toEqual(snapshot);
  });

  test("batch append is atomic when a later event violates the contract", () => {
    const writer = new WireEventWriter({
      runId: RUN_ID,
      eventId: (sequence) => wireEventId(`batch-event:${sequence}`),
      now: () => "2026-08-10T00:00:00.000Z",
    });
    expectContractError(() => writer.appendAll([
      { kind: "run.started", lifecycle: "running" },
      {
        kind: "tool.terminal",
        toolCallId: TOOL_ID,
        status: "failed",
        origin: "provider",
      },
    ]), "state_violation");
    expect(writer.events()).toEqual([]);
    expect(writer.snapshot()).toBeUndefined();

		const eventBound = new WireEventWriter({
			runId: wireRunId("run:event-bound"),
			maxEvents: 2,
		});
		eventBound.append({ kind: "run.started", lifecycle: "running" });
		expectContractError(() => eventBound.append({
			kind: "run.progress",
			lifecycle: "running",
			progress: {},
		}), "state_violation");
		expect(eventBound.events()).toHaveLength(1);
		eventBound.terminate({ lifecycle: "completed", reason: { code: "completed" } });
		expect(eventBound.events().map((event) => event.kind)).toEqual([
			"run.started", "run.terminated",
		]);

		const byteBound = new WireEventWriter({
			runId: wireRunId("run:byte-bound"),
			maxBytes: 1,
		});
		expectContractError(
			() => byteBound.append({ kind: "run.started", lifecycle: "running" }),
			"state_violation",
		);
		expect(byteBound.snapshot()).toBeUndefined();
  });

	test("reserves enough event slots to close every admitted lifecycle exactly once", () => {
		const runId = wireRunId("run:event-termination-reserve");
		const modelCallId = wireModelCallId("model-call:event-termination-reserve");
		const messageId = wireMessageId("message:event-termination-reserve");
		const toolCallId = wireToolCallId("tool:event-termination-reserve");
		const writer = new WireEventWriter({
			runId,
			maxEvents: 8,
			eventId: (sequence) => wireEventId(`event-reserve:${sequence}`),
			now: () => "2026-08-10T00:00:00.000Z",
		});
		writer.append({ kind: "run.started", lifecycle: "running" });
		writer.append({
			kind: "model-call.started",
			modelCallId,
			model: { provider: "anthropic", tier: "senior" },
			effort: "high",
			attempt: 1,
		});
		writer.append({
			kind: "message.recorded",
			messageId,
			modelCallId,
			stage: "started",
			role: "assistant",
		});
		writer.append({
			kind: "tool.admitted",
			toolCallId,
			messageId,
			modelCallId,
			name: "exec",
			schema: { status: "unavailable", reason: "provider omitted schema" },
		});

		const beforeEvents = writer.events();
		const beforeSnapshot = writer.snapshot();
		expectContractError(() => writer.append({
			kind: "run.progress",
			lifecycle: "running",
			progress: { currentAction: "would consume the terminal reserve" },
		}), "state_violation");
		expect(writer.events()).toEqual(beforeEvents);
		expect(writer.snapshot()).toBe(beforeSnapshot);

		const emitted = writer.terminate({
			lifecycle: "failed",
			reason: { code: "provider_error" },
		});
		expect(emitted.map((event) => event.kind)).toEqual([
			"tool.terminal", "message.recorded", "model-call.completed", "run.terminated",
		]);
		expect(writer.events()).toHaveLength(8);
		expect(writer.events().filter((event) =>
			event.kind === "tool.terminal" && event.toolCallId === toolCallId)).toHaveLength(1);
		expect(writer.events().filter((event) =>
			event.kind === "message.recorded" && event.messageId === messageId
			&& event.stage === "completed")).toHaveLength(1);
		expect(writer.events().filter((event) =>
			event.kind === "model-call.completed" && event.modelCallId === modelCallId)).toHaveLength(1);
		expect(writer.events().filter((event) => event.kind === "run.terminated")).toHaveLength(1);
	});

	test("reserves enough bytes to close every admitted lifecycle exactly once", () => {
		const runId = wireRunId("run:byte-termination-reserve");
		const modelCallId = wireModelCallId("model-call:byte-termination-reserve");
		const messageId = wireMessageId("message:byte-termination-reserve");
		const toolCallId = wireToolCallId("tool:byte-termination-reserve");
		const maxBytes = WIRE_JSON_MAX_BYTES + 32_000;
		const writer = new WireEventWriter({
			runId,
			maxEvents: 128,
			maxBytes,
			eventId: (sequence) => wireEventId(`byte-reserve:${sequence}`),
			now: () => "2026-08-10T00:00:00.000Z",
		});
		writer.append({ kind: "run.started", lifecycle: "running" });
		writer.append({
			kind: "model-call.started",
			modelCallId,
			model: { provider: "openai", tier: "senior" },
			effort: "high",
			attempt: 1,
		});
		writer.append({
			kind: "message.recorded",
			messageId,
			modelCallId,
			stage: "started",
			role: "assistant",
		});
		writer.append({
			kind: "tool.admitted",
			toolCallId,
			messageId,
			modelCallId,
			name: "command",
			schema: { status: "unavailable", reason: "provider omitted schema" },
		});

		let rejectedSequence: number | undefined;
		for (let index = 0; index < 32; index += 1) {
			const sequence = writer.events().length;
			const beforeEvents = writer.events();
			const beforeSnapshot = writer.snapshot();
			try {
				writer.append({
					kind: "run.progress",
					lifecycle: "running",
					progress: { currentAction: "x".repeat(4_096) },
				});
			} catch (error) {
				expect(error).toBeInstanceOf(WireContractError);
				if (!(error instanceof WireContractError)) throw error;
				expect(error.code).toBe("state_violation");
				expect(writer.events()).toEqual(beforeEvents);
				expect(writer.snapshot()).toBe(beforeSnapshot);
				rejectedSequence = sequence;
				break;
			}
		}
		expect(rejectedSequence).toBeDefined();
		if (rejectedSequence === undefined) throw new Error("expected byte-reserve admission refusal");
		const encoder = new TextEncoder();
		const bytesBefore = writer.events().reduce(
			(total, event) => total + encoder.encode(JSON.stringify(event)).byteLength + 1,
			0,
		);
		const rejectedEvent = decodeWireEvent({
			version: WIRE_VERSION,
			id: `byte-reserve:${rejectedSequence}`,
			runId,
			parentId: wireParentId(runId),
			sequence: rejectedSequence,
			at: "2026-08-10T00:00:00.000Z",
			kind: "run.progress",
			essential: true,
			requiredSemantics: WIRE_REQUIRED_SEMANTICS,
			lifecycle: "running",
			progress: { currentAction: "x".repeat(4_096) },
		});
		const rejectedBytes = encoder.encode(JSON.stringify(rejectedEvent)).byteLength + 1;
		expect(bytesBefore + rejectedBytes).toBeLessThanOrEqual(maxBytes);

		const eventsBeforeTermination = writer.events().length;
		const emitted = writer.terminate({
			lifecycle: "failed",
			reason: { code: "provider_error" },
		});
		expect(emitted.map((event) => event.kind)).toEqual([
			"tool.terminal", "message.recorded", "model-call.completed", "run.terminated",
		]);
		expect(writer.events()).toHaveLength(eventsBeforeTermination + 4);
		expect(writer.events().filter((event) =>
			event.kind === "tool.terminal" && event.toolCallId === toolCallId)).toHaveLength(1);
		expect(writer.events().filter((event) =>
			event.kind === "message.recorded" && event.messageId === messageId
			&& event.stage === "completed")).toHaveLength(1);
		expect(writer.events().filter((event) =>
			event.kind === "model-call.completed" && event.modelCallId === modelCallId)).toHaveLength(1);
		expect(writer.events().filter((event) => event.kind === "run.terminated")).toHaveLength(1);
		const finalBytes = writer.events().reduce(
			(total, event) => total + encoder.encode(JSON.stringify(event)).byteLength + 1,
			0,
		);
		expect(finalBytes).toBeLessThanOrEqual(maxBytes);
	});

	test("derives every known event parent from typed ancestry", () => {
		const runId = wireRunId("run:derived-parentage");
		const parentRunId = wireRunId("run:parent");
		const resourceId = wireResourceId("resource:derived-parentage");
		const artifactId = wireArtifactId("artifact:derived-parentage");
		const modelCallId = wireModelCallId("model-call:derived-parentage");
		const messageId = wireMessageId("message:derived-parentage");
		const toolCallId = wireToolCallId("tool:derived-parentage");
		const writer = new WireEventWriter({
			runId,
			eventId: (sequence) => wireEventId(`derived-parentage:${sequence}`),
			now: () => "2026-08-10T00:00:00.000Z",
		});
		writer.append({ kind: "run.started", lifecycle: "running" });
		writer.append({
			kind: "artifact.published",
			artifactId,
			resourceId,
			mediaType: "text/plain",
			bytes: 1,
		});
		writer.append({
			kind: "resource.pressure",
			resourceId,
			scope: "run",
			resource: "bytes",
			used: 1,
			reserved: 0,
			limit: 2,
			advisory: true,
		});
		writer.append({
			kind: "model-call.started",
			modelCallId,
			model: { provider: "anthropic" },
			attempt: 1,
		});
		writer.append({
			kind: "message.recorded",
			messageId,
			modelCallId,
			stage: "started",
			role: "assistant",
		});
		writer.append({
			kind: "message.recorded",
			messageId,
			modelCallId,
			stage: "delta",
			role: "assistant",
			content: "working",
		});
		writer.append({
			kind: "tool.admitted",
			toolCallId,
			messageId,
			modelCallId,
			name: "read",
			schema: { status: "unavailable", reason: "fixture" },
		});
		writer.append({ kind: "tool.progress", toolCallId, progress: { used: 1 } });
		writer.append({
			kind: "tool.terminal",
			toolCallId,
			status: "succeeded",
			origin: "provider",
		});
		writer.append({
			kind: "message.recorded",
			messageId,
			modelCallId,
			stage: "completed",
			role: "assistant",
		});
		writer.append({
			kind: "model-call.completed",
			modelCallId,
			status: "succeeded",
			origin: "provider",
			usage: usage(1, 1),
			usageCoverage: "exact",
		});
		writer.append({
			kind: "run.terminated",
			lifecycle: "completed",
			reason: { code: "completed" },
		});

		const events = writer.events();
		expect(events.map((event) => event.parentId)).toEqual([
			undefined,
			wireParentId(resourceId),
			wireParentId(resourceId),
			wireParentId(runId),
			wireParentId(modelCallId),
			wireParentId(messageId),
			wireParentId(messageId),
			wireParentId(toolCallId),
			wireParentId(toolCallId),
			wireParentId(messageId),
			wireParentId(modelCallId),
			wireParentId(runId),
		]);

		const child = new WireEventWriter({ runId: wireRunId("run:child") });
		const childStart = child.append({
			kind: "run.started",
			lifecycle: "running",
			parentRunId,
		});
		expect(childStart.parentId).toBe(wireParentId(parentRunId));
	});

	test("rejects missing, mismatched, orphaned, and caller-authored parentage", () => {
		const initial = reduceWireEvent(undefined, started());
		const missingParent = rawEvent(1, "run.progress", {
			lifecycle: "running",
			progress: {},
		});
		delete missingParent.parentId;
		expectContractError(
			() => reduceWireEvent(initial, decodeWireEvent(missingParent)),
			"state_violation",
		);
		expectContractError(() => reduceWireEvent(initial, decodeWireEvent(rawEvent(1, "run.progress", {
			lifecycle: "running",
			progress: {},
			parentId: wireParentId("run:wrong-parent"),
		}))), "state_violation");
		expectContractError(() => reduceWireEvent(initial, decodeWireEvent(rawEvent(1, "model-call.completed", {
			modelCallId: wireModelCallId("model-call:orphan"),
			status: "failed",
			origin: "provider",
			usage: usage(0, 0, 0),
			usageCoverage: "unavailable",
		}))), "state_violation");

		const writer = new WireEventWriter({ runId: RUN_ID });
		const callerAuthored = {
			kind: "run.started",
			lifecycle: "running",
			parentId: wireParentId(RUN_ID),
		} as unknown as WireEventDraft;
		expectContractError(() => writer.append(callerAuthored), "state_violation");

		const opaque = decodeWireEvent({
			...rawEvent(1, "provider.future", { futurePayload: true }),
			version: "north:wire:v3",
			essential: false,
		});
		expect(reduceWireEvent(initial, opaque).opaqueEvents).toEqual([opaque]);
	});

	test("rejects cross-kind entity ID collisions that would make parentage ambiguous", () => {
		const shared = "entity:cross-kind-collision";
		const modelCallId = wireModelCallId(shared);
		const withModel = reduceWireEvents([
			started(),
			decodeWireEvent(rawEvent(1, "model-call.started", {
				modelCallId,
				model: { provider: "anthropic" },
				attempt: 1,
			})),
		]);
		expectContractError(() => reduceWireEvent(withModel, decodeWireEvent(rawEvent(2, "message.recorded", {
			messageId: wireMessageId(shared),
			modelCallId,
			stage: "started",
			role: "assistant",
		}))), "state_violation");

		const initial = reduceWireEvent(undefined, started());
		expectContractError(() => reduceWireEvent(initial, decodeWireEvent(rawEvent(1, "message.recorded", {
			messageId: wireMessageId(RUN_ID),
			stage: "started",
			role: "assistant",
		}))), "state_violation");
		expectContractError(() => reduceWireEvent(initial, decodeWireEvent(rawEvent(1, "artifact.published", {
			artifactId: wireArtifactId(shared),
			resourceId: wireResourceId(shared),
			mediaType: "text/plain",
			bytes: 1,
		}))), "state_violation");
		expectContractError(() => reduceWireEvent(initial, decodeWireEvent(rawEvent(1, "resource.pressure", {
			resourceId: wireResourceId(RUN_ID),
			scope: "run",
			resource: "bytes",
			used: 1,
			reserved: 0,
			limit: 2,
			advisory: true,
		}))), "state_violation");
	});

	test("bounds referenced resource and nested-run identities transactionally", () => {
		const resourceIds = Array.from(
			{ length: WIRE_MAX_ENTITIES_PER_KIND + 1 },
			(_, index) => wireResourceId(`resource:entity-capacity:${index}`),
		);
		const resources = new WireEventWriter({ runId: wireRunId("run:resource-entity-capacity") });
		resources.append({ kind: "run.started", lifecycle: "running" });
		for (let offset = 0; offset < WIRE_MAX_ENTITIES_PER_KIND; offset += 256) {
			resources.append({
				kind: "run.progress",
				lifecycle: "running",
				progress: {
					outputReferences: resourceIds.slice(offset, offset + 256).map((resourceId) => ({
						kind: "resource" as const,
						resourceId,
					})),
				},
			});
		}
		const resourceBoundary = resources.snapshot();
		if (!resourceBoundary) throw new Error("expected resource capacity snapshot");
		expect(resourceBoundary.lastSequence).toBe(4);
		expectContractError(() => resources.append({
			kind: "run.progress",
			lifecycle: "running",
			progress: {
				outputReferences: [{
					kind: "resource",
					resourceId: resourceIds[WIRE_MAX_ENTITIES_PER_KIND]!,
				}],
			},
		}), "state_violation");
		expect(resources.snapshot()).toBe(resourceBoundary);
		const reusedResource = resources.append({
			kind: "run.progress",
			lifecycle: "running",
			progress: {
				outputReferences: [{ kind: "resource", resourceId: resourceIds[0]! }],
			},
		});
		expect(reusedResource.sequence).toBe(5);

		const nestedIds = Array.from(
			{ length: WIRE_MAX_ENTITIES_PER_KIND },
			(_, index) => wireRunId(`run:nested-entity-capacity:${index}`),
		);
		const nested = new WireEventWriter({ runId: wireRunId("run:nested-entity-capacity-root") });
		nested.append({ kind: "run.started", lifecycle: "running" });
		for (let offset = 0; offset < WIRE_MAX_ENTITIES_PER_KIND - 1; offset += 128) {
			nested.append({
				kind: "run.progress",
				lifecycle: "running",
				progress: {
					nested: nestedIds.slice(
						offset,
						Math.min(offset + 128, WIRE_MAX_ENTITIES_PER_KIND - 1),
					).map((runId) => ({ runId, lifecycle: "running" as const })),
				},
			});
		}
		const nestedBoundary = nested.snapshot();
		if (!nestedBoundary) throw new Error("expected nested-run capacity snapshot");
		expect(nestedBoundary.lastSequence).toBe(8);
		expectContractError(() => nested.append({
			kind: "run.progress",
			lifecycle: "running",
			progress: {
				nested: [{
					runId: nestedIds[WIRE_MAX_ENTITIES_PER_KIND - 1]!,
					lifecycle: "running",
				}],
			},
		}), "state_violation");
		expect(nested.snapshot()).toBe(nestedBoundary);
		const reusedNested = nested.append({
			kind: "run.progress",
			lifecycle: "running",
			progress: {
				nested: [{ runId: nestedIds[0]!, lifecycle: "running" }],
			},
		});
		expect(reusedNested.sequence).toBe(9);
	});

	test("restores and replays an explicit-parent prefix before synthetic closure", () => {
		const modelCallId = wireModelCallId("model-call:restore-parentage");
		const messageId = wireMessageId("message:restore-parentage");
		const toolCallId = wireToolCallId("tool:restore-parentage");
		const source = new WireEventWriter({
			runId: RUN_ID,
			eventId: (sequence) => wireEventId(`restore-prefix:${sequence}`),
			now: () => "2026-08-10T00:00:00.000Z",
		});
		source.append({ kind: "run.started", lifecycle: "running" });
		source.append({
			kind: "model-call.started",
			modelCallId,
			model: { provider: "openai" },
			attempt: 1,
		});
		source.append({
			kind: "message.recorded",
			messageId,
			modelCallId,
			stage: "started",
			role: "assistant",
		});
		source.append({
			kind: "tool.admitted",
			toolCallId,
			messageId,
			modelCallId,
			name: "exec",
			schema: { status: "unavailable", reason: "fixture" },
		});

		const restored = WireEventWriter.restore(source.events(), {
			eventId: (sequence) => wireEventId(`restore-suffix:${sequence}`),
			now: () => "2026-08-10T00:00:01.000Z",
		});
		const closure = restored.terminate({
			lifecycle: "failed",
			reason: { code: "provider_error" },
		});
		expect(closure.map((event) => event.parentId)).toEqual([
			wireParentId(toolCallId),
			wireParentId(messageId),
			wireParentId(modelCallId),
			wireParentId(RUN_ID),
		]);
		expect(restored.snapshot()).toEqual(reduceWireEvents(restored.events()));

		const damaged = source.events().map((event) => ({ ...event }));
		delete damaged[1]?.parentId;
		expectContractError(() => WireEventWriter.restore(damaged), "state_violation");
	});

  test("rejects duplicate event IDs in replay and writer-owned streams", () => {
    const replayDuplicate = decodeWireEvent({
      ...rawEvent(1, "run.progress", { lifecycle: "running", progress: {} }),
      id: started().id,
    });
    expectContractError(() => reduceWireEvents([started(), replayDuplicate]), "sequence_violation");
		const incremental = reduceWireEvent(undefined, started());
		expectContractError(
			() => reduceWireEvent(incremental, replayDuplicate),
			"sequence_violation",
		);

    const duplicateId = wireEventId("writer-event:duplicate");
    const writer = new WireEventWriter({
      runId: RUN_ID,
      eventId: () => duplicateId,
      now: () => "2026-08-10T00:00:00.000Z",
    });
    writer.append({ kind: "run.started", lifecycle: "running" });
    expectContractError(() => writer.append({
      kind: "run.progress",
      lifecycle: "running",
      progress: {},
    }), "sequence_violation");
    expect(writer.events().map((event) => event.kind)).toEqual(["run.started"]);
    expect(writer.snapshot()?.lastSequence).toBe(0);
  });

	test("incremental duplicate detection survives a balanced event-id index", () => {
		let snapshot = reduceWireEvent(undefined, started());
		const ids: string[] = [];
		for (let sequence = 1; sequence <= 128; sequence++) {
			const id = `event:balanced:${(sequence * 73) % 257}`;
			ids.push(id);
			snapshot = reduceWireEvent(snapshot, decodeWireEvent({
				...rawEvent(sequence, "run.progress", {
					lifecycle: "running",
					progress: { currentAction: `step-${sequence}` },
				}),
				id,
			}));
		}
		expectContractError(() => reduceWireEvent(snapshot, decodeWireEvent({
			...rawEvent(129, "run.progress", {
				lifecycle: "running",
				progress: { currentAction: "duplicate" },
			}),
			id: ids[37]!,
		})), "sequence_violation");
	});

  test("tracks lifetime and current-context usage independently", () => {
    const events = [
      started(),
      decodeWireEvent(rawEvent(1, "run.progress", {
        lifecycle: "running",
        progress: { usage: usage(100, 80, 0) },
      })),
      decodeWireEvent(rawEvent(2, "run.progress", {
        lifecycle: "running",
        progress: { usage: usage(140, 30, 0) },
      })),
    ];
    const snapshot = reduceWireEvents(events);
    expect(snapshot.usage.lifetime.inputTokens).toBe(140);
    expect(snapshot.usage.context.tokens).toBe(30);
		expectContractError(() => reduceWireEvent(snapshot, decodeWireEvent(rawEvent(3, "run.progress", {
			lifecycle: "running",
			progress: { usage: usage(141, 31, 1) },
		}))), "state_violation");

    const decreasing = decodeWireEvent(rawEvent(3, "run.progress", {
      lifecycle: "running",
      progress: { usage: usage(139, 20, 0) },
    }));
    expectContractError(() => reduceWireEvent(snapshot, decreasing), "state_violation");
  });

	test("derives cumulative usage authority from replayable completion coverage", () => {
		const firstCall = wireModelCallId("model-call:coverage-first");
		const secondCall = wireModelCallId("model-call:coverage-second");
		const writer = new WireEventWriter({ runId: RUN_ID });
		writer.append({ kind: "run.started", lifecycle: "running" });
		writer.append({
			kind: "model-call.started",
			modelCallId: firstCall,
			model: { provider: "openai", tier: "standard" },
			attempt: 1,
		});
		writer.append({
			kind: "model-call.completed",
			modelCallId: firstCall,
			status: "succeeded",
			origin: "provider",
			usage: usage(10, 10),
			usageCoverage: "exact",
		});
		expect(writer.snapshot()?.usageCoverage).toEqual({
			providerTerminalCount: 1,
			scope: "wire_run_cumulative",
			totalStatus: "exact",
		});

		writer.append({
			kind: "run.progress",
			lifecycle: "running",
			progress: { usage: usage(10, 10) },
		});
		expect(writer.snapshot()?.usageCoverage.totalStatus).toBe("partial");
		writer.append({
			kind: "model-call.started",
			modelCallId: secondCall,
			model: { provider: "openai", tier: "standard" },
			attempt: 1,
		});
		writer.append({
			kind: "model-call.completed",
			modelCallId: secondCall,
			status: "succeeded",
			origin: "provider",
			usage: usage(20, 20, 2),
			usageCoverage: "exact",
		});
		expect(writer.snapshot()?.usageCoverage).toEqual({
			providerTerminalCount: 2,
			scope: "wire_run_cumulative",
			totalStatus: "exact",
		});
		expect(reduceWireEvents(writer.events()).usageCoverage).toEqual(
			writer.snapshot()?.usageCoverage,
		);
	});

	test("unavailable usage cannot change cumulative counters", () => {
		const modelCallId = wireModelCallId("model-call:coverage-unavailable");
		const writer = new WireEventWriter({ runId: RUN_ID });
		writer.append({ kind: "run.started", lifecycle: "running" });
		writer.append({
			kind: "model-call.started",
			modelCallId,
			model: { provider: "openai", tier: "standard" },
			attempt: 1,
		});
		expectContractError(() => writer.append({
			kind: "model-call.completed",
			modelCallId,
			status: "failed",
			origin: "provider",
			usage: usage(1, 1),
			usageCoverage: "unavailable",
		}), "state_violation");
		writer.append({
			kind: "model-call.completed",
			modelCallId,
			status: "failed",
			origin: "provider",
			usage: writer.snapshot()!.usage,
			usageCoverage: "unavailable",
		});
		expect(writer.snapshot()?.usageCoverage).toEqual({
			providerTerminalCount: 1,
			scope: "wire_run_cumulative",
			totalStatus: "unknown_incomplete_terminal",
		});
	});

	test("rejects a provider switch after the first model call", () => {
		const first = wireModelCallId("model-call:provider-immutable-first");
		const writer = new WireEventWriter({ runId: RUN_ID });
		writer.append({ kind: "run.started", lifecycle: "running" });
		writer.append({
			kind: "model-call.started",
			modelCallId: first,
			model: { provider: "anthropic", tier: "standard" },
			attempt: 1,
		});
		writer.append({
			kind: "model-call.completed",
			modelCallId: first,
			status: "succeeded",
			origin: "provider",
			usage: usage(1, 1),
			usageCoverage: "exact",
		});
		expectContractError(() => writer.append({
			kind: "run.progress",
			lifecycle: "running",
			progress: { model: { provider: "openai", tier: "standard" } },
		}), "state_violation");
		expectContractError(() => writer.append({
			kind: "model-call.started",
			modelCallId: wireModelCallId("model-call:provider-immutable-second"),
			model: { provider: "openai", tier: "standard" },
			attempt: 2,
		}), "state_violation");
	});

  test("keeps provider-confirmed context compactions monotonic", () => {
    const snapshot = reduceWireEvents([
      started(),
      decodeWireEvent(rawEvent(1, "run.progress", {
        lifecycle: "running",
        progress: { compactions: 2 },
      })),
    ]);
    expect(snapshot.compactions).toBe(2);
    expectContractError(() => reduceWireEvent(snapshot, decodeWireEvent(rawEvent(2, "run.progress", {
      lifecycle: "running",
      progress: { compactions: 1 },
    }))), "state_violation");
  });

  test("nullable progress fields remove prior state during replay", () => {
    const withState = reduceWireEvents([
      started(),
      decodeWireEvent(rawEvent(1, "run.progress", {
        lifecycle: "waiting",
        progress: {
          currentAction: "waiting for approval",
          effort: "high",
          branch: { name: "wire-v2" },
        },
      })),
    ]);
    const cleared = reduceWireEvent(withState, decodeWireEvent(rawEvent(2, "run.progress", {
      lifecycle: "running",
      progress: { currentAction: null, effort: null, branch: null },
    })));
    expect(cleared.currentAction).toBeUndefined();
    expect(cleared.effort).toBeUndefined();
    expect(cleared.branch).toBeUndefined();
  });

  test("does not retain mutable references from direct event objects", () => {
    const initial = reduceWireEvent(undefined, rawEvent(0, "run.started", {
      lifecycle: "running",
    }) as unknown as WireEvent);
    const rawModel = rawEvent(1, "model-call.started", {
      modelCallId: "model-call:mutable",
      model: { provider: "openai", tier: "senior" },
      attempt: 1,
    });
    const snapshot = reduceWireEvent(initial, rawModel as unknown as WireEvent);
    const model = rawModel.model;
    if (model === null || typeof model !== "object" || Array.isArray(model)) {
      throw new Error("model fixture must be an object");
    }
    (model as Record<string, unknown>).tier = "frontier";
    expect(snapshot.model?.tier).toBe("senior");
  });

  test("enforces run and nested identity references", () => {
    expectContractError(() => reduceWireEvent(undefined, decodeWireEvent(rawEvent(0, "run.started", {
      lifecycle: "running",
      parentRunId: RUN_ID,
    }))), "state_violation");

    const initial = reduceWireEvent(undefined, started());
    expectContractError(() => reduceWireEvent(initial, decodeWireEvent(rawEvent(1, "message.recorded", {
      messageId: "message:orphan",
      modelCallId: "model-call:missing",
      stage: "started",
      role: "assistant",
    }))), "state_violation");
    expectContractError(() => reduceWireEvent(initial, decodeWireEvent(rawEvent(1, "tool.admitted", {
      toolCallId: TOOL_ID,
      parentToolCallId: TOOL_ID,
      name: "nested",
      schema: { status: "unavailable", reason: "fixture" },
    }))), "state_violation");
    expectContractError(() => reduceWireEvent(initial, decodeWireEvent(rawEvent(1, "run.progress", {
      lifecycle: "running",
      progress: { nested: [{ runId: RUN_ID, lifecycle: "running" }] },
    }))), "state_violation");

		const firstCall = wireModelCallId("model-call:first-ancestry");
		const secondCall = wireModelCallId("model-call:second-ancestry");
		const messageId = wireMessageId("message:first-ancestry");
		const ancestry = reduceWireEvents([
			started(),
			decodeWireEvent(rawEvent(1, "model-call.started", {
				modelCallId: firstCall,
				model: { provider: "anthropic" },
				attempt: 1,
			})),
			decodeWireEvent(rawEvent(2, "message.recorded", {
				messageId,
				modelCallId: firstCall,
				stage: "started",
				role: "assistant",
			})),
			decodeWireEvent(rawEvent(3, "model-call.started", {
				modelCallId: secondCall,
				model: { provider: "anthropic" },
				attempt: 1,
			})),
		]);
		expectContractError(() => reduceWireEvent(ancestry, decodeWireEvent(rawEvent(4, "tool.admitted", {
			toolCallId: TOOL_ID,
			messageId,
			modelCallId: secondCall,
			name: "mismatched-ancestry",
			schema: { status: "unavailable", reason: "fixture" },
		}))), "state_violation");

		const firstParent = wireToolCallId("tool:first-parent-ancestry");
		const secondParent = wireToolCallId("tool:second-parent-ancestry");
		const nestedMessageId = wireMessageId("message:parent-tool-ancestry");
		const nested = reduceWireEvents([
			started(),
			decodeWireEvent(rawEvent(1, "tool.admitted", {
				toolCallId: firstParent,
				name: "first-parent",
				schema: { status: "unavailable", reason: "fixture" },
			})),
			decodeWireEvent(rawEvent(2, "tool.admitted", {
				toolCallId: secondParent,
				name: "second-parent",
				schema: { status: "unavailable", reason: "fixture" },
			})),
			decodeWireEvent(rawEvent(3, "message.recorded", {
				messageId: nestedMessageId,
				parentToolCallId: firstParent,
				stage: "started",
				role: "assistant",
			})),
		]);
		expectContractError(() => reduceWireEvent(nested, decodeWireEvent(rawEvent(4, "tool.admitted", {
			toolCallId: wireToolCallId("tool:mismatched-parent-ancestry"),
			messageId: nestedMessageId,
			parentToolCallId: secondParent,
			name: "mismatched-parent-ancestry",
			schema: { status: "unavailable", reason: "fixture" },
		}))), "state_violation");
  });

	test("requires artifact publication before any artifact reference", () => {
		const artifactId = wireArtifactId("artifact:published-before-reference");
		const digest = "a".repeat(64);
		expectContractError(() => reduceWireEvents([
			started(),
			decodeWireEvent(rawEvent(1, "run.progress", {
				lifecycle: "running",
				progress: { outputReferences: [{ kind: "artifact", artifactId }] },
			})),
		]), "state_violation");
		expectContractError(() => reduceWireEvents([
			started(),
			decodeWireEvent(rawEvent(1, "tool.admitted", {
				toolCallId: TOOL_ID,
				name: "artifact-tool",
				schema: { status: "unavailable", reason: "fixture" },
				argumentArtifactId: artifactId,
			})),
		]), "state_violation");
		expectContractError(() => reduceWireEvents([
			started(),
			decodeWireEvent(rawEvent(1, "tool.admitted", {
				toolCallId: TOOL_ID,
				name: "artifact-tool",
				schema: { status: "unavailable", reason: "fixture" },
			})),
			decodeWireEvent(rawEvent(2, "tool.terminal", {
				toolCallId: TOOL_ID,
				status: "succeeded",
				origin: "provider",
				resultArtifactId: artifactId,
				resultArtifactDigest: digest,
			})),
		]), "state_violation");
		expectContractError(() => reduceWireEvents([
			started(),
			decodeWireEvent(rawEvent(1, "artifact.published", {
				artifactId,
				mediaType: "application/json",
				bytes: 12,
				digest,
			})),
			decodeWireEvent(rawEvent(2, "tool.admitted", {
				toolCallId: TOOL_ID,
				name: "artifact-tool",
				schema: { status: "unavailable", reason: "fixture" },
			})),
			decodeWireEvent(rawEvent(3, "tool.terminal", {
				toolCallId: TOOL_ID,
				status: "succeeded",
				origin: "provider",
				resultArtifactId: artifactId,
				resultArtifactDigest: "b".repeat(64),
			})),
		]), "state_violation");

		const snapshot = reduceWireEvents([
			started(),
			decodeWireEvent(rawEvent(1, "artifact.published", {
				artifactId,
				mediaType: "application/json",
				bytes: 12,
				digest,
			})),
			decodeWireEvent(rawEvent(2, "tool.admitted", {
				toolCallId: TOOL_ID,
				name: "artifact-tool",
				schema: { status: "unavailable", reason: "fixture" },
				argumentArtifactId: artifactId,
			})),
			decodeWireEvent(rawEvent(3, "tool.progress", {
				toolCallId: TOOL_ID,
				outputArtifactId: artifactId,
			})),
			decodeWireEvent(rawEvent(4, "tool.terminal", {
				toolCallId: TOOL_ID,
				status: "succeeded",
				origin: "provider",
				resultArtifactId: artifactId,
				resultArtifactDigest: digest,
			})),
			decodeWireEvent(rawEvent(5, "run.progress", {
				lifecycle: "running",
				progress: {
					outputReferences: [{ kind: "artifact", artifactId }],
					patch: { artifactId, filesChanged: 1 },
				},
			})),
		]);
		expect(snapshot.outputReferences).toEqual([{ kind: "artifact", artifactId }]);
		expect(snapshot.patch?.artifactId).toBe(artifactId);
		expect(snapshot.toolCalls[TOOL_ID]?.resultArtifactId).toBe(artifactId);
		expect(snapshot.toolCalls[TOOL_ID]?.resultArtifactDigest).toBe(digest);
	});

  test("treats valid prototype-named identities as ordinary record keys", () => {
    const messageId = wireMessageId("constructor");
    const snapshot = reduceWireEvents([
      started(),
      decodeWireEvent(rawEvent(1, "message.recorded", {
        messageId,
        stage: "started",
        role: "assistant",
      })),
      decodeWireEvent(rawEvent(2, "message.recorded", {
        messageId,
        stage: "completed",
        role: "assistant",
      })),
    ]);
    expect(Object.hasOwn(snapshot.messages, messageId)).toBe(true);
    expect(snapshot.messages[messageId]?.stage).toBe("completed");
  });

  test("rejects orphan and duplicate tool terminals", () => {
    const initial = reduceWireEvent(undefined, started());
    const orphan = decodeWireEvent(rawEvent(1, "tool.terminal", {
      toolCallId: TOOL_ID,
      status: "failed",
      origin: "provider",
    }));
    expectContractError(() => reduceWireEvent(initial, orphan), "state_violation");

    const admitted = reduceWireEvent(initial, decodeWireEvent(rawEvent(1, "tool.admitted", {
      toolCallId: TOOL_ID,
      name: "bash",
      schema: { status: "valid", source: "provider", digest: "a".repeat(64) },
    })));
    const terminal = decodeWireEvent(rawEvent(2, "tool.terminal", {
      toolCallId: TOOL_ID,
      status: "succeeded",
      origin: "provider",
    }));
    const settled = reduceWireEvent(admitted, terminal);
    const duplicate = decodeWireEvent({
      ...rawEvent(3, "tool.terminal", {
        toolCallId: TOOL_ID,
        status: "succeeded",
        origin: "provider",
      }),
      id: "event:duplicate-terminal",
    });
    expectContractError(() => reduceWireEvent(settled, duplicate), "state_violation");
  });

  test("synthesizes one tool terminal before cancellation terminates a run", () => {
    let sequence = 0;
    const writer = new WireEventWriter({
      runId: RUN_ID,
      eventId: (value) => wireEventId(`cancel-event:${value}`),
      now: () => TIMES[sequence++] ?? "2026-08-10T00:01:00.000Z",
    });
    writer.append({ kind: "run.started", lifecycle: "running" });
    writer.append({
      kind: "tool.admitted",
      toolCallId: TOOL_ID,
      name: "exec",
      schema: { status: "unavailable", reason: "legacy provider" },
    });

    const emitted = writer.terminate({
      lifecycle: "cancelled",
      reason: { code: "cancelled", detail: "cancelled" },
      abort: {
        requestedAt: "2026-08-10T00:00:01.000Z",
        source: "operator",
        reason: "operator request",
      },
    });
    expect(emitted.map((event) => event.kind)).toEqual(["tool.terminal", "run.terminated"]);
    expect(writer.snapshot()?.toolCalls[TOOL_ID]?.status).toBe("cancelled");
    expect(writer.snapshot()?.lifecycle).toBe("cancelled");
    expect(writer.snapshot()?.abort?.source).toBe("operator");
  });

	test("retains bounded provider-neutral watchdog inactivity evidence through replay", () => {
		const decision = wireTerminalDecision("watchdog_aborted", undefined, {
			reason: "north_watchdog_execution_inactivity",
			silenceMs: 20_000,
			lastOuter: {
				origin: "outer",
				kind: "wire.message.completed",
				observedAt: "2026-08-10T00:00:01.000Z",
			},
			lastProvider: {
				origin: "provider",
				kind: "provider.codex.mcp.progress",
				observedAt: "2026-08-10T00:00:02.000Z",
			},
		});
		const writer = new WireEventWriter({ runId: RUN_ID });
		writer.append({ kind: "run.started", lifecycle: "running" });
		const terminal = writer.terminate(decision);
		const replay = reduceWireEvents(writer.events());
		expect(terminal.at(-1)?.kind).toBe("run.terminated");
		expect(replay.abort?.watchdog).toEqual({
			silenceMs: 20_000,
			lastOuter: {
				origin: "outer",
				kind: "message",
				observedAt: "2026-08-10T00:00:01.000Z",
			},
			lastProvider: {
				origin: "provider",
				kind: "tool",
				observedAt: "2026-08-10T00:00:02.000Z",
			},
		});

		expectContractError(() => decodeWireEvent(rawEvent(1, "run.terminated", {
			lifecycle: "cancelled",
			reason: { code: "aborted" },
			abort: {
				requestedAt: "2026-08-10T00:00:03.000Z",
				source: "watchdog",
				reason: "north_watchdog_execution_inactivity",
				watchdog: {
					silenceMs: 20_000,
					lastOuter: {
						origin: "provider",
						kind: "tool",
						observedAt: "2026-08-10T00:00:02.000Z",
					},
				},
			},
		})), "malformed_event");

		expectContractError(() => decodeWireEvent(rawEvent(1, "run.terminated", {
			lifecycle: "cancelled",
			reason: { code: "aborted" },
			abort: {
				requestedAt: "2026-08-10T00:00:03.000Z",
				source: "watchdog",
				reason: "north_watchdog_execution_inactivity",
				watchdog: {
					silenceMs: 20_000,
					lastProvider: {
						origin: "provider",
						kind: "provider.codex.mcp.progress",
						observedAt: "2026-08-10T00:00:02.000Z",
					},
				},
			},
		})), "malformed_event");

		expectContractError(() => decodeWireEvent(rawEvent(1, "run.terminated", {
			lifecycle: "cancelled",
			reason: { code: "aborted" },
			abort: {
				requestedAt: "2026-08-10T00:00:03.000Z",
				source: "watchdog",
				reason: "north_watchdog_execution_inactivity",
			},
		})), "malformed_event");
		expect(() => wireTerminalDecision("watchdog_aborted", undefined, undefined)).toThrow(
			"watchdog_aborted requires authenticated inactivity evidence",
		);
		expectContractError(() => decodeWireEvent(rawEvent(1, "run.terminated", {
			lifecycle: "cancelled",
			reason: { code: "aborted" },
			abort: {
				requestedAt: "2026-08-10T00:00:03.000Z",
				source: "watchdog",
				reason: "provider_private_watchdog_reason",
			},
		})), "malformed_event");
	});

  test("closes open messages and model calls before an abnormal run terminal", () => {
    const messageId = wireMessageId("message:open");
    const modelCallId = wireModelCallId("model-call:open");
    let sequence = 0;
    const writer = new WireEventWriter({
      runId: RUN_ID,
      eventId: (value) => wireEventId(`failure-event:${value}`),
      now: () => TIMES[sequence++] ?? "2026-08-10T00:01:00.000Z",
    });
    writer.append({ kind: "run.started", lifecycle: "running" });
    writer.append({
      kind: "model-call.started",
      modelCallId,
      model: { provider: "anthropic", tier: "senior", capabilityClass: "authoring" },
      effort: "high",
      attempt: 1,
    });
    writer.append({
      kind: "message.recorded",
      messageId,
      modelCallId,
      stage: "started",
      role: "assistant",
    });

    const emitted = writer.terminate({
      lifecycle: "failed",
      reason: { code: "provider_error" },
    });
    expect(emitted.map((event) => event.kind)).toEqual([
      "message.recorded", "model-call.completed", "run.terminated",
    ]);
	const modelTerminal = emitted.find((event) => event.kind === "model-call.completed");
	expect(modelTerminal?.kind === "model-call.completed" ? modelTerminal.origin : undefined).toBe("north");
    expect(writer.snapshot()?.messages[messageId]?.stage).toBe("completed");
    expect(writer.snapshot()?.modelCalls[modelCallId]?.status).toBe("failed");
    expect(writer.snapshot()?.usage.lifetime.modelCalls).toBe(1);
  });

  test("completed termination rejects open work without mutating the writer", () => {
    let sequence = 0;
    const writer = new WireEventWriter({
      runId: RUN_ID,
      eventId: (value) => wireEventId(`complete-event:${value}`),
      now: () => TIMES[sequence++] ?? "2026-08-10T00:01:00.000Z",
    });
    writer.append({ kind: "run.started", lifecycle: "running" });
    writer.append({
      kind: "tool.admitted",
      toolCallId: TOOL_ID,
      name: "edit",
      schema: { status: "unavailable", reason: "legacy provider" },
    });
    expectContractError(() => writer.terminate({
      lifecycle: "completed",
      reason: { code: "completed" },
    }), "state_violation");
    expect(writer.events().map((event) => event.kind)).toEqual(["run.started", "tool.admitted"]);
    expect(writer.snapshot()?.lifecycle).toBe("running");
  });

  test("rejects gaps, terminal mismatches, and post-terminal events", () => {
    const initial = reduceWireEvent(undefined, started());
    expectContractError(() => reduceWireEvent(initial, decodeWireEvent(rawEvent(2, "run.progress", {
      lifecycle: "running",
      progress: {},
    }))), "sequence_violation");

    expectContractError(() => reduceWireEvent(initial, decodeWireEvent({
      ...rawEvent(1, "run.progress", { lifecycle: "running", progress: {} }),
      at: "2026-08-09T23:59:59.000Z",
    })), "sequence_violation");

		const extendedYear = reduceWireEvent(undefined, decodeWireEvent({
			...rawEvent(0, "run.started", { lifecycle: "running" }),
			at: "+010000-01-01T00:00:00.000Z",
		}));
		expectContractError(() => reduceWireEvent(extendedYear, decodeWireEvent({
			...rawEvent(1, "run.progress", { lifecycle: "running", progress: {} }),
			at: "9999-12-31T23:59:59.999Z",
		})), "sequence_violation");

    const mismatched = decodeWireEvent(rawEvent(1, "run.terminated", {
      lifecycle: "completed",
      reason: { code: "failed" },
    }));
    expectContractError(() => reduceWireEvent(initial, mismatched), "state_violation");

    const terminal = reduceWireEvent(initial, decodeWireEvent(rawEvent(1, "run.terminated", {
      lifecycle: "completed",
      reason: { code: "completed" },
    })));
    expectContractError(() => reduceWireEvent(terminal, decodeWireEvent(rawEvent(2, "run.progress", {
      lifecycle: "running",
      progress: {},
    }))), "state_violation");
  });
});

function usage(inputTokens: number, contextTokens: number, modelCalls = 1) {
  return {
    lifetime: {
      inputTokens,
      outputTokens: 10,
      cacheReadTokens: 0,
      cacheWriteTokens: 0,
      reasoningTokens: 0,
      modelCalls,
    },
    context: { tokens: contextTokens, window: 200 },
  };
}
