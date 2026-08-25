import { describe, expect, test } from "bun:test";
import type {
	Options,
	Query,
	SDKUserMessage,
} from "@anthropic-ai/claude-agent-sdk";
import {
	createAnthropicQuery,
	type AnthropicQueryRuntime,
} from "../src/providers/anthropic";
import { observeAnthropicQuery } from "../src/providers/anthropic-observations";
import { resolveTier } from "../src/providers/catalog";
import { ProviderRetrySafeError, type AgentProviderQuery } from "../src/providers/types";
import {
	WireEventWriter,
	wireEventId,
	wireRunId,
	type WireArtifactMaterial,
	type WireArtifactSink,
	type WireEvent,
	type WireQueryInput,
} from "../src/wire";

const USAGE = {
	input_tokens: 11,
	output_tokens: 7,
	cache_creation_input_tokens: 2,
	cache_read_input_tokens: 3,
} as const;

function providerArgs(
	label: string,
	input: WireQueryInput = "first turn",
	artifacts?: WireArtifactSink,
): AgentProviderQuery {
	const writer = new WireEventWriter({
		runId: wireRunId(`run:anthropic-provider:${label}`),
		eventId: (sequence) => wireEventId(`event:anthropic-provider:${label}:${sequence}`),
		now: () => "2026-08-10T00:00:00.000Z",
	});
	writer.append({ kind: "run.started", lifecycle: "running", owner: "test" });
	return {
		input,
		options: {},
		context: {
			writer,
			...(artifacts === undefined ? {} : { artifacts }),
			route: {
				model: { provider: "anthropic", tier: "senior", capabilityClass: "authoring" },
				effort: "high",
				attempt: 1,
				contextWindow: 200_000,
			},
		},
	};
}

function result(uuid: string, sessionId: string): Record<string, unknown> {
	return {
		type: "result",
		subtype: "success",
		uuid,
		session_id: sessionId,
		is_error: false,
		duration_ms: 25,
		num_turns: 1,
		result: "done",
		usage: USAGE,
	};
}

interface FakeRuntimeState {
	resumes: Array<string | undefined>;
	options: Array<Options & { continueConversation?: unknown }>;
	inputs: Array<string | SDKUserMessage[]>;
	models: string[];
	efforts: Array<string | null | undefined>;
	interrupts: number;
	returns: number;
	settles: number;
}

function fakeRuntime(
	turns: readonly (readonly unknown[])[],
	state: FakeRuntimeState,
	failureAfterMessages?: Error,
): AnthropicQueryRuntime {
	let turnIndex = 0;
	return {
		query: ((parameters: {
			prompt: string | AsyncIterable<SDKUserMessage>;
			options?: Options;
		}) => {
			const messages = turns[turnIndex++] ?? [];
			state.resumes.push(parameters.options?.resume);
			state.options.push({ ...parameters.options });
			const generator = (async function*() {
				if (typeof parameters.prompt === "string") {
					state.inputs.push(parameters.prompt);
				} else {
					const input: SDKUserMessage[] = [];
					for await (const message of parameters.prompt) input.push(message);
					state.inputs.push(input);
				}
				for (const message of messages) yield message;
				if (failureAfterMessages) throw failureAfterMessages;
			})();
			return {
				interrupt: async () => { state.interrupts += 1; },
				setModel: async (model?: string) => {
					if (model) state.models.push(model);
				},
				applyFlagSettings: async (settings: { effortLevel?: string | null }) => {
					state.efforts.push(settings.effortLevel);
				},
				return: async () => {
					state.returns += 1;
					return generator.return(undefined);
				},
				[Symbol.asyncIterator]: () => generator,
			} as unknown as Query;
		}),
		observe: observeAnthropicQuery,
		createLifecycle: () => ({
			spawnClaudeCodeProcess: () => { throw new Error("not used"); },
			settle: async () => { state.settles += 1; },
			forceKill: () => {},
			started: () => false,
		}),
	};
}

