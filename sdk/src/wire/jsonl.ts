import { dlopen, FFIType, type Library } from "bun:ffi";
import * as fs from "node:fs/promises";
import * as path from "node:path";

import { decodeWireEvent } from "./decode";
import {
	WIRE_MAX_EVENTS_PER_RUN,
	WIRE_MAX_STREAM_BYTES,
	type WireEvent,
} from "./events";
import { WIRE_JSON_MAX_BYTES, type JsonObject, type JsonValue } from "./json";
import { reduceWireEvent, type WireRunSnapshot } from "./reducer";

export const WIRE_JSONL_MAX_LINE_BYTES = WIRE_JSON_MAX_BYTES;
export const WIRE_JSONL_MAX_STREAM_BYTES = WIRE_MAX_STREAM_BYTES;
export const WIRE_JSONL_MAX_EVENTS = WIRE_MAX_EVENTS_PER_RUN;
export const WIRE_JSONL_LOCK_VERSION = "north:wire-jsonl-lock:v2" as const;
export const WIRE_JSONL_MAX_LOCK_BYTES = 16_384;

export type WireJsonlErrorCode =
	| "malformed"
	| "torn"
	| "oversized"
	| "noncanonical"
	| "noncontiguous"
	| "mixed_run"
	| "post_terminal"
	| "writer_locked"
	| "writer_closed"
	| "rotation_refused"
	| "concurrent_modification"
	| "io";

export interface WireJsonlLockOwner {
	readonly version: typeof WIRE_JSONL_LOCK_VERSION;
	readonly token: string;
	readonly pid: number;
	readonly process: WireJsonlProcessIdentity;
	readonly createdAt: string;
	readonly filePath: string;
}

export interface WireJsonlProcessIdentity {
	readonly bootId: string;
	readonly startTicks: string;
}

export type WireJsonlLockEvidence =
	| { readonly status: "owner"; readonly owner: WireJsonlLockOwner }
	| { readonly status: "malformed"; readonly bytes: number; readonly reason: string }
	| { readonly status: "unreadable"; readonly reason: string };

export interface WireJsonlErrorContext {
	line?: number;
	byteOffset?: number;
	limit?: number;
	observed?: number;
	lockPath?: string;
	lockEvidence?: WireJsonlLockEvidence;
}

export class WireJsonlError extends Error {
	readonly code: WireJsonlErrorCode;
	readonly line?: number;
	readonly byteOffset?: number;
	readonly limit?: number;
	readonly observed?: number;
	readonly lockPath?: string;
	readonly lockEvidence?: WireJsonlLockEvidence;

	constructor(
		code: WireJsonlErrorCode,
		message: string,
		context: WireJsonlErrorContext = {},
		options?: ErrorOptions,
	) {
		super(message, options);
		this.name = "WireJsonlError";
		this.code = code;
		this.line = context.line;
		this.byteOffset = context.byteOffset;
		this.limit = context.limit;
		this.observed = context.observed;
		this.lockPath = context.lockPath;
		this.lockEvidence = context.lockEvidence;
	}
}

export interface WireJsonlLineOptions {
	maxLineBytes?: number;
}

export interface WireJsonlOptions extends WireJsonlLineOptions {
	maxStreamBytes?: number;
	maxEvents?: number;
}

export interface WireJsonlOpenOptions extends WireJsonlOptions {
	/**
	 * Archive a prior terminal stream before opening a fresh stream at `filePath`.
	 * The archive must be a new path in the same directory. Rotation happens while
	 * the stable path's adjacent writer lock remains held.
	 */
	rotateExistingTo?: string;
	/** Bridge crash recovery only. Generic JSONL writers leave stale locks fail-closed. */
	recoverDeadOwnerLock?: boolean;
}

export interface WireJsonlReplay {
	readonly events: readonly WireEvent[];
	readonly snapshot?: WireRunSnapshot;
	readonly bytes: number;
}

export interface WireJsonlWriter {
	readonly filePath: string;
	readonly lockPath: string;
	append(event: WireEvent): Promise<WireEvent>;
	replay(): WireJsonlReplay;
	close(): Promise<void>;
	[Symbol.asyncDispose](): Promise<void>;
}

interface ResolvedWireJsonlOptions {
	maxLineBytes: number;
	maxStreamBytes: number;
	maxEvents: number;
}

interface LineContext {
	line?: number;
	byteOffset?: number;
}

interface HeldWireJsonlLock {
	handle: fs.FileHandle;
	path: string;
	owner: WireJsonlLockOwner;
}

interface HeldWireJsonlRecoveryGuard {
	handle: fs.FileHandle;
	path: string;
	identity: WireJsonlFileIdentity;
}

interface WireJsonlFileIdentity {
	dev: number;
	ino: number;
}

interface WireJsonlFileObservation extends WireJsonlFileIdentity {
	size: number;
	mtimeMs: number;
	ctimeMs: number;
}

interface WireJsonlLockObservation extends WireJsonlFileIdentity {
	readonly evidence: WireJsonlLockEvidence;
}

interface OpenWireJsonlDataFile {
	handle: fs.FileHandle;
	identity: WireJsonlFileIdentity;
}

const TEXT_ENCODER = new TextEncoder();
const LINUX_FLOCK_EXCLUSIVE_NONBLOCKING = 2 | 4;
const LINUX_FLOCK_UNLOCK = 8;
const LINUX_FLOCK_SYMBOLS = {
	flock: {
		args: [FFIType.i32, FFIType.i32],
		returns: FFIType.i32,
	},
} as const;
let linuxLibc: Library<typeof LINUX_FLOCK_SYMBOLS> | undefined;

function positiveLimit(value: number | undefined, fallback: number, label: string): number {
	const resolved = value ?? fallback;
	if (!Number.isSafeInteger(resolved) || resolved <= 0) {
		throw new RangeError(`${label} must be a positive safe integer`);
	}
	return resolved;
}

function resolveOptions(options: WireJsonlOptions = {}): ResolvedWireJsonlOptions {
	return {
		maxLineBytes: positiveLimit(options.maxLineBytes, WIRE_JSONL_MAX_LINE_BYTES, "maxLineBytes"),
		maxStreamBytes: positiveLimit(options.maxStreamBytes, WIRE_JSONL_MAX_STREAM_BYTES, "maxStreamBytes"),
		maxEvents: positiveLimit(options.maxEvents, WIRE_JSONL_MAX_EVENTS, "maxEvents"),
	};
}

function canonicalJson(value: JsonValue): string {
	if (value === null || typeof value !== "object") {
		const encoded = JSON.stringify(value);
		if (encoded === undefined) throw new TypeError("JSON primitive could not be encoded");
		return encoded;
	}
	if (Array.isArray(value)) return `[${value.map(canonicalJson).join(",")}]`;
	const entries = Object.entries(value as JsonObject)
		.sort(([left], [right]) => left < right ? -1 : left > right ? 1 : 0);
	return `{${entries.map(([key, item]) =>
		`${JSON.stringify(key)}:${canonicalJson(item)}`
	).join(",")}}`;
}

function lineError(
	code: WireJsonlErrorCode,
	message: string,
	context: LineContext,
	options?: ErrorOptions,
): WireJsonlError {
	return new WireJsonlError(code, message, context, options);
}

