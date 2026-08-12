import { describe, expect, test } from "bun:test";
import {
	OpenAIWireNormalizationError,
	OpenAIWireNormalizer,
	type OpenAIWireIdFactory,
	type OpenAIWireNormalizationErrorCode,
} from "../src/providers/openai-wire";
import {
	WireEventWriter,
	decodeWireEvents,
	reduceWireEvents,
	wireArtifactId,
	wireEventId,
	wireMessageId,
	wireModelCallId,
	wireRunId,
	wireToolCallId,
	type WireArtifactMaterial,
	type WireArtifactSink,
} from "../src/wire";
import { RETAINED_PROVIDER_MATERIAL_MAX_BYTES } from "../src/providers/retained-artifact";

const RUN_ID = wireRunId("run:openai-wire-test");

const IDS: OpenAIWireIdFactory = {
	modelCall: (sequence) => wireModelCallId(`model-call:test:${sequence}`),
	message: (sequence) => wireMessageId(`message:test:${sequence}`),
	toolCall: (sequence) => wireToolCallId(`tool:test:${sequence}`),
	artifact: (sequence) => wireArtifactId(`artifact:test:${sequence}`),
};

interface Harness {
	writer: WireEventWriter;
	normalizer: OpenAIWireNormalizer;
	artifacts: Map<string, Readonly<WireArtifactMaterial>>;
}

function harness(artifactSink?: WireArtifactSink | null): Harness {
	let tick = 0;
	const artifacts = new Map<string, Readonly<WireArtifactMaterial>>();
	const writer = new WireEventWriter({
		runId: RUN_ID,
		eventId: (sequence) => wireEventId(`event:openai-wire:${sequence}`),
		now: () => new Date(Date.UTC(2026, 7, 10, 0, 0, tick++)).toISOString(),
	});
	writer.append({ kind: "run.started", lifecycle: "running", owner: "test" });
	return {
		writer,
		artifacts,
		normalizer: new OpenAIWireNormalizer({
			writer,
			ids: IDS,
			...(artifactSink === null ? {} : { artifacts: artifactSink ?? {
				persist(artifact) {
					if (artifacts.has(artifact.artifactId)) throw new Error("duplicate artifact id");
					artifacts.set(artifact.artifactId, artifact);
					return { artifactId: artifact.artifactId, digest: artifact.digest };
				},
			} }),
			route: {
				model: {
					provider: "openai",
					tier: "senior",
					capabilityClass: "authoring",
				},
				effort: "high",
				attempt: 1,
				contextWindow: 200_000,
			},
		}),
	};
}

function expectNormalizationError(
	action: () => unknown,
	code: OpenAIWireNormalizationErrorCode,
): void {
	try {
		action();
	} catch (error) {
		expect(error).toBeInstanceOf(OpenAIWireNormalizationError);
		if (!(error instanceof OpenAIWireNormalizationError)) throw error;
		expect(error.code).toBe(code);
		return;
	}
	throw new Error(`expected OpenAI wire normalization error ${code}`);
}

function startTurn(normalizer: OpenAIWireNormalizer, turnId = "provider-turn-1"): void {
	normalizer.normalize("turn/started", {
		threadId: "provider-thread-private",
		turn: {
			id: turnId,
			status: "inProgress",
			model: "gpt-provider-private-model-id",
		},
	});
}

