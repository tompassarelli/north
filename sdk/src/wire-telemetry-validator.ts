import {
	wireLedgerSummary,
	type WireRunLedgerIdentity,
} from "./run-ledger";
import { wireRunTelemetryFacts } from "./telemetry";
import { decodeWireJsonl } from "./wire/jsonl";

interface ValidationInput {
	readonly subject: string;
	readonly facts: readonly (readonly [string, string])[];
	readonly wireJsonl: string;
}

const OPTIONAL_EVENT_CORE_PREDICATES = Object.freeze([
	"tokens",
	"context_window_tokens",
	"parent_run",
	"run_owner",
	"model_tier",
	"capability_class",
	"effort",
	"watchdog_reason",
	"watchdog_silence_ms",
	"watchdog_last_outer_activity",
	"watchdog_last_provider_activity",
	"provider_join_key_version",
	"provider_join_coverage",
	"provider_session_key",
	"provider_turn_key",
	"provider_duration_ms",
	"num_turns",
	"provider_turn_units",
	"provider_tool_items",
	"provider_turn_metric_comparable",
]);

function values(
	facts: readonly (readonly [string, string])[],
	predicate: string,
): readonly string[] {
	return facts.filter(([candidate]) => candidate === predicate)
		.map(([, value]) => value)
		.sort();
}

function input(value: unknown): ValidationInput {
	if (value === null || typeof value !== "object" || Array.isArray(value)) {
		throw new TypeError("wire telemetry validator input must be an object");
	}
	const source = value as Record<string, unknown>;
	if (typeof source.subject !== "string" || typeof source.wireJsonl !== "string"
		|| !Array.isArray(source.facts)
		|| source.facts.some((fact) => !Array.isArray(fact) || fact.length !== 2
			|| fact.some((item) => typeof item !== "string"))) {
		throw new TypeError("wire telemetry validator input is malformed");
	}
	return {
		subject: source.subject,
		wireJsonl: source.wireJsonl,
		facts: source.facts as readonly (readonly [string, string])[],
	};
}

try {
	const request = input(JSON.parse(await Bun.stdin.text()));
	const replay = decodeWireJsonl(request.wireJsonl);
	if (replay.snapshot === undefined) throw new TypeError("wire telemetry has no reduced snapshot");
	const submitted = new Map(request.facts);
	const identity: WireRunLedgerIdentity = {
		thread: submitted.get("thread") ?? "",
		agent: submitted.get("agent") ?? "",
		...(submitted.get("parent_thread") === undefined
			? {} : { parentThread: submitted.get("parent_thread")! }),
		...(submitted.get("run_coordinator") === undefined
			? {} : { coordinator: submitted.get("run_coordinator")! }),
	};
	const summary = wireLedgerSummary(replay.events);
	const expected = wireRunTelemetryFacts(
		identity,
		replay.snapshot,
		{ status: "recorded", summary },
		{},
	);
	if (expected.subject !== request.subject) {
		throw new TypeError("wire telemetry subject differs from the reduced run");
	}
	const expectedFacts = new Map(expected.facts);
	for (const predicate of OPTIONAL_EVENT_CORE_PREDICATES) {
		if (submitted.has(predicate) !== expectedFacts.has(predicate)) {
			throw new TypeError("wire telemetry optional core fact " + predicate
				+ " differs from the reducer");
		}
	}
	for (const [predicate, value] of expected.facts) {
		if (predicate === "provider_turn_key") continue;
		if (submitted.get(predicate) !== value) {
			throw new TypeError("wire telemetry core fact " + predicate + " differs from the reducer");
		}
	}
	if (JSON.stringify(values(request.facts, "provider_turn_key"))
		!== JSON.stringify(values(expected.facts, "provider_turn_key"))) {
		throw new TypeError("wire telemetry provider turn keys differ from the reducer");
	}
} catch {
	process.exitCode = 1;
}