describe("Anthropic public wire adapter", () => {
	test("carries the production artifact sink into public tool-result normalization", async () => {
		const persisted: WireArtifactMaterial[] = [];
		const sink: WireArtifactSink = {
			persist(artifact) {
				persisted.push(artifact);
				return { artifactId: artifact.artifactId, digest: artifact.digest };
			},
		};
		const messages = [[
			{
				type: "assistant",
				uuid: "artifact-assistant",
				session_id: "artifact-session",
				parent_tool_use_id: null,
				message: {
					id: "artifact-turn",
					role: "assistant",
					model: "private-model",
					content: [{
						type: "tool_use",
						id: "artifact-tool",
						name: "Bash",
						input: { command: "printf retained" },
					}],
				},
			},
			{
				type: "user",
				uuid: "artifact-result",
				session_id: "artifact-session",
				parent_tool_use_id: null,
				message: {
					role: "user",
					content: [{
						type: "tool_result",
						tool_use_id: "artifact-tool",
						content: "public adapter retained output",
					}],
				},
			},
			result("artifact-terminal", "artifact-session"),
		]];
		const state: FakeRuntimeState = {
			resumes: [], options: [], inputs: [], models: [], efforts: [],
			interrupts: 0, returns: 0, settles: 0,
		};
		const providerQuery = createAnthropicQuery(
			providerArgs("artifact-sink", "retain this", sink),
			true,
			fakeRuntime(messages, state),
		);
		const events: WireEvent[] = [];
		for await (const event of providerQuery) events.push(event);
		expect(persisted).toHaveLength(1);
		expect(persisted[0]).toMatchObject({ content: "public adapter retained output" });
		const publishedIndex = events.findIndex((event) => event.kind === "artifact.published");
		const terminalIndex = events.findIndex((event) => event.kind === "tool.terminal");
		expect(publishedIndex).toBeGreaterThan(-1);
		expect(terminalIndex).toBeGreaterThan(publishedIndex);
		expect(events[terminalIndex]).toMatchObject({
			resultArtifactId: persisted[0]!.artifactId,
			resultArtifactDigest: persisted[0]!.digest,
		});
	});

	test("strips caller-authored provider session continuation at the public adapter boundary", async () => {
		const state: FakeRuntimeState = {
			resumes: [], options: [], inputs: [], models: [], efforts: [], interrupts: 0, returns: 0, settles: 0,
		};
		const args = providerArgs("caller-session-smuggling");
		args.options = {
			resume: "caller-resume",
			continue: true,
			forkSession: true,
			resumeSessionAt: "caller-message",
			sessionId: "caller-session",
			extraArgs: {
				continue: null,
				resume: "nested-caller-resume",
				"fork-session": null,
				resumeSessionAt: "nested-caller-message",
				"session-id=caller-session": null,
				verbose: null,
			},
		};
		const providerQuery = createAnthropicQuery(
			args,
			true,
			fakeRuntime([[result("result-session-smuggling", "provider-session")]], state),
		);
		for await (const _event of providerQuery) {
			// Consuming the turn forces construction of the exact provider options.
		}

		expect(state.resumes).toEqual([undefined]);
		const options = state.options[0];
		if (!options) throw new Error("expected captured Anthropic options");
		for (const key of [
			"resume", "continue", "forkSession", "resumeSessionAt", "sessionId",
		]) {
			expect(Object.hasOwn(options, key)).toBe(false);
		}
		expect(options.extraArgs).toEqual({ verbose: null });
	});

	test("normalizes two separately-consumed turns on one query and retains continuation privately", async () => {
		const rawSession = "raw-session-canary";
		const rawModel = "raw-model-canary";
		const firstMessages = [
			{ type: "system", subtype: "init", apiKeySource: "oauth", session_id: rawSession },
			{
				type: "assistant",
				uuid: "raw-assistant-canary",
				session_id: rawSession,
				parent_tool_use_id: null,
				message: {
					id: "raw-turn-canary",
					role: "assistant",
					model: rawModel,
					content: [{
						type: "tool_use",
						id: "raw-tool-canary",
						name: "mcp__north__tell",
						input: { message: "private first input" },
					}],
				},
			},
			{
				type: "user",
				uuid: "raw-user-canary",
				session_id: rawSession,
				parent_tool_use_id: null,
				message: {
					role: "user",
					content: [{
						type: "tool_result",
						tool_use_id: "raw-tool-canary",
						content: "ok",
					}],
				},
			},
			result("raw-result-canary", rawSession),
		];
		// A valid resumed turn may contain only its result message. This forces the
		// adapter's explicit next-turn transition to reopen the normalizer; incidental
		// assistant/user activity cannot hide a stale completed-turn state.
		const secondMessages = [result("raw-second-result", rawSession)];
		const state: FakeRuntimeState = {
			resumes: [], options: [], inputs: [], models: [], efforts: [], interrupts: 0, returns: 0, settles: 0,
		};
		const args = providerArgs("continuation");
		const providerQuery = createAnthropicQuery(
			args,
			true,
			fakeRuntime([firstMessages, secondMessages], state),
		);
		expect(providerQuery.executionTransport).toBe("sdk-stream");
		const executionActivity = providerQuery.executionActivity;
		expect(executionActivity?.snapshot().sequence).toBe(0);
		const subscribed: WireEvent[] = [];
		providerQuery.subscribeProviderEvents?.((event) => { subscribed.push(event); });
		await providerQuery.interruptTurn?.();
		await providerQuery.setModel?.({
			provider: "anthropic",
			tier: "frontier",
			capabilityClass: "authoring",
		});
		await providerQuery.applyFlagSettings?.({ effortLevel: "xhigh" });
		const continuedInput: WireQueryInput = {
			async *[Symbol.asyncIterator]() {
				yield { kind: "user.input", text: "continue safely" };
			},
		};
		const first: WireEvent[] = [];
		for await (const event of providerQuery) first.push(event);
		expect(first.filter((event) => event.kind === "model-call.completed")).toHaveLength(1);
		await providerQuery.continueTurn?.(continuedInput);
		const second: WireEvent[] = [];
		for await (const event of providerQuery) second.push(event);
		const combined = [...first, ...second];
		const modelTerminals = combined.filter((event) => event.kind === "model-call.completed");
		expect(modelTerminals).toHaveLength(2);
		expect(modelTerminals.map(({ status, origin }) => ({ status, origin }))).toEqual([
			{ status: "succeeded", origin: "provider" },
			{ status: "succeeded", origin: "provider" },
		]);
		expect(modelTerminals[0]!.modelCallId).not.toBe(modelTerminals[1]!.modelCallId);
		expect(modelTerminals.some(({ origin }) => origin === "north")).toBe(false);

		expect(state.interrupts).toBe(1);
		expect(first.filter((event) => event.kind === "tool.terminal")).toHaveLength(1);
		expect(first.some((event) => event.kind === "run.terminated")).toBe(false);
		const firstTerminal = first.find((event) => event.kind === "model-call.completed");
		expect(firstTerminal).toMatchObject({
			kind: "model-call.completed",
			origin: "provider",
			evidence: {
				turns: { unit: "assistant-turn", count: 1, comparable: true },
				providerDurationMs: 25,
				providerJoin: { version: "north-provider-join:v1", coverage: "exact" },
			},
		});
		const encodedFirst = JSON.stringify(first);
		for (const secret of [rawSession, rawModel, "raw-assistant-canary", "raw-tool-canary"]) {
			expect(encodedFirst).not.toContain(secret);
		}
		expect(encodedFirst).not.toContain("session_id");
		expect(encodedFirst).not.toContain('"_north_');

		expect(state.resumes).toEqual([undefined, rawSession]);
		expect(state.inputs[0]).toBe("first turn");
		expect(state.inputs[1]).toEqual([{
			type: "user",
			message: { role: "user", content: "continue safely" },
			parent_tool_use_id: null,
		}]);
		expect(state.models).toEqual([resolveTier("anthropic", "frontier").model]);
		expect(state.efforts).toEqual(["xhigh"]);
		const secondStarted = second.find((event) => event.kind === "model-call.started");
		expect(secondStarted).toMatchObject({
			kind: "model-call.started",
			model: { provider: "anthropic", tier: "frontier", capabilityClass: "authoring" },
			effort: "xhigh",
		});
		expect(subscribed).toEqual(combined);
		expect(executionActivity?.snapshot()).toMatchObject({
			lastProvider: {
				origin: "provider",
				kind: "provider.anthropic.event.accepted",
			},
		});
		expect(executionActivity?.snapshot().sequence).toBeGreaterThan(0);
		expect(providerQuery.mcpActivity?.()).toEqual({
			source: "anthropic-agent-sdk:assistant-tool-use",
			coverage: "exact",
				totalCalls: 1,
				tools: [{ server: "north", tool: "tell", count: 1 }],
			operationReceipts: [],
			operationAggregates: [],
		});
		expect(JSON.stringify(providerQuery)).not.toContain(rawSession);
		expect(state.returns).toBe(2);
		expect(state.settles).toBe(2);
	});

	test("rejects unsafe subscription authentication as a stable post-acceptance failure", async () => {
		const rawCanary = "RAW_UNSAFE_AUTH_CANARY";
		const state: FakeRuntimeState = {
			resumes: [], options: [], inputs: [], models: [], efforts: [], interrupts: 0, returns: 0, settles: 0,
		};
		const providerQuery = createAnthropicQuery(
			providerArgs("unsafe-auth"),
			true,
			fakeRuntime([[
				{
					type: "system",
					subtype: "init",
					apiKeySource: "user",
					diagnostic: rawCanary,
				},
			]], state),
		);

		let failure: unknown;
		try {
			for await (const _event of providerQuery) {
				throw new Error("unsafe authentication emitted a wire event");
			}
		} catch (error) {
			failure = error;
		}

		expect(failure).toBeInstanceOf(Error);
		expect(failure).not.toBeInstanceOf(ProviderRetrySafeError);
		expect((failure as Error).message).toBe("anthropic_provider_execution_failed");
		expect(String(failure)).not.toContain(rawCanary);
		expect(state.returns).toBe(1);
		expect(state.settles).toBe(1);
	});

	test("synthetically settles abrupt raw failure with one tool terminal and stable diagnostics", async () => {
		const rawCanary = "RAW_PROVIDER_FAILURE_CANARY";
		const messages = [[{
			type: "assistant",
			uuid: "raw-abrupt-assistant",
			session_id: "raw-abrupt-session",
			parent_tool_use_id: null,
			message: {
				id: "raw-abrupt-turn",
				role: "assistant",
				model: "raw-abrupt-model",
				content: [{
					type: "tool_use",
					id: "raw-abrupt-tool",
					name: "Bash",
					input: { command: "false" },
				}],
			},
		}]];
		const state: FakeRuntimeState = {
			resumes: [], options: [], inputs: [], models: [], efforts: [], interrupts: 0, returns: 0, settles: 0,
		};
		const providerQuery = createAnthropicQuery(
			providerArgs("abrupt"),
			true,
			fakeRuntime(messages, state, new Error(rawCanary)),
		);
		const events: WireEvent[] = [];
		let error: unknown;
		try {
			for await (const event of providerQuery) events.push(event);
		} catch (caught) {
			error = caught;
		}

		expect(error).toBeInstanceOf(Error);
		expect((error as Error).message).toBe("anthropic_provider_execution_failed");
		expect(String(error)).not.toContain(rawCanary);
		expect(events.filter((event) => event.kind === "tool.terminal")).toHaveLength(1);
		expect(events.find((event) => event.kind === "tool.terminal")).toMatchObject({
			status: "synthetic_failure",
			origin: "north",
			errorCode: "provider_error",
		});
		expect(events.find((event) => event.kind === "model-call.completed")).toMatchObject({
			status: "failed",
			origin: "north",
			errorCode: "provider_error",
			evidence: { failure: { detail: "provider_error" } },
		});
		expect(events.some((event) => event.kind === "run.terminated")).toBe(false);
		expect(JSON.stringify(events)).not.toContain(rawCanary);
	});
});
