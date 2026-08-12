import { expect, test } from "bun:test";

import {
	type ShadowReviewerExecutionProvenance,
	wireRunProvenanceFacts,
} from "../src/run-provenance";
import {
	SHADOW_REVIEWER_VERSION,
	type ShadowReviewerSummary,
} from "../src/shadow-reviewer";
import { wireRunId } from "../src/wire";

const summary = Object.freeze({
	version: SHADOW_REVIEWER_VERSION,
	targetId: "reviewer-standard",
	status: "partial",
	eligibleUpdates: 3,
	reviewedUpdates: 2,
	droppedUpdates: 1,
	emittedNotes: 1,
	quarantinedOutputs: 0,
	failedReviews: 0,
	usageStatus: "exact",
	tokens: 77,
	durationMs: 19,
} satisfies ShadowReviewerSummary);

const execution = Object.freeze({
	version: SHADOW_REVIEWER_VERSION,
	targetId: "reviewer-standard",
	sourceRunId: wireRunId("run:source-primary"),
	sourceFromSequence: 2,
	sourceThroughSequence: 8,
	privacyOmittedEvents: 1,
	capacityOmittedEvents: 4,
	inputSha256: "a".repeat(64),
} satisfies ShadowReviewerExecutionProvenance);

test("primary shadow reviewer summaries publish separate exact usage facts", () => {
	expect(wireRunProvenanceFacts({ shadowReviewerSummary: summary }, 100)).toEqual([
		["shadow_reviewer_version", SHADOW_REVIEWER_VERSION],
		["shadow_reviewer_target", "reviewer-standard"],
		["shadow_reviewer_status", "partial"],
		["shadow_reviewer_eligible_updates", "3"],
		["shadow_reviewer_reviewed_updates", "2"],
		["shadow_reviewer_dropped_updates", "1"],
		["shadow_reviewer_emitted_notes", "1"],
		["shadow_reviewer_quarantined_outputs", "0"],
		["shadow_reviewer_failed_reviews", "0"],
		["shadow_reviewer_usage_status", "exact"],
		["shadow_reviewer_tokens", "77"],
		["shadow_reviewer_duration_ms", "19"],
	]);

	const inexact = { ...summary, usageStatus: "partial" as const, tokens: undefined };
	const facts = wireRunProvenanceFacts({ shadowReviewerSummary: inexact }, 100);
	expect(facts).toContainEqual(["shadow_reviewer_usage_status", "partial"]);
	expect(facts.some(([predicate]) => predicate === "shadow_reviewer_tokens")).toBe(false);
});

test("child shadow reviewer execution facts retain only bounded source lineage", () => {
	expect(wireRunProvenanceFacts({ shadowReviewerExecution: execution }, 100)).toEqual([
		["shadow_reviewer_version", SHADOW_REVIEWER_VERSION],
		["shadow_reviewer_target", "reviewer-standard"],
		["shadow_reviewer_source_run", "run:source-primary"],
		["shadow_reviewer_source_from_sequence", "2"],
		["shadow_reviewer_source_through_sequence", "8"],
		["shadow_reviewer_privacy_omitted_events", "1"],
		["shadow_reviewer_capacity_omitted_events", "4"],
		["shadow_reviewer_input_sha256", "a".repeat(64)],
	]);
});

test("shadow reviewer provenance rejects unreconciled or mixed claims", () => {
	const { tokens: _tokens, ...withoutTokens } = summary;
	expect(() => wireRunProvenanceFacts({
		shadowReviewerSummary: withoutTokens,
	}, 100)).toThrow("exact usage and token total");
	expect(() => wireRunProvenanceFacts({
		shadowReviewerSummary: { ...summary, reviewedUpdates: 4 },
	}, 100)).toThrow("counts do not reconcile");
	expect(() => wireRunProvenanceFacts({
		shadowReviewerExecution: { ...execution, sourceFromSequence: 9 },
	}, 100)).toThrow("sequence interval is inverted");
	expect(() => wireRunProvenanceFacts({
		shadowReviewerExecution: { ...execution, inputSha256: "not-a-digest" },
	}, 100)).toThrow("input digest");
	expect(() => wireRunProvenanceFacts({
		shadowReviewerSummary: summary,
		shadowReviewerExecution: execution,
	}, 100)).toThrow("both shadow reviewer summary and execution");
});
