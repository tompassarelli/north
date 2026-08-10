import type { WireEventId, WireRunId } from "./ids";

export type WireContractErrorCode =
	| "malformed_event"
	| "unsupported_version"
	| "unsupported_required_semantics"
	| "unsupported_event_kind"
	| "sequence_violation"
	| "state_violation";

export class WireContractError extends Error {
	readonly code: WireContractErrorCode;
	readonly eventId?: WireEventId;
	readonly runId?: WireRunId;
	readonly sequence?: number;

	constructor(
		code: WireContractErrorCode,
		message: string,
		context: { eventId?: WireEventId; runId?: WireRunId; sequence?: number } = {},
		options?: ErrorOptions,
	) {
		super(message, options);
		this.name = "WireContractError";
		this.code = code;
		this.eventId = context.eventId;
		this.runId = context.runId;
		this.sequence = context.sequence;
	}
}

export class WireDecodeError extends WireContractError {
	constructor(code: Extract<WireContractErrorCode,
		"malformed_event" | "unsupported_version" | "unsupported_required_semantics" | "unsupported_event_kind">,
		message: string,
		context: { eventId?: WireEventId; runId?: WireRunId; sequence?: number } = {},
		options?: ErrorOptions,
	) {
		super(code, message, context, options);
		this.name = "WireDecodeError";
	}
}

export class WireReductionError extends WireContractError {
	constructor(
		code: Extract<WireContractErrorCode, "sequence_violation" | "state_violation">,
		message: string,
		context: { eventId?: WireEventId; runId?: WireRunId; sequence?: number } = {},
		options?: ErrorOptions,
	) {
		super(code, message, context, options);
		this.name = "WireReductionError";
	}
}
