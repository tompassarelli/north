import { Buffer } from "node:buffer";
import { createHash } from "node:crypto";
import {
	RUN_SHARE_AES_GCM_AAD,
	RUN_SHARE_AUTH_TAG_BYTES,
	RUN_SHARE_BUNDLE_PROTOCOL,
	RUN_SHARE_BUNDLE_VERSION,
	RUN_SHARE_KEY_BYTES,
	RUN_SHARE_MAX_EVENTS,
	RUN_SHARE_MAX_PLAINTEXT_BYTES,
	RUN_SHARE_MAX_SEALED_BYTES,
	RUN_SHARE_NONCE_BYTES,
	RUN_SHARE_REDACTION_POLICY,
	RUN_SHARE_REPLAY_MODE,
	RUN_SHARE_SEALED_HEADER,
} from "./run-share-contract";

const HEADER_BYTES = new TextEncoder().encode(RUN_SHARE_SEALED_HEADER);

const VIEWER_STYLE = `
:root {
	color-scheme: light dark;
	font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
}
body {
	box-sizing: border-box;
	max-width: 80rem;
	margin: 0 auto;
	padding: 2rem;
	background: Canvas;
	color: CanvasText;
}
h1 {
	font-family: system-ui, sans-serif;
	font-size: 1.25rem;
	margin: 0 0 0.75rem;
}
#status {
	font-family: system-ui, sans-serif;
	margin: 0 0 1rem;
	color: GrayText;
}
#status[data-state="error"] {
	color: #b42318;
}
#events {
	box-sizing: border-box;
	width: 100%;
	margin: 0;
	padding: 1rem;
	border: 1px solid GrayText;
	border-radius: 0.25rem;
	white-space: pre-wrap;
	overflow-wrap: anywhere;
	line-height: 1.45;
}
`;

