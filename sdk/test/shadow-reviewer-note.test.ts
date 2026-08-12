import { expect, test } from "bun:test";
import {
	SHADOW_REVIEWER_NOTE_SUBJECT,
	mintShadowReviewerNoteCapability,
	publishShadowReviewerNote,
	type ShadowReviewerNoteCommand,
} from "../src/shadow-reviewer-note";
import {
	SHADOW_REVIEWER_VERSION,
	shadowReviewerAgentId,
	type ShadowReviewerNote,
} from "../src/shadow-reviewer";
import { wireRunId } from "../src/wire";

const note: ShadowReviewerNote = {
	version: SHADOW_REVIEWER_VERSION,
	reviewerRunId: wireRunId("run:reviewer-child"),
	sourceRunId: wireRunId("run:reviewer-source"),
	sourceThroughSequence: 4,
	severity: "nit",
	issueCode: "unresolved_failure",
	sourceSequence: 4,
	note: "The latest update contains an unresolved failure (source event 4).",
	noteSha256: "a".repeat(64),
};

test("validated notes enter the self-only durable managed follow-up producer", async () => {
	let observed: ShadowReviewerNoteCommand | undefined;
	const capability = mintShadowReviewerNoteCapability();
	await publishShadowReviewerNote(
		"lane-review-source",
		note,
		capability,
		new AbortController().signal,
		{
			env: {
				NORTH_PORT: "9000",
				FRAM_HOME: "/fixture/fram",
				FRAM_BIN: "/fixture/fram/bin",
				FRAM_OUT: "/fixture/fram/out",
			},
			publish: async (command) => {
				observed = command;
				return "recorded";
			},
		},
	);
	const args = observed?.args ?? [];
	expect(args).toContain("review");
	expect(args).not.toContain("send");
	expect(args).not.toContain("mention");
	expect(args).not.toContain("interrupt");
	expect(args).toContain(shadowReviewerAgentId("lane-review-source"));
	expect(args).toContain("lane-review-source");
	expect(SHADOW_REVIEWER_NOTE_SUBJECT).toBe("msg");
	expect(args).toContain("[nit] The latest update contains an unresolved failure (source event 4).");
	expect(observed?.stdin).toBe(capability.preimage);
	expect(Object.values(observed?.env ?? {})).not.toContain(capability.preimage);
});

test("an already-aborted source cannot publish or wake a lane", async () => {
	const abort = new AbortController();
	abort.abort(new Error("operator abort"));
	let calls = 0;
	await expect(publishShadowReviewerNote(
		"lane-review-source", note, mintShadowReviewerNoteCapability(), abort.signal, {
		publish: async () => {
			calls += 1;
			return "recorded";
		},
	})).rejects.toThrow("operator abort");
	expect(calls).toBe(0);
});