function encodedBytes(value: string): number {
	return TEXT_ENCODER.encode(value).byteLength;
}

function assertLineBound(bytes: number, maxLineBytes: number, context: LineContext): void {
	if (bytes <= maxLineBytes) return;
	throw new WireJsonlError(
		"oversized",
		`wire JSONL line is ${bytes} bytes, exceeding the ${maxLineBytes}-byte limit`,
		{ ...context, limit: maxLineBytes, observed: bytes },
	);
}

function canonicalEventJson(event: WireEvent): { event: WireEvent; json: string; bytes: number } {
	const decoded = decodeWireEvent(event);
	const json = canonicalJson(decoded as unknown as JsonValue);
	return { event: decoded, json, bytes: encodedBytes(json) };
}

export function encodeWireJsonlLine(
	event: WireEvent,
	options: WireJsonlLineOptions = {},
): string {
	const maxLineBytes = positiveLimit(
		options.maxLineBytes,
		WIRE_JSONL_MAX_LINE_BYTES,
		"maxLineBytes",
	);
	const canonical = canonicalEventJson(event);
	assertLineBound(canonical.bytes, maxLineBytes, {});
	return `${canonical.json}\n`;
}

function decodeLine(
	line: string,
	maxLineBytes: number,
	context: LineContext,
): WireEvent {
	if (!line.endsWith("\n")) {
		throw lineError("torn", "wire JSONL line is missing its final LF", context);
	}
	const json = line.slice(0, -1);
	if (json.length === 0 || json.includes("\n") || json.includes("\r")) {
		throw lineError("malformed", "wire JSONL line must contain exactly one JSON event", context);
	}
	const bytes = encodedBytes(json);
	assertLineBound(bytes, maxLineBytes, context);

	let parsed: unknown;
	try {
		parsed = JSON.parse(json);
	} catch (error) {
		throw lineError("malformed", "wire JSONL line is not valid JSON", context, { cause: error });
	}

	let event: WireEvent;
	try {
		event = decodeWireEvent(parsed);
	} catch (error) {
		throw lineError("malformed", "wire JSONL line is not a valid wire event", context, { cause: error });
	}
	const canonical = canonicalEventJson(event);
	if (canonical.json !== json) {
		throw lineError("noncanonical", "wire JSONL line is not canonically encoded", context);
	}
	return event;
}

export function decodeWireJsonlLine(
	line: string,
	options: WireJsonlLineOptions = {},
): WireEvent {
	const maxLineBytes = positiveLimit(
		options.maxLineBytes,
		WIRE_JSONL_MAX_LINE_BYTES,
		"maxLineBytes",
	);
	return decodeLine(line, maxLineBytes, {});
}

function terminal(snapshot: WireRunSnapshot): boolean {
	return snapshot.lifecycle === "completed"
		|| snapshot.lifecycle === "failed"
		|| snapshot.lifecycle === "cancelled"
		|| snapshot.lifecycle === "blocked";
}

function advanceSnapshot(
	snapshot: WireRunSnapshot | undefined,
	event: WireEvent,
	context: LineContext,
): WireRunSnapshot {
	if (snapshot) {
		if (terminal(snapshot)) {
			throw lineError("post_terminal", "wire event arrived after run termination", context);
		}
		if (event.runId !== snapshot.runId) {
			throw lineError(
				"mixed_run",
				`wire event belongs to run ${event.runId}, expected ${snapshot.runId}`,
				context,
			);
		}
		if (event.sequence !== snapshot.lastSequence + 1) {
			throw lineError(
				"noncontiguous",
				`wire event sequence ${event.sequence} is not contiguous after ${snapshot.lastSequence}`,
				context,
			);
		}
	} else if (event.sequence !== 0) {
		throw lineError(
			"noncontiguous",
			`the first wire event sequence must be zero, received ${event.sequence}`,
			context,
		);
	}

	try {
		return reduceWireEvent(snapshot, event);
	} catch (error) {
		throw lineError("malformed", "wire JSONL event violates the wire stream contract", context, { cause: error });
	}
}

function frozenReplay(
	events: readonly WireEvent[],
	snapshot: WireRunSnapshot | undefined,
	bytes: number,
): WireJsonlReplay {
	return Object.freeze({
		events: Object.freeze([...events]),
		...(snapshot === undefined ? {} : { snapshot }),
		bytes,
	});
}

function decodeWireJsonlWithOptions(
	source: string,
	bytes: number,
	options: ResolvedWireJsonlOptions,
): WireJsonlReplay {
	if (bytes > options.maxStreamBytes) {
		throw new WireJsonlError(
			"oversized",
			`wire JSONL stream is ${bytes} bytes, exceeding the ${options.maxStreamBytes}-byte limit`,
			{ limit: options.maxStreamBytes, observed: bytes },
		);
	}
	if (source.length === 0) return frozenReplay([], undefined, 0);
	if (!source.endsWith("\n")) {
		throw new WireJsonlError("torn", "wire JSONL stream ends with a torn line", {
			byteOffset: bytes,
		});
	}

	const events: WireEvent[] = [];
	const eventIds = new Set<string>();
	let snapshot: WireRunSnapshot | undefined;
	let start = 0;
	let lineNumber = 1;
	let byteOffset = 0;
	while (start < source.length) {
		if (events.length >= options.maxEvents) {
			throw new WireJsonlError(
				"oversized",
				`wire JSONL stream exceeds the ${options.maxEvents}-event limit`,
				{ line: lineNumber, byteOffset, limit: options.maxEvents, observed: events.length + 1 },
			);
		}
		const end = source.indexOf("\n", start);
		const line = source.slice(start, end + 1);
		const context = { line: lineNumber, byteOffset };
		const event = decodeLine(line, options.maxLineBytes, context);
		if (eventIds.has(event.id)) {
			throw lineError(
				"noncontiguous",
				`wire event id ${event.id} is duplicated`,
				context,
			);
		}
		snapshot = advanceSnapshot(snapshot, event, context);
		events.push(event);
		eventIds.add(event.id);
		byteOffset += encodedBytes(line);
		start = end + 1;
		lineNumber += 1;
	}
	return frozenReplay(events, snapshot, bytes);
}

export function decodeWireJsonl(
	source: string,
	options: WireJsonlOptions = {},
): WireJsonlReplay {
	const resolved = resolveOptions(options);
	return decodeWireJsonlWithOptions(source, encodedBytes(source), resolved);
}

export async function readWireJsonl(
	filePath: string,
	options: WireJsonlOptions = {},
): Promise<WireJsonlReplay> {
	const resolved = resolveOptions(options);
	const dataFile = await openWireJsonlReadFile(filePath);
	let replay: WireJsonlReplay | undefined;
	let failure: unknown;
	try {
		replay = await readHeldWireJsonl(filePath, dataFile, resolved);
	} catch (error) {
		failure = error;
	}
	try { await dataFile.handle.close(); }
	catch (error) { failure ??= error; }
	if (failure !== undefined) {
		if (failure instanceof WireJsonlError) throw failure;
		throw new WireJsonlError(
			"io",
			`failed to read wire JSONL file ${filePath}`,
			{},
			{ cause: failure },
		);
	}
	if (!replay) {
		throw new WireJsonlError("io", `wire JSONL read produced no replay: ${filePath}`);
	}
	return replay;
}

