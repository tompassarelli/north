import type { WireRunSnapshot, WireRunUsageCoverage } from "./wire/reducer";

export type TokenTotalStatus = WireRunUsageCoverage["totalStatus"]
	| "unknown_provider"
	| "unknown_overflow";

export interface NormalizedTokenUsage {
	inputTokens: number;
	outputTokens: number;
	cacheReadTokens: number;
	cacheWriteTokens: number;
	reasoningTokens: number;
	modelCalls: number;
	completedModelCalls: number;
	contextTokens: number;
	contextWindow?: number;
	total?: number;
	totalStatus: TokenTotalStatus;
}

function safeSum(values: readonly number[]): number | undefined {
	const total = values.reduce((sum, value) => sum + value, 0);
	return Number.isSafeInteger(total) ? total : undefined;
}

/**
 * Project canonical lifetime/context usage into the telemetry shape. Provider
 * selection is semantic wire metadata; raw provider terminal formulas never
 * reach this boundary.
 */
export function normalizeUsage(snapshot: WireRunSnapshot): NormalizedTokenUsage {
	const lifetime = snapshot.usage.lifetime;
	const provider = snapshot.model?.provider;
	const formulaTotal = provider === "anthropic"
		? safeSum([
			lifetime.inputTokens,
			lifetime.outputTokens,
			lifetime.cacheReadTokens,
			lifetime.cacheWriteTokens,
		])
		: provider === "openai"
			? safeSum([lifetime.inputTokens, lifetime.outputTokens])
			: undefined;
	const coverageStatus = snapshot.usageCoverage.totalStatus;
	const totalStatus: TokenTotalStatus = coverageStatus !== "exact"
		? coverageStatus
		: provider === undefined
			? "unknown_provider"
			: formulaTotal === undefined ? "unknown_overflow" : "exact";
	const total = totalStatus === "exact" ? formulaTotal : undefined;
	const completedModelCalls = Object.values(snapshot.modelCalls)
		.filter((modelCall) => modelCall.status !== "running").length;
	return Object.freeze({
		inputTokens: lifetime.inputTokens,
		outputTokens: lifetime.outputTokens,
		cacheReadTokens: lifetime.cacheReadTokens,
		cacheWriteTokens: lifetime.cacheWriteTokens,
		reasoningTokens: lifetime.reasoningTokens,
		modelCalls: lifetime.modelCalls,
		completedModelCalls,
		contextTokens: snapshot.usage.context.tokens,
		...(snapshot.usage.context.window === undefined
			? {} : { contextWindow: snapshot.usage.context.window }),
		...(total === undefined ? {} : { total }),
		totalStatus,
	});
}

/** Exact graph literal for the provider's disjoint cumulative token categories. */
export function tokenTotalLiteral(snapshot: WireRunSnapshot): string | undefined {
	if (snapshot.usageCoverage.totalStatus !== "exact") return undefined;
	const lifetime = snapshot.usage.lifetime;
	const values = snapshot.model?.provider === "anthropic"
		? [
			lifetime.inputTokens,
			lifetime.outputTokens,
			lifetime.cacheReadTokens,
			lifetime.cacheWriteTokens,
		]
		: snapshot.model?.provider === "openai"
			? [lifetime.inputTokens, lifetime.outputTokens]
			: undefined;
	if (values === undefined) return undefined;
	return values.reduce((total, value) => total + BigInt(value), 0n).toString();
}

export function tokensOf(snapshot: WireRunSnapshot): number | undefined {
	return normalizeUsage(snapshot).total;
}
