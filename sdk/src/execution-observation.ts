export const EXECUTION_OBSERVATION_VERSION = "agent-execution-observation/v1" as const;

export type ExecutionMode = "standard" | "fast";

export interface ExecutionObservationEvidence {
	readonly provider: string;
	readonly attempt_sha256: string;
	readonly session_sha256: string;
}

export interface ExecutionModeSegment {
	readonly mode: ExecutionMode;
	readonly turn_count: number;
	readonly tool_call_count: number;
	readonly turn_sha256: readonly string[];
}

export type ExecutionObservation =
	| Readonly<{
		version: typeof EXECUTION_OBSERVATION_VERSION;
		coverage: "exact";
		source: string;
		turn_unit: "assistant-turn";
		tool_call_unit: "admitted-tool-call";
		evidence: ExecutionObservationEvidence;
		segments: readonly ExecutionModeSegment[];
	}>
	| Readonly<{
		version: typeof EXECUTION_OBSERVATION_VERSION;
		coverage: "unknown";
		source: string;
		turn_unit: "unknown";
		tool_call_unit: "unknown";
		evidence: Readonly<Record<string, never>>;
		segments: readonly [];
	}>;

export type CodexExecutionObservationEvent =
	| Readonly<{
		kind: "thread_settings_applied";
		attempt_sha256: string;
		session_sha256: string;
		service_tier: "default" | "priority";
	}>
	| Readonly<{
		kind: "task_started";
		attempt_sha256: string;
		session_sha256: string;
		turn_sha256: string;
	}>
	| Readonly<{
		kind: "tool_call_admitted";
		attempt_sha256: string;
		session_sha256: string;
		turn_sha256: string;
		tool_call_sha256: string;
	}>;

const SOURCE = /^[a-z0-9][a-z0-9._:/-]*$/u;
const SHA256 = /^[a-f0-9]{64}$/u;

function count(value: unknown, label: string, positive = false): number {
	if (!Number.isSafeInteger(value) || (positive ? (value as number) <= 0 : (value as number) < 0)) {
		throw new TypeError(`${label} must be a safe ${positive ? "positive" : "nonnegative"} integer`);
	}
	return value as number;
}

function source(value: unknown): string {
	if (typeof value !== "string" || !SOURCE.test(value)) {
		throw new TypeError("execution observation source is invalid");
	}
	return value;
}

function digest(value: unknown, label: string): string {
	if (typeof value !== "string" || !SHA256.test(value)) {
		throw new TypeError(`${label} must be a SHA-256 digest`);
	}
	return value;
}

function record(value: unknown, label: string): Readonly<Record<string, unknown>> {
	if (value === null || typeof value !== "object" || Array.isArray(value)) {
		throw new TypeError(`${label} must be an object`);
	}
	return value as Readonly<Record<string, unknown>>;
}

function exactKeys(
	value: Readonly<Record<string, unknown>>,
	expected: readonly string[],
	label: string,
): void {
	const actual = Object.keys(value).sort();
	if (JSON.stringify(actual) !== JSON.stringify([...expected].sort())) {
		throw new TypeError(`${label} has an unsupported field set`);
	}
}

