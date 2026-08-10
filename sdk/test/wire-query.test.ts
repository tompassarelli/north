import { expect, test } from "bun:test";
import {
	createExecutionActivityEmitter,
	type ExecutionActivitySource,
	type ExecutionActivityEmitter,
} from "../src/execution-activity";
import type { NativeCommandActivityObservation } from "../src/native-command-activity";
import type { McpActivityObservation } from "../src/tool-activity";
import {
	WIRE_REQUIRED_SEMANTICS,
	WIRE_VERSION,
	type WireEvent,
	type WireModelSelection,
} from "../src/wire/events";
import { wireEventId, wireRunId } from "../src/wire/ids";
import {
	proxyWireQuery,
	wireQueryRoute,
	type WireEventListener,
	type WireQuery,
	type WireQueryFlagSettings,
	type WireQueryInput,
} from "../src/wire/query";

const sourceEvent: WireEvent = {
	version: WIRE_VERSION,
	id: wireEventId("event-source"),
	runId: wireRunId("run-query"),
	sequence: 1,
	at: "2026-08-10T00:00:00.000Z",
	kind: "run.started",
	essential: true,
	requiredSemantics: WIRE_REQUIRED_SEMANTICS,
	lifecycle: "running",
};

const replacementEvent: WireEvent = {
	version: WIRE_VERSION,
	id: wireEventId("event-replacement"),
	runId: wireRunId("run-query"),
	sequence: 2,
	at: "2026-08-10T00:00:01.000Z",
	kind: "run.progress",
	essential: true,
	requiredSemantics: WIRE_REQUIRED_SEMANTICS,
	lifecycle: "waiting",
	progress: { currentAction: "waiting for input" },
};

const mcpObservation: McpActivityObservation = {
	source: "wire-query-test",
	coverage: "exact",
	totalCalls: 1,
	tools: [{ server: "north", tool: "tell", count: 1 }],
	operationReceipts: [],
	operationAggregates: [],
};

const nativeCommandObservation: NativeCommandActivityObservation = {
	source: "wire-query-test",
	coverage: "exact",
	totalCommands: 0,
	northBinaryProbe: "not_observed",
	completions: [],
};

class FullWireQuery implements WireQuery {
	readonly executionTransport = "managed-app-server" as const;
	readonly executionActivity: ExecutionActivitySource;
	readonly calls: string[] = [];
	modelSelection?: WireModelSelection;
	flagSettings?: WireQueryFlagSettings;
	continuedInput?: WireQueryInput;
	#activity: ExecutionActivityEmitter;
	#listeners = new Set<WireEventListener>();

	constructor() {
		this.#activity = createExecutionActivityEmitter(
			() => new Date("2026-08-10T00:00:02.000Z"),
		);
		this.executionActivity = this.#activity.source;
	}

	subscribeProviderEvents(listener: WireEventListener): () => void {
		this.#listeners.add(listener);
		return () => this.#listeners.delete(listener);
	}

	publish(event: WireEvent): void {
		for (const listener of this.#listeners) listener(event);
	}

	recordActivity(): void {
		this.#activity.record("provider", "provider.test.completed");
	}

	async continueTurn(input: WireQueryInput): Promise<void> {
		this.calls.push("continueTurn");
		this.continuedInput = input;
	}

	async interruptTurn(): Promise<void> {
		this.calls.push("interruptTurn");
	}

	async interrupt(): Promise<void> {
		this.calls.push("interrupt");
	}

	async close(): Promise<void> {
		this.calls.push("close");
	}

	forceClose(): void {
		this.calls.push("forceClose");
	}

	setModel(selection: WireModelSelection): void {
		this.calls.push("setModel");
		this.modelSelection = selection;
	}

	applyFlagSettings(settings: WireQueryFlagSettings): void {
		this.calls.push("applyFlagSettings");
		this.flagSettings = settings;
	}

	supportsInFlightEscalation(): boolean {
		this.calls.push("supportsInFlightEscalation");
		return true;
	}

	mcpActivity(): McpActivityObservation {
		this.calls.push("mcpActivity");
		return mcpObservation;
	}

	nativeCommandActivity(): NativeCommandActivityObservation {
		this.calls.push("nativeCommandActivity");
		return nativeCommandObservation;
	}

	async *[Symbol.asyncIterator](): AsyncIterator<WireEvent> {
		yield sourceEvent;
	}
}