const VIEWER_SCRIPT = `(() => {
	"use strict";

	const HEADER = ${JSON.stringify(RUN_SHARE_SEALED_HEADER)};
	const AAD = ${JSON.stringify(RUN_SHARE_AES_GCM_AAD)};
	const PROTOCOL = ${JSON.stringify(RUN_SHARE_BUNDLE_PROTOCOL)};
	const VERSION = ${RUN_SHARE_BUNDLE_VERSION};
	const REDACTION_POLICY = ${JSON.stringify(RUN_SHARE_REDACTION_POLICY)};
	const REPLAY_MODE = ${JSON.stringify(RUN_SHARE_REPLAY_MODE)};
	const KEY_BYTES = ${RUN_SHARE_KEY_BYTES};
	const NONCE_BYTES = ${RUN_SHARE_NONCE_BYTES};
	const AUTH_TAG_BYTES = ${RUN_SHARE_AUTH_TAG_BYTES};
	const MAX_SEALED_BYTES = ${RUN_SHARE_MAX_SEALED_BYTES};
	const MAX_DECOMPRESSED_BYTES = ${RUN_SHARE_MAX_PLAINTEXT_BYTES};
	const MAX_EVENTS = ${RUN_SHARE_MAX_EVENTS};
	const MAX_VALUE_DEPTH = 128;
	const MAX_VALUE_NODES = 250000;
	const encoder = new TextEncoder();
	const headerBytes = encoder.encode(HEADER);
	const aadBytes = encoder.encode(AAD);
	const status = document.getElementById("status");
	const events = document.getElementById("events");
	const sealedElement = document.getElementById("sealed-bundle");

	function showError(message) {
		status.textContent = message;
		status.dataset.state = "error";
		events.hidden = true;
	}

	function decodeBase64Url(text) {
		if (!/^[A-Za-z0-9_-]+$/.test(text) || text.length % 4 === 1) throw new Error("invalid base64url");
		const padding = "=".repeat((4 - text.length % 4) % 4);
		const binary = atob(text.replace(/-/g, "+").replace(/_/g, "/") + padding);
		const bytes = new Uint8Array(binary.length);
		for (let index = 0; index < binary.length; index += 1) bytes[index] = binary.charCodeAt(index);
		return bytes;
	}

	function encodeBase64Url(bytes) {
		let binary = "";
		const chunkSize = 32768;
		for (let offset = 0; offset < bytes.length; offset += chunkSize) {
			binary += String.fromCharCode(...bytes.subarray(offset, Math.min(offset + chunkSize, bytes.length)));
		}
		return btoa(binary).replace(/\\+/g, "-").replace(/\\//g, "_").replace(/=+$/, "");
	}

	function fragmentKey() {
		const encoded = location.hash.startsWith("#") ? location.hash.slice(1) : "";
		if (encoded === "") return null;
		if (!/^[A-Za-z0-9_-]{43}$/.test(encoded)) throw new Error("invalid key");
		const key = decodeBase64Url(encoded);
		if (key.length !== KEY_BYTES || encodeBase64Url(key) !== encoded) throw new Error("invalid key");
		return key;
	}

	function sealedBytes() {
		const encoded = sealedElement.textContent || "";
		sealedElement.remove();
		if (encoded.length > Math.ceil(MAX_SEALED_BYTES * 4 / 3)) throw new Error("invalid sealed bundle");
		const sealed = decodeBase64Url(encoded);
		if (sealed.length > MAX_SEALED_BYTES || sealed.length < headerBytes.length + NONCE_BYTES + AUTH_TAG_BYTES) {
			throw new Error("invalid sealed bundle");
		}
		for (let index = 0; index < headerBytes.length; index += 1) {
			if (sealed[index] !== headerBytes[index]) throw new Error("invalid sealed bundle");
		}
		return sealed;
	}

	async function decrypt(sealed, rawKey) {
		const nonceOffset = headerBytes.length;
		const ciphertextOffset = nonceOffset + NONCE_BYTES;
		const key = await crypto.subtle.importKey("raw", rawKey, { name: "AES-GCM" }, false, ["decrypt"]);
		const plaintext = await crypto.subtle.decrypt({
			name: "AES-GCM",
			iv: sealed.subarray(nonceOffset, ciphertextOffset),
			additionalData: aadBytes,
			tagLength: AUTH_TAG_BYTES * 8,
		}, key, sealed.subarray(ciphertextOffset));
		return new Uint8Array(plaintext);
	}

	async function gunzipBounded(compressed) {
		const source = new ReadableStream({
			start(controller) {
				controller.enqueue(compressed);
				controller.close();
			},
		});
		const reader = source.pipeThrough(new DecompressionStream("gzip")).getReader();
		const chunks = [];
		let length = 0;
		try {
			while (true) {
				const result = await reader.read();
				if (result.done) break;
				if (!(result.value instanceof Uint8Array)) throw new Error("invalid decompression output");
				length += result.value.byteLength;
				if (length > MAX_DECOMPRESSED_BYTES) throw new Error("decompressed bundle is too large");
				chunks.push(result.value);
			}
		} catch (error) {
			await reader.cancel().catch(() => undefined);
			throw error;
		}
		const output = new Uint8Array(length);
		let offset = 0;
		for (const chunk of chunks) {
			output.set(chunk, offset);
			offset += chunk.byteLength;
		}
		return output;
	}

	function validateBundle(value) {
		if (value === null || typeof value !== "object" || Array.isArray(value)) throw new Error("invalid bundle");
		if (value.protocol !== PROTOCOL || value.version !== VERSION) throw new Error("invalid bundle");
		if (value.redactionPolicy !== REDACTION_POLICY || value.replay !== REPLAY_MODE) throw new Error("invalid bundle");
		if (!Array.isArray(value.events) || value.events.length > MAX_EVENTS) throw new Error("invalid bundle");

		let nodes = 0;
		function visit(item, depth) {
			nodes += 1;
			if (nodes > MAX_VALUE_NODES || depth > MAX_VALUE_DEPTH) throw new Error("invalid bundle");
			if (item === null || typeof item === "string" || typeof item === "boolean") return;
			if (typeof item === "number") {
				if (!Number.isFinite(item)) throw new Error("invalid bundle");
				return;
			}
			if (Array.isArray(item)) {
				for (const child of item) visit(child, depth + 1);
				return;
			}
			if (typeof item !== "object" || Object.getPrototypeOf(item) !== Object.prototype) {
				throw new Error("invalid bundle");
			}
			for (const key of Object.keys(item)) {
				const normalized = key.replace(/[-_]/g, "").toLowerCase();
				if (normalized === "sourcemap" || normalized === "sourcemaps"
					|| (normalized.startsWith("original") && (normalized.endsWith("id") || normalized.endsWith("ids")))) {
					throw new Error("invalid bundle");
				}
				visit(item[key], depth + 1);
			}
		}

		visit(value, 0);
		for (const event of value.events) {
			if (event === null || typeof event !== "object" || Array.isArray(event)) throw new Error("invalid bundle");
		}
		return value.events;
	}

	function stableValue(value) {
		if (Array.isArray(value)) return value.map(stableValue);
		if (value !== null && typeof value === "object") {
			const stable = Object.create(null);
			for (const key of Object.keys(value).sort()) stable[key] = stableValue(value[key]);
			return stable;
		}
		return value;
	}

	async function openViewer() {
		let rawKey;
		try {
			rawKey = fragmentKey();
		} catch {
			showError("This run share could not be decrypted. The key is invalid or the file was changed.");
			return;
		}
		if (rawKey === null) {
			showError("This run share is missing its decryption key.");
			return;
		}

		let compressed;
		try {
			compressed = await decrypt(sealedBytes(), rawKey);
		} catch {
			showError("This run share could not be decrypted. The key is invalid or the file was changed.");
			return;
		}

		try {
			const plaintext = await gunzipBounded(compressed);
			const text = new TextDecoder("utf-8", { fatal: true }).decode(plaintext);
			const redactedEvents = validateBundle(JSON.parse(text));
			events.textContent = JSON.stringify(stableValue(redactedEvents));
			events.hidden = false;
			status.textContent = redactedEvents.length + (redactedEvents.length === 1 ? " redacted event" : " redacted events");
			status.dataset.state = "ready";
		} catch {
			showError("This run share is invalid or was changed.");
		}
	}

	void openViewer();
})();`;

