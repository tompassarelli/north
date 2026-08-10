import { AGENT_RUN_LEDGER_CONTRACT } from "./run-ledger";

export type RunEstimateClassification = "under" | "on" | "over";

export interface RunEstimateSnapshot {
	readonly hours: string;
	readonly durationMs: number;
}

export interface RunEstimateComparison {
	readonly deltaMs: number;
	readonly ratio: string;
	readonly classification: RunEstimateClassification;
}

export class InvalidRunEstimateError extends Error {
	readonly code = "NORTH_INVALID_RUN_ESTIMATE";
	readonly preSideEffect = true;

	constructor(readonly reason: "duplicate" | "not-positive-finite-hours") {
		super(`invalid thread estimate_hours: ${reason}`);
		this.name = "InvalidRunEstimateError";
	}
}

const ESTIMATE_HOURS_NUMBER = /^(?:0|[1-9][0-9]*)(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?$/;
const MS_PER_HOUR = 60 * 60 * 1_000;
const ESTIMATE_RATIO_SPEC = AGENT_RUN_LEDGER_CONTRACT.telemetry.estimateRatio;
const ESTIMATE_RATIO_SCALE = ESTIMATE_RATIO_SPEC.scale;
const ESTIMATE_RATIO_DIGITS = String(ESTIMATE_RATIO_SCALE).length - 1;

if (ESTIMATE_RATIO_SPEC.rounding !== "nearest-half-up"
	|| ESTIMATE_RATIO_SPEC.trailingFractionZeros !== "omit"
	|| !/^10*$/u.test(String(ESTIMATE_RATIO_SCALE))) {
	throw new TypeError("invalid wire run estimate ratio contract");
}

function estimateDurationMs(hours: string): number {
	if (!ESTIMATE_HOURS_NUMBER.test(hours)) {
		throw new InvalidRunEstimateError("not-positive-finite-hours");
	}
	const parsed = Number(hours);
	const durationMs = Math.round(parsed * MS_PER_HOUR);
	if (!Number.isFinite(parsed) || parsed <= 0
		|| !Number.isSafeInteger(durationMs) || durationMs < 1) {
		throw new InvalidRunEstimateError("not-positive-finite-hours");
	}
	return durationMs;
}

/** Capture the sole thread estimate before dispatch; absence is valid input. */
export function runEstimateFromThreadFacts(
	facts: readonly { readonly predicate: string; readonly value: string }[],
): RunEstimateSnapshot | undefined {
	const estimates = facts.filter(({ predicate }) => predicate === "estimate_hours");
	if (estimates.length === 0) return undefined;
	if (estimates.length !== 1) throw new InvalidRunEstimateError("duplicate");
	const hours = estimates[0]!.value;
	return Object.freeze({ hours, durationMs: estimateDurationMs(hours) });
}

export function compareRunEstimate(
	estimate: RunEstimateSnapshot,
	actualDurationMs: number,
): RunEstimateComparison {
	if (estimateDurationMs(estimate.hours) !== estimate.durationMs) {
		throw new InvalidRunEstimateError("not-positive-finite-hours");
	}
	if (!Number.isSafeInteger(actualDurationMs) || actualDurationMs < 0) {
		throw new TypeError("invalid actual duration for run estimate comparison");
	}
	const deltaMs = actualDurationMs - estimate.durationMs;
	const classification: RunEstimateClassification = deltaMs < 0 ? "under" : deltaMs > 0 ? "over" : "on";
	const denominator = BigInt(estimate.durationMs);
	const scaledNumerator = BigInt(actualDurationMs) * BigInt(ESTIMATE_RATIO_SCALE);
	const quotient = scaledNumerator / denominator;
	const remainder = scaledNumerator % denominator;
	const rounded = quotient + (remainder * 2n >= denominator ? 1n : 0n);
	const whole = rounded / BigInt(ESTIMATE_RATIO_SCALE);
	const fraction = (rounded % BigInt(ESTIMATE_RATIO_SCALE))
		.toString()
		.padStart(ESTIMATE_RATIO_DIGITS, "0")
		.replace(/0+$/u, "");
	const ratio = fraction ? `${whole}.${fraction}` : String(whole);
	return Object.freeze({ deltaMs, ratio, classification });
}
