export type JsonPrimitive = string | number | boolean | null;
export type JsonValue = JsonPrimitive | JsonObject | JsonArray;
export interface JsonObject {
	readonly [key: string]: JsonValue;
}
export interface JsonArray extends ReadonlyArray<JsonValue> {}

export const WIRE_JSON_MAX_DEPTH = 64;
export const WIRE_JSON_MAX_NODES = 100_000;
export const WIRE_JSON_MAX_BYTES = 2_097_152;
export const WIRE_JSON_MAX_STRING_BYTES = 1_048_576;
export const WIRE_JSON_MAX_KEY_BYTES = 4_096;

export class WireJsonValueError extends TypeError {
	readonly path: string;

	constructor(path: string, reason: string) {
		super(`${path} is not JSON-safe: ${reason}`);
		this.name = "WireJsonValueError";
		this.path = path;
	}
}

interface JsonTraversal {
	count: number;
	textBytes: number;
	seen: WeakSet<object>;
}

function utf8Bytes(value: string, limit: number, path: string, label: string): number {
	let bytes = 0;
	for (let index = 0; index < value.length; index += 1) {
		const code = value.charCodeAt(index);
		if (code < 0x80) bytes += 1;
		else if (code < 0x800) bytes += 2;
		else if (code >= 0xd800 && code <= 0xdbff
			&& index + 1 < value.length
			&& value.charCodeAt(index + 1) >= 0xdc00
			&& value.charCodeAt(index + 1) <= 0xdfff) {
			bytes += 4;
			index += 1;
		} else bytes += 3;
		if (bytes > limit) throw new WireJsonValueError(path, `${label} exceeds ${limit} UTF-8 bytes`);
	}
	return bytes;
}

function accountText(
	value: string,
	path: string,
	limit: number,
	label: string,
	traversal: JsonTraversal,
): void {
	traversal.textBytes += utf8Bytes(value, limit, path, label);
	if (traversal.textBytes > WIRE_JSON_MAX_BYTES) {
		throw new WireJsonValueError(path, `text content exceeds ${WIRE_JSON_MAX_BYTES} UTF-8 bytes`);
	}
}

function normalizedJsonValue(
	value: unknown,
	path: string,
	depth: number,
	traversal: JsonTraversal,
): JsonValue {
	traversal.count += 1;
	if (traversal.count > WIRE_JSON_MAX_NODES) {
		throw new WireJsonValueError(path, `node count exceeds ${WIRE_JSON_MAX_NODES}`);
	}
	if (depth > WIRE_JSON_MAX_DEPTH) {
		throw new WireJsonValueError(path, `depth exceeds ${WIRE_JSON_MAX_DEPTH}`);
	}
	if (value === null || typeof value === "boolean") return value;
	if (typeof value === "string") {
		accountText(value, path, WIRE_JSON_MAX_STRING_BYTES, "string", traversal);
		return value;
	}
	if (typeof value === "number") {
		if (!Number.isFinite(value)) throw new WireJsonValueError(path, "number must be finite");
		return Object.is(value, -0) ? 0 : value;
	}
	if (typeof value !== "object") {
		throw new WireJsonValueError(path, `${typeof value} values are unsupported`);
	}
	if (traversal.seen.has(value)) throw new WireJsonValueError(path, "cycles are unsupported");
	traversal.seen.add(value);
	try {
		if (Array.isArray(value)) {
			return Object.freeze(Array.from(value, (item, index) =>
				normalizedJsonValue(item, `${path}[${index}]`, depth + 1, traversal)
			));
		}
		const prototype = Object.getPrototypeOf(value);
		if (prototype !== Object.prototype && prototype !== null) {
			throw new WireJsonValueError(path, "only plain objects are supported");
		}
		if (Object.getOwnPropertySymbols(value).length > 0) {
			throw new WireJsonValueError(path, "symbol keys are unsupported");
		}
		const entries = Object.entries(value as Record<string, unknown>).map(([key, item]) => {
			accountText(key, path, WIRE_JSON_MAX_KEY_BYTES, "object key", traversal);
			return [
				key,
				normalizedJsonValue(item, `${path}.${key}`, depth + 1, traversal),
			] as const;
		});
		return Object.freeze(Object.fromEntries(entries)) as JsonObject;
	} finally {
		traversal.seen.delete(value);
	}
}

export function jsonValue(value: unknown, label = "value"): JsonValue {
	const normalized = normalizedJsonValue(value, label, 0, {
		count: 0,
		textBytes: 0,
		seen: new WeakSet(),
	});
	utf8Bytes(JSON.stringify(normalized), WIRE_JSON_MAX_BYTES, label, "encoded value");
	return normalized;
}

export function jsonObject(value: unknown, label = "value"): JsonObject {
	const normalized = jsonValue(value, label);
	if (normalized === null || typeof normalized !== "object" || Array.isArray(normalized)) {
		throw new WireJsonValueError(label, "expected an object");
	}
	return normalized as JsonObject;
}
