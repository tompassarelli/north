import type { WireEvent, WireModelCallCompletedEvent } from "./events";

const PROVIDER_NAMED_ERROR_CODE_COMPONENT =
	/(?:^|[^A-Za-z0-9]|_)(codex|openai|anthropic|claude)(?=$|[^A-Za-z0-9]|_)/i;
const PUBLIC_ERROR_CODE = /^[a-z][a-z0-9_]{0,127}$/;

/** Public Wire error codes classify semantics, never a concrete provider. */
export function isProviderNeutralWireErrorCode(value: string): boolean {
	return PUBLIC_ERROR_CODE.test(value)
		&& !PROVIDER_NAMED_ERROR_CODE_COMPONENT.test(value);
}

type IntermediateProviderSessionReplacement = WireModelCallCompletedEvent & {
	status: "failed";
	origin: "north";
	errorCode: "provider_session_replaced";
};

/** North-owned dead-attempt settlement emitted before a replacement provider session starts. */
export function isIntermediateProviderSessionReplacement(
	event: WireEvent,
): event is IntermediateProviderSessionReplacement {
	return event.essential
		&& event.kind === "model-call.completed"
		&& event.status === "failed"
		&& event.origin === "north"
		&& event.errorCode === "provider_session_replaced";
}