function decodeWireJsonlChunks(
	chunks: readonly Uint8Array[],
	bytes: number,
	resolved: ResolvedWireJsonlOptions,
): WireJsonlReplay {
	const contents = new Uint8Array(bytes);
	let offset = 0;
	for (const chunk of chunks) {
		contents.set(chunk, offset);
		offset += chunk.byteLength;
	}
	let source: string;
	try {
		source = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(contents);
	} catch (error) {
		throw new WireJsonlError("malformed", "wire JSONL file is not valid UTF-8", {}, { cause: error });
	}
	return decodeWireJsonlWithOptions(source, bytes, resolved);
}

function errorCode(error: unknown): string | undefined {
	if (error === null || typeof error !== "object" || !("code" in error)) return undefined;
	return typeof error.code === "string" ? error.code : undefined;
}

function errorMessage(error: unknown): string {
	return error instanceof Error ? error.message : String(error);
}

function sameFileIdentity(
	left: WireJsonlFileIdentity,
	right: WireJsonlFileIdentity,
): boolean {
	return left.dev === right.dev && left.ino === right.ino;
}

function linuxFlock(fd: number, operation: number): number {
	if (process.platform !== "linux") {
		throw new Error("wire JSONL dead-owner recovery requires Linux flock");
	}
	linuxLibc ??= dlopen("libc.so.6", LINUX_FLOCK_SYMBOLS);
	return linuxLibc.symbols.flock(fd, operation);
}

async function observeHeldRegularPath(
	filePath: string,
	handle: fs.FileHandle,
	expected?: WireJsonlFileIdentity,
): Promise<WireJsonlFileIdentity> {
	const [held, current] = await Promise.all([
		handle.stat(),
		fs.lstat(filePath),
	]);
	const identity = { dev: held.dev, ino: held.ino };
	if (!held.isFile() || !current.isFile()
		|| held.nlink !== 1 || current.nlink !== 1
		|| !sameFileIdentity(identity, current)
		|| (expected !== undefined && !sameFileIdentity(identity, expected))) {
		throw new Error(`held regular-file identity changed: ${filePath}`);
	}
	return identity;
}

async function acquireWireJsonlRecoveryGuard(lockPath: string): Promise<HeldWireJsonlRecoveryGuard> {
	const guardPath = `${lockPath}.recovery`;
	let handle: fs.FileHandle | undefined;
	let created = false;
	try {
		for (let attempt = 0; attempt < 3 && handle === undefined; attempt += 1) {
			try {
				handle = await fs.open(
					guardPath,
					fs.constants.O_RDWR | fs.constants.O_NOFOLLOW | fs.constants.O_NONBLOCK,
				);
			} catch (error) {
				if (errorCode(error) !== "ENOENT") throw error;
				try {
					handle = await fs.open(
						guardPath,
						fs.constants.O_RDWR
							| fs.constants.O_CREAT
							| fs.constants.O_EXCL
							| fs.constants.O_NOFOLLOW
							| fs.constants.O_NONBLOCK,
						0o600,
					);
					created = true;
				} catch (createError) {
					if (errorCode(createError) !== "EEXIST" || attempt === 2) throw createError;
				}
			}
		}
		if (!handle) throw new Error("wire JSONL recovery guard could not be opened");
		const identity = await observeHeldRegularPath(guardPath, handle);
		if (created) {
			await handle.sync();
			await syncDirectory(path.dirname(guardPath));
		}
		if (linuxFlock(handle.fd, LINUX_FLOCK_EXCLUSIVE_NONBLOCKING) !== 0) {
			throw new WireJsonlError(
				"writer_locked",
				`wire JSONL dead-owner recovery is already active: ${guardPath}`,
				{ lockPath },
			);
		}
		await observeHeldRegularPath(guardPath, handle, identity);
		return { handle, path: guardPath, identity };
	} catch (error) {
		try { await handle?.close(); }
		catch { /* The acquisition failure remains load-bearing. */ }
		if (error instanceof WireJsonlError) throw error;
		throw new WireJsonlError(
			"concurrent_modification",
			`failed to acquire wire JSONL recovery guard: ${guardPath}`,
			{ lockPath },
			{ cause: error },
		);
	}
}

async function assertWireJsonlRecoveryGuard(guard: HeldWireJsonlRecoveryGuard): Promise<void> {
	try {
		await observeHeldRegularPath(guard.path, guard.handle, guard.identity);
	} catch (error) {
		throw new WireJsonlError(
			"concurrent_modification",
			`wire JSONL recovery guard identity changed: ${guard.path}`,
			{ lockPath: guard.path.slice(0, -".recovery".length) },
			{ cause: error },
		);
	}
}

async function releaseWireJsonlRecoveryGuard(guard: HeldWireJsonlRecoveryGuard): Promise<void> {
	let failure: unknown;
	try {
		await assertWireJsonlRecoveryGuard(guard);
		if (linuxFlock(guard.handle.fd, LINUX_FLOCK_UNLOCK) !== 0) {
			throw new Error("Linux flock unlock failed");
		}
	} catch (error) {
		failure = error;
	}
	try { await guard.handle.close(); }
	catch (error) { failure ??= error; }
	if (failure !== undefined) {
		if (failure instanceof WireJsonlError) throw failure;
		throw new WireJsonlError(
			"io",
			`failed to release wire JSONL recovery guard: ${guard.path}`,
			{ lockPath: guard.path.slice(0, -".recovery".length) },
			{ cause: failure },
		);
	}
}

async function observeWireJsonlDataFile(
	filePath: string,
	handle: fs.FileHandle,
	expected?: WireJsonlFileIdentity,
): Promise<WireJsonlFileObservation> {
	const [held, current] = await Promise.all([
		handle.stat(),
		fs.lstat(filePath),
	]).catch((error: unknown) => {
		throw new WireJsonlError(
			"concurrent_modification",
			`failed to prove wire JSONL stable-file identity: ${filePath}`,
			{},
			{ cause: error },
		);
	});
	const identity = { dev: held.dev, ino: held.ino };
	if (!held.isFile() || !current.isFile()
		|| held.nlink !== 1 || current.nlink !== 1
		|| !sameFileIdentity(identity, current)
		|| (expected !== undefined && !sameFileIdentity(identity, expected))) {
		throw new WireJsonlError(
			"concurrent_modification",
			`wire JSONL stable path is not the held regular file: ${filePath}`,
		);
	}
	return {
		...identity,
		size: held.size,
		mtimeMs: held.mtimeMs,
		ctimeMs: held.ctimeMs,
	};
}

function unsafeOpenRace(error: unknown): boolean {
	const code = errorCode(error);
	return code === "EEXIST" || code === "ELOOP" || code === "ENOENT"
		|| code === "ENXIO" || code === "ENOTDIR";
}

