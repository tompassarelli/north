import { describe, expect, test } from "bun:test";
import {
	AnthropicWireNormalizer,
	type AnthropicWireTurnOutcome,
} from "../src/providers/anthropic-wire";
import {
	WireContractError,
	WireEventWriter,
	wireEventId,
	wireRunId,
	type WireArtifactMaterial,
	type WireArtifactSink,
	type WireKnownEvent,
} from "../src/wire";

const RESULT_USAGE = {
	input_tokens: 100,
	output_tokens: 20,
	cache_creation_input_tokens: 4,
	cache_read_input_tokens: 3,
} as const;

function setup(label: string, contextWindow = 200_000, artifacts?: WireArtifactSink): {
	writer: WireEventWriter;
	normalizer: AnthropicWireNormalizer;
} {
	const writer = new WireEventWriter({
		runId: wireRunId(`run:anthropic-wire:${label}`),
		eventId: (sequence) => wireEventId(`event:anthropic-wire:${label}:${sequence}`),
		now: () => "2026-08-10T00:00:00.000Z",
	});
	writer.append({ kind: "run.started", lifecycle: "running", owner: "test" });
	return {
		writer,
		normalizer: new AnthropicWireNormalizer(writer, {
			model: {
				provider: "anthropic",
				tier: "senior",
				capabilityClass: "authoring",
			},
			effort: "high",
			attempt: 1,
			contextWindow,
		}, artifacts),
	};
}

function setupWithArtifacts(label: string, sink?: WireArtifactSink): {
	writer: WireEventWriter;
	normalizer: AnthropicWireNormalizer;
	artifacts: Map<string, Readonly<WireArtifactMaterial>>;
} {
	const artifacts = new Map<string, Readonly<WireArtifactMaterial>>();
	const harness = setup(label, 200_000, sink ?? {
		persist(artifact) {
			if (artifacts.has(artifact.artifactId)) throw new Error("duplicate artifact id");
			artifacts.set(artifact.artifactId, artifact);
			return { artifactId: artifact.artifactId, digest: artifact.digest };
		},
	});
	return { ...harness, artifacts };
}

function assistantWithTool(toolId = "provider-tool-secret"): Record<string, unknown> {
	return {
		type: "assistant",
		uuid: "provider-assistant-secret",
		parent_tool_use_id: null,
		message: {
			role: "assistant",
			model: "provider-model-secret",
			content: [
				{ type: "text", text: "checking\tpath\u001b[31m" },
				{
					type: "tool_use",
					id: toolId,
					name: "Read",
					input: { file_path: `/tmp/${"x".repeat(4_000)}\u001b[2J` },
				},
			],
		},
	};
}

function result(
	subtype: string,
	overrides: Readonly<Record<string, unknown>> = {},
): Record<string, unknown> {
	return {
		type: "result",
		uuid: `provider-result-${subtype}`,
		subtype,
		is_error: subtype !== "success",
		duration_ms: 321,
		num_turns: 2,
		usage: RESULT_USAGE,
		result: subtype === "success" ? "done" : undefined,
		...overrides,
	};
}

function requiredOutcome(value: AnthropicWireTurnOutcome | undefined): AnthropicWireTurnOutcome {
	if (!value) throw new Error("expected an anthropic turn outcome");
	return value;
}

function kinds(events: readonly WireKnownEvent[]): readonly string[] {
	return events.map((event) => event.kind);
}

