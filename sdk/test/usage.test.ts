import { expect, test } from "bun:test";
import { normalizeUsage, tokenTotalLiteral, tokensOf } from "../src/usage";
import {
	WireEventWriter,
	wireEventId,
	wireModelCallId,
	wireRunId,
	type WireRunSnapshot,
	type WireUsageSnapshot,
} from "../src/wire";

function completedRun(
	label: string,
	provider: "anthropic" | "openai",
	usage: WireUsageSnapshot,
): WireRunSnapshot {
	const writer = new WireEventWriter({
		runId: wireRunId(`run:usage-${label}`),
		eventId: (sequence) => wireEventId(`event:usage-${label}-${sequence}`),
		now: () => "2026-08-10T00:00:00.000Z",
	});
	const modelCallId = wireModelCallId(`model-call:usage-${label}`);
	writer.append({ kind: "run.started", lifecycle: "running" });
	writer.append({
		kind: "model-call.started",
		modelCallId,
		model: { provider, tier: "standard" },
		attempt: 1,
	});
	writer.append({
		kind: "model-call.completed",
		modelCallId,
		status: "succeeded",
		origin: "provider",
		usage,
		usageCoverage: "exact",
	});
	return writer.snapshot()!;
}

test("Anthropic wire usage sums disjoint lifetime categories", () => {
	const snapshot = completedRun("anthropic", "anthropic", {
		lifetime: {
			inputTokens: 101,
			outputTokens: 23,
			cacheReadTokens: 59,
			cacheWriteTokens: 0,
			reasoningTokens: 0,
			modelCalls: 1,
		},
		context: { tokens: 160, window: 200_000 },
	});
	expect(normalizeUsage(snapshot)).toEqual({
		inputTokens: 101,
		outputTokens: 23,
		cacheReadTokens: 59,
		cacheWriteTokens: 0,
		reasoningTokens: 0,
		modelCalls: 1,
		completedModelCalls: 1,
		contextTokens: 160,
		contextWindow: 200_000,
		total: 183,
		totalStatus: "exact",
	});
	expect(tokensOf(snapshot)).toBe(183);
});

test("OpenAI cache and reasoning counters remain subsets of input and output totals", () => {
	const snapshot = completedRun("openai", "openai", {
		lifetime: {
			inputTokens: 100,
			outputTokens: 20,
			cacheReadTokens: 60,
			cacheWriteTokens: 0,
			reasoningTokens: 7,
			modelCalls: 1,
		},
		context: { tokens: 90 },
	});
	expect(normalizeUsage(snapshot)).toMatchObject({
		inputTokens: 100,
		outputTokens: 20,
		cacheReadTokens: 60,
		reasoningTokens: 7,
		total: 120,
		totalStatus: "exact",
	});
	expect(tokensOf(snapshot)).toBe(120);
});

test("a started run without semantic model authority keeps aggregate usage unknown", () => {
	const writer = new WireEventWriter({ runId: wireRunId("run:usage-unrouted") });
	writer.append({ kind: "run.started", lifecycle: "running" });
	expect(normalizeUsage(writer.snapshot()!)).toMatchObject({
		modelCalls: 0,
		completedModelCalls: 0,
		contextTokens: 0,
		totalStatus: "unknown_no_terminal",
	});
	expect(tokensOf(writer.snapshot()!)).toBeUndefined();
});

test("authoritative exact zero stays distinct from a North-synthesized abrupt zero", () => {
	const exactZero = completedRun("exact-zero", "openai", {
		lifetime: {
			inputTokens: 0,
			outputTokens: 0,
			cacheReadTokens: 0,
			cacheWriteTokens: 0,
			reasoningTokens: 0,
			modelCalls: 1,
		},
		context: { tokens: 0 },
	});
	expect(normalizeUsage(exactZero)).toMatchObject({ total: 0, totalStatus: "exact" });
	expect(tokensOf(exactZero)).toBe(0);

	const writer = new WireEventWriter({ runId: wireRunId("run:usage-abrupt-zero") });
	const modelCallId = wireModelCallId("model-call:usage-abrupt-zero");
	writer.append({ kind: "run.started", lifecycle: "running" });
	writer.append({
		kind: "model-call.started",
		modelCallId,
		model: { provider: "openai", tier: "standard" },
		attempt: 1,
	});
	writer.terminate({ lifecycle: "failed", reason: { code: "provider_process_died" } });
	const abrupt = normalizeUsage(writer.snapshot()!);
	expect(abrupt.totalStatus).toBe("unknown_no_terminal");
	expect(abrupt).not.toHaveProperty("total");
	expect(tokensOf(writer.snapshot()!)).toBeUndefined();
});

test("number projection reports exact cumulative totals that overflow as unknown_overflow", () => {
	const snapshot = completedRun("overflow", "openai", {
		lifetime: {
			inputTokens: Number.MAX_SAFE_INTEGER,
			outputTokens: 1,
			cacheReadTokens: 0,
			cacheWriteTokens: 0,
			reasoningTokens: 0,
			modelCalls: 1,
		},
		context: { tokens: 1 },
	});
	const normalized = normalizeUsage(snapshot);
	expect(normalized.totalStatus).toBe("unknown_overflow");
	expect(normalized).not.toHaveProperty("total");
	expect(tokensOf(snapshot)).toBeUndefined();
	expect(tokenTotalLiteral(snapshot)).toBe("9007199254740992");
});

test("a later abrupt call downgrades an earlier exact cumulative total to partial", () => {
	const writer = new WireEventWriter({ runId: wireRunId("run:usage-exact-then-abrupt") });
	const first = wireModelCallId("model-call:usage-exact-first");
	const second = wireModelCallId("model-call:usage-abrupt-second");
	writer.append({ kind: "run.started", lifecycle: "running" });
	writer.append({
		kind: "model-call.started",
		modelCallId: first,
		model: { provider: "openai", tier: "standard" },
		attempt: 1,
	});
	writer.append({
		kind: "model-call.completed",
		modelCallId: first,
		status: "succeeded",
		origin: "provider",
		usage: {
			lifetime: {
				inputTokens: 12,
				outputTokens: 3,
				cacheReadTokens: 0,
				cacheWriteTokens: 0,
				reasoningTokens: 0,
				modelCalls: 1,
			},
			context: { tokens: 15 },
		},
		usageCoverage: "exact",
	});
	writer.append({
		kind: "model-call.started",
		modelCallId: second,
		model: { provider: "openai", tier: "standard" },
		attempt: 1,
	});
	writer.terminate({ lifecycle: "failed", reason: { code: "provider_process_died" } });
	expect(writer.snapshot()?.modelCalls[second]?.usageCoverage).toBe("unavailable");
	const normalized = normalizeUsage(writer.snapshot()!);
	expect(normalized.totalStatus).toBe("partial");
	expect(normalized).not.toHaveProperty("total");
	expect(tokensOf(writer.snapshot()!)).toBeUndefined();
});
