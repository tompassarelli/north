const WIRE_ID_PATTERN = /^[A-Za-z0-9@][A-Za-z0-9@_.:/-]{0,255}$/;

declare const runIdBrand: unique symbol;
declare const parentIdBrand: unique symbol;
declare const eventIdBrand: unique symbol;
declare const messageIdBrand: unique symbol;
declare const modelCallIdBrand: unique symbol;
declare const toolCallIdBrand: unique symbol;
declare const artifactIdBrand: unique symbol;
declare const resourceIdBrand: unique symbol;

export type WireRunId = string & { readonly [runIdBrand]: true };
export type WireParentId = string & { readonly [parentIdBrand]: true };
export type WireEventId = string & { readonly [eventIdBrand]: true };
export type WireMessageId = string & { readonly [messageIdBrand]: true };
export type WireModelCallId = string & { readonly [modelCallIdBrand]: true };
export type WireToolCallId = string & { readonly [toolCallIdBrand]: true };
export type WireArtifactId = string & { readonly [artifactIdBrand]: true };
export type WireResourceId = string & { readonly [resourceIdBrand]: true };

function validatedId(value: string, label: string): string {
	if (!WIRE_ID_PATTERN.test(value)) {
		throw new TypeError(`${label} must match ${WIRE_ID_PATTERN.source}`);
	}
	return value;
}

export function wireRunId(value: string): WireRunId {
	return validatedId(value, "wire run id") as WireRunId;
}

export function wireParentId(value: string): WireParentId {
	return validatedId(value, "wire parent id") as WireParentId;
}

export function wireEventId(value: string): WireEventId {
	return validatedId(value, "wire event id") as WireEventId;
}

export function wireMessageId(value: string): WireMessageId {
	return validatedId(value, "wire message id") as WireMessageId;
}

export function wireModelCallId(value: string): WireModelCallId {
	return validatedId(value, "wire model-call id") as WireModelCallId;
}

export function wireToolCallId(value: string): WireToolCallId {
	return validatedId(value, "wire tool-call id") as WireToolCallId;
}

export function wireArtifactId(value: string): WireArtifactId {
	return validatedId(value, "wire artifact id") as WireArtifactId;
}

export function wireResourceId(value: string): WireResourceId {
	return validatedId(value, "wire resource id") as WireResourceId;
}