async function openWireJsonlDataFile(
	filePath: string,
	requireNew = false,
): Promise<OpenWireJsonlDataFile> {
	const initial = await fs.lstat(filePath).catch((error: unknown) => {
		if (errorCode(error) !== "ENOENT") {
			throw new WireJsonlError(
				"io",
				`failed to inspect wire JSONL stable path ${filePath}`,
				{},
				{ cause: error },
			);
		}
		return undefined;
	});
	if (initial !== undefined && (!initial.isFile() || requireNew)) {
		throw new WireJsonlError(
			"concurrent_modification",
			`wire JSONL stable path must be a ${requireNew ? "new " : ""}regular file: ${filePath}`,
		);
	}

	const create = initial === undefined;
	const flags = fs.constants.O_RDWR
		| fs.constants.O_APPEND
		| fs.constants.O_NOFOLLOW
		| fs.constants.O_NONBLOCK
		| (create ? fs.constants.O_CREAT | fs.constants.O_EXCL : 0);
	let handle: fs.FileHandle;
	try {
		handle = await fs.open(filePath, flags, 0o600);
	} catch (error) {
		throw new WireJsonlError(
			unsafeOpenRace(error) ? "concurrent_modification" : "io",
			`failed to securely open wire JSONL stable file ${filePath}`,
			{},
			{ cause: error },
		);
	}

	try {
		const observed = await observeWireJsonlDataFile(filePath, handle, initial);
		return {
			handle,
			identity: { dev: observed.dev, ino: observed.ino },
		};
	} catch (error) {
		try { await handle.close(); }
		catch { /* The identity failure remains load-bearing. */ }
		throw error;
	}
}

async function openWireJsonlReadFile(filePath: string): Promise<OpenWireJsonlDataFile> {
	let handle: fs.FileHandle;
	try {
		handle = await fs.open(
			filePath,
			fs.constants.O_RDONLY | fs.constants.O_NOFOLLOW | fs.constants.O_NONBLOCK,
		);
	} catch (error) {
		throw new WireJsonlError(
			unsafeOpenRace(error) ? "concurrent_modification" : "io",
			`failed to securely open wire JSONL file for reading ${filePath}`,
			{},
			{ cause: error },
		);
	}

	try {
		const observed = await observeWireJsonlDataFile(filePath, handle);
		return {
			handle,
			identity: { dev: observed.dev, ino: observed.ino },
		};
	} catch (error) {
		try { await handle.close(); }
		catch { /* The identity failure remains load-bearing. */ }
		throw error;
	}
}

async function readHeldWireJsonl(
	filePath: string,
	dataFile: OpenWireJsonlDataFile,
	resolved: ResolvedWireJsonlOptions,
): Promise<WireJsonlReplay> {
	const before = await observeWireJsonlDataFile(
		filePath,
		dataFile.handle,
		dataFile.identity,
	);
	const chunks: Uint8Array[] = [];
	let bytes = 0;
	try {
		while (true) {
			const capacity = Math.min(64 * 1_024, resolved.maxStreamBytes - bytes + 1);
			const chunk = new Uint8Array(capacity);
			const result = await dataFile.handle.read(chunk, 0, capacity, bytes);
			if (result.bytesRead === 0) break;
			bytes += result.bytesRead;
			if (bytes > resolved.maxStreamBytes) {
				throw new WireJsonlError(
					"oversized",
					`wire JSONL stream exceeds the ${resolved.maxStreamBytes}-byte limit`,
					{ limit: resolved.maxStreamBytes, observed: bytes },
				);
			}
			chunks.push(chunk.slice(0, result.bytesRead));
		}
	} catch (error) {
		if (error instanceof WireJsonlError) throw error;
		throw new WireJsonlError(
			"io",
			`failed to read held wire JSONL file ${filePath}`,
			{},
			{ cause: error },
		);
	}
	const after = await observeWireJsonlDataFile(
		filePath,
		dataFile.handle,
		dataFile.identity,
	);
	if (before.size !== bytes || after.size !== bytes
		|| before.mtimeMs !== after.mtimeMs || before.ctimeMs !== after.ctimeMs) {
		throw new WireJsonlError(
			"concurrent_modification",
			`wire JSONL stable file changed while it was read: ${filePath}`,
		);
	}
	return decodeWireJsonlChunks(chunks, bytes, resolved);
}

async function linuxBootId(): Promise<string> {
	const bootId = (await Bun.file("/proc/sys/kernel/random/boot_id").text()).trim();
	if (!/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(bootId)) {
		throw new Error("Linux boot identity is malformed");
	}
	return bootId;
}

function linuxProcessStartTicks(stat: string): string {
	const commandEnd = stat.lastIndexOf(")");
	if (commandEnd < 0) throw new Error("Linux process stat command is malformed");
	const fields = stat.slice(commandEnd + 1).trim().split(/\s+/);
	const startTicks = fields[19];
	if (startTicks === undefined || !/^\d+$/.test(startTicks)) {
		throw new Error("Linux process start identity is malformed");
	}
	return startTicks;
}

async function linuxProcessIdentity(pid: number): Promise<WireJsonlProcessIdentity> {
	const [bootId, stat] = await Promise.all([
		linuxBootId(),
		Bun.file(`/proc/${pid}/stat`).text(),
	]);
	return Object.freeze({ bootId, startTicks: linuxProcessStartTicks(stat) });
}

async function deadProcessIdentityProved(owner: WireJsonlLockOwner): Promise<boolean> {
	let bootId: string;
	try {
		bootId = await linuxBootId();
	} catch {
		return false;
	}
	if (bootId !== owner.process.bootId) return true;
	try {
		const stat = await Bun.file(`/proc/${owner.pid}/stat`).text();
		return linuxProcessStartTicks(stat) !== owner.process.startTicks;
	} catch (error) {
		return errorCode(error) === "ENOENT";
	}
}

function parsedLockOwner(value: unknown): WireJsonlLockOwner | undefined {
	if (value === null || typeof value !== "object" || Array.isArray(value)) return undefined;
	const source = value as Record<string, unknown>;
	const processIdentity = source.process;
	if (source.version !== WIRE_JSONL_LOCK_VERSION
		|| typeof source.token !== "string" || source.token.length === 0 || source.token.length > 128
		|| typeof source.pid !== "number" || !Number.isSafeInteger(source.pid) || source.pid <= 0
		|| processIdentity === null || typeof processIdentity !== "object" || Array.isArray(processIdentity)
		|| typeof source.createdAt !== "string"
		|| typeof source.filePath !== "string" || source.filePath.length === 0) {
		return undefined;
	}
	const processSource = processIdentity as Record<string, unknown>;
	if (typeof processSource.bootId !== "string"
		|| !/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(processSource.bootId)
		|| typeof processSource.startTicks !== "string"
		|| !/^\d+$/.test(processSource.startTicks)) {
		return undefined;
	}
	const timestamp = Date.parse(source.createdAt);
	if (!Number.isFinite(timestamp) || new Date(timestamp).toISOString() !== source.createdAt) return undefined;
	return Object.freeze({
		version: WIRE_JSONL_LOCK_VERSION,
		token: source.token,
		pid: source.pid,
		process: Object.freeze({
			bootId: processSource.bootId,
			startTicks: processSource.startTicks,
		}),
		createdAt: source.createdAt,
		filePath: source.filePath,
	});
}