function expectContractError(action: () => unknown, code: WireContractError["code"]): void {
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

describe("Anthropic wire-v2 normalization", () => {
	test("records compact boundaries as cumulative provider-neutral run progress", () => {
		const { writer, normalizer } = setup("compaction");
		const first = normalizer.accept({ type: "system", subtype: "compact_boundary" });
		const second = normalizer.accept({ type: "system", subtype: "compact_boundary" });
		expect(kinds([...first.events, ...second.events])).toEqual([
			"run.progress", "run.progress",
		]);
		expect(writer.snapshot()?.compactions).toBe(2);
	});

	test("ignores only known non-semantic system controls and rejects unknown subtypes", () => {
		const { writer, normalizer } = setup("system-subtypes");
		for (const subtype of ["init", "status", "mirror_error"]) {
			expect(normalizer.accept({ type: "system", subtype }).events).toEqual([]);
		}
		const before = writer.events();
		expectContractError(() => normalizer.accept({
			type: "system",
			subtype: "future_semantic_control",
		}), "unsupported_event_kind");
		expect(writer.events()).toEqual(before);
	});

	test("pairs tool admission and result with hashed IDs, parentage, and bounded safe previews", () => {
		const { writer, normalizer } = setup("tool-pairing");
		const admission = normalizer.accept(assistantWithTool());
		const admitted = admission.events.find((event) => event.kind === "tool.admitted");
		expect(admitted?.kind).toBe("tool.admitted");
		if (!admitted || admitted.kind !== "tool.admitted") throw new Error("missing tool admission");
		expect(admitted.toolCallId).toStartWith("tool-call:anthropic:");
		expect(admitted.toolCallId).not.toContain("provider-tool-secret");
		expect(admitted.argumentPreview).toBeDefined();
		expect(new TextEncoder().encode(admitted.argumentPreview).byteLength).toBeLessThanOrEqual(2_048);
		expect(admitted.argumentPreview).not.toContain("\u001b");
		const assistantStart = admission.events.find((event) =>
			event.kind === "message.recorded" && event.stage === "started");
		expect(admitted.parentId).toBe(assistantStart?.kind === "message.recorded"
			? assistantStart.messageId : undefined);

		const terminalMessage = normalizer.accept({
			type: "user",
			uuid: "provider-user-secret",
			parent_tool_use_id: null,
			message: {
				role: "user",
				content: [{
					type: "tool_result",
					tool_use_id: "provider-tool-secret",
					content: "contents\tready\u001b[H",
					is_error: false,
				}],
			},
		});
		const terminal = terminalMessage.events.find((event) => event.kind === "tool.terminal");
		expect(terminal?.kind).toBe("tool.terminal");
		if (!terminal || terminal.kind !== "tool.terminal") throw new Error("missing tool terminal");
		expect(terminal.toolCallId).toBe(admitted.toolCallId);
		expect(terminal.status).toBe("succeeded");
		expect(terminal.origin).toBe("provider");
		expect(terminal.parentId).toBe(admitted.toolCallId);
		expect(terminal.resultPreview).not.toContain("\u001b");
		expect(writer.events().filter((event) => event.kind === "tool.terminal")).toHaveLength(1);

		const encoded = JSON.stringify(writer.events());
		expect(encoded).not.toContain("provider-tool-secret");
		expect(encoded).not.toContain("provider-assistant-secret");
		expect(encoded).not.toContain("provider-model-secret");
		expect(encoded).not.toContain("\u001b");
	});

	test("retains tool and background results before terminal references with exact digests", () => {
		const { writer, normalizer, artifacts } = setupWithArtifacts("retained-tool-results");
		const admission = normalizer.accept(assistantWithTool());
		const admitted = admission.events.find((event) => event.kind === "tool.admitted");
		if (admitted?.kind !== "tool.admitted") throw new Error("missing tool admission");
		const resultText = `HEAD-${"x".repeat(1_048_700)}-TAIL`;
		const accepted = normalizer.accept({
			type: "user",
			uuid: "provider-user-secret",
			parent_tool_use_id: null,
			message: {
				role: "user",
				content: [{
					type: "tool_result",
					tool_use_id: "provider-tool-secret",
					content: resultText,
					is_error: false,
				}],
			},
		});
		expect(kinds(accepted.events)).toEqual([
			"message.recorded", "message.recorded", "artifact.published", "tool.terminal",
		]);
		const published = accepted.events[2];
		const terminal = accepted.events[3];
		if (published?.kind !== "artifact.published" || terminal?.kind !== "tool.terminal") {
			throw new Error("missing retained tool result events");
		}
		expect(terminal.toolCallId).toBe(admitted.toolCallId);
		expect(terminal.resultArtifactId).toBe(published.artifactId);
		expect(terminal.resultArtifactDigest).toBe(published.digest);
		expect(terminal.resultArtifactId).not.toContain("provider-tool-secret");
		expect(new TextEncoder().encode(terminal.resultPreview).byteLength)
			.toBeLessThanOrEqual(2_048);
		const retained = artifacts.get(published.artifactId);
		if (!retained) throw new Error("missing retained tool result");
		expect(new TextEncoder().encode(retained.content).byteLength).toBeLessThanOrEqual(1_048_576);
		expect(retained.content).toStartWith("HEAD-");
		expect(retained.content).toContain("north retained output truncated from");
		expect(retained.content).toEndWith("-TAIL");
		expect(retained.digest).toBe(
			new Bun.CryptoHasher("sha256").update(retained.content).digest("hex"),
		);

		normalizer.accept({
			type: "system",
			subtype: "task_started",
			task_id: "background-result-private",
			description: "bounded background work",
		});
		const background = normalizer.accept({
			type: "system",
			subtype: "task_notification",
			task_id: "background-result-private",
			status: "completed",
			summary: "background retained result",
		});
		expect(kinds(background.events)).toEqual(["artifact.published", "tool.terminal"]);
		const backgroundTerminal = background.events[1];
		if (backgroundTerminal?.kind !== "tool.terminal") throw new Error("missing background terminal");
		expect(artifacts.get(backgroundTerminal.resultArtifactId!)).toMatchObject({
			content: "background retained result",
			digest: backgroundTerminal.resultArtifactDigest,
		});
		expect(writer.snapshot()?.toolCalls[backgroundTerminal.toolCallId]).toMatchObject({
			resultArtifactId: backgroundTerminal.resultArtifactId,
			resultArtifactDigest: backgroundTerminal.resultArtifactDigest,
		});
	});

	test("does not recursively retain artifact_read pages", () => {
		const { normalizer, artifacts } = setupWithArtifacts("artifact-read-result");
		const pageReader = normalizer.accept({
			type: "assistant",
			uuid: "artifact-read-admission",
			parent_tool_use_id: null,
			message: {
				role: "assistant",
				model: "provider-model-secret",
				content: [{
					type: "tool_use",
					id: "artifact-read-provider-id",
					name: "mcp__north__artifact_read",
					input: { artifactId: "artifact:source" },
				}],
			},
		});
		expect(pageReader.events.some((event) => event.kind === "tool.admitted")).toBe(true);
		const before = artifacts.size;
		const result = normalizer.accept({
			type: "user",
			uuid: "artifact-read-result",
			parent_tool_use_id: null,
			message: {
				role: "user",
				content: [{
					type: "tool_result",
					tool_use_id: "artifact-read-provider-id",
					content: '{"protocol":"north.page","content":"bounded"}',
				}],
			},
		});
		expect(result.events.map((event) => event.kind)).toEqual([
			"message.recorded", "message.recorded", "tool.terminal",
		]);
		expect(result.events.at(-1)).not.toHaveProperty("resultArtifactId");
		expect(artifacts.size).toBe(before);
	});

	test("does not publish or reference tool material when durable persistence fails", () => {
		const { writer, normalizer } = setupWithArtifacts("artifact-persistence-failure", {
			persist() {
				throw new Error("artifact store unavailable");
			},
		});
		normalizer.accept(assistantWithTool());
		const before = writer.events();
		expectContractError(() => normalizer.accept({
			type: "user",
			uuid: "provider-user-secret",
			parent_tool_use_id: null,
			message: {
				role: "user",
				content: [{
					type: "tool_result",
					tool_use_id: "provider-tool-secret",
					content: "not durable",
				}],
			},
		}), "state_violation");
		expect(writer.events()).toEqual(before);
		expect(writer.snapshot()?.artifacts).toEqual({});
	});

	test("tool argument evidence canonicalizes keys and ignores intent-only fields", () => {
		const first = setup("tool-digest-first").normalizer.accept({
			type: "assistant",
			uuid: "assistant-first",
			parent_tool_use_id: null,
			message: {
				role: "assistant",
				model: "provider-model-private",
				content: [{
					type: "tool_use", id: "tool-first", name: "Read",
					input: { nested: { z: 2, i: "CANARY-INTENT-ONE", a: 1 }, path: "README.md" },
				}],
			},
		}).events.find((event) => event.kind === "tool.admitted");
		const second = setup("tool-digest-second").normalizer.accept({
			type: "assistant",
			uuid: "assistant-second",
			parent_tool_use_id: null,
			message: {
				role: "assistant",
				model: "provider-model-private",
				content: [{
					type: "tool_use", id: "tool-second", name: "Read",
					input: { path: "README.md", nested: { a: 1, __intent: "CANARY-INTENT-TWO", z: 2 } },
				}],
			},
		}).events.find((event) => event.kind === "tool.admitted");
		if (first?.kind !== "tool.admitted" || second?.kind !== "tool.admitted") {
			throw new Error("missing tool admissions");
		}
		expect(first.argumentDigest).toMatch(/^[a-f0-9]{64}$/);
		expect(first.argumentDigest).toBe(second.argumentDigest);
		const evidence = JSON.stringify([first.argumentDigest, second.argumentDigest]);
		expect(evidence).not.toContain("CANARY-INTENT-ONE");
		expect(evidence).not.toContain("CANARY-INTENT-TWO");
	});

	test("returns a successful model-call terminal with split usage and bounded completion evidence", () => {
		const { writer, normalizer } = setup("success");
		normalizer.accept({
			type: "assistant",
			uuid: "assistant-success",
			parent_tool_use_id: null,
			message: { role: "assistant", model: "raw-model", content: [{ type: "text", text: "done" }] },
		});
		const accepted = normalizer.accept(result("success"));
		const outcome = requiredOutcome(accepted.turnOutcome);
		expect(outcome.status).toBe("succeeded");
		expect(outcome.usage).toEqual({
			lifetime: {
				inputTokens: 100,
				outputTokens: 20,
				cacheReadTokens: 3,
				cacheWriteTokens: 4,
				reasoningTokens: 0,
				modelCalls: 1,
			},
			context: { tokens: 107, window: 200_000 },
		});
		expect(kinds(accepted.events)).toEqual(["model-call.completed"]);
		expect(writer.events().some((event) => event.kind === "run.terminated")).toBe(false);
		const call = accepted.events[0];
		expect(call.kind === "model-call.completed" ? call.status : undefined).toBe("succeeded");
		expect(call.kind === "model-call.completed" ? call.origin : undefined).toBe("provider");
		expect(call.kind === "model-call.completed" ? call.usageCoverage : undefined).toBe("exact");
		expect(call.kind === "model-call.completed" ? call.evidence : undefined).toEqual({
			turns: { unit: "assistant-turn", count: 2, comparable: true },
			providerDurationMs: 321,
		});
		expect(writer.snapshot()?.lifecycle).toBe("running");
	});

	test("synthesizes pending tool failures and reports a failed turn without terminating the run", () => {
		const { writer, normalizer } = setup("failure");
		normalizer.accept(assistantWithTool("failed-provider-tool"));
		const accepted = normalizer.accept(result("error_during_execution"));
		const outcome = requiredOutcome(accepted.turnOutcome);
		expect(outcome.status).toBe("failed");
		expect(outcome.errorCode).toBe("provider_error");
		const terminal = accepted.events.find((event) => event.kind === "tool.terminal");
		expect(terminal?.kind === "tool.terminal" ? terminal.status : undefined).toBe("synthetic_failure");
		expect(terminal?.kind === "tool.terminal" ? terminal.origin : undefined).toBe("north");
		expect(accepted.events.some((event) => event.kind === "run.terminated")).toBe(false);
		expect(writer.snapshot()?.lifecycle).toBe("running");

		expect(accepted.events.find((event) => event.kind === "model-call.completed"))
			.toMatchObject({ origin: "provider", evidence: { failure: { detail: "provider_error" } } });
		expect(writer.snapshot()?.lifecycle).toBe("running");
	});

	test("maps provider cancellation separately and leaves run termination caller-owned", () => {
		const { writer, normalizer } = setup("cancelled");
		normalizer.accept(assistantWithTool("cancelled-provider-tool"));
		const accepted = normalizer.accept(result("success", {
			terminal_reason: "aborted_streaming",
			is_error: false,
		}));
		const outcome = requiredOutcome(accepted.turnOutcome);
		expect(outcome.status).toBe("cancelled");
		expect(outcome.errorCode).toBe("provider_cancelled");
		const terminal = accepted.events.find((event) => event.kind === "tool.terminal");
		expect(terminal?.kind === "tool.terminal" ? terminal.status : undefined).toBe("cancelled");
		expect(writer.events().some((event) => event.kind === "run.terminated")).toBe(false);

		expect(writer.snapshot()?.lifecycle).toBe("running");
	});

	test("carries background task liveness as one generic tool lifecycle across turn terminals", () => {
		const { writer, normalizer } = setup("background-task");
		const started = normalizer.accept({
			type: "system",
			subtype: "task_started",
			task_id: "provider-background-secret",
			description: "sleep\t20\u001b[2J",
		});
		const admission = started.events[0];
		expect(admission.kind).toBe("tool.admitted");
		if (admission.kind !== "tool.admitted") throw new Error("missing background task admission");
		expect(admission.name).toBe("background-task");
		expect(admission.toolCallId).not.toContain("provider-background-secret");
		expect(admission.argumentPreview).not.toContain("\u001b");

		const progress = normalizer.accept({
			type: "system",
			subtype: "task_progress",
			task_id: "provider-background-secret",
			description: "still running",
			usage: { total_tokens: 30, tool_uses: 2, duration_ms: 500 },
		});
		expect(kinds(progress.events)).toEqual(["tool.progress"]);

		const turn = requiredOutcome(normalizer.accept(result("success")).turnOutcome);
		expect(writer.snapshot()?.lifecycle).toBe("running");
		expect(turn.status).toBe("succeeded");
		expect(writer.events().some((event) => event.kind === "run.terminated")).toBe(false);

		const settled = normalizer.accept({
			type: "system",
			subtype: "task_updated",
			task_id: "provider-background-secret",
			patch: { status: "completed" },
		});
		expect(settled.events[0]?.kind).toBe("tool.terminal");
		expect(settled.events[0]?.kind === "tool.terminal" ? settled.events[0].status : undefined)
			.toBe("succeeded");
		const duplicate = normalizer.accept({
			type: "system",
			subtype: "task_notification",
			task_id: "provider-background-secret",
			status: "completed",
			summary: "done",
		});
		expect(duplicate.events).toEqual([]);
		expect(writer.events().filter((event) =>
			event.kind === "tool.terminal" && event.toolCallId === admission.toolCallId)).toHaveLength(1);
		normalizer.accept({
			type: "system",
			subtype: "task_started",
			task_id: "provider-background-failed",
			description: "eventual failure",
		});
		const notification = normalizer.accept({
			type: "system",
			subtype: "task_notification",
			task_id: "provider-background-failed",
			status: "failed",
			summary: "task failed\u001b[31m",
		});
		expect(notification.events[0]?.kind === "tool.terminal"
			? notification.events[0].status : undefined).toBe("failed");
		expect(notification.events[0]?.kind === "tool.terminal"
			? notification.events[0].resultPreview : undefined).not.toContain("\u001b");

		expect(writer.snapshot()?.lifecycle).toBe("running");
	});

	test("rejects an explicit unknown background-task parent instead of rooting the task", () => {
		const { writer, normalizer } = setup("background-task-parent");
		const before = writer.events();
		expectContractError(() => normalizer.accept({
			type: "system",
			subtype: "task_started",
			task_id: "provider-background-child",
			tool_use_id: "provider-tool-never-admitted",
			description: "must retain explicit ancestry",
		}), "state_violation");
		expect(writer.events()).toEqual(before);
	});

	test("settles an abrupt provider failure without terminating the run or duplicating tool terminals", () => {
		const { writer, normalizer } = setup("abrupt");
		normalizer.accept(assistantWithTool("provider-tool-abrupt"));
		const settled = normalizer.settleAbrupt("failed");
		expect(kinds(settled.events)).toEqual(["tool.terminal", "model-call.completed"]);
		expect(settled.events[0]).toMatchObject({
			kind: "tool.terminal",
			status: "synthetic_failure",
			origin: "north",
			errorCode: "provider_error",
		});
		expect(settled.events[1]).toMatchObject({
			kind: "model-call.completed",
			status: "failed",
			origin: "north",
			usageCoverage: "unavailable",
			errorCode: "provider_error",
			evidence: { failure: { detail: "provider_error" } },
		});
		expect(normalizer.settleAbrupt("failed").events).toEqual([]);
		expect(writer.events().filter((event) => event.kind === "tool.terminal")).toHaveLength(1);
		expect(writer.events().some((event) => event.kind === "run.terminated")).toBe(false);
		expect(writer.snapshot()?.lifecycle).toBe("running");
		expect(writer.snapshot()?.usageCoverage.totalStatus).toBe("unknown_no_terminal");
	});

	test("rejects provider context usage above the semantic route window without partial events", () => {
		const { writer, normalizer } = setup("context-window", 50);
		const before = writer.events();
		expectContractError(() => normalizer.accept(result("success")), "malformed_event");
		expect(writer.events()).toEqual(before);
	});

	test("rejects malformed and orphan lifecycle messages atomically with typed errors", () => {
		const malformed = setup("malformed");
		const beforeMalformed = malformed.writer.events();
		expectContractError(() => malformed.normalizer.accept({
			type: "assistant",
			uuid: "assistant-malformed",
			parent_tool_use_id: null,
			message: {
				role: "assistant",
				content: [
					{ type: "text", text: "must remain atomic" },
					{ type: "tool_use", name: "Read", input: {} },
				],
			},
		}), "malformed_event");
		expect(malformed.writer.events()).toEqual(beforeMalformed);

		const orphan = setup("orphan");
		const beforeOrphan = orphan.writer.events();
		expectContractError(() => orphan.normalizer.accept({
			type: "user",
			uuid: "user-orphan",
			parent_tool_use_id: null,
			message: {
				role: "user",
				content: [{ type: "tool_result", tool_use_id: "never-admitted", content: "nope" }],
			},
		}), "state_violation");
		expect(orphan.writer.events()).toEqual(beforeOrphan);
	});
});
