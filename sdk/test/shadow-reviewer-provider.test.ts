import { describe, expect, test } from "bun:test";
import type { Options } from "@anthropic-ai/claude-agent-sdk";

import { hasCanonicalHarnessAuthority } from "../src/harness";
import { admitAnthropic } from "../src/providers/anthropic";
import { providerJoinEvidence } from "../src/providers/provider-join";
import {
	runAnthropicShadowReview,
	type AnthropicShadowReviewerRuntime,
} from "../src/providers/shadow-reviewer";
import type { AgentProviderQuery } from "../src/providers/types";
import type { WireRunProvenance } from "../src/run-provenance";
import type { ShadowReviewerUpdate } from "../src/shadow-reviewer";
import type { WireRunSnapshot } from "../src/wire/reducer";
import {
	wireMessageId,
	wireModelCallId,
	wireRunId,
	wireToolCallId,
	type WireEvent,
	type WireQuery,
} from "../src/wire";

const SOURCE_RUN_ID = wireRunId("run:shadow-reviewer-source");
const CHILD_RUN_ID = wireRunId("run:shadow-reviewer-child");
const RAW_OUTPUT = JSON.stringify({
	kind: "note",
	severity: "nit",
	issueCode: "unresolved_failure",
	sourceSequence: 9,
});
const UPDATE: ShadowReviewerUpdate = Object.freeze({
	sourceRunId: SOURCE_RUN_ID,
	sourceFromSequence: 4,
	sourceThroughSequence: 9,
	privacyOmittedEvents: 2,
	capacityOmittedEvents: 1,
	projectedSequences: Object.freeze([4, 9]),
	inputSha256: "a".repeat(64),
	projection: "{\"bounded\":true}",
});
const USAGE = Object.freeze({
	lifetime: {
		inputTokens: 11,
		outputTokens: 7,
		cacheReadTokens: 3,
		cacheWriteTokens: 2,
		reasoningTokens: 0,
		modelCalls: 1,
	},
	context: { tokens: 18 },
});

interface ProviderFixture {
	readonly runtime: AnthropicShadowReviewerRuntime;
	readonly transcript: WireEvent[];
	readonly published: WireEvent[];
	readonly providerOptions: () => Options | undefined;
	readonly providerArgs: () => AgentProviderQuery | undefined;
	readonly provenance: () => WireRunProvenance | undefined;
	readonly telemetrySnapshot: () => WireRunSnapshot | undefined;
	readonly reviewerAgent: () => string | undefined;
	readonly queryCalls: () => number;
	readonly queryClosed: () => number;
}

