import {
	wireArtifactId,
	type WireArtifactId,
	type WireArtifactSink,
	type WireRunId,
} from "../wire";

export const RETAINED_PROVIDER_MATERIAL_MAX_BYTES = 1_048_576;
export const RETAINED_PROVIDER_PREVIEW_MAX_BYTES = 2_048;

const STRUCTURED_MATERIAL_MAX_DEPTH = 64;
const STRUCTURED_MATERIAL_MAX_NODES = 100_000;
const STRUCTURED_MATERIAL_MAX_TEXT_BYTES = 8 * 1024 * 1024;

const TEXT_ENCODER = new TextEncoder();
const TEXT_DECODER = new TextDecoder("utf-8", { fatal: true });

export interface RetainedProviderMaterial {
	readonly artifactId: WireArtifactId;
	readonly mediaType: string;
	readonly content: string;
	readonly digest: string;
	readonly label: string;
	readonly bytes: number;
}

export interface RetainedProviderMaterialInput {
	readonly runId: WireRunId;
	readonly provider: "anthropic" | "openai";
	readonly kind: string;
	readonly identity: string;
	readonly value: unknown;
	readonly label: string;
}

export class RetainedArtifactPersistenceError extends Error {
	constructor(message: string, options?: ErrorOptions) {
		super(message, options);
		this.name = "RetainedArtifactPersistenceError";
	}
}

export function isArtifactReadToolName(
	provider: "anthropic" | "openai",
	name: string,
): boolean {
	return provider === "openai"
		? name === "mcp:north/artifact_read"
		: name === "mcp__north__artifact_read";
}

function utf8Bytes(value: string): Uint8Array {
	return TEXT_ENCODER.encode(value);
}

function validateStructuredMaterial(value: unknown): void {
	const ancestors = new WeakSet<object>();
	let nodes = 0;
	let textBytes = 0;
	const visit = (current: unknown, depth: number): void => {
		nodes += 1;
		if (nodes > STRUCTURED_MATERIAL_MAX_NODES || depth > STRUCTURED_MATERIAL_MAX_DEPTH) {
			throw new TypeError("provider tool material exceeds structural bounds");
		}
		if (typeof current === "string") {
			textBytes += utf8Bytes(current).byteLength;
			if (textBytes > STRUCTURED_MATERIAL_MAX_TEXT_BYTES) {
				throw new TypeError("provider tool material exceeds text bounds");
			}
			return;
		}
		if (current === null || typeof current === "boolean") return;
		if (typeof current === "number" && Number.isFinite(current)) return;
		if (typeof current !== "object") {
			throw new TypeError("provider tool material contains a non-JSON value");
		}
		if (ancestors.has(current)) {
			throw new TypeError("provider tool material contains a cycle");
		}
		const prototype = Object.getPrototypeOf(current);
		if (prototype !== Object.prototype && prototype !== null && !Array.isArray(current)) {
			throw new TypeError("provider tool material must contain only plain JSON objects");
		}
		if (Array.isArray(current) && current.length > STRUCTURED_MATERIAL_MAX_NODES) {
			throw new TypeError("provider tool material exceeds structural bounds");
		}
		if (Object.getOwnPropertySymbols(current).length > 0) {
			throw new TypeError("provider tool material contains symbol keys");
		}
		ancestors.add(current);
		try {
			for (const key of Object.keys(current)) {
				textBytes += utf8Bytes(key).byteLength;
				if (textBytes > STRUCTURED_MATERIAL_MAX_TEXT_BYTES) {
					throw new TypeError("provider tool material exceeds text bounds");
				}
				const descriptor = Object.getOwnPropertyDescriptor(current, key);
				if (!descriptor || !("value" in descriptor)) {
					throw new TypeError("provider tool material contains an accessor");
				}
				visit(descriptor.value, depth + 1);
			}
		} finally {
			ancestors.delete(current);
		}
	};
	visit(value, 0);
}

function serializedMaterial(value: unknown): {
	content: string;
	mediaType: string;
} {
	if (typeof value === "string") {
		return { content: value, mediaType: "text/plain" };
	}
	validateStructuredMaterial(value);
	let content: string | undefined;
	try {
		content = JSON.stringify(value);
	} catch (cause) {
		throw new TypeError("provider tool material is not JSON serializable", { cause });
	}
	if (content === undefined) {
		throw new TypeError("provider tool material is not JSON serializable");
	}
	return { content, mediaType: "application/json" };
}