async function* replacementEvents(): AsyncIterableIterator<WireEvent> {
	yield replacementEvent;
}

test("a wire query proxy replaces iteration and forwards every control and observation", async () => {
	const source = new FullWireQuery();
	const query = proxyWireQuery(source, replacementEvents());

	const observed: WireEvent[] = [];
	const unsubscribeProvider = query.subscribeProviderEvents!((event) => observed.push(event));
	source.publish(sourceEvent);
	unsubscribeProvider();
	source.publish(replacementEvent);

	let activityNotifications = 0;
	const unsubscribeActivity = query.executionActivity!.subscribe(() => activityNotifications++);
	source.recordActivity();
	unsubscribeActivity();

	await query.continueTurn!("follow-up");
	await query.interruptTurn!();
	await query.interrupt!();
	await query.close!();
	query.forceClose!();
	query.setModel!({ provider: "openai", tier: "frontier" });
	query.applyFlagSettings!({ effortLevel: "high" });

	expect(query.executionTransport).toBe("managed-app-server");
	expect(query.executionActivity).toBe(source.executionActivity);
	expect(query.supportsInFlightEscalation!()).toBe(true);
	expect(query.mcpActivity!()).toBe(mcpObservation);
	expect(query.nativeCommandActivity!()).toBe(nativeCommandObservation);
	expect(observed).toEqual([sourceEvent]);
	expect(activityNotifications).toBe(1);
	expect(source.modelSelection).toEqual({ provider: "openai", tier: "frontier" });
	expect(source.flagSettings).toEqual({ effortLevel: "high" });
	expect(source.continuedInput).toBe("follow-up");
	expect(source.calls).toEqual([
		"continueTurn",
		"interruptTurn",
		"interrupt",
		"close",
		"forceClose",
		"setModel",
		"applyFlagSettings",
		"supportsInFlightEscalation",
		"mcpActivity",
		"nativeCommandActivity",
	]);

	const events: WireEvent[] = [];
	for await (const event of query) events.push(event);
	expect(events).toEqual([replacementEvent]);
});

test("a wire query proxy does not synthesize omitted optional fields", async () => {
	const source: WireQuery = {
		async *[Symbol.asyncIterator](): AsyncIterator<WireEvent> {
			yield sourceEvent;
		},
	};
	const query = proxyWireQuery(source);
	const optionalFields = [
		"executionTransport",
		"executionActivity",
		"subscribeProviderEvents",
		"continueTurn",
		"interruptTurn",
		"interrupt",
		"close",
		"forceClose",
		"setModel",
		"applyFlagSettings",
		"supportsInFlightEscalation",
		"mcpActivity",
		"nativeCommandActivity",
	] as const;

	for (const field of optionalFields) expect(field in query).toBe(false);
	const events: WireEvent[] = [];
	for await (const event of query) events.push(event);
	expect(events).toEqual([sourceEvent]);
});

test("a wire query route requires positive bounded counters and copies semantic selection", () => {
	const model: WireModelSelection = { provider: "anthropic", tier: "senior" };
	const route = wireQueryRoute({ model, effort: "high", attempt: 2, contextWindow: 200_000 });
	model.tier = "economy";
	expect(route).toEqual({
		model: { provider: "anthropic", tier: "senior" },
		effort: "high",
		attempt: 2,
		contextWindow: 200_000,
	});
	expect(() => wireQueryRoute({ model, effort: "low", attempt: 0 })).toThrow(RangeError);
	expect(() => wireQueryRoute({
		model,
		effort: "low",
		attempt: 1,
		contextWindow: Number.NaN,
	})).toThrow(RangeError);
	expect(() => wireQueryRoute({
		model: {
			provider: "openai",
			modelId: "provider-private-model",
		} as unknown as WireModelSelection,
		effort: "high",
		attempt: 1,
	})).toThrow(TypeError);
});