function providerFixture(options: { readonly admitTool?: boolean } = {}): ProviderFixture {
	const transcript: WireEvent[] = [];
	const published: WireEvent[] = [];
	let capturedOptions: Options | undefined;
	let capturedArgs: AgentProviderQuery | undefined;
	let capturedProvenance: WireRunProvenance | undefined;
	let capturedSnapshot: WireRunSnapshot | undefined;
	let capturedAgent: string | undefined;
	let queryCalls = 0;
	let queryClosed = 0;
	let clock = 100;
	const createQuery = (args: AgentProviderQuery, admitted: boolean): WireQuery => {
		queryCalls += 1;
		expect(admitted).toBe(false);
		capturedArgs = args;
		capturedOptions = args.options;
		const modelCallId = wireModelCallId("model-call:shadow-reviewer");
		const messageId = wireMessageId("message:shadow-reviewer");
		const toolCallId = wireToolCallId("tool:shadow-reviewer");
		const events = args.context.writer.appendAll([
			{
				kind: "model-call.started",
				modelCallId,
				model: args.context.route.model,
				effort: args.context.route.effort,
				attempt: 1,
			},
			{
				kind: "message.recorded",
				messageId,
				stage: "started",
				role: "assistant",
				modelCallId,
			},
			{
				kind: "message.recorded",
				messageId,
				stage: "delta",
				role: "assistant",
				content: RAW_OUTPUT,
				modelCallId,
			},
			...(options.admitTool ? [
				{
					kind: "tool.admitted" as const,
					toolCallId,
					name: "unexpected-reviewer-tool",
					messageId,
					modelCallId,
					schema: { status: "unavailable" as const, reason: "fixture" },
					argumentPreview: "RAW_TOOL_ARGUMENT_CANARY",
				},
				{
					kind: "tool.terminal" as const,
					toolCallId,
					status: "succeeded" as const,
					origin: "provider" as const,
					resultPreview: "RAW_TOOL_RESULT_CANARY",
				},
			] : []),
			{
				kind: "message.recorded",
				messageId,
				stage: "completed",
				role: "assistant",
				modelCallId,
			},
			{
				kind: "model-call.completed",
				modelCallId,
				status: "succeeded",
				origin: "provider",
				usage: USAGE,
				usageCoverage: "exact",
				evidence: {
					providerJoin: providerJoinEvidence("anthropic", {
						sessionId: "RAW_PROVIDER_SESSION_CANARY",
						turnIds: ["RAW_PROVIDER_TURN_CANARY"],
						sessionPersistence: "persisted",
					}),
					turns: { unit: "assistant-turn", count: 1, comparable: true },
					providerDurationMs: 21,
				},
			},
		]);
		return {
			executionTransport: "sdk-stream",
			close: async () => { queryClosed += 1; },
			async *[Symbol.asyncIterator]() {
				for (const event of events) yield event;
			},
		};
	};
	return {
		runtime: {
			createQuery,
			createRunId: () => CHILD_RUN_ID,
			nowMs: () => {
				const value = clock;
				clock += 25;
				return value;
			},
			openTranscript: async (agentId) => {
				capturedAgent = agentId;
				return {
					writeWireEvent: async (event) => {
						transcript.push(event);
						return event;
					},
					close: async () => {},
				};
			},
			publishEvents: async (_identity, events) => {
				published.push(...events);
				return "recorded";
			},
			recordTelemetry: async (_identity, snapshot, _ledger, provenance) => {
				capturedSnapshot = snapshot;
				capturedProvenance = provenance;
				return "recorded";
			},
		},
		transcript,
		published,
		providerOptions: () => capturedOptions,
		providerArgs: () => capturedArgs,
		provenance: () => capturedProvenance,
		telemetrySnapshot: () => capturedSnapshot,
		reviewerAgent: () => capturedAgent,
		queryCalls: () => queryCalls,
		queryClosed: () => queryClosed,
	};
}

