import type { WireEvent } from "./wire/events";
import type { WireToolCallId } from "./wire/ids";

export const STRUGGLE_DETECTOR_POLICY_VERSION = "north:struggle-observer:v2";
export const STRUGGLE_THRESHOLD_MAX = 1_000;

export type StruggleTopology = "worker" | "orchestrator";
export type StruggleTrigger = "consecutive_errors" | "tool_loop" | "no_progress";

export interface StrugglePolicy {
	readonly version: typeof STRUGGLE_DETECTOR_POLICY_VERSION;
	readonly topology: StruggleTopology;
	readonly errorStreak: number;
	readonly loopRepeat: number;
	/** Memory bound for the consecutive identical-call streak. */
	readonly loopWindow: number;
	readonly noProgressTurns: number;
}

export interface StruggleState {
	readonly policy: StrugglePolicy;
	/** Completed provider-neutral model calls observed. */
	turn: number;
	/** Model-call/message work units that admitted at least one tool. */
	workTurns: number;
	consecutiveErrors: number;
	totalErrors: number;
	lastProgressTurn: number;
	fingerprints: string[];
	pending: Map<WireToolCallId, string>;
	workUnits: Set<string>;
}

export interface StruggleObservation {
	readonly policyVersion: typeof STRUGGLE_DETECTOR_POLICY_VERSION;
	readonly topology: StruggleTopology;
	readonly errorStreakThreshold: number;
	readonly loopRepeatThreshold: number;
	readonly loopWindow: number;
	readonly noProgressTurnThreshold: number;
	readonly errorCount: number;
	readonly triggers: ReadonlyArray<StruggleTrigger>;
}

export interface StruggleObserver {
	readonly state: StruggleState;
	observe(event: WireEvent): StruggleTrigger | null;
	snapshot(): StruggleObservation;
}

const DEFAULTS = {
	errorStreak: 3,
	loopRepeat: 3,
	loopWindow: 20,
	workerNoProgress: 6,
	orchestratorNoProgress: 12,
} as const;

const STRUGGLE_PROGRESS_TOOLS: ReadonlySet<string> = new Set([
	"Read", "Grep", "Glob", "Bash", "WebSearch", "WebFetch",
	"Edit", "Write", "NotebookEdit",
	"command", "file-change", "web-search", "todo-list",
	"mcp__north__show", "mcp__north__ready", "mcp__north__next",
	"mcp__north__blocked",
	"mcp__north__agenda", "mcp__north__leverage", "mcp__north__needs_review",
	"mcp__north__validate",
	"mcp__north__capture", "mcp__north__tell", "mcp__north__retract",
	"mcp__north__evidence_record", "mcp__north__spawn", "mcp__north__dispatch",
]);

type StruggleEnvironment = Readonly<Record<string, string | undefined>>;

function boundedPositiveInteger(
	env: StruggleEnvironment,
	name: string,
	fallback: number,
): number {
	const raw = env[name];
	if (raw === undefined || raw === "") return fallback;
	if (!/^[1-9][0-9]*$/.test(raw)) {
		throw new Error(`${name} must be a positive integer between 1 and ${STRUGGLE_THRESHOLD_MAX}`);
	}
	const value = Number(raw);
	if (!Number.isSafeInteger(value) || value > STRUGGLE_THRESHOLD_MAX) {
		throw new Error(`${name} must be a positive integer between 1 and ${STRUGGLE_THRESHOLD_MAX}`);
	}
	return value;
}

export function resolveStrugglePolicy(
	topology: StruggleTopology,
	env: StruggleEnvironment = process.env,
): StrugglePolicy {
	if (topology !== "worker" && topology !== "orchestrator") {
		throw new Error(`struggle observer requires worker|orchestrator topology, got ${String(topology)}`);
	}
	const errorStreak = boundedPositiveInteger(
		env, "STRUGGLE_ERROR_STREAK", DEFAULTS.errorStreak,
	);
	const loopRepeat = boundedPositiveInteger(
		env, "STRUGGLE_LOOP_REPEAT", DEFAULTS.loopRepeat,
	);
	const loopWindow = boundedPositiveInteger(
		env, "STRUGGLE_LOOP_WINDOW", DEFAULTS.loopWindow,
	);
	if (loopRepeat > loopWindow) {
		throw new Error("STRUGGLE_LOOP_REPEAT must be less than or equal to STRUGGLE_LOOP_WINDOW");
	}
	const workerNoProgress = boundedPositiveInteger(
		env, "STRUGGLE_STALL_TURNS", DEFAULTS.workerNoProgress,
	);
	const orchestratorNoProgress = boundedPositiveInteger(
		env, "STRUGGLE_STALL_TURNS_ORCHESTRATOR", DEFAULTS.orchestratorNoProgress,
	);
	if (orchestratorNoProgress < workerNoProgress) {
		throw new Error(
			"STRUGGLE_STALL_TURNS_ORCHESTRATOR must be greater than or equal to STRUGGLE_STALL_TURNS",
		);
	}
	return Object.freeze({
		version: STRUGGLE_DETECTOR_POLICY_VERSION,
		topology,
		errorStreak,
		loopRepeat,
		loopWindow,
		noProgressTurns: topology === "orchestrator" ? orchestratorNoProgress : workerNoProgress,
	});
}