function parseWireJsonlLockEvidence(contents: Uint8Array): WireJsonlLockEvidence {
	if (contents.byteLength > WIRE_JSONL_MAX_LOCK_BYTES) {
		return Object.freeze({
			status: "malformed",
			bytes: contents.byteLength,
			reason: `lock evidence exceeds ${WIRE_JSONL_MAX_LOCK_BYTES} bytes`,
		});
	}
	let text: string;
	try {
		text = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(contents);
	} catch {
		return Object.freeze({
			status: "malformed",
			bytes: contents.byteLength,
			reason: "lock evidence is not valid UTF-8",
		});
	}
	let parsed: unknown;
	try {
		parsed = JSON.parse(text);
	} catch {
		return Object.freeze({
			status: "malformed",
			bytes: contents.byteLength,
			reason: "lock evidence is not valid JSON",
		});
	}
	const owner = parsedLockOwner(parsed);
	if (!owner) {
		return Object.freeze({
			status: "malformed",
			bytes: contents.byteLength,
			reason: "lock evidence does not match the supported owner schema",
		});
	}
	return Object.freeze({ status: "owner", owner });
}

async function inspectWireJsonlLockHandle(handle: fs.FileHandle): Promise<WireJsonlLockEvidence> {
	const contents = new Uint8Array(WIRE_JSONL_MAX_LOCK_BYTES + 1);
	let bytes = 0;
	while (bytes < contents.byteLength) {
		const result = await handle.read(contents, bytes, contents.byteLength - bytes, bytes);
		if (result.bytesRead === 0) break;
		bytes += result.bytesRead;
	}
	return parseWireJsonlLockEvidence(contents.slice(0, bytes));
}

async function inspectWireJsonlLock(lockPath: string): Promise<WireJsonlLockEvidence> {
	let handle: fs.FileHandle | undefined;
	try {
		handle = await fs.open(
			lockPath,
			fs.constants.O_RDONLY | fs.constants.O_NOFOLLOW | fs.constants.O_NONBLOCK,
		);
		return await inspectWireJsonlLockHandle(handle);
	} catch (error) {
		return Object.freeze({ status: "unreadable", reason: errorMessage(error) });
	} finally {
		try { await handle?.close(); }
		catch { /* Lock evidence remains safely unreadable or was already captured. */ }
	}
}

async function observeWireJsonlLock(lockPath: string): Promise<WireJsonlLockObservation> {
	let handle: fs.FileHandle | undefined;
	let failure: unknown;
	let observation: WireJsonlLockObservation | undefined;
	try {
		handle = await fs.open(
			lockPath,
			fs.constants.O_RDONLY | fs.constants.O_NOFOLLOW | fs.constants.O_NONBLOCK,
		);
		const stat = await handle.stat();
		const evidence = await inspectWireJsonlLockHandle(handle);
		if (!stat.isFile() || stat.nlink < 1) {
			throw new Error("wire JSONL lock is not a linked regular file");
		}
		observation = { dev: stat.dev, ino: stat.ino, evidence };
	} catch (error) {
		failure = error;
	}
	try { await handle?.close(); }
	catch (error) { failure ??= error; }
	if (failure !== undefined || observation === undefined) {
		throw new WireJsonlError(
			"concurrent_modification",
			`failed to inspect wire JSONL lock identity: ${lockPath}`,
			{ lockPath },
			failure === undefined ? undefined : { cause: failure },
		);
	}
	return observation;
}

async function createWireJsonlLock(
	filePath: string,
	ownedReclaimMarker?: string,
): Promise<HeldWireJsonlLock> {
	const lockPath = `${filePath}.lock`;
	let processIdentity: WireJsonlProcessIdentity;
	try {
		processIdentity = await linuxProcessIdentity(process.pid);
	} catch (error) {
		throw new WireJsonlError(
			"io",
			"failed to capture Linux process identity for the wire JSONL writer lock",
			{ lockPath },
			{ cause: error },
		);
	}
	const owner: WireJsonlLockOwner = Object.freeze({
		version: WIRE_JSONL_LOCK_VERSION,
		token: crypto.randomUUID(),
		pid: process.pid,
		process: processIdentity,
		createdAt: new Date().toISOString(),
		filePath,
	});
	let handle: fs.FileHandle;
	try {
		handle = await fs.open(lockPath, "wx", 0o600);
	} catch (error) {
		if (errorCode(error) === "EEXIST") {
			const lockEvidence = await inspectWireJsonlLock(lockPath);
			throw new WireJsonlError(
				"writer_locked",
				`wire JSONL file already has an exclusive writer lock: ${lockPath}`,
				{ lockPath, lockEvidence },
				{ cause: error },
			);
		}
		throw new WireJsonlError("io", `failed to acquire wire JSONL writer lock ${lockPath}`, {
			lockPath,
		}, { cause: error });
	}

	try {
		const evidence = TEXT_ENCODER.encode(`${JSON.stringify(owner)}\n`);
		await handle.writeFile(evidence);
		await handle.sync();
	} catch (error) {
		let closed = false;
		try {
			await handle.close();
			closed = true;
		} finally {
			if (closed) {
				try {
					await fs.unlink(lockPath);
				} catch {
					// The acquisition error remains load-bearing; a surviving lock safely fails closed.
				}
			}
		}
		throw new WireJsonlError("io", `failed to persist wire JSONL lock evidence ${lockPath}`, {
			lockPath,
		}, { cause: error });
	}
	const lock = { handle, path: lockPath, owner };
	const markerPath = `${lockPath}.reclaiming`;
	if (ownedReclaimMarker === undefined) {
		let markerExists = false;
		try {
			await fs.lstat(markerPath);
			markerExists = true;
		} catch (error) {
			if (errorCode(error) !== "ENOENT") markerExists = true;
		}
		if (markerExists) {
			const lockEvidence = await inspectWireJsonlLock(markerPath);
			await releaseOwnedWireJsonlLock(lock);
			throw new WireJsonlError(
				"writer_locked",
				`wire JSONL dead-owner recovery is in progress: ${markerPath}`,
				{ lockPath, lockEvidence },
			);
		}
	} else if (ownedReclaimMarker !== markerPath) {
		await releaseOwnedWireJsonlLock(lock);
		throw new WireJsonlError(
			"concurrent_modification",
			"wire JSONL recovery marker does not belong to the stable lock path",
			{ lockPath },
		);
	}
	return lock;
}

async function acquireWireJsonlLock(
	filePath: string,
	recoverDeadOwnerLock: boolean,
): Promise<HeldWireJsonlLock> {
	try {
		return await createWireJsonlLock(filePath);
	} catch (error) {
		if (!recoverDeadOwnerLock
			|| !(error instanceof WireJsonlError)
			|| error.code !== "writer_locked") {
			throw error;
		}
		return reclaimDeadWireJsonlLock(filePath);
	}
}