describe("Anthropic shadow reviewer provider", () => {
	test("executes with no tools and publishes only a separate text-free ephemeral child run", async () => {
		const fixture = providerFixture();
		const result = await runAnthropicShadowReview({
			update: UPDATE,
			target: { id: "reviewer-anthropic", provider: "anthropic" },
			sourceAgentId: "source-lane",
			thread: "thread-shadow-reviewer",
			parentThread: "thread-parent",
			coordinator: "root",
			signal: new AbortController().signal,
		}, fixture.runtime);

		const providerOptions = fixture.providerOptions();
		expect(providerOptions).toBeDefined();
		expect(providerOptions).toMatchObject({
			allowedTools: [],
			maxTurns: 1,
			mcpServers: {},
			permissionMode: "default",
			persistSession: false,
			settingSources: [],
			strictMcpConfig: true,
			tools: [],
		});
		expect(providerOptions?.outputFormat).toMatchObject({ type: "json_schema" });
		expect(typeof providerOptions?.systemPrompt).toBe("string");
		expect(providerOptions?.systemPrompt).toContain("no authority");
		expect(hasCanonicalHarnessAuthority(providerOptions!, "anthropic")).toBe(true);
		await expect(admitAnthropic(providerOptions!, {
			id: "reviewer-anthropic",
			provider: "anthropic",
		})).resolves.toBeUndefined();
		expect(fixture.providerArgs()?.input).toBe(UPDATE.projection);
		expect(fixture.providerArgs()?.target).toEqual({
			id: "reviewer-anthropic",
			provider: "anthropic",
		});

		expect(result).toEqual({
			runId: CHILD_RUN_ID,
			status: "succeeded",
			output: RAW_OUTPUT,
			usageStatus: "exact",
			tokens: 23,
			durationMs: 25,
		});
		expect(result.runId).not.toBe(SOURCE_RUN_ID);
		expect(fixture.reviewerAgent()).toContain("shadow-reviewer");
		expect(fixture.queryClosed()).toBe(1);
		expect(fixture.published).toEqual(fixture.transcript);
		expect(fixture.published.map(({ kind }) => kind)).toEqual([
			"run.started",
			"model-call.started",
			"model-call.completed",
			"run.terminated",
		]);
		const durableJson = JSON.stringify(fixture.published);
		for (const canary of [
			"RAW_REVIEW_TEXT_CANARY",
			"RAW_PROVIDER_SESSION_CANARY",
			"RAW_PROVIDER_TURN_CANARY",
		]) {
			expect(durableJson).not.toContain(canary);
		}
		const completed = fixture.published.find((event) => event.kind === "model-call.completed");
		expect(completed).toMatchObject({
			kind: "model-call.completed",
			evidence: { providerJoin: { sessionPersistence: "ephemeral" } },
		});
		expect(fixture.telemetrySnapshot()?.parentRunId).toBe(SOURCE_RUN_ID);
		expect(fixture.provenance()).toMatchObject({
			role: "shadow-reviewer",
			provider: "anthropic",
			providerTarget: "reviewer-anthropic",
			executionSource: "north-managed",
			executionTransport: "sdk-stream",
			shadowReviewerExecution: {
				targetId: "reviewer-anthropic",
				sourceRunId: SOURCE_RUN_ID,
				sourceFromSequence: 4,
				sourceThroughSequence: 9,
				privacyOmittedEvents: 2,
				capacityOmittedEvents: 1,
				inputSha256: "a".repeat(64),
			},
		});
		expect(fixture.provenance()?.learningAssignment).toBeUndefined();
	});

	test("quarantines an impossible tool admission without persisting tool detail", async () => {
		const fixture = providerFixture({ admitTool: true });
		const result = await runAnthropicShadowReview({
			update: UPDATE,
			target: { id: "reviewer-anthropic", provider: "anthropic" },
			sourceAgentId: "source-lane",
			thread: "thread-shadow-reviewer",
			signal: new AbortController().signal,
		}, fixture.runtime);

		expect(result).toMatchObject({ status: "failed", unsafeOutput: true });
		expect(result.output).toBeUndefined();
		const durableJson = JSON.stringify(fixture.published);
		expect(durableJson).not.toContain("RAW_TOOL_ARGUMENT_CANARY");
		expect(durableJson).not.toContain("RAW_TOOL_RESULT_CANARY");
		expect(durableJson).not.toContain("unexpected-reviewer-tool");
		expect(fixture.published).toContainEqual(expect.objectContaining({
			kind: "tool.admitted",
			name: "shadow-reviewer-data-only-violation",
		}));
		expect(fixture.published).toContainEqual(expect.objectContaining({
			kind: "tool.terminal",
			status: "synthetic_failure",
			origin: "north",
		}));
		expect(fixture.telemetrySnapshot()?.lifecycle).toBe("failed");
	});

	test("an already-aborted update never reaches the provider", async () => {
		const fixture = providerFixture();
		const controller = new AbortController();
		controller.abort(new Error("operator abort"));
		const result = await runAnthropicShadowReview({
			update: UPDATE,
			target: { id: "reviewer-anthropic", provider: "anthropic" },
			sourceAgentId: "source-lane",
			thread: "thread-shadow-reviewer",
			signal: controller.signal,
		}, fixture.runtime);

		expect(result.status).toBe("cancelled");
		expect(fixture.queryCalls()).toBe(0);
		expect(fixture.published.map(({ kind }) => kind)).toEqual([
			"run.started",
			"run.terminated",
		]);
		expect(fixture.telemetrySnapshot()?.lifecycle).toBe("cancelled");
	});
});
