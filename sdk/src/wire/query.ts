import type { ExecutionActivitySource } from "../execution-activity";
import type { NativeCommandActivityObservation } from "../native-command-activity";
import type { McpActivityObservation } from "../tool-activity";
import {
	WIRE_CAPABILITY_CLASSES,
	WIRE_EFFORTS,
	WIRE_SEMANTIC_TIERS,
	type WireEffort,
	type WireEvent,
	type WireModelSelection,
} from "./events";
import type { WireArtifactId } from "./ids";
import type { WireEventWriter } from "./writer";

export interface WireUserInputFrame {
	kind: "user.input";
	text: string;
}

export type WireQueryInput = string | AsyncIterable<WireUserInputFrame>;

export interface WireQueryRoute {
	readonly model: WireModelSelection;
	readonly effort: WireEffort;
	readonly attempt: number;
	readonly contextWindow?: number;
}

/** Canonicalize the provider-neutral route before it reaches an adapter. */
export function wireQueryRoute(route: WireQueryRoute): WireQueryRoute {
	const routeKeys = Object.keys(route);
	if (routeKeys.some((key) => !["model", "effort", "attempt", "contextWindow"].includes(key))) {
		throw new TypeError("wire query route contains unknown fields");
	}
	const modelKeys = Object.keys(route.model);
	if (modelKeys.some((key) => !["provider", "tier", "capabilityClass"].includes(key))) {
		throw new TypeError("wire query route model contains unknown fields");
	}
	if (route.model.provider !== "anthropic" && route.model.provider !== "openai") {
		throw new RangeError("wire query route model provider is unsupported");
	}
	if (route.model.tier !== undefined
		&& !(WIRE_SEMANTIC_TIERS as readonly string[]).includes(route.model.tier)) {
		throw new RangeError("wire query route model tier is unsupported");
	}
	if (route.model.capabilityClass !== undefined
		&& !(WIRE_CAPABILITY_CLASSES as readonly string[]).includes(route.model.capabilityClass)) {
		throw new RangeError("wire query route model capabilityClass is unsupported");
	}
	if (!(WIRE_EFFORTS as readonly string[]).includes(route.effort)) {
		throw new RangeError("wire query route effort is unsupported");
	}
	if (!Number.isSafeInteger(route.attempt) || route.attempt < 1) {
		throw new RangeError("wire query route attempt must be a positive safe integer");
	}
	if (route.contextWindow !== undefined
		&& (!Number.isSafeInteger(route.contextWindow) || route.contextWindow < 1)) {
		throw new RangeError("wire query route contextWindow must be a positive safe integer");
	}
	return Object.freeze({
		model: Object.freeze({ ...route.model }),
		effort: route.effort,
		attempt: route.attempt,
		...(route.contextWindow === undefined ? {} : { contextWindow: route.contextWindow }),
	});
}

export interface WireArtifactMaterial {
	readonly artifactId: WireArtifactId;
	readonly mediaType: string;
	readonly content: string;
	readonly digest: string;
	readonly label?: string;
}

export interface WireArtifactReceipt {
	readonly artifactId: WireArtifactId;
	readonly digest: string;
}

export interface WireArtifactSink {
	/** Returns only after the artifact is durably addressable by artifactId. */
	persist(artifact: Readonly<WireArtifactMaterial>): WireArtifactReceipt;
}

export interface WireQueryContext {
	readonly writer: WireEventWriter;
	readonly route: WireQueryRoute;
	readonly artifacts?: WireArtifactSink;
}

export type WireExecutionTransport =
	| "sdk-stream"
	| "managed-app-server"
	| "cli-jsonl";

export interface WireQueryFlagSettings {
	effortLevel?: WireEffort | null;
}

export type WireEventListener = (event: WireEvent) => void;
export type WireQueryUnsubscribe = () => void;

export interface WireQueryObservations {
	/** Exact adapter transport selected for this query; absent until unknowable. */
	readonly executionTransport?: WireExecutionTransport;
	/** Authenticated execution activity retained for watchdog and liveness consumers. */
	readonly executionActivity?: ExecutionActivitySource;
	/** Normalized lifecycle events for observers that cannot consume the query iterator. */
	subscribeProviderEvents?(listener: WireEventListener): WireQueryUnsubscribe;
	/** Argument-free actual MCP activity observed by the selected adapter. */
	mcpActivity?(): McpActivityObservation;
	/** Privacy-bounded native command completion evidence observed by the adapter. */
	nativeCommandActivity?(): NativeCommandActivityObservation;
}

export interface WireQueryControls {
	/** Submit a later user turn without exposing a provider session or thread identifier. */
	continueTurn?(input: WireQueryInput): Promise<void>;
	/** Interrupt only the active provider turn while retaining its session or thread. */
	interruptTurn?(): Promise<void>;
	interrupt?(): Promise<void>;
	/** Idempotently dispose the query and await its owned process boundary. */
	close?(): Promise<void>;
	/** Synchronous second-signal and host-exit defense; never a normal cleanup path. */
	forceClose?(): void;
	setModel?(selection: WireModelSelection): Promise<void> | void;
	applyFlagSettings?(settings: WireQueryFlagSettings): Promise<void> | void;
	/** True only when model selection and effort can both change on the active run. */
	supportsInFlightEscalation?(): boolean;
}

export interface WireQuery
	extends AsyncIterable<WireEvent>, WireQueryControls, WireQueryObservations {}

/**
 * Replace a query's event iterable without losing adapter-owned controls or
 * observation sources. A Proxy intentionally keeps optional-field presence and
 * dynamic getters identical to the source query.
 */
export function proxyWireQuery(
	source: WireQuery,
	events: AsyncIterable<WireEvent> = source,
): WireQuery {
	return new Proxy(source, {
		get(target: WireQuery, property: string | symbol): unknown {
			if (property === Symbol.asyncIterator) {
				return () => events[Symbol.asyncIterator]();
			}
			const value: unknown = Reflect.get(target, property, target);
			if (typeof value !== "function") return value;
			const method = value as (...args: never[]) => unknown;
			return method.bind(target);
		},
	});
}