async function assertOwnedWireJsonlLock(lock: HeldWireJsonlLock): Promise<void> {
	let currentHandle: fs.FileHandle;
	try {
		currentHandle = await fs.open(
			lock.path,
			fs.constants.O_RDONLY | fs.constants.O_NOFOLLOW | fs.constants.O_NONBLOCK,
		);
	} catch (error) {
		throw new WireJsonlError(
			"concurrent_modification",
			`failed to prove wire JSONL lock ownership: ${lock.path}`,
			{ lockPath: lock.path },
			{ cause: error },
		);
	}
	let failure: unknown;
	try {
		const [held, current] = await Promise.all([
			lock.handle.stat(),
			currentHandle.stat(),
		]);
		const evidence = await inspectWireJsonlLockHandle(currentHandle);
		if (!held.isFile() || !current.isFile()
			|| held.nlink !== 1 || current.nlink !== 1
			|| held.dev !== current.dev || held.ino !== current.ino
			|| evidence.status !== "owner"
			|| evidence.owner.token !== lock.owner.token
			|| evidence.owner.filePath !== lock.owner.filePath) {
			throw new WireJsonlError(
				"concurrent_modification",
				`wire JSONL lock ownership changed: ${lock.path}`,
				{ lockPath: lock.path, lockEvidence: evidence },
			);
		}
	} catch (error) {
		failure = error;
	}
	try {
		await currentHandle.close();
	} catch (error) {
		if (failure === undefined) failure = error;
	}
	if (failure !== undefined) {
		if (failure instanceof WireJsonlError) throw failure;
		throw new WireJsonlError(
			"concurrent_modification",
			`failed to prove wire JSONL lock ownership: ${lock.path}`,
			{ lockPath: lock.path },
			{ cause: failure },
		);
	}
}

async function releaseOwnedWireJsonlLock(lock: HeldWireJsonlLock): Promise<void> {
	let ownershipFailure: unknown;
	try {
		await assertOwnedWireJsonlLock(lock);
	} catch (error) {
		ownershipFailure = error;
	}
	if (ownershipFailure !== undefined) {
		try { await lock.handle.close(); }
		catch { /* The ownership failure remains load-bearing; the path stays fail-closed. */ }
		if (ownershipFailure instanceof WireJsonlError) throw ownershipFailure;
		throw new WireJsonlError(
			"concurrent_modification",
			`failed to prove wire JSONL lock ownership before release: ${lock.path}`,
			{ lockPath: lock.path },
			{ cause: ownershipFailure },
		);
	}
	try {
		await fs.unlink(lock.path);
	} catch (error) {
		try { await lock.handle.close(); }
		catch { /* The unlink failure leaves the lock path fail-closed. */ }
		throw new WireJsonlError("io", `failed to release owned wire JSONL writer lock ${lock.path}`, {
			lockPath: lock.path,
		}, { cause: error });
	}
	try {
		await lock.handle.close();
	} catch (error) {
		throw new WireJsonlError("io", `failed to close released wire JSONL writer lock ${lock.path}`, {
			lockPath: lock.path,
		}, { cause: error });
	}
}

async function appendAndSync(handle: fs.FileHandle, contents: Uint8Array): Promise<void> {
	let offset = 0;
	while (offset < contents.byteLength) {
		const result = await handle.write(contents, offset, contents.byteLength - offset, null);
		if (result.bytesWritten <= 0) throw new Error("wire JSONL append made no forward progress");
		offset += result.bytesWritten;
	}
	await handle.sync();
}

async function syncDirectory(directory: string): Promise<void> {
	let handle: fs.FileHandle | undefined;
	try {
		handle = await fs.open(directory, "r");
		await handle.sync();
		await handle.close();
	} catch (error) {
		try { await handle?.close(); }
		catch { /* The directory sync failure remains load-bearing. */ }
		throw new WireJsonlError("io", `failed to sync wire JSONL directory ${directory}`, {}, {
			cause: error,
		});
	}
}

function sameWireJsonlLockOwner(
	left: WireJsonlLockOwner,
	right: WireJsonlLockOwner,
): boolean {
	return left.version === right.version
		&& left.token === right.token
		&& left.pid === right.pid
		&& left.process.bootId === right.process.bootId
		&& left.process.startTicks === right.process.startTicks
		&& left.createdAt === right.createdAt
		&& left.filePath === right.filePath;
}

async function observeOptionalWireJsonlLock(
	lockPath: string,
): Promise<WireJsonlLockObservation | undefined> {
	try {
		return await observeWireJsonlLock(lockPath);
	} catch (error) {
		if (error instanceof WireJsonlError && errorCode(error.cause) === "ENOENT") return undefined;
		throw error;
	}
}

async function proveDeadWireJsonlLockObservation(
	filePath: string,
	lockPath: string,
	initial: WireJsonlLockObservation,
): Promise<WireJsonlLockObservation> {
	if (initial.evidence.status !== "owner"
		|| initial.evidence.owner.filePath !== filePath
		|| !(await deadProcessIdentityProved(initial.evidence.owner))) {
		throw new WireJsonlError(
			"writer_locked",
			`wire JSONL lock owner is not proved dead: ${lockPath}`,
			{ lockPath, lockEvidence: initial.evidence },
		);
	}
	const verified = await observeWireJsonlLock(lockPath);
	if (!sameFileIdentity(initial, verified)
		|| verified.evidence.status !== "owner"
		|| !sameWireJsonlLockOwner(initial.evidence.owner, verified.evidence.owner)
		|| !(await deadProcessIdentityProved(verified.evidence.owner))) {
		throw new WireJsonlError(
			"concurrent_modification",
			`wire JSONL dead-owner evidence changed: ${lockPath}`,
			{ lockPath, lockEvidence: verified.evidence },
		);
	}
	return verified;
}

async function createReclaimedWireJsonlLock(
	filePath: string,
	markerPath: string,
): Promise<HeldWireJsonlLock> {
	const lockPath = `${filePath}.lock`;
	for (let attempt = 0; attempt < 3; attempt += 1) {
		try {
			return await createWireJsonlLock(filePath, markerPath);
		} catch (error) {
			if (!(error instanceof WireJsonlError) || error.code !== "writer_locked" || attempt === 2) {
				throw error;
			}
			await Bun.sleep(1);
		}
	}
	throw new WireJsonlError(
		"concurrent_modification",
		`wire JSONL recovery could not acquire the stable lock: ${lockPath}`,
		{ lockPath },
	);
}

