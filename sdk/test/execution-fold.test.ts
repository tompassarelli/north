import { expect, test } from "bun:test";
import { makeExecutionFold } from "../src/execution-fold";
import { providerJoinEvidence } from "../src/providers/provider-join";
import { resolveStrugglePolicy } from "../src/struggle";
import {
	WireEventWriter,
	wireEventId,
	wireMessageId,
	wireModelCallId,
	wireRunId,
	wireToolCallId,
} from "../src/wire";

function writer(label: string): WireEventWriter {
	return new WireEventWriter({
		runId: wireRunId(`run:execution-fold-${label}`),
		eventId: (sequence) => wireEventId(`event:execution-fold-${label}-${sequence}`),
		now: () => "2026-08-10T00:00:00.000Z",
	});
}

test("one event fold derives outer execution state without terminating the run", () => {
	const source = writer("complete");
	const modelCallId = wireModelCallId("model-call:execution-fold-complete");
	const messageId = wireMessageId("message:execution-fold-complete");
	const readId = wireToolCallId("tool:execution-fold-read");
	const backgroundId = wireToolCallId("tool:execution-fold-background");
	const join = providerJoinEvidence("openai", {
		sessionId: "private-session",
		turnIds: ["private-turn"],
		sessionPersistence: "ephemeral",
	});

	source.append({ kind: "run.started", lifecycle: "running" });
	source.append({
		kind: "model-call.started",
		modelCallId,
		model: { provider: "openai", tier: "senior" },
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
		kind: "message.recorded",
		messageId,
		modelCallId,
		stage: "delta",
		role: "assistant",
		content: "work ",
	});
	source.append({
		kind: "tool.admitted",
		toolCallId: readId,
		messageId,
		modelCallId,
		name: "Read",
		argumentPreview: "{\"path\":\"README.md\"}",
		schema: { status: "unavailable", reason: "test" },
	});
	source.append({
		kind: "tool.terminal",
		toolCallId: readId,
		status: "succeeded",
		origin: "provider",
		resultPreview: "contents",
	});
	source.append({
		kind: "message.recorded",
		messageId,
		modelCallId,
		stage: "delta",
		role: "assistant",
		content: "done",
	});
	source.append({
		kind: "message.recorded",
		messageId,
		modelCallId,
		stage: "completed",
		role: "assistant",
	});
	source.append({
		kind: "tool.admitted",
		toolCallId: backgroundId,
		name: "background-task",
		schema: { status: "unavailable", reason: "provider lifecycle" },
	});
	source.append({
		kind: "tool.progress",
		toolCallId: backgroundId,
		progress: { status: "completed" },
	});
	source.append({
		kind: "run.progress",
		lifecycle: "running",
		progress: { compactions: 2 },
	});
	source.append({
		kind: "model-call.completed",
		modelCallId,
		status: "succeeded",
		origin: "provider",
		usage: {
			lifetime: {
				inputTokens: 100,
				outputTokens: 20,
				cacheReadTokens: 60,
				cacheWriteTokens: 0,
				reasoningTokens: 7,
				modelCalls: 1,
			},
			context: { tokens: 90, window: 200_000 },
		},
		usageCoverage: "exact",
		evidence: {
			providerJoin: join,
			turns: { unit: "provider-turn", count: 1, toolItems: 2, comparable: false },
			providerDurationMs: 1_250,
		},
	});

	const fold = makeExecutionFold(resolveStrugglePolicy("worker", {}));
	for (const event of source.events()) fold.observe(event);
	const state = fold.snapshot()!;
	expect(state.run.lifecycle).toBe("running");
	expect(state.lastCompletedAssistantOutput).toBe("work done");
	expect(state.latestModelCallTerminal).toMatchObject({
		id: modelCallId,
		status: "succeeded",
		origin: "provider",
	});
	expect(state.pendingBackgroundTasks).toEqual([backgroundId]);
	expect(state.compactions).toBe(2);
	expect(state.usage).toMatchObject({ total: 120, totalStatus: "exact" });
	expect(state.completionEvidence).toHaveLength(1);
	expect(state.providerJoin).toEqual(join);
	expect(state.turnEvidence).toEqual([
		{ unit: "provider-turn", count: 1, toolItems: 2, comparable: false },
	]);
	expect(state.toolActivity).toEqual({
		admitted: 2,
		progressed: 1,
		terminal: 1,
		succeeded: 1,
		failed: 0,
		cancelled: 0,
		syntheticFailures: 0,
		pending: 1,
	});
	expect(state.struggle).toMatchObject({ errorCount: 0, triggers: [] });
	expect(state.activityCount).toBe(9);

	const terminal = source.append({
		kind: "tool.terminal",
		toolCallId: backgroundId,
		status: "succeeded",
		origin: "provider",
	});
	const observation = fold.observe(terminal);
	expect(observation.backgroundTask).toEqual({ kind: "settled", toolCallId: backgroundId });
	expect(observation.state.pendingBackgroundTasks).toEqual([]);
});