function utf8Prefix(value: Uint8Array, maximumBytes: number): string {
	let end = Math.min(maximumBytes, value.byteLength);
	while (end > 0) {
		try {
			return TEXT_DECODER.decode(value.subarray(0, end));
		} catch {
			end -= 1;
		}
	}
	return "";
}

function utf8Suffix(value: Uint8Array, maximumBytes: number): string {
	let start = Math.max(0, value.byteLength - maximumBytes);
	while (start < value.byteLength) {
		try {
			return TEXT_DECODER.decode(value.subarray(start));
		} catch {
			start += 1;
		}
	}
	return "";
}

function boundedMaterial(value: string, mediaType: string): {
	content: string;
	mediaType: string;
} {
	const encoded = utf8Bytes(value);
	if (encoded.byteLength <= RETAINED_PROVIDER_MATERIAL_MAX_BYTES) {
		return { content: value, mediaType };
	}
	const marker = `\n...[north retained output truncated from ${encoded.byteLength} UTF-8 bytes]\n`;
	const markerBytes = utf8Bytes(marker).byteLength;
	const retainedBytes = RETAINED_PROVIDER_MATERIAL_MAX_BYTES - markerBytes;
	const prefix = utf8Prefix(encoded, Math.ceil(retainedBytes / 2));
	const suffix = utf8Suffix(encoded, Math.floor(retainedBytes / 2));
	return {
		content: `${prefix}${marker}${suffix}`,
		mediaType: "text/plain",
	};
}

function digest(value: string): string {
	return new Bun.CryptoHasher("sha256").update(value).digest("hex");
}

export function retainedProviderMaterial(
	input: RetainedProviderMaterialInput,
): RetainedProviderMaterial {
	const serialized = serializedMaterial(input.value);
	const retained = boundedMaterial(serialized.content, serialized.mediaType);
	const identityDigest = new Bun.CryptoHasher("sha256")
		.update(`north.retained-provider-material.v1\0${input.runId}\0${input.provider}\0${input.kind}\0${input.identity}`)
		.digest("hex");
	const artifactId = wireArtifactId(`artifact:${input.provider}:retained:${identityDigest}`);
	const contentDigest = digest(retained.content);
	return Object.freeze({
		artifactId,
		mediaType: retained.mediaType,
		content: retained.content,
		digest: contentDigest,
		label: input.label,
		bytes: utf8Bytes(retained.content).byteLength,
	});
}

export function persistRetainedProviderMaterial(
	sink: WireArtifactSink,
	material: RetainedProviderMaterial,
): void {
	try {
		const receipt = sink.persist(material);
		if (!receipt || receipt.artifactId !== material.artifactId
			|| receipt.digest !== material.digest) {
			throw new RetainedArtifactPersistenceError(
				"provider tool artifact persistence receipt does not match",
			);
		}
	} catch (cause) {
		if (cause instanceof RetainedArtifactPersistenceError) throw cause;
		throw new RetainedArtifactPersistenceError(
			"provider tool artifact could not be persisted",
			{ cause },
		);
	}
}

export function retainedProviderPreview(value: unknown): string {
	let source: string;
	try {
		source = serializedMaterial(value).content;
	} catch {
		return "[unavailable]";
	}
	let output = "";
	let outputBytes = 0;
	for (let index = 0; index < source.length;) {
		const code = source.codePointAt(index);
		if (code === undefined) break;
		const width = code > 0xffff ? 2 : 1;
		let next: string;
		if (code === 0x0d) {
			next = "\n";
			if (source.charCodeAt(index + width) === 0x0a) index += 1;
		} else if (code === 0x09) next = "  ";
		else if ((code >= 0 && code <= 0x08) || (code >= 0x0b && code <= 0x1f)
			|| (code >= 0x7f && code <= 0x9f)) next = "�";
		else next = String.fromCodePoint(code);
		const nextBytes = utf8Bytes(next).byteLength;
		if (outputBytes + nextBytes > RETAINED_PROVIDER_PREVIEW_MAX_BYTES - 3) {
			return `${output}…`;
		}
		output += next;
		outputBytes += nextBytes;
		index += width;
	}
	return output;
}
