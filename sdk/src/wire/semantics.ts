import type { WireEvent, WireModelCallCompletedEvent } from "./events";
import { jsonObject, type JsonObject, type JsonValue } from "./json";

const PROVIDER_NAMED_ERROR_CODE_COMPONENT =
	/(?:^|[^A-Za-z0-9]|_)(codex|openai|anthropic|claude)(?=$|[^A-Za-z0-9]|_)/i;
const PUBLIC_ERROR_CODE = /^[a-z][a-z0-9_]{0,127}$/;
const TOOL_ARGUMENT_DIGEST_DOMAIN = "north:wire-tool-arguments:v1\0";
const TOOL_INTENT_ONLY_FIELDS: ReadonlySet<string> = new Set(["i", "__intent"]);

function canonicalToolArguments(value: JsonValue): JsonValue {
	if (Array.isArray(value)) return Object.freeze(value.map(canonicalToolArguments));
	if (value === null || typeof value !== "object") return value;
	const entries = Object.entries(value as JsonObject)
		.filter(([key]) => !TOOL_INTENT_ONLY_FIELDS.has(key))
		.sort(([left], [right]) => left < right ? -1 : left > right ? 1 : 0)
		.map(([key, child]) => [key, canonicalToolArguments(child)] as const);
	return Object.freeze(Object.fromEntries(entries)) as JsonObject;
}

/**
 * Privacy-bounded equality evidence for one structured tool-argument object.
 * The canonical text exists only while hashing and never crosses Wire.
 */
export function wireToolArgumentDigest(value: unknown): string | undefined {
	let argumentsObject: JsonObject;
	try {
		argumentsObject = jsonObject(value, "tool arguments");
	} catch {
		return undefined;
	}
	const canonical = JSON.stringify(canonicalToolArguments(argumentsObject));
	const hasher = new Bun.CryptoHasher("sha256");
	hasher.update(TOOL_ARGUMENT_DIGEST_DOMAIN);
	hasher.update(canonical);
	return hasher.digest("hex");
}

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
