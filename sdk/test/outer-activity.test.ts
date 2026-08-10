import { expect, test } from "bun:test";
import {
	isOuterExecutionActivity,
	outerExecutionActivityKind,
} from "../src/providers/outer-activity";
import {
	WireEventWriter,
	wireArtifactId,
	wireEventId,
	wireMessageId,
	wireModelCallId,
	wireRunId,
	wireToolCallId,
} from "../src/wire";

function writer(): WireEventWriter {
	const result = new WireEventWriter({
		runId: wireRunId("run:outer-activity"),
		eventId: (sequence) => wireEventId(`event:outer-activity-${sequence}`),
		now: () => "2026-08-10T00:00:00.000Z",
	});
	result.append({ kind: "run.started", lifecycle: "running" });
	return result;
}

test("canonical assistant, tool, model, artifact, and compaction events are execution activity", () => {
	const source = writer();
	const messageId = wireMessageId("message:outer-activity");
	const toolCallId = wireToolCallId("tool:outer-activity");
	const modelCallId = wireModelCallId("model-call:outer-activity");
	const events = [
		source.append({
			kind: "message.recorded",
			messageId,
			stage: "started",
			role: "assistant",
		}),
		source.append({
			kind: "message.recorded",
			messageId,
			stage: "delta",
			role: "assistant",
			content: "working",
		}),
		source.append({
			kind: "message.recorded",
			messageId,
			stage: "completed",
			role: "assistant",
		}),
		source.append({
			kind: "tool.admitted",
			toolCallId,
			name: "Read",
			schema: { status: "unavailable", reason: "test" },
		}),
		source.append({
			kind: "tool.progress",
			toolCallId,
			progress: "reading",
		}),
		source.append({
			kind: "tool.terminal",
			toolCallId,
			status: "succeeded",
			origin: "provider",
		}),
		source.append({
			kind: "model-call.started",
			modelCallId,
			model: { provider: "openai", tier: "standard" },
			attempt: 1,
		}),
		source.append({
			kind: "model-call.completed",
			modelCallId,
			status: "succeeded",
			origin: "provider",
			usage: source.snapshot()!.usage,
			usageCoverage: "exact",
		}),
		source.append({
			kind: "artifact.published",
			artifactId: wireArtifactId("artifact:outer-activity"),
			mediaType: "text/plain",
			bytes: 3,
		}),
		source.append({
			kind: "run.progress",
			lifecycle: "running",
			progress: { compactions: 1 },
		}),
	];
	expect(events.map(outerExecutionActivityKind)).toEqual([
		undefined,
		"wire.message.assistant.delta",
		"wire.message.assistant.completed",
		"wire.tool.admitted",
		"wire.tool.progress",
		"wire.tool.succeeded",
		undefined,
		"wire.model-call.succeeded",
		"wire.artifact.published",
		"wire.run.compacted",
	]);
});

test("admission/status bookkeeping cannot manufacture execution activity", () => {
	const source = writer();
	const userMessageId = wireMessageId("message:outer-user");
	const events = [
		source.events()[0],
		source.append({
			kind: "run.progress",
			lifecycle: "waiting",
			progress: { currentAction: "provider status" },
		}),
		source.append({
			kind: "message.recorded",
			messageId: userMessageId,
			stage: "started",
			role: "user",
			content: "continue",
		}),
		source.append({
			kind: "message.recorded",
			messageId: userMessageId,
			stage: "completed",
			role: "user",
		}),
		source.append({
			kind: "resource.pressure",
			scope: "run",
			resource: "memory",
			used: 1,
			reserved: 0,
			limit: 10,
			advisory: true,
		}),
	];
	for (const event of events) {
		expect(outerExecutionActivityKind(event)).toBeUndefined();
		expect(isOuterExecutionActivity(event)).toBe(false);
	}
});
