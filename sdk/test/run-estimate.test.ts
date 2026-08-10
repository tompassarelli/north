import { expect, test } from "bun:test";

import {
	compareRunEstimate,
	InvalidRunEstimateError,
	runEstimateFromThreadFacts,
} from "../src/run-estimate";

test("captures one positive finite estimate before execution", () => {
	expect(runEstimateFromThreadFacts([
		{ predicate: "title", value: "bounded lane" },
		{ predicate: "estimate_hours", value: "0.25" },
	])).toEqual({ hours: "0.25", durationMs: 900_000 });
	expect(runEstimateFromThreadFacts([])).toBeUndefined();
});

test("rejects ambiguous or non-positive estimates with a pre-side-effect typed error", () => {
	for (const facts of [
		[
			{ predicate: "estimate_hours", value: "1" },
			{ predicate: "estimate_hours", value: "2" },
		],
		[{ predicate: "estimate_hours", value: "0" }],
		[{ predicate: "estimate_hours", value: "not-a-number" }],
	]) {
		try {
			runEstimateFromThreadFacts(facts);
		} catch (error) {
			expect(error).toBeInstanceOf(InvalidRunEstimateError);
			if (!(error instanceof InvalidRunEstimateError)) throw error;
			expect(error.preSideEffect).toBe(true);
			continue;
		}
		throw new Error("expected an invalid estimate");
	}
});

test("compares terminal duration against the immutable dispatch estimate", () => {
	const estimate = runEstimateFromThreadFacts([
		{ predicate: "estimate_hours", value: "0.001" },
	]);
	if (!estimate) throw new Error("expected a captured estimate");
	expect(compareRunEstimate(estimate, 2_000)).toEqual({
		deltaMs: -1_600,
		ratio: "0.555556",
		classification: "under",
	});
	expect(compareRunEstimate(estimate, 3_600)).toEqual({
		deltaMs: 0,
		ratio: "1",
		classification: "on",
	});
	const halfBoundary = runEstimateFromThreadFacts([
		{ predicate: "estimate_hours", value: "2.7777777777777777" },
	]);
	if (!halfBoundary) throw new Error("expected a half-boundary estimate");
	expect(halfBoundary.durationMs).toBe(10_000_000);
	expect(compareRunEstimate(halfBoundary, 10_000_015)).toEqual({
		deltaMs: 15,
		ratio: "1.000002",
		classification: "over",
	});
});
