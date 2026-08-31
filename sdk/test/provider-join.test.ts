import { expect, test } from "bun:test";
import {
	foldProviderJoinEvidence,
	providerJoinEvidence,
	providerJoinEvidenceEqual,
	providerSessionKey,
	providerTurnKey,
} from "../src/providers/provider-join";

test("adapter-private raw IDs become stable domain-separated wire keys", () => {
	const session = providerSessionKey("session-1");
	const anthropicTurn = providerTurnKey("anthropic", "turn-1");
	const openaiTurn = providerTurnKey("openai", "turn-1");
	expect(session).toMatch(/^[a-f0-9]{64}$/);
	expect(providerSessionKey("session-1")).toBe(session);
	expect(anthropicTurn).toMatch(/^[a-f0-9]{64}$/);
	expect(openaiTurn).not.toBe(anthropicTurn);
	expect(session).not.toContain("session-1");
});

test("typed provider join evidence merges turns for one private session", () => {
	const first = providerJoinEvidence("openai", {
		sessionId: "session-1",
		turnIds: ["turn-2", "turn-1"],
		sessionPersistence: "ephemeral",
	});
	const second = providerJoinEvidence("openai", {
		sessionId: "session-1",
		turnIds: ["turn-2", "turn-3"],
		sessionPersistence: "ephemeral",
	});
	const folded = foldProviderJoinEvidence([first, second]);
	expect(folded).toEqual({
		version: "north-provider-join:v1",
		sessionKey: first.sessionKey,
		turnKeys: [...new Set([...first.turnKeys, ...second.turnKeys])].sort(),
		sessionPersistence: "ephemeral",
		coverage: "exact",
	});
	expect(Object.isFrozen(folded)).toBe(true);
	expect(Object.isFrozen(folded?.turnKeys)).toBe(true);
});

test("managed session replacement preserves turn joins without claiming one exact session", () => {
	const first = providerJoinEvidence("anthropic", {
		sessionId: "session-1",
		turnIds: ["turn-1"],
		sessionPersistence: "persisted",
	});
	const second = providerJoinEvidence("anthropic", {
		sessionId: "session-2",
		turnIds: ["turn-2"],
		sessionPersistence: "persisted",
	});
	expect(foldProviderJoinEvidence([first, second])).toEqual({
		version: "north-provider-join:v1",
		turnKeys: [...first.turnKeys, ...second.turnKeys].sort(),
		sessionPersistence: "persisted",
		coverage: "partial",
	});
});

test("folding never upgrades an adapter's partial coverage claim", () => {
	const exact = providerJoinEvidence("openai", {
		sessionId: "session-1",
		turnIds: ["turn-1"],
		sessionPersistence: "ephemeral",
	});
	expect(foldProviderJoinEvidence([{ ...exact, coverage: "partial" }])?.coverage)
		.toBe("partial");
});

test("provider join equality ignores object field insertion order", () => {
	const expected = providerJoinEvidence("openai", {
		sessionId: "session-1",
		turnIds: ["turn-1"],
		sessionPersistence: "ephemeral",
	});
	const decoded = {
		coverage: expected.coverage,
		sessionKey: expected.sessionKey,
		sessionPersistence: expected.sessionPersistence,
		turnKeys: expected.turnKeys,
		version: expected.version,
	};
	expect(JSON.stringify(decoded)).not.toBe(JSON.stringify(expected));
	expect(providerJoinEvidenceEqual(decoded, expected)).toBe(true);
	expect(providerJoinEvidenceEqual({
		...decoded,
		turnKeys: [providerTurnKey("openai", "turn-2")],
	}, expected)).toBe(false);
});