async function reclaimDeadWireJsonlLockWhileGuarded(
	filePath: string,
	guard: HeldWireJsonlRecoveryGuard,
): Promise<HeldWireJsonlLock> {
	const lockPath = `${filePath}.lock`;
	const markerPath = `${lockPath}.reclaiming`;
	let current = await observeOptionalWireJsonlLock(lockPath);
	let marker = await observeOptionalWireJsonlLock(markerPath);
	let acquired: HeldWireJsonlLock | undefined;
	try {
		if (current === undefined && marker === undefined) {
			await assertWireJsonlRecoveryGuard(guard);
			return await createWireJsonlLock(filePath);
		}

		if (current !== undefined) {
			current = await proveDeadWireJsonlLockObservation(filePath, lockPath, current);
			if (marker !== undefined) {
				marker = await proveDeadWireJsonlLockObservation(filePath, markerPath, marker);
				current = await proveDeadWireJsonlLockObservation(filePath, lockPath, current);
				const sameClaim = sameFileIdentity(current, marker)
					&& current.evidence.status === "owner"
					&& marker.evidence.status === "owner"
					&& sameWireJsonlLockOwner(current.evidence.owner, marker.evidence.owner);
				if (!sameClaim) {
					marker = await proveDeadWireJsonlLockObservation(filePath, markerPath, marker);
					current = await proveDeadWireJsonlLockObservation(filePath, lockPath, current);
					await assertWireJsonlRecoveryGuard(guard);
					await fs.unlink(markerPath);
					await syncDirectory(path.dirname(lockPath));
					marker = undefined;
				}
			}

			if (marker === undefined) {
				await assertWireJsonlRecoveryGuard(guard);
				await fs.link(lockPath, markerPath);
				marker = await proveDeadWireJsonlLockObservation(
					filePath,
					markerPath,
					current,
				);
			}

			current = await proveDeadWireJsonlLockObservation(filePath, lockPath, current);
			marker = await proveDeadWireJsonlLockObservation(filePath, markerPath, marker);
			if (!sameFileIdentity(current, marker)
				|| current.evidence.status !== "owner"
				|| marker.evidence.status !== "owner"
				|| !sameWireJsonlLockOwner(current.evidence.owner, marker.evidence.owner)) {
				throw new WireJsonlError(
					"concurrent_modification",
					`wire JSONL dead-owner recovery claim is inconsistent: ${lockPath}`,
					{ lockPath, lockEvidence: marker.evidence },
				);
			}
			await assertWireJsonlRecoveryGuard(guard);
			await fs.unlink(lockPath);
			await syncDirectory(path.dirname(lockPath));
		}

		if (marker === undefined) {
			throw new WireJsonlError(
				"concurrent_modification",
				`wire JSONL recovery lost its marker: ${markerPath}`,
				{ lockPath },
			);
		}
		marker = await proveDeadWireJsonlLockObservation(filePath, markerPath, marker);
		await assertWireJsonlRecoveryGuard(guard);
		acquired = await createReclaimedWireJsonlLock(filePath, markerPath);
		marker = await proveDeadWireJsonlLockObservation(filePath, markerPath, marker);
		await assertWireJsonlRecoveryGuard(guard);
		await fs.unlink(markerPath);
		await syncDirectory(path.dirname(lockPath));
		await assertWireJsonlRecoveryGuard(guard);
		return acquired;
	} catch (error) {
		if (acquired !== undefined) {
			try { await releaseOwnedWireJsonlLock(acquired); }
			catch { /* The recovery error and surviving marker remain fail-closed. */ }
		}
		if (error instanceof WireJsonlError) throw error;
		throw new WireJsonlError(
			"concurrent_modification",
			`failed to recover dead wire JSONL lock: ${lockPath}`,
			{ lockPath },
			{ cause: error },
		);
	}
}

async function reclaimDeadWireJsonlLock(filePath: string): Promise<HeldWireJsonlLock> {
	const lockPath = `${filePath}.lock`;
	const guard = await acquireWireJsonlRecoveryGuard(lockPath);
	let acquired: HeldWireJsonlLock | undefined;
	let failure: unknown;
	try {
		acquired = await reclaimDeadWireJsonlLockWhileGuarded(filePath, guard);
	} catch (error) {
		failure = error;
	}
	try {
		await releaseWireJsonlRecoveryGuard(guard);
	} catch (error) {
		failure ??= error;
	}
	if (failure !== undefined) {
		if (acquired !== undefined) {
			try { await releaseOwnedWireJsonlLock(acquired); }
			catch { /* The recovery/guard failure remains load-bearing. */ }
		}
		throw failure;
	}
	if (!acquired) {
		throw new WireJsonlError(
			"concurrent_modification",
			`wire JSONL recovery finished without an owned lock: ${lockPath}`,
			{ lockPath },
		);
	}
	return acquired;
}

function resolvedRotationArchive(filePath: string, archivePath: string): string {
	const resolvedFile = path.resolve(filePath);
	const resolvedArchive = path.resolve(archivePath);
	if (resolvedArchive === resolvedFile
		|| path.dirname(resolvedArchive) !== path.dirname(resolvedFile)) {
		throw new WireJsonlError(
			"rotation_refused",
			"wire JSONL rotation archive must be a distinct path in the stable file directory",
		);
	}
	return resolvedArchive;
}

async function rotateTerminalWireJsonl(
	filePath: string,
	archivePath: string,
	replay: WireJsonlReplay,
	dataHandle: fs.FileHandle,
): Promise<OpenWireJsonlDataFile> {
	if (replay.events.length === 0) {
		const observed = await observeWireJsonlDataFile(filePath, dataHandle);
		return {
			handle: dataHandle,
			identity: { dev: observed.dev, ino: observed.ino },
		};
	}
	if (!replay.snapshot || !terminal(replay.snapshot)) {
		throw new WireJsonlError(
			"rotation_refused",
			`wire JSONL stream is incomplete and cannot be rotated: ${filePath}`,
		);
	}

	const resolvedFile = path.resolve(filePath);
	const resolvedArchive = resolvedRotationArchive(resolvedFile, archivePath);
	const directory = path.dirname(resolvedFile);
	const [held, current] = await Promise.all([
		dataHandle.stat(),
		fs.lstat(resolvedFile),
	]).catch((error: unknown) => {
		throw new WireJsonlError(
			"concurrent_modification",
			`failed to prove wire JSONL stable-file identity before rotation: ${resolvedFile}`,
			{},
			{ cause: error },
		);
	});
	if (!current.isFile()
		|| held.nlink !== 1
		|| current.nlink !== 1
		|| held.dev !== current.dev
		|| held.ino !== current.ino
		|| held.size !== replay.bytes) {
		throw new WireJsonlError(
			"concurrent_modification",
			`wire JSONL stable-file identity changed before rotation: ${resolvedFile}`,
		);
	}
	await dataHandle.close();

	try {
		// A hard link gives rotation a no-overwrite archive publication point. If
		// the process dies before unlinking the stable name, both names retain the
		// complete terminal inode and the adjacent lock fails future writers closed.
		await fs.link(resolvedFile, resolvedArchive);
	} catch (error) {
		const code = errorCode(error);
		throw new WireJsonlError(
			code === "EEXIST" ? "rotation_refused" : "io",
			code === "EEXIST"
				? `wire JSONL rotation archive already exists: ${resolvedArchive}`
				: `failed to publish wire JSONL rotation archive ${resolvedArchive}`,
			{},
			{ cause: error },
		);
	}
	await syncDirectory(directory);

	try {
		const stable = await fs.lstat(resolvedFile);
		const archived = await fs.lstat(resolvedArchive);
		if (!stable.isFile() || !archived.isFile()
			|| stable.dev !== held.dev || stable.ino !== held.ino
			|| archived.dev !== held.dev || archived.ino !== held.ino) {
			throw new WireJsonlError(
				"concurrent_modification",
				`wire JSONL stable-file identity changed during rotation: ${resolvedFile}`,
			);
		}
		await fs.unlink(resolvedFile);
		await syncDirectory(directory);
	} catch (error) {
		if (error instanceof WireJsonlError) throw error;
		throw new WireJsonlError(
			"io",
			`failed to remove the archived wire JSONL stable name ${resolvedFile}`,
			{},
			{ cause: error },
		);
	}

	let fresh: OpenWireJsonlDataFile | undefined;
	try {
		fresh = await openWireJsonlDataFile(resolvedFile, true);
		await fresh.handle.sync();
		await syncDirectory(directory);
	} catch (error) {
		try { await fresh?.handle.close(); }
		catch { /* The creation failure remains load-bearing. */ }
		throw new WireJsonlError(
			"io",
			`failed to create the fresh wire JSONL stable file ${resolvedFile}`,
			{},
			{ cause: error },
		);
	}
	return fresh;
}