describe("OpenAIWireNormalizer", () => {
	test("normalizes a validated app-server turn without exposing raw provider material", () => {
		const { writer, normalizer, artifacts } = harness();
		startTurn(normalizer);

		normalizer.normalize("item/started", {
			threadId: "provider-thread-private",
			turnId: "provider-turn-1",
			startedAtMs: 1,
			item: {
				id: "provider-mcp-item-private",
				type: "mcpToolCall",
				server: "north",
				tool: "tell",
				arguments: { secret: "CANARY-ARGUMENT" },
			},
		});
		normalizer.normalize("item/mcpToolCall/progress", {
			threadId: "provider-thread-private",
			turnId: "provider-turn-1",
			itemId: "provider-mcp-item-private",
			message: "CANARY-PROVIDER-PROGRESS",
		});

		normalizer.normalize("item/started", {
			threadId: "provider-thread-private",
			turnId: "provider-turn-1",
			startedAtMs: 2,
			item: { id: "provider-file-item-private", type: "fileChange" },
		});
		const patch = normalizer.normalize("item/fileChange/patchUpdated", {
			threadId: "provider-thread-private",
			turnId: "provider-turn-1",
			itemId: "provider-file-item-private",
			changes: [{ path: "private.ts", diff: "CANARY-PRIVATE-PATCH" }],
		});
		expect(patch.type).toBe("events");
		expect(patch.events.map((event) => event.kind)).toEqual([
			"artifact.published", "tool.progress", "run.progress",
		]);
		expect(artifacts.get(wireArtifactId("artifact:test:0"))).toMatchObject({
			mediaType: "application/vnd.north.patch+json",
			content: expect.stringContaining("CANARY-PRIVATE-PATCH"),
		});

		normalizer.normalize("item/completed", {
			threadId: "provider-thread-private",
			turnId: "provider-turn-1",
			completedAtMs: 3,
			item: {
				id: "provider-file-item-private",
				type: "fileChange",
				status: "completed",
				result: "CANARY-FILE-RESULT",
			},
		});
			normalizer.normalize("item/completed", {
			threadId: "provider-thread-private",
			turnId: "provider-turn-1",
			completedAtMs: 4,
			item: {
				id: "provider-mcp-item-private",
				type: "mcpToolCall",
				status: "completed",
				result: "CANARY-MCP-RESULT",
			},
		});

		const implicitMessage = normalizer.normalize("item/agentMessage/delta", {
			threadId: "provider-thread-private",
			turnId: "provider-turn-1",
			itemId: "provider-message-item-private",
			delta: "Implemented the change.",
		});
		expect(implicitMessage.events.map((event) => event.kind)).toEqual([
			"message.recorded", "message.recorded",
		]);
		normalizer.normalize("item/completed", {
			threadId: "provider-thread-private",
			turnId: "provider-turn-1",
			completedAtMs: 6,
			item: {
				id: "provider-message-item-private",
				type: "agentMessage",
				text: "Implemented the change.",
			},
		});
		normalizer.normalize("item/started", {
			threadId: "provider-thread-private",
			turnId: "provider-turn-1",
			startedAtMs: 7,
			item: { id: "provider-completion-message-private", type: "agentMessage" },
		});
		normalizer.normalize("item/completed", {
			threadId: "provider-thread-private",
			turnId: "provider-turn-1",
			completedAtMs: 8,
			item: {
				id: "provider-completion-message-private",
				type: "agentMessage",
				text: "Completion-only answer.",
			},
		});

		normalizer.normalize("thread/tokenUsage/updated", {
			threadId: "provider-thread-private",
			turnId: "provider-turn-1",
			tokenUsage: {
				total: {
					totalTokens: 100,
					inputTokens: 90,
					cachedInputTokens: 40,
					outputTokens: 10,
					reasoningOutputTokens: 5,
				},
				last: {
					totalTokens: 30,
					inputTokens: 25,
					cachedInputTokens: 10,
					outputTokens: 5,
					reasoningOutputTokens: 2,
				},
				modelContextWindow: 200_000,
			},
		});
		const terminal = normalizer.normalize("turn/completed", {
			threadId: "provider-thread-private",
			turn: {
				id: "provider-turn-1",
				status: "completed",
				model: "gpt-provider-private-model-id",
			},
		});
		expect(terminal.type).toBe("turn.terminal");
		if (terminal.type !== "turn.terminal") throw new Error("expected a turn terminal");
		expect(terminal.outcome).toMatchObject({
			status: "succeeded",
			modelCallId: wireModelCallId("model-call:test:0"),
		});
		expect(terminal.events.map((event) => event.kind)).toEqual(["model-call.completed"]);

		const snapshot = writer.snapshot();
		expect(snapshot?.model).toEqual({
			provider: "openai", tier: "senior", capabilityClass: "authoring",
		});
		expect(snapshot?.effort).toBe("high");
		expect(snapshot?.usage).toEqual({
			lifetime: {
				inputTokens: 90,
				outputTokens: 10,
				cacheReadTokens: 40,
				cacheWriteTokens: 0,
				reasoningTokens: 5,
				modelCalls: 1,
			},
			context: { tokens: 30, window: 200_000 },
		});
		expect(snapshot?.toolCalls[wireToolCallId("tool:test:0")]?.status).toBe("succeeded");
		expect(snapshot?.toolCalls[wireToolCallId("tool:test:0")]?.resultPreview)
			.toBe("CANARY-MCP-RESULT");
		expect(snapshot?.toolCalls[wireToolCallId("tool:test:0")]?.resultArtifactId)
			.toStartWith("artifact:openai:retained:");
		expect(snapshot?.toolCalls[wireToolCallId("tool:test:1")]?.resultArtifactId)
			.toBe(wireArtifactId("artifact:test:0"));
		expect(snapshot?.messages[wireMessageId("message:test:0")]?.contents).toEqual([
			"Implemented the change.",
		]);
		expect(snapshot?.messages[wireMessageId("message:test:1")]?.contents)
			.toEqual(["Completion-only answer."]);
		expect(snapshot?.artifacts[wireArtifactId("artifact:test:0")]).toMatchObject({
			mediaType: "application/vnd.north.patch+json",
			label: "workspace patch",
		});

		expect(writer.events().some((event) => event.kind === "run.terminated")).toBe(false);
		const serialized = JSON.stringify(writer.events());
		for (const providerPrivate of [
			"provider-thread-private",
			"provider-turn-1",
			"provider-mcp-item-private",
			"provider-file-item-private",
			"provider-message-item-private",
			"provider-completion-message-private",
			"gpt-provider-private-model-id",
			"CANARY-ARGUMENT",
			"CANARY-PROVIDER-PROGRESS",
			"CANARY-PRIVATE-PATCH",
			"CANARY-FILE-RESULT",
			"item/started",
			"thread/tokenUsage/updated",
		]) expect(serialized).not.toContain(providerPrivate);
		for (const event of writer.events()) {
			expect(event).not.toHaveProperty("method");
			expect(event).not.toHaveProperty("params");
		}

		const persisted: unknown = JSON.parse(serialized);
		if (!Array.isArray(persisted)) throw new Error("wire fixture must encode an event array");
		const replayed = reduceWireEvents(decodeWireEvents(persisted as readonly unknown[]));
		expect(replayed).toEqual(writer.snapshot());
	});

	test("accepts provider input and hook-prompt item lifecycles without projecting work", () => {
		const { writer, normalizer } = harness();
		startTurn(normalizer);
		const eventCount = writer.events().length;
		for (const type of ["userMessage", "hookPrompt"] as const) {
			const id = `provider-${type}-private`;
			expect(normalizer.normalize("item/started", {
				threadId: "provider-thread-private",
				turnId: "provider-turn-1",
				item: { id, type },
			}).events).toEqual([]);
			expect(normalizer.normalize("item/completed", {
				threadId: "provider-thread-private",
				turnId: "provider-turn-1",
				item: { id, type },
			}).events).toEqual([]);
		}
		expect(writer.events()).toHaveLength(eventCount);
	});

	test("durably retains bounded command and MCP terminal material before referencing it", () => {
		const { normalizer, artifacts } = harness();
		startTurn(normalizer);
		const commandOutput = `HEAD-${"x".repeat(RETAINED_PROVIDER_MATERIAL_MAX_BYTES + 128)}-TAIL`;
		normalizer.normalize("item/started", {
			threadId: "provider-thread-private",
			turnId: "provider-turn-1",
			startedAtMs: 1,
			item: { id: "command-output-private", type: "commandExecution", command: "run" },
		});
		const command = normalizer.normalize("item/completed", {
			threadId: "provider-thread-private",
			turnId: "provider-turn-1",
			completedAtMs: 2,
			item: {
				id: "command-output-private",
				type: "commandExecution",
				status: "completed",
				exitCode: 0,
				aggregatedOutput: commandOutput,
			},
		});
		expect(command.events.map((event) => event.kind)).toEqual([
			"artifact.published", "tool.terminal",
		]);
		const commandTerminal = command.events[1];
		if (commandTerminal?.kind !== "tool.terminal") throw new Error("missing command terminal");
		expect(commandTerminal.resultArtifactId).toStartWith("artifact:openai:retained:");
		expect(commandTerminal.resultArtifactId).not.toContain("command-output-private");
		expect(commandTerminal.resultArtifactDigest).toMatch(/^[a-f0-9]{64}$/);
		expect(new TextEncoder().encode(commandTerminal.resultPreview).byteLength)
			.toBeLessThanOrEqual(2_048);
		expect(commandTerminal.resultPreview).not.toContain("-TAIL");
		const retainedCommand = artifacts.get(commandTerminal.resultArtifactId!);
		expect(retainedCommand).toBeDefined();
		if (!retainedCommand) throw new Error("missing retained command output");
		expect(new TextEncoder().encode(retainedCommand.content).byteLength)
			.toBeLessThanOrEqual(RETAINED_PROVIDER_MATERIAL_MAX_BYTES);
		expect(retainedCommand.content).toContain("HEAD-");
		expect(retainedCommand.content).toContain("north retained output truncated from");
		expect(retainedCommand.content).toEndWith("-TAIL");
		expect(retainedCommand.digest).toBe(
			new Bun.CryptoHasher("sha256").update(retainedCommand.content).digest("hex"),
		);
		expect(commandTerminal.resultArtifactDigest).toBe(retainedCommand.digest);

		normalizer.normalize("item/started", {
			threadId: "provider-thread-private",
			turnId: "provider-turn-1",
			startedAtMs: 3,
			item: { id: "mcp-result-private", type: "mcpToolCall", server: "north", tool: "show" },
		});
		const mcp = normalizer.normalize("item/completed", {
			threadId: "provider-thread-private",
			turnId: "provider-turn-1",
			completedAtMs: 4,
			item: {
				id: "mcp-result-private",
				type: "mcpToolCall",
				status: "completed",
				result: { ok: true, items: ["one", "two"] },
			},
		});
		expect(mcp.events.map((event) => event.kind)).toEqual([
			"artifact.published", "tool.terminal",
		]);
		const mcpTerminal = mcp.events[1];
		if (mcpTerminal?.kind !== "tool.terminal") throw new Error("missing MCP terminal");
		const retainedMcp = artifacts.get(mcpTerminal.resultArtifactId!);
		expect(retainedMcp).toMatchObject({
			mediaType: "application/json",
			content: '{"ok":true,"items":["one","two"]}',
			digest: mcpTerminal.resultArtifactDigest,
		});
		expect(mcpTerminal.resultPreview).toBe('{"ok":true,"items":["one","two"]}');

		normalizer.normalize("item/started", {
			threadId: "provider-thread-private",
			turnId: "provider-turn-1",
			startedAtMs: 5,
			item: {
				id: "artifact-read-private",
				type: "mcpToolCall",
				server: "north",
				tool: "artifact_read",
			},
		});
		const artifactCount = artifacts.size;
		const read = normalizer.normalize("item/completed", {
			threadId: "provider-thread-private",
			turnId: "provider-turn-1",
			completedAtMs: 6,
			item: {
				id: "artifact-read-private",
				type: "mcpToolCall",
				status: "completed",
				result: { protocol: "north.page", artifactId: retainedMcp?.artifactId },
			},
		});
		expect(read.events.map((event) => event.kind)).toEqual(["tool.terminal"]);
		expect(read.events[0]).not.toHaveProperty("resultArtifactId");
		expect(artifacts.size).toBe(artifactCount);
	});

	test("keeps a concise terminal preview without inventing an artifact when no sink exists", () => {
		const { writer, normalizer } = harness(null);
		startTurn(normalizer);
		normalizer.normalize("item/started", {
			threadId: "provider-thread-private",
			turnId: "provider-turn-1",
			startedAtMs: 1,
			item: { id: "command-1", type: "commandExecution", command: "printf ok" },
		});
		const result = normalizer.normalize("item/completed", {
			threadId: "provider-thread-private",
			turnId: "provider-turn-1",
			completedAtMs: 2,
			item: {
				id: "command-1",
				type: "commandExecution",
				status: "completed",
				exitCode: 0,
				aggregatedOutput: "ok\tready\u001b[31m",
			},
		});
		expect(result.events).toHaveLength(1);
		expect(result.events[0]).toMatchObject({
			kind: "tool.terminal",
			resultPreview: "ok  ready\ufffd[31m",
		});
		expect(result.events[0]).not.toHaveProperty("resultArtifactId");
		expect(writer.snapshot()?.artifacts).toEqual({});
	});

	test("does not publish or reference terminal material when persistence fails", () => {
		const { writer, normalizer } = harness({
			persist() {
				throw new Error("artifact store unavailable");
			},
		});
		startTurn(normalizer);
		normalizer.normalize("item/started", {
			threadId: "provider-thread-private",
			turnId: "provider-turn-1",
			startedAtMs: 1,
			item: { id: "command-1", type: "commandExecution", command: "printf output" },
		});
		const before = writer.events();
		expectNormalizationError(() => normalizer.normalize("item/completed", {
			threadId: "provider-thread-private",
			turnId: "provider-turn-1",
			completedAtMs: 2,
			item: {
				id: "command-1",
				type: "commandExecution",
				status: "completed",
				exitCode: 0,
				aggregatedOutput: "not durable",
			},
		}), "artifact_persistence_failed");
		expect(writer.events()).toEqual(before);
		expect(writer.snapshot()?.artifacts).toEqual({});
	});

	test("rejects pathological structured terminal material before persistence", () => {
		const { writer, normalizer, artifacts } = harness();
		startTurn(normalizer);
		normalizer.normalize("item/started", {
			threadId: "provider-thread-private",
			turnId: "provider-turn-1",
			startedAtMs: 1,
			item: { id: "mcp-1", type: "mcpToolCall", server: "north", tool: "show" },
		});
		const cyclic: Record<string, unknown> = {};
		cyclic.self = cyclic;
		const before = writer.events();
		expectNormalizationError(() => normalizer.normalize("item/completed", {
			threadId: "provider-thread-private",
			turnId: "provider-turn-1",
			completedAtMs: 2,
			item: {
				id: "mcp-1",
				type: "mcpToolCall",
				status: "completed",
				result: cyclic,
			},
		}), "malformed_notification");
		expect(writer.events()).toEqual(before);
		expect(artifacts.size).toBe(0);
	});

	test("managed tool admissions retain only canonical argument equality evidence", () => {
		const admit = (label: string, argumentsValue: unknown) => {
			const { writer, normalizer } = harness();
			startTurn(normalizer, `provider-turn-${label}`);
			normalizer.normalize("item/started", {
				threadId: "provider-thread-private",
				turnId: `provider-turn-${label}`,
				item: {
					id: `provider-mcp-${label}`,
					type: "mcpToolCall",
					server: "north",
					tool: "tell",
					arguments: argumentsValue,
				},
			});
			const event = writer.events().find((candidate) => candidate.kind === "tool.admitted");
			if (event?.kind !== "tool.admitted") throw new Error("missing managed tool admission");
			return event;
		};
		const first = admit("first", {
			to: "worker", nested: { z: 2, i: "CANARY-INTENT-ONE", a: 1 },
		});
		const reordered = admit("reordered", {
			nested: { a: 1, __intent: "CANARY-INTENT-TWO", z: 2 }, to: "worker",
		});
		const different = admit("different", {
			nested: { a: 9, z: 2 }, to: "worker",
		});
		expect(first.argumentDigest).toMatch(/^[a-f0-9]{64}$/);
		expect(first.argumentDigest).toBe(reordered.argumentDigest);
		expect(first.argumentDigest).not.toBe(different.argumentDigest);
		const evidence = JSON.stringify([
			first.argumentDigest, reordered.argumentDigest, different.argumentDigest,
		]);
		expect(evidence).not.toContain("CANARY-INTENT-ONE");
		expect(evidence).not.toContain("CANARY-INTENT-TWO");
	});

	test("accepts every validated app-server notification without leaking raw progress", () => {
		const { writer, normalizer } = harness();
		const observed = new Set<string>();
		const accept = (method: string, params: unknown) => {
			observed.add(method);
			return normalizer.normalize(method, params);
		};

		for (const method of [
			"configWarning",
			"deprecationNotice",
			"remoteControl/status/changed",
			"mcpServer/startupStatus/updated",
			"model/safetyBuffering/updated",
			"account/rateLimits/updated",
			"serverRequest/resolved",
			"thread/started",
			"thread/status/changed",
		] as const) accept(method, { ignored: "IGNORED-CANARY" });

		accept("turn/started", {
			threadId: "provider-thread-private",
			turn: { id: "provider-turn-1", status: "inProgress" },
		});
		accept("hook/started", { ignored: "HOOK-START-CANARY" });
		accept("hook/completed", { ignored: "HOOK-COMPLETE-CANARY" });
		accept("item/started", {
			threadId: "provider-thread-private",
			turnId: "provider-turn-1",
			item: { id: "reasoning-1", type: "reasoning" },
		});
		accept("item/reasoning/summaryPartAdded", { ignored: "REASONING-PART-CANARY" });
		accept("item/reasoning/summaryTextDelta", { ignored: "REASONING-SUMMARY-CANARY" });
		accept("item/reasoning/textDelta", { ignored: "REASONING-TEXT-CANARY" });
		accept("item/completed", {
			threadId: "provider-thread-private",
			turnId: "provider-turn-1",
			item: { id: "reasoning-1", type: "reasoning" },
		});
		accept("item/plan/delta", {
			threadId: "provider-thread-private",
			turnId: "provider-turn-1",
			itemId: "plan-1",
			delta: "PLAN-CANARY",
		});
		accept("turn/plan/updated", {
			threadId: "provider-thread-private",
			turnId: "provider-turn-1",
			explanation: "EXPLANATION-CANARY",
			plan: [{ step: "PLAN-STEP-CANARY", status: "inProgress" }],
		});
		accept("turn/diff/updated", {
			threadId: "provider-thread-private",
			turnId: "provider-turn-1",
			diff: "DIFF-CANARY",
		});

		accept("item/started", {
			threadId: "provider-thread-private",
			turnId: "provider-turn-1",
			item: { id: "command-1", type: "commandExecution" },
		});
		accept("item/commandExecution/outputDelta", {
			threadId: "provider-thread-private",
			turnId: "provider-turn-1",
			itemId: "command-1",
			delta: "COMMAND-OUTPUT-CANARY",
		});
		accept("item/commandExecution/terminalInteraction", {
			threadId: "provider-thread-private",
			turnId: "provider-turn-1",
			itemId: "command-1",
			processId: "process-1",
			stdin: "STDIN-CANARY",
		});
		accept("item/completed", {
			threadId: "provider-thread-private",
			turnId: "provider-turn-1",
			item: { id: "command-1", type: "commandExecution", status: "completed", exitCode: 0 },
		});

		accept("item/started", {
			threadId: "provider-thread-private",
			turnId: "provider-turn-1",
			item: { id: "file-1", type: "fileChange" },
		});
		accept("item/fileChange/outputDelta", {
			threadId: "provider-thread-private",
			turnId: "provider-turn-1",
			itemId: "file-1",
			delta: "FILE-OUTPUT-CANARY",
		});
		accept("item/fileChange/patchUpdated", {
			threadId: "provider-thread-private",
			turnId: "provider-turn-1",
			itemId: "file-1",
			changes: [],
		});
		accept("item/completed", {
			threadId: "provider-thread-private",
			turnId: "provider-turn-1",
			item: { id: "file-1", type: "fileChange", status: "completed" },
		});

		accept("item/started", {
			threadId: "provider-thread-private",
			turnId: "provider-turn-1",
			item: { id: "mcp-1", type: "mcpToolCall", server: "north", tool: "tell" },
		});
		accept("item/mcpToolCall/progress", {
			threadId: "provider-thread-private",
			turnId: "provider-turn-1",
			itemId: "mcp-1",
			message: "MCP-PROGRESS-CANARY",
		});
		accept("item/completed", {
			threadId: "provider-thread-private",
			turnId: "provider-turn-1",
			item: { id: "mcp-1", type: "mcpToolCall", status: "completed" },
		});

		accept("item/agentMessage/delta", {
			threadId: "provider-thread-private",
			turnId: "provider-turn-1",
			itemId: "message-1",
			delta: "semantic answer",
		});
		accept("item/completed", {
			threadId: "provider-thread-private",
			turnId: "provider-turn-1",
			item: { id: "message-1", type: "agentMessage", text: "semantic answer" },
		});
		accept("thread/tokenUsage/updated", {
			threadId: "provider-thread-private",
			turnId: "provider-turn-1",
			tokenUsage: { total: {
				totalTokens: 2,
				inputTokens: 1,
				cachedInputTokens: 0,
				outputTokens: 1,
				reasoningOutputTokens: 0,
			} },
		});
		accept("turn/completed", {
			threadId: "provider-thread-private",
			turn: { id: "provider-turn-1", status: "completed" },
		});

		expect([...observed].sort()).toEqual([
			"account/rateLimits/updated",
			"configWarning",
			"deprecationNotice",
			"hook/completed",
			"hook/started",
			"item/agentMessage/delta",
			"item/commandExecution/outputDelta",
			"item/commandExecution/terminalInteraction",
			"item/completed",
			"item/fileChange/outputDelta",
			"item/fileChange/patchUpdated",
			"item/mcpToolCall/progress",
			"item/plan/delta",
			"item/reasoning/summaryPartAdded",
			"item/reasoning/summaryTextDelta",
			"item/reasoning/textDelta",
			"item/started",
			"mcpServer/startupStatus/updated",
			"model/safetyBuffering/updated",
			"remoteControl/status/changed",
			"serverRequest/resolved",
			"thread/started",
			"thread/status/changed",
			"thread/tokenUsage/updated",
			"turn/completed",
			"turn/diff/updated",
			"turn/plan/updated",
			"turn/started",
		]);
		const serialized = JSON.stringify(writer.events());
		for (const raw of [
			"IGNORED-CANARY", "HOOK-START-CANARY", "HOOK-COMPLETE-CANARY",
			"REASONING-PART-CANARY", "REASONING-SUMMARY-CANARY", "REASONING-TEXT-CANARY",
			"PLAN-CANARY", "EXPLANATION-CANARY", "PLAN-STEP-CANARY", "DIFF-CANARY",
			"COMMAND-OUTPUT-CANARY", "STDIN-CANARY", "FILE-OUTPUT-CANARY", "MCP-PROGRESS-CANARY",
		]) expect(serialized).not.toContain(raw);
	});

	test("rejects orphan, duplicate, malformed, and unsupported provider notifications", () => {
		const { writer, normalizer } = harness();
		startTurn(normalizer);

		expectNormalizationError(() => normalizer.normalize("item/completed", {
			threadId: "provider-thread-private",
			turnId: "provider-turn-1",
			item: { id: "orphan", type: "mcpToolCall", status: "completed" },
		}), "lifecycle_violation");

		const started = {
			threadId: "provider-thread-private",
			turnId: "provider-turn-1",
			item: { id: "call-1", type: "mcpToolCall", server: "north", tool: "tell" },
		};
		normalizer.normalize("item/started", started);
		const admittedCount = writer.events().length;
		expectNormalizationError(
			() => normalizer.normalize("item/started", started),
			"lifecycle_violation",
		);
		expect(writer.events()).toHaveLength(admittedCount);

		const completed = {
			threadId: "provider-thread-private",
			turnId: "provider-turn-1",
			item: { id: "call-1", type: "mcpToolCall", status: "completed" },
		};
		normalizer.normalize("item/completed", completed);
		const terminalCount = writer.events().length;
		expectNormalizationError(
			() => normalizer.normalize("item/completed", completed),
			"lifecycle_violation",
		);
		expect(writer.events()).toHaveLength(terminalCount);

		expectNormalizationError(() => normalizer.normalize("thread/tokenUsage/updated", {
			threadId: "provider-thread-private",
			turnId: "provider-turn-1",
			tokenUsage: { total: {
				totalTokens: 1,
				inputTokens: -1,
				cachedInputTokens: 0,
				outputTokens: 2,
				reasoningOutputTokens: 0,
			} },
		}), "malformed_notification");
		expectNormalizationError(
			() => normalizer.normalize("future/provider/event", {}),
			"unsupported_notification",
		);
	});

	test("does not publish a patch artifact until durable persistence succeeds", () => {
		const { writer, normalizer } = harness({
			persist() {
				throw new Error("artifact store unavailable");
			},
		});
		startTurn(normalizer);
		normalizer.normalize("item/started", {
			threadId: "provider-thread-private",
			turnId: "provider-turn-1",
			item: { id: "file-1", type: "fileChange" },
		});
		const eventCount = writer.events().length;
		expectNormalizationError(() => normalizer.normalize("item/fileChange/patchUpdated", {
			threadId: "provider-thread-private",
			turnId: "provider-turn-1",
			itemId: "file-1",
			changes: [{ path: "private.ts", diff: "CANARY-UNPERSISTED-PATCH" }],
		}), "artifact_persistence_failed");
		expect(writer.events()).toHaveLength(eventCount);
		expect(writer.snapshot()?.artifacts).toEqual({});
		expect(JSON.stringify(writer.events())).not.toContain("CANARY-UNPERSISTED-PATCH");
	});

	test("reports bounded patch progress without inventing an artifact when no sink exists", () => {
		const { writer, normalizer } = harness(null);
		startTurn(normalizer);
		normalizer.normalize("item/started", {
			threadId: "provider-thread-private",
			turnId: "provider-turn-1",
			item: { id: "file-1", type: "fileChange" },
		});
		const result = normalizer.normalize("item/fileChange/patchUpdated", {
			threadId: "provider-thread-private",
			turnId: "provider-turn-1",
			itemId: "file-1",
			changes: [{ path: "private.ts", diff: "CANARY-UNPERSISTED-PATCH" }],
		});
		expect(result.events.map((event) => event.kind)).toEqual([
			"tool.progress", "run.progress",
		]);
		expect(writer.snapshot()?.artifacts).toEqual({});
		expect(JSON.stringify(writer.events())).not.toContain("CANARY-UNPERSISTED-PATCH");
	});

	test("marks explicitly supplied incomplete terminal usage as a partial contribution", () => {
		const { writer, normalizer } = harness();
		startTurn(normalizer);
		const settled = normalizer.settleTurn({
			status: "failed",
			origin: "provider",
			errorCode: "provider_turn_failed",
			usage: {
				lifetime: {
					inputTokens: 3,
					outputTokens: 1,
					cacheReadTokens: 0,
					cacheWriteTokens: 0,
					reasoningTokens: 0,
					modelCalls: 1,
				},
				context: { tokens: 4 },
			},
		});
		expect(settled.events.at(-1)).toMatchObject({
			kind: "model-call.completed",
			usageCoverage: "partial",
		});
		expect(writer.snapshot()?.usageCoverage).toEqual({
			providerTerminalCount: 1,
			scope: "wire_run_cumulative",
			totalStatus: "partial",
		});
	});

	test("settles a dead managed attempt before a replacement provider turn starts", () => {
		const { writer, normalizer } = harness();
		startTurn(normalizer);
		normalizer.normalize("item/started", {
			threadId: "provider-thread-private",
			turnId: "provider-turn-1",
			item: { id: "command-1", type: "commandExecution" },
		});

		const dead = normalizer.settleProviderRespawn();
		expect(dead.events.map((event) => event.kind)).toEqual([
			"tool.terminal", "model-call.completed",
		]);
		expect(dead.events.at(-1)).toMatchObject({
			kind: "model-call.completed",
			status: "failed",
			origin: "north",
			usageCoverage: "unavailable",
			errorCode: "provider_session_replaced",
			evidence: {
				turns: { unit: "provider-turn", count: 1, toolItems: 0, comparable: false },
				failure: {
					detail: "provider_session_replaced",
					landed: { completedTurns: 0, toolItems: 0 },
				},
			},
		});

		normalizer.normalize("turn/started", {
			threadId: "replacement-thread-private",
			turn: { id: "replacement-turn-private", status: "inProgress" },
		});
		expect(writer.events().filter((event) => event.kind === "model-call.started"))
			.toHaveLength(2);
		expect(writer.snapshot()?.modelCalls[wireModelCallId("model-call:test:0")]?.status)
			.toBe("failed");
		expect(writer.snapshot()?.modelCalls[wireModelCallId("model-call:test:1")]?.status)
			.toBe("running");
	});

	test("settles interrupted turns synthetically without terminating the run", () => {
		const { writer, normalizer } = harness();
		startTurn(normalizer);
		normalizer.normalize("item/started", {
			threadId: "provider-thread-private",
			turnId: "provider-turn-1",
			item: {
				id: "command-1",
				type: "commandExecution",
				command: "CANARY-PRIVATE-COMMAND",
			},
		});
		normalizer.normalize("item/started", {
			threadId: "provider-thread-private",
			turnId: "provider-turn-1",
			item: { id: "message-1", type: "agentMessage" },
		});

		const interrupted = normalizer.settleTurn({
			status: "cancelled",
			origin: "north",
			errorCode: "turn_interrupted",
		});
		expect(interrupted.type).toBe("turn.terminal");
		expect(interrupted.outcome.status).toBe("cancelled");
		expect(interrupted.events.map((event) => event.kind)).toEqual([
			"tool.terminal", "message.recorded", "model-call.completed",
		]);
		expect(writer.snapshot()?.toolCalls[wireToolCallId("tool:test:0")]).toMatchObject({
			status: "cancelled", origin: "north", errorCode: "turn_interrupted",
		});
		expect(writer.snapshot()?.lifecycle).toBe("running");
		expect(writer.events().some((event) => event.kind === "run.terminated")).toBe(false);

		startTurn(normalizer, "provider-turn-2");
		normalizer.normalize("item/started", {
			threadId: "provider-thread-private",
			turnId: "provider-turn-2",
			item: { id: "mcp-2", type: "mcpToolCall", server: "north", tool: "tell" },
		});
		const failed = normalizer.settleTurn({
			status: "failed",
			origin: "provider",
			errorCode: "provider_turn_failed",
		});
		expect(failed.events.map((event) => event.kind)).toEqual([
			"tool.terminal", "model-call.completed",
		]);
		expect(failed.events.at(-1)).toMatchObject({
			kind: "model-call.completed",
			origin: "provider",
			usageCoverage: "unavailable",
		});
		expect(writer.snapshot()?.toolCalls[wireToolCallId("tool:test:1")]).toMatchObject({
			status: "synthetic_failure", origin: "north", errorCode: "provider_turn_failed",
		});
		expect(writer.snapshot()?.lifecycle).toBe("running");
		expect(writer.snapshot()?.usageCoverage).toEqual({
			providerTerminalCount: 1,
			scope: "wire_run_cumulative",
			totalStatus: "unknown_incomplete_terminal",
		});

		expect(writer.events().some((event) => event.kind === "run.terminated")).toBe(false);
		expect(writer.snapshot()?.lifecycle).toBe("running");
		expect(JSON.stringify(writer.events())).not.toContain("CANARY-PRIVATE-COMMAND");
	});
});
