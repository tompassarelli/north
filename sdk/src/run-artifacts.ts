import * as fs from "node:fs";
import * as path from "node:path";
import { randomUUID } from "node:crypto";

import { streamDirectory } from "./stream-writer";
import {
	wireArtifactId,
	wireRunId,
	type WireArtifactId,
	type WireArtifactMaterial,
	type WireArtifactReceipt,
	type WireArtifactSink,
	type WireRunId,
} from "./wire";

export const RUN_ARTIFACT_MAX_BYTES = 2_097_152;
export const RUN_ARTIFACT_MAX_COUNT = 1_024;
export const RUN_ARTIFACT_MAX_TOTAL_BYTES = 67_108_864;
export const RUN_ARTIFACT_PAGE_DEFAULT_LIMIT = 16_384;
export const RUN_ARTIFACT_PAGE_MAX_LIMIT = 65_536;
export const RUN_ARTIFACT_PAGE_MAX_OFFSET = 2_147_483_647;
export const RUN_ARTIFACT_PAGE_MAX_ENCODED_BYTES = 524_288;

const ARTIFACT_PROTOCOL = "north.run-artifact";
const PAGE_PROTOCOL = "north.page";
const VERSION = 1;
const DIGEST_PATTERN = /^[a-f0-9]{64}$/;
const MEDIA_TYPE_PATTERN = /^[a-z0-9][a-z0-9!#$&^_.+-]*\/[a-z0-9][a-z0-9!#$&^_.+-]*(?:\s*;\s*[a-z0-9!#$&^_.+-]+=[^\r\n;]+)*$/i;
const ARTIFACT_FILE_PATTERN = /^artifact-[a-f0-9]{64}\.json$/;
const RUN_DIRECTORY_PATTERN = /^run-[a-f0-9]{64}$/;
const RUN_DIRECTORY_DOMAIN = "north.run-artifact.directory.v1";
const ARTIFACT_FILE_DOMAIN = "north.run-artifact.file.v1";
const TEXT_ENCODER = new TextEncoder();
const FATAL_DECODER = new TextDecoder("utf-8", { fatal: true });
const MAX_RECORD_BYTES = RUN_ARTIFACT_MAX_BYTES * 6 + 8_192;

export type RunArtifactErrorCode =
	| "invalid_artifact"
	| "invalid_artifact_directory"
	| "artifact_conflict"
	| "artifact_limit_exceeded"
	| "artifact_not_found"
	| "artifact_corrupt"
	| "invalid_artifact_page"
	| "invalid_utf8_offset"
	| "artifact_page_limit_too_small"
	| "stale_snapshot";

export class RunArtifactError extends Error {
	readonly code: RunArtifactErrorCode;
	readonly expected?: string;
	readonly actual?: string;

	constructor(
		code: RunArtifactErrorCode,
		message: string,
		context: { expected?: string; actual?: string } = {},
		options?: ErrorOptions,
	) {
		super(message, options);
		this.name = "RunArtifactError";
		this.code = code;
		this.expected = context.expected;
		this.actual = context.actual;
	}
}

interface RunArtifactRecord {
	readonly protocol: typeof ARTIFACT_PROTOCOL;
	readonly version: typeof VERSION;
	readonly runId: WireRunId;
	readonly artifactId: WireArtifactId;
	readonly mediaType: string;
	readonly bytes: number;
	readonly digest: string;
	readonly label?: string;
	readonly content: string;
}

export interface RunArtifactPageInput {
	readonly artifactId: string;
	readonly offset?: number;
	readonly limit?: number;
	readonly snapshot?: string;
}

export interface RunArtifactPage {
	readonly protocol: typeof PAGE_PROTOCOL;
	readonly version: typeof VERSION;
	readonly artifactId: WireArtifactId;
	readonly mediaType: string;
	readonly snapshot: string;
	readonly offset: number;
	readonly limit: number;
	readonly total: number;
	readonly nextOffset: number | null;
	readonly complete: boolean;
	readonly state: "incomplete" | "complete" | "exhausted";
	readonly content: string;
}

function sha256(value: string | Uint8Array): string {
	return new Bun.CryptoHasher("sha256").update(value).digest("hex");
}

function utf8Bytes(value: string): number {
	return TEXT_ENCODER.encode(value).byteLength;
}

function wellFormed(value: string): boolean {
	return value.isWellFormed();
}

function fsyncDirectory(directory: string): void {
	const descriptor = fs.openSync(directory, "r");
	try { fs.fsyncSync(descriptor); }
	finally { fs.closeSync(descriptor); }
}

function requireDirectory(directory: string, create = false, privateDirectory = false): string {
	if (!directory || directory.includes("\0") || !path.isAbsolute(directory)) {
		throw new RunArtifactError(
			"invalid_artifact_directory",
			"run artifact directory must be a non-empty absolute path",
		);
	}
	const resolved = path.resolve(directory);
	if (resolved === path.parse(resolved).root) {
		throw new RunArtifactError(
			"invalid_artifact_directory",
			"run artifact directory cannot be a filesystem root",
		);
	}
	if (create) fs.mkdirSync(resolved, { recursive: true, mode: 0o700 });
	let stat: fs.Stats;
	try { stat = fs.lstatSync(resolved); }
	catch (cause) {
		throw new RunArtifactError(
			"invalid_artifact_directory",
			"run artifact directory is unavailable",
			{},
			{ cause },
		);
	}
	if (!stat.isDirectory() || stat.isSymbolicLink()) {
		throw new RunArtifactError(
			"invalid_artifact_directory",
			"run artifact path must be a directory, not a symbolic link",
		);
	}
	if (create && privateDirectory) {
		fs.chmodSync(resolved, 0o700);
	}
	if (create) {
		fsyncDirectory(resolved);
		const parent = path.dirname(resolved);
		if (parent !== resolved) fsyncDirectory(parent);
	}
	return resolved;
}

function runDirectoryName(runId: string): string {
	return `run-${sha256(`${RUN_DIRECTORY_DOMAIN}\0${runId}`)}`;
}

function artifactFileName(artifactId: string): string {
	return `artifact-${sha256(`${ARTIFACT_FILE_DOMAIN}\0${artifactId}`)}.json`;
}

function artifactPath(directory: string, artifactId: string): string {
	const filePath = path.resolve(directory, artifactFileName(artifactId));
	if (path.dirname(filePath) !== directory) {
		throw new RunArtifactError("invalid_artifact_directory", "artifact path escapes its run directory");
	}
	return filePath;
}

function safeFileText(filePath: string): string {
	let before: fs.Stats;
	try { before = fs.lstatSync(filePath); }
	catch (cause) {
		if ((cause as NodeJS.ErrnoException).code === "ENOENT") {
			throw new RunArtifactError("artifact_not_found", "run artifact does not exist", {}, { cause });
		}
		throw cause;
	}
	if (!before.isFile() || before.isSymbolicLink() || before.size > MAX_RECORD_BYTES) {
		throw new RunArtifactError("artifact_corrupt", "run artifact record is unsafe");
	}
	let descriptor: number;
	try {
		descriptor = fs.openSync(
			filePath,
			fs.constants.O_RDONLY | (fs.constants.O_NOFOLLOW ?? 0),
		);
	} catch (cause) {
		throw new RunArtifactError("artifact_corrupt", "run artifact record cannot be opened safely", {}, { cause });
	}
	try {
		const opened = fs.fstatSync(descriptor);
		if (!opened.isFile() || opened.size !== before.size || opened.ino !== before.ino
			|| opened.dev !== before.dev || opened.size > MAX_RECORD_BYTES) {
			throw new RunArtifactError("artifact_corrupt", "run artifact record changed during lookup");
		}
		return fs.readFileSync(descriptor, "utf8");
	} finally {
		fs.closeSync(descriptor);
	}
}

function exactKeys(source: Record<string, unknown>, allowed: readonly string[]): boolean {
	const keys = Object.keys(source);
	return keys.length === allowed.length && keys.every((key) => allowed.includes(key));
}

function parseRecord(serialized: string, expectedDirectory: string): RunArtifactRecord {
	let value: unknown;
	try { value = JSON.parse(serialized); }
	catch (cause) {
		throw new RunArtifactError("artifact_corrupt", "run artifact record is not valid JSON", {}, { cause });
	}
	if (!value || typeof value !== "object" || Array.isArray(value)) {
		throw new RunArtifactError("artifact_corrupt", "run artifact record must be an object");
	}
	const source = value as Record<string, unknown>;
	const allowed = [
		"protocol", "version", "runId", "artifactId", "mediaType", "bytes", "digest", "content",
		...(source.label === undefined ? [] : ["label"]),
	];
	if (!exactKeys(source, allowed)
		|| source.protocol !== ARTIFACT_PROTOCOL || source.version !== VERSION
		|| typeof source.runId !== "string" || typeof source.artifactId !== "string"
		|| typeof source.mediaType !== "string" || !wellFormed(source.mediaType)
		|| utf8Bytes(source.mediaType) > 256 || !MEDIA_TYPE_PATTERN.test(source.mediaType)
		|| typeof source.bytes !== "number" || !Number.isSafeInteger(source.bytes) || source.bytes < 0
		|| typeof source.digest !== "string" || !DIGEST_PATTERN.test(source.digest)
		|| typeof source.content !== "string" || !wellFormed(source.content)
		|| (source.label !== undefined
			&& (typeof source.label !== "string" || !wellFormed(source.label)))) {
		throw new RunArtifactError("artifact_corrupt", "run artifact record has an invalid shape");
	}
	let runId: WireRunId;
	let artifactId: WireArtifactId;
	try {
		runId = wireRunId(source.runId);
		artifactId = wireArtifactId(source.artifactId);
	} catch (cause) {
		throw new RunArtifactError("artifact_corrupt", "run artifact record has invalid identities", {}, { cause });
	}
	if (path.basename(expectedDirectory) !== runDirectoryName(runId)
		|| path.basename(path.dirname(expectedDirectory)) !== "run-artifacts") {
		throw new RunArtifactError("artifact_corrupt", "run artifact record belongs to another run directory");
	}
	const bytes = utf8Bytes(source.content);
	if (bytes !== source.bytes || bytes > RUN_ARTIFACT_MAX_BYTES || sha256(source.content) !== source.digest
		|| (source.label !== undefined && utf8Bytes(source.label as string) > 512)) {
		throw new RunArtifactError("artifact_corrupt", "run artifact record failed integrity validation");
	}
	return Object.freeze({
		protocol: ARTIFACT_PROTOCOL,
		version: VERSION,
		runId,
		artifactId,
		mediaType: source.mediaType,
		bytes,
		digest: source.digest,
		...(source.label === undefined ? {} : { label: source.label as string }),
		content: source.content,
	});
}

function readRecord(directory: string, artifactId: WireArtifactId): RunArtifactRecord {
	const record = parseRecord(safeFileText(artifactPath(directory, artifactId)), directory);
	if (record.artifactId !== artifactId) {
		throw new RunArtifactError("artifact_corrupt", "run artifact lookup identity does not match its record");
	}
	return record;
}

function writeOnce(filePath: string, serialized: string): boolean {
	const directory = path.dirname(filePath);
	const temporary = `${filePath}.${process.pid}.${randomUUID()}.tmp`;
	const descriptor = fs.openSync(temporary, "wx", 0o600);
	try {
		fs.writeFileSync(descriptor, serialized, "utf8");
		fs.fsyncSync(descriptor);
	} finally {
		fs.closeSync(descriptor);
	}
	try {
		try {
			fs.linkSync(temporary, filePath);
			const published = fs.openSync(filePath, "r");
			try { fs.fsyncSync(published); }
			finally { fs.closeSync(published); }
			fsyncDirectory(directory);
			return true;
		} catch (cause) {
			if ((cause as NodeJS.ErrnoException).code !== "EEXIST") throw cause;
			if (safeFileText(filePath) !== serialized) {
				throw new RunArtifactError("artifact_conflict", "artifact identity already has different material");
			}
			return false;
		}
	} finally {
		try { fs.unlinkSync(temporary); }
		catch (cause) {
			if ((cause as NodeJS.ErrnoException).code !== "ENOENT") throw cause;
		}
		fsyncDirectory(directory);
	}
}

function validatePageInput(input: RunArtifactPageInput): {
	artifactId: WireArtifactId;
	offset: number;
	limit: number;
	snapshot?: string;
} {
	let artifactId: WireArtifactId;
	try { artifactId = wireArtifactId(input.artifactId); }
	catch (cause) {
		throw new RunArtifactError("invalid_artifact_page", "artifactId must be a valid Wire artifact ID", {}, { cause });
	}
	const offset = input.offset ?? 0;
	const limit = input.limit ?? RUN_ARTIFACT_PAGE_DEFAULT_LIMIT;
	if (!Number.isInteger(offset) || offset < 0 || offset > RUN_ARTIFACT_PAGE_MAX_OFFSET) {
		throw new RunArtifactError("invalid_artifact_page", "artifact page offset is out of bounds");
	}
	if (!Number.isInteger(limit) || limit < 1 || limit > RUN_ARTIFACT_PAGE_MAX_LIMIT) {
		throw new RunArtifactError("invalid_artifact_page", "artifact page limit is out of bounds");
	}
	if (input.snapshot !== undefined && !DIGEST_PATTERN.test(input.snapshot)) {
		throw new RunArtifactError("invalid_artifact_page", "artifact page snapshot is invalid");
	}
	return { artifactId, offset, limit, ...(input.snapshot === undefined ? {} : { snapshot: input.snapshot }) };
}

function utf8Page(bytes: Uint8Array, offset: number, limit: number): { content: string; end: number } {
	if (offset < bytes.byteLength) {
		try { FATAL_DECODER.decode(bytes.subarray(0, offset)); }
		catch (cause) {
			throw new RunArtifactError("invalid_utf8_offset", "artifact page offset splits a UTF-8 sequence", {}, { cause });
		}
	}
	let end = Math.min(bytes.byteLength, offset + limit);
	while (end > offset) {
		try { return { content: FATAL_DECODER.decode(bytes.subarray(offset, end)), end }; }
		catch { end -= 1; }
	}
	if (offset < bytes.byteLength) {
		throw new RunArtifactError(
			"artifact_page_limit_too_small",
			"artifact page limit cannot include the next complete UTF-8 character",
		);
	}
	return { content: "", end: offset };
}

export class RunArtifactStore implements WireArtifactSink {
	readonly runId: WireRunId;
	readonly directory: string;

	constructor(runId: string) {
		this.runId = wireRunId(runId);
		const root = requireDirectory(streamDirectory(), true);
		const artifactsRoot = requireDirectory(path.join(root, "run-artifacts"), true, true);
		this.directory = requireDirectory(
			path.join(artifactsRoot, runDirectoryName(this.runId)),
			true,
			true,
		);
	}

	persist(artifact: Readonly<WireArtifactMaterial>): WireArtifactReceipt {
		let artifactId: WireArtifactId;
		try { artifactId = wireArtifactId(artifact.artifactId); }
		catch (cause) {
			throw new RunArtifactError("invalid_artifact", "artifact ID is invalid", {}, { cause });
		}
		if (typeof artifact.content !== "string" || !wellFormed(artifact.content)
			|| typeof artifact.mediaType !== "string" || !wellFormed(artifact.mediaType)
			|| utf8Bytes(artifact.mediaType) > 256 || !MEDIA_TYPE_PATTERN.test(artifact.mediaType)
			|| typeof artifact.digest !== "string" || !DIGEST_PATTERN.test(artifact.digest)
			|| (artifact.label !== undefined
				&& (typeof artifact.label !== "string" || !wellFormed(artifact.label)
					|| utf8Bytes(artifact.label) > 512))) {
			throw new RunArtifactError("invalid_artifact", "artifact material has an invalid shape");
		}
		const bytes = utf8Bytes(artifact.content);
		if (bytes > RUN_ARTIFACT_MAX_BYTES) {
			throw new RunArtifactError("artifact_limit_exceeded", "artifact exceeds the per-artifact byte limit");
		}
		if (sha256(artifact.content) !== artifact.digest) {
			throw new RunArtifactError("invalid_artifact", "artifact digest does not match its content");
		}
		const record: RunArtifactRecord = Object.freeze({
			protocol: ARTIFACT_PROTOCOL,
			version: VERSION,
			runId: this.runId,
			artifactId,
			mediaType: artifact.mediaType,
			bytes,
			digest: artifact.digest,
			...(artifact.label === undefined ? {} : { label: artifact.label }),
			content: artifact.content,
		});
		const serialized = JSON.stringify(record);
		const target = artifactPath(this.directory, artifactId);
		try {
			const existing = readRecord(this.directory, artifactId);
			if (JSON.stringify(existing) !== serialized) {
				throw new RunArtifactError("artifact_conflict", "artifact identity already has different material");
			}
			return Object.freeze({ artifactId, digest: artifact.digest });
		} catch (cause) {
			if (!(cause instanceof RunArtifactError) || cause.code !== "artifact_not_found") throw cause;
		}
		let count = 0;
		let totalBytes = 0;
		for (const entry of fs.readdirSync(this.directory, { withFileTypes: true })) {
			if (!ARTIFACT_FILE_PATTERN.test(entry.name)) continue;
			if (!entry.isFile() || entry.isSymbolicLink()) {
				throw new RunArtifactError("artifact_corrupt", "run artifact directory contains an unsafe record");
			}
			const existing = parseRecord(safeFileText(path.join(this.directory, entry.name)), this.directory);
			count += 1;
			totalBytes += existing.bytes;
		}
		if (count >= RUN_ARTIFACT_MAX_COUNT || totalBytes + bytes > RUN_ARTIFACT_MAX_TOTAL_BYTES) {
			throw new RunArtifactError("artifact_limit_exceeded", "run artifact store exceeds its bounded capacity");
		}
		writeOnce(target, serialized);
		return Object.freeze({ artifactId, digest: artifact.digest });
	}
}

export function readRunArtifactPage(
	directory: string,
	input: RunArtifactPageInput,
): RunArtifactPage {
	const confinedDirectory = requireDirectory(directory);
	if (!RUN_DIRECTORY_PATTERN.test(path.basename(confinedDirectory))
		|| path.basename(path.dirname(confinedDirectory)) !== "run-artifacts") {
		throw new RunArtifactError("invalid_artifact_directory", "artifact lookup is not scoped to one run");
	}
	const { artifactId, offset, limit, snapshot } = validatePageInput(input);
	const record = readRecord(confinedDirectory, artifactId);
	if (snapshot !== undefined && snapshot !== record.digest) {
		throw new RunArtifactError(
			"stale_snapshot",
			"artifact continuation snapshot is stale",
			{ expected: snapshot, actual: record.digest },
		);
	}
	const bytes = TEXT_ENCODER.encode(record.content);
	const exhausted = bytes.byteLength > 0 && offset >= bytes.byteLength;
	const page = exhausted
		? { content: "", end: offset }
		: utf8Page(bytes, offset, limit);
	const complete = page.end >= bytes.byteLength;
	const result: RunArtifactPage = Object.freeze({
		protocol: PAGE_PROTOCOL,
		version: VERSION,
		artifactId,
		mediaType: record.mediaType,
		snapshot: record.digest,
		offset,
		limit,
		total: record.bytes,
		nextOffset: complete ? null : page.end,
		complete,
		state: exhausted ? "exhausted" : complete ? "complete" : "incomplete",
		content: page.content,
	});
	if (utf8Bytes(JSON.stringify(result)) > RUN_ARTIFACT_PAGE_MAX_ENCODED_BYTES) {
		throw new RunArtifactError("artifact_limit_exceeded", "encoded artifact page exceeds its response limit");
	}
	return result;
}