export function normalizeExecutionObservation(value: unknown): ExecutionObservation {
	const input = record(value, "execution observation");
	if (input.version !== EXECUTION_OBSERVATION_VERSION) {
		throw new TypeError("execution observation version is unsupported");
	}
	const observationSource = source(input.source);
	if (input.coverage === "unknown") {
		exactKeys(
			input,
			["version", "coverage", "source", "turn_unit", "tool_call_unit", "evidence", "segments"],
			"unknown execution observation",
		);
		const evidence = record(input.evidence, "unknown execution observation evidence");
		if (input.turn_unit !== "unknown" || input.tool_call_unit !== "unknown"
			|| Object.keys(evidence).length !== 0
			|| !Array.isArray(input.segments) || input.segments.length !== 0) {
			throw new TypeError("unknown execution observation must not contain evidence or segments");
		}
		return Object.freeze({
			version: EXECUTION_OBSERVATION_VERSION,
			coverage: "unknown",
			source: observationSource,
			turn_unit: "unknown",
			tool_call_unit: "unknown",
			evidence: Object.freeze({}) as Readonly<Record<string, never>>,
			segments: Object.freeze([] as []),
		});
	}
	if (input.coverage !== "exact") {
		throw new TypeError("execution observation coverage is invalid");
	}
	exactKeys(
		input,
		["version", "coverage", "source", "turn_unit", "tool_call_unit", "evidence", "segments"],
		"exact execution observation",
	);
	if (input.turn_unit !== "assistant-turn" || input.tool_call_unit !== "admitted-tool-call") {
		throw new TypeError("exact execution observation units are not comparable");
	}
	const rawEvidence = record(input.evidence, "execution observation evidence");
	exactKeys(
		rawEvidence,
		["provider", "attempt_sha256", "session_sha256"],
		"execution observation evidence",
	);
	if (typeof rawEvidence.provider !== "string" || !SOURCE.test(rawEvidence.provider)) {
		throw new TypeError("execution observation provider is invalid");
	}
	const evidence = Object.freeze({
		provider: rawEvidence.provider,
		attempt_sha256: digest(rawEvidence.attempt_sha256, "execution attempt key"),
		session_sha256: digest(rawEvidence.session_sha256, "execution session key"),
	});
	if (!Array.isArray(input.segments) || input.segments.length === 0) {
		throw new TypeError("exact execution observation requires a segment");
	}
	let precedingMode: ExecutionMode | undefined;
	const seenTurns = new Set<string>();
	let totalTurns = 0;
	let totalTools = 0;
	const segments = input.segments.map((candidate, index): ExecutionModeSegment => {
		const segment = record(candidate, `execution observation segment ${index}`);
		exactKeys(
			segment,
			["mode", "turn_count", "tool_call_count", "turn_sha256"],
			"execution observation segment",
		);
		if (segment.mode !== "standard" && segment.mode !== "fast") {
			throw new TypeError("execution observation segment mode is invalid");
		}
		if (segment.mode === precedingMode) {
			throw new TypeError("execution observation contains adjacent equal modes");
		}
		precedingMode = segment.mode;
		const turns = count(segment.turn_count, "execution observation turn count", true);
		const tools = count(segment.tool_call_count, "execution observation tool-call count");
		totalTurns = count(totalTurns + turns, "execution observation total turn count");
		totalTools = count(totalTools + tools, "execution observation total tool-call count");
		if (!Array.isArray(segment.turn_sha256) || segment.turn_sha256.length !== turns) {
			throw new TypeError("execution observation turn keys do not reconcile with its turn count");
		}
		const turnKeys = segment.turn_sha256.map((key) => {
			const validated = digest(key, "execution turn key");
			if (seenTurns.has(validated)) throw new TypeError("execution observation repeats a turn key");
			seenTurns.add(validated);
			return validated;
		});
		return Object.freeze({
			mode: segment.mode,
			turn_count: turns,
			tool_call_count: tools,
			turn_sha256: Object.freeze(turnKeys),
		});
	});
	return Object.freeze({
		version: EXECUTION_OBSERVATION_VERSION,
		coverage: "exact",
		source: observationSource,
		turn_unit: "assistant-turn",
		tool_call_unit: "admitted-tool-call",
		evidence,
		segments: Object.freeze(segments),
	});
}

export function unknownExecutionObservation(source: string): ExecutionObservation {
	return normalizeExecutionObservation({
		version: EXECUTION_OBSERVATION_VERSION,
		coverage: "unknown",
		source,
		turn_unit: "unknown",
		tool_call_unit: "unknown",
		evidence: {},
		segments: [],
	});
}

function unknownCodexObservation(reason: string): ExecutionObservation {
	return unknownExecutionObservation(`codex_rollout_${reason}`);
}