class AppendOnlyWireJsonlWriter implements WireJsonlWriter {
	readonly filePath: string;
	readonly lockPath: string;
	#options: ResolvedWireJsonlOptions;
	#events: WireEvent[];
	#eventIds: Set<string>;
	#snapshot?: WireRunSnapshot;
	#bytes: number;
	#dataHandle: fs.FileHandle;
	#dataIdentity: WireJsonlFileIdentity;
	#lock: HeldWireJsonlLock;
	#busy = false;
	#poisoned = false;
	#closed = false;

	constructor(
		filePath: string,
		options: ResolvedWireJsonlOptions,
		replay: WireJsonlReplay,
		dataHandle: fs.FileHandle,
		dataIdentity: WireJsonlFileIdentity,
		lock: HeldWireJsonlLock,
	) {
		this.filePath = filePath;
		this.lockPath = lock.path;
		this.#options = options;
		this.#events = [...replay.events];
		this.#eventIds = new Set(replay.events.map((event) => event.id));
		this.#snapshot = replay.snapshot;
		this.#bytes = replay.bytes;
		this.#dataHandle = dataHandle;
		this.#dataIdentity = dataIdentity;
		this.#lock = lock;
	}

	replay(): WireJsonlReplay {
		return frozenReplay(this.#events, this.#snapshot, this.#bytes);
	}

	async append(event: WireEvent): Promise<WireEvent> {
		if (this.#closed) {
			throw new WireJsonlError("writer_closed", "wire JSONL writer is closed", {
				lockPath: this.lockPath,
			});
		}
		if (this.#poisoned) {
			throw new WireJsonlError(
				"concurrent_modification",
				"wire JSONL writer must be closed and reopened after a failed append",
				{ lockPath: this.lockPath },
			);
		}
		if (this.#busy) {
			throw new WireJsonlError(
				"concurrent_modification",
				"concurrent appends through one wire JSONL writer are unsupported",
			);
		}
		this.#busy = true;
		try {
			if (this.#events.length >= this.#options.maxEvents) {
				throw new WireJsonlError(
					"oversized",
					`wire JSONL stream exceeds the ${this.#options.maxEvents}-event limit`,
					{ limit: this.#options.maxEvents, observed: this.#events.length + 1 },
				);
			}
			const line = encodeWireJsonlLine(event, this.#options);
			const lineBytes = encodedBytes(line);
			const nextBytes = this.#bytes + lineBytes;
			if (nextBytes > this.#options.maxStreamBytes) {
				throw new WireJsonlError(
					"oversized",
					`wire JSONL stream would exceed the ${this.#options.maxStreamBytes}-byte limit`,
					{ limit: this.#options.maxStreamBytes, observed: nextBytes },
				);
			}
			const decoded = decodeWireJsonlLine(line, this.#options);
			const context = { line: this.#events.length + 1, byteOffset: this.#bytes };
			if (this.#eventIds.has(decoded.id)) {
				throw lineError(
					"noncontiguous",
					`wire event id ${decoded.id} is duplicated`,
					context,
				);
			}
			const snapshot = advanceSnapshot(this.#snapshot, decoded, context);
			try {
				await assertOwnedWireJsonlLock(this.#lock);
				const before = await observeWireJsonlDataFile(
					this.filePath,
					this.#dataHandle,
					this.#dataIdentity,
				);
				if (before.size !== this.#bytes) {
					throw new WireJsonlError(
						"concurrent_modification",
						`wire JSONL data size changed before append: ${this.filePath}`,
						{ observed: before.size },
					);
				}
				await appendAndSync(this.#dataHandle, TEXT_ENCODER.encode(line));
				const after = await observeWireJsonlDataFile(
					this.filePath,
					this.#dataHandle,
					this.#dataIdentity,
				);
				if (after.size !== nextBytes) {
					throw new WireJsonlError(
						"concurrent_modification",
						`wire JSONL data size changed during append: ${this.filePath}`,
						{ observed: after.size },
					);
				}
				await assertOwnedWireJsonlLock(this.#lock);
			} catch (error) {
				this.#poisoned = true;
				if (error instanceof WireJsonlError) throw error;
				throw new WireJsonlError("io", `failed to append wire JSONL file ${this.filePath}`, {}, { cause: error });
			}
			this.#events.push(decoded);
			this.#eventIds.add(decoded.id);
			this.#snapshot = snapshot;
			this.#bytes = nextBytes;
			return decoded;
		} finally {
			this.#busy = false;
		}
	}

	async close(): Promise<void> {
		if (this.#closed) return;
		if (this.#busy) {
			throw new WireJsonlError(
				"concurrent_modification",
				"wire JSONL writer cannot close during an active append",
				{ lockPath: this.lockPath },
			);
		}
		this.#closed = true;
		let failure: unknown;
		try {
			await this.#dataHandle.close();
		} catch (error) {
			failure = error;
		}
		if (failure !== undefined) {
			try { await this.#lock.handle.close(); }
			catch (error) { failure = new AggregateError([failure, error]); }
			throw new WireJsonlError("io", `failed to close wire JSONL writer ${this.filePath}; lock retained`, {
				lockPath: this.lockPath,
			}, { cause: failure });
		}
		await releaseOwnedWireJsonlLock(this.#lock);
	}

	async [Symbol.asyncDispose](): Promise<void> {
		await this.close();
	}
}

export async function openWireJsonlWriter(
	filePath: string,
	options: WireJsonlOpenOptions = {},
): Promise<WireJsonlWriter> {
	const resolved = resolveOptions(options);
	try {
		await fs.mkdir(path.dirname(filePath), { recursive: true });
	} catch (error) {
		throw new WireJsonlError("io", `failed to open wire JSONL file ${filePath}`, {}, { cause: error });
	}
	const lock = await acquireWireJsonlLock(
		filePath,
		options.recoverDeadOwnerLock === true,
	);
	let dataFile: OpenWireJsonlDataFile | undefined;
	try {
		dataFile = await openWireJsonlDataFile(filePath);
		let replay = await readHeldWireJsonl(filePath, dataFile, resolved);
		if (options.rotateExistingTo !== undefined && replay.events.length > 0) {
			dataFile = await rotateTerminalWireJsonl(
				filePath,
				options.rotateExistingTo,
				replay,
				dataFile.handle,
			);
			replay = frozenReplay([], undefined, 0);
		}
		await observeWireJsonlDataFile(filePath, dataFile.handle, dataFile.identity);
		return new AppendOnlyWireJsonlWriter(
			filePath,
			resolved,
			replay,
			dataFile.handle,
			dataFile.identity,
			lock,
		);
	} catch (error) {
		try {
			await dataFile?.handle.close();
			await releaseOwnedWireJsonlLock(lock);
		} catch (cleanupError) {
			throw new WireJsonlError("io", `failed to clean up wire JSONL writer ${filePath}`, {
				lockPath: lock.path,
			}, { cause: cleanupError });
		}
		if (error instanceof WireJsonlError) throw error;
		throw new WireJsonlError("io", `failed to open wire JSONL file ${filePath}`, {
			lockPath: lock.path,
		}, { cause: error });
	}
}