function contentHash(content: string): string {
	return createHash("sha256").update(content, "utf8").digest("base64");
}

function validateSealedBundle(sealedBundle: Uint8Array): void {
	if (!(sealedBundle instanceof Uint8Array)) throw new TypeError("sealedBundle must be a Uint8Array");
	if (sealedBundle.byteLength > RUN_SHARE_MAX_SEALED_BYTES) {
		throw new RangeError(`sealedBundle exceeds ${RUN_SHARE_MAX_SEALED_BYTES} bytes`);
	}
	if (sealedBundle.byteLength < HEADER_BYTES.byteLength + RUN_SHARE_NONCE_BYTES + RUN_SHARE_AUTH_TAG_BYTES) {
		throw new TypeError("sealedBundle is shorter than the run-share envelope");
	}
	for (let index = 0; index < HEADER_BYTES.byteLength; index += 1) {
		if (sealedBundle[index] !== HEADER_BYTES[index]) {
			throw new TypeError("sealedBundle has an invalid run-share header");
		}
	}
}

/** Renders a self-contained, network-isolated viewer for one sealed run-share bundle. */
export function renderRunShareViewer(sealedBundle: Uint8Array): string {
	validateSealedBundle(sealedBundle);
	const encodedBundle = Buffer.from(
		sealedBundle.buffer,
		sealedBundle.byteOffset,
		sealedBundle.byteLength,
	).toString("base64url");
	const policy = [
		"default-src 'none'",
		`script-src 'sha256-${contentHash(VIEWER_SCRIPT)}'`,
		`style-src 'sha256-${contentHash(VIEWER_STYLE)}'`,
		"connect-src 'none'",
		"font-src 'none'",
		"form-action 'none'",
		"img-src 'none'",
		"media-src 'none'",
		"object-src 'none'",
		"worker-src 'none'",
		"base-uri 'none'",
	].join("; ");

	return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="referrer" content="no-referrer">
<meta http-equiv="Content-Security-Policy" content="${policy}">
<title>North run share</title>
<style>${VIEWER_STYLE}</style>
</head>
<body>
<main>
<h1>North run share</h1>
<p id="status" role="status" aria-live="polite">Opening encrypted run share…</p>
<pre id="events" hidden></pre>
</main>
<div id="sealed-bundle" hidden>${encodedBundle}</div>
<script>${VIEWER_SCRIPT}</script>
</body>
</html>
`;
}