export function assertExpectedStrugglePolicy(
	policy: StrugglePolicy,
	expected = process.env.NORTH_STRUGGLE_POLICY_EXPECTED,
): void {
	if (expected === undefined) return;
	if (expected !== JSON.stringify(policy)) {
		throw new Error("struggle detector policy changed between adapter preview and execution");
	}
}

export function makeStruggleState(
	topologyOrPolicy: StruggleTopology | StrugglePolicy = "worker",
): StruggleState {
	const resolved = typeof topologyOrPolicy === "string"
		? resolveStrugglePolicy(topologyOrPolicy)
		: topologyOrPolicy;
	const policy = Object.freeze({ ...resolved });
	return {
		policy,
		turn: 0,
		workTurns: 0,
		consecutiveErrors: 0,
		totalErrors: 0,
		lastProgressTurn: 0,
		fingerprints: [],
		pending: new Map(),
		workUnits: new Set(),
	};
}

function fingerprint(
	event: Extract<WireEvent, { essential: true; kind: "tool.admitted" }>,
): string | undefined {
	return event.argumentDigest === undefined
		? undefined : JSON.stringify([event.name, event.argumentDigest]);
}

function progressTool(name: string): boolean {
	return STRUGGLE_PROGRESS_TOOLS.has(name) || name.startsWith("mcp:");
}

function workUnit(event: Extract<WireEvent, { essential: true; kind: "tool.admitted" }>): string {
	return event.modelCallId ?? event.messageId ?? event.toolCallId;
}

export function updateStruggle(event: WireEvent, state: StruggleState): void {
	if (!event.essential) return;
	if (event.kind === "model-call.completed") {
		state.turn += 1;
		return;
	}
	if (event.kind === "tool.admitted") {
		if (event.name === "background-task") return;
		state.pending.set(event.toolCallId, event.name);
		const observed = fingerprint(event);
		const previous = state.fingerprints.at(-1);
		if (observed === undefined) state.fingerprints = [];
		else if (observed === previous) state.fingerprints.push(observed);
		else state.fingerprints = [observed];
		if (state.fingerprints.length > state.policy.loopWindow) state.fingerprints.shift();
		const unit = workUnit(event);
		if (!state.workUnits.has(unit)) {
			state.workUnits.add(unit);
			state.workTurns += 1;
		}
		return;
	}
	if (event.kind !== "tool.terminal") return;
	const name = state.pending.get(event.toolCallId);
	if (name === undefined) return;
	state.pending.delete(event.toolCallId);
	if (event.status === "failed" || event.status === "synthetic_failure") {
		state.consecutiveErrors += 1;
		state.totalErrors += 1;
		return;
	}
	if (event.status === "succeeded") {
		state.consecutiveErrors = 0;
		if (progressTool(name)) state.lastProgressTurn = state.workTurns;
	}
}

export function checkStruggle(state: StruggleState): StruggleTrigger | null {
	if (state.consecutiveErrors >= state.policy.errorStreak) return "consecutive_errors";
	if (state.fingerprints.length >= state.policy.loopRepeat) return "tool_loop";
	if (state.workTurns - state.lastProgressTurn >= state.policy.noProgressTurns) return "no_progress";
	return null;
}

export function makeStruggleObserver(policy: StrugglePolicy): StruggleObserver {
	const state = makeStruggleState(policy);
	const fired = new Set<StruggleTrigger>();
	return {
		state,
		observe(event: WireEvent): StruggleTrigger | null {
			updateStruggle(event, state);
			const trigger = checkStruggle(state);
			if (!trigger || fired.has(trigger)) return null;
			fired.add(trigger);
			return trigger;
		},
		snapshot(): StruggleObservation {
			return Object.freeze({
				policyVersion: state.policy.version,
				topology: state.policy.topology,
				errorStreakThreshold: state.policy.errorStreak,
				loopRepeatThreshold: state.policy.loopRepeat,
				loopWindow: state.policy.loopWindow,
				noProgressTurnThreshold: state.policy.noProgressTurns,
				errorCount: state.totalErrors,
				triggers: Object.freeze([...fired]),
			});
		},
	};
}

if (import.meta.main) {
	const [command, topology, ...extra] = process.argv.slice(2);
	if (command !== "policy" || !topology || extra.length > 0) {
		console.error("usage: bun run struggle.ts policy <worker|orchestrator>");
		process.exit(2);
	}
	try {
		process.stdout.write(`${JSON.stringify(resolveStrugglePolicy(topology as StruggleTopology))}\n`);
	} catch (error) {
		console.error(error instanceof Error ? error.message : "invalid struggle detector policy");
		process.exit(1);
	}
}