test("the streaming fold retains turn joins across a managed provider session replacement", () => {
	const source = writer("session-replacement");
	const joins = ["private-session-1", "private-session-2"].map((sessionId, index) => (
		providerJoinEvidence("openai", {
			sessionId,
			turnIds: [`private-turn-${index + 1}`],
			sessionPersistence: "ephemeral",
		})
	));
	source.append({ kind: "run.started", lifecycle: "running" });
	for (const [index, join] of joins.entries()) {
		const modelCallId = wireModelCallId(`model-call:execution-fold-session-replacement:${index + 1}`);
		source.append({
			kind: "model-call.started",
			modelCallId,
			model: { provider: "openai", tier: "senior" },
			attempt: index + 1,
		});
		source.append({
			kind: "model-call.completed",
			modelCallId,
			status: "succeeded",
			origin: "provider",
			usage: {
				lifetime: {
					inputTokens: (index + 1) * 100,
					outputTokens: (index + 1) * 20,
					cacheReadTokens: 0,
					cacheWriteTokens: 0,
					reasoningTokens: 0,
					modelCalls: index + 1,
				},
				context: { tokens: (index + 1) * 100 },
			},
			usageCoverage: "exact",
			evidence: { providerJoin: join },
		});
	}

	const fold = makeExecutionFold(resolveStrugglePolicy("worker", {}));
	for (const event of source.events()) fold.observe(event);
	expect(fold.snapshot()?.providerJoin).toEqual({
		version: "north-provider-join:v1",
		turnKeys: joins.flatMap((join) => join.turnKeys).sort(),
		sessionPersistence: "ephemeral",
		coverage: "partial",
	});
});

test("a completed call without join evidence prevents exact folded join coverage", () => {
	const source = writer("incomplete-join-coverage");
	const joinedCallId = wireModelCallId("model-call:execution-fold-incomplete-join:1");
	const unjoinedCallId = wireModelCallId("model-call:execution-fold-incomplete-join:2");
	const join = providerJoinEvidence("openai", {
		sessionId: "private-session",
		turnIds: ["private-turn"],
		sessionPersistence: "ephemeral",
	});
	source.append({ kind: "run.started", lifecycle: "running" });
	source.append({
		kind: "model-call.started",
		modelCallId: joinedCallId,
		model: { provider: "openai", tier: "senior" },
		attempt: 1,
	});
	source.append({
		kind: "model-call.completed",
		modelCallId: joinedCallId,
		status: "succeeded",
		origin: "provider",
		usage: {
			lifetime: {
				inputTokens: 100,
				outputTokens: 20,
				cacheReadTokens: 0,
				cacheWriteTokens: 0,
				reasoningTokens: 0,
				modelCalls: 1,
			},
			context: { tokens: 100 },
		},
		usageCoverage: "exact",
		evidence: { providerJoin: join },
	});
	source.append({
		kind: "model-call.started",
		modelCallId: unjoinedCallId,
		model: { provider: "openai", tier: "senior" },
		attempt: 2,
	});
	source.append({
		kind: "model-call.completed",
		modelCallId: unjoinedCallId,
		status: "failed",
		origin: "north",
		usage: source.snapshot()!.usage,
		usageCoverage: "unavailable",
		errorCode: "provider_session_replaced",
	});

	const fold = makeExecutionFold(resolveStrugglePolicy("worker", {}));
	for (const event of source.events()) fold.observe(event);
	expect(fold.snapshot()?.providerJoin).toEqual({ ...join, coverage: "partial" });
});

test("failure and interrupt evidence become provider-neutral outer diagnostics", () => {
	const source = writer("failure");
	const modelCallId = wireModelCallId("model-call:execution-fold-failure");
	source.append({ kind: "run.started", lifecycle: "running" });
	source.append({
		kind: "model-call.started",
		modelCallId,
		model: { provider: "openai", tier: "standard" },
		attempt: 1,
	});
	source.append({
		kind: "model-call.completed",
		modelCallId,
		status: "failed",
		origin: "north",
		errorCode: "north_turn_deadline",
		usage: source.snapshot()!.usage,
		usageCoverage: "unavailable",
		evidence: {
			failure: {
				detail: "north_turn_deadline",
				landed: { completedTurns: 0, toolItems: 1 },
			},
			interrupt: {
			reason: "north_post_tool_silence",
				deadlineMs: 60_000,
				inactivityThresholdMs: 10_000,
				lastActivityAgeMs: 10_500,
				eventCount: 2,
			},
		},
	});
	const fold = makeExecutionFold(resolveStrugglePolicy("worker", {}));
	for (const event of source.events()) fold.observe(event);
	const state = fold.snapshot()!;
	expect(state.providerErrorDetail).toContain("code=north_turn_deadline");
	expect(state.providerErrorDetail).toContain("failure=north_turn_deadline");
	expect(state.providerErrorDetail).not.toContain("turn became silent");
	expect(state.deadlineExceededDetail).toContain('"reason":"north_post_tool_silence"');
	expect(state.run.lifecycle).toBe("running");
});

test("the streaming fold rejects a repeated event ID before state mutation", () => {
	const source = writer("duplicate");
	source.append({ kind: "run.started", lifecycle: "running" });
	source.append({
		kind: "run.progress",
		lifecycle: "waiting",
		progress: { currentAction: "waiting" },
	});
	const [started, progress] = source.events();
	const fold = makeExecutionFold(resolveStrugglePolicy("worker", {}));
	fold.observe(started!);
	fold.observe(progress!);
	expect(() => fold.observe(started!)).toThrow("duplicated");
	expect(fold.snapshot()?.run.lastSequence).toBe(1);
});