/** Reduce privacy-bounded durable Codex events without reading transcript prose. */
export function reduceCodexExecutionObservation(
	events: readonly CodexExecutionObservationEvent[],
): ExecutionObservation {
	if (events.length === 0) return unknownCodexObservation("events_unavailable");
	const attemptKeys = new Set(events.map((event) => event.attempt_sha256));
	const sessionKeys = new Set(events.map((event) => event.session_sha256));
	if (attemptKeys.size !== 1 || sessionKeys.size !== 1) {
		return unknownCodexObservation("attempt_session_join_unavailable");
	}
	let attemptKey: string;
	let sessionKey: string;
	try {
		attemptKey = digest(events[0]!.attempt_sha256, "Codex attempt key");
		sessionKey = digest(events[0]!.session_sha256, "Codex session key");
	} catch {
		return unknownCodexObservation("attempt_session_join_unavailable");
	}
	let activeMode: ExecutionMode | undefined;
	const turns: Array<{ mode: ExecutionMode; key: string }> = [];
	const turnKeys = new Set<string>();
	const callsByTurn = new Map<string, Set<string>>();
	for (const event of events) {
		if (event.kind === "thread_settings_applied") {
			activeMode = event.service_tier === "priority" ? "fast"
				: event.service_tier === "default" ? "standard" : undefined;
			if (activeMode === undefined) return unknownCodexObservation("service_tier_unsupported");
			continue;
		}
		let turnKey: string;
		try {
			turnKey = digest(event.turn_sha256, "Codex turn key");
		} catch {
			return unknownCodexObservation("turn_evidence_invalid");
		}
		if (event.kind === "task_started") {
			if (activeMode === undefined) return unknownCodexObservation("initial_settings_unavailable");
			if (turnKeys.has(turnKey)) return unknownCodexObservation("turn_evidence_invalid");
			turnKeys.add(turnKey);
			turns.push({ mode: activeMode, key: turnKey });
			callsByTurn.set(turnKey, new Set());
			continue;
		}
		const calls = callsByTurn.get(turnKey);
		if (calls === undefined) return unknownCodexObservation("tool_turn_join_unavailable");
		let callKey: string;
		try {
			callKey = digest(event.tool_call_sha256, "Codex tool-call key");
		} catch {
			return unknownCodexObservation("tool_evidence_invalid");
		}
		if (calls.has(callKey)) return unknownCodexObservation("tool_evidence_invalid");
		calls.add(callKey);
	}
	if (turns.length === 0) return unknownCodexObservation("turn_evidence_unavailable");
	const segments: Array<{
		mode: ExecutionMode;
		turn_count: number;
		tool_call_count: number;
		turn_sha256: string[];
	}> = [];
	for (const turn of turns) {
		const preceding = segments.at(-1);
		const toolCount = callsByTurn.get(turn.key)!.size;
		if (preceding?.mode === turn.mode) {
			preceding.turn_count += 1;
			preceding.tool_call_count += toolCount;
			preceding.turn_sha256.push(turn.key);
		} else {
			segments.push({
				mode: turn.mode,
				turn_count: 1,
				tool_call_count: toolCount,
				turn_sha256: [turn.key],
			});
		}
	}
	return normalizeExecutionObservation({
		version: EXECUTION_OBSERVATION_VERSION,
		coverage: "exact",
		source: "codex_rollout",
		turn_unit: "assistant-turn",
		tool_call_unit: "admitted-tool-call",
		evidence: {
			provider: "openai",
			attempt_sha256: attemptKey,
			session_sha256: sessionKey,
		},
		segments,
	});
}

export function executionObservationTotals(
	observation: ExecutionObservation,
): Readonly<{ turn_count: number; tool_call_count: number }> | undefined {
	if (observation.coverage !== "exact") return undefined;
	let turns = 0;
	let tools = 0;
	for (const segment of observation.segments) {
		turns = count(turns + segment.turn_count, "execution observation total turn count");
		tools = count(tools + segment.tool_call_count, "execution observation total tool-call count");
	}
	return Object.freeze({ turn_count: turns, tool_call_count: tools });
}

export function executionObservationMode(
	observation: ExecutionObservation,
): ExecutionMode | "mixed" | "unknown" {
	if (observation.coverage !== "exact") return "unknown";
	const modes = new Set(observation.segments.map((segment) => segment.mode));
	return modes.size === 1 ? observation.segments[0]!.mode : "mixed";
}

export function executionObservationJson(observation: ExecutionObservation): string {
	return JSON.stringify(normalizeExecutionObservation(observation));
}
