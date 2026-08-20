import * as fs from "node:fs/promises";
import * as path from "node:path";

import {
	openWireJsonlWriter,
	readWireJsonl,
	encodeWireJsonlLine,
	type WireEvent,
	type WireEventCommitBarrier,
	type WireEventWriter,
	type WireJsonlWriter,
} from "./wire";
import type { WireEventStorePublisher } from "./run-ledger";

const AGENT_ID_PATTERN = /^[A-Za-z0-9][A-Za-z0-9._:-]*$/;
const MAX_AGENT_ID_BYTES = 256;
const MAX_STREAM_ARCHIVES = 32;
const TEXT_ENCODER = new TextEncoder();

export type StreamWriterErrorCode =
	| "invalid_agent_id"
	| "invalid_stream_directory"
	| "writer_closed"
	| "run_mismatch"
	| "archive_prune_failed";

export class StreamWriterError extends Error {
	readonly code: StreamWriterErrorCode;
	readonly agentId?: string;
	readonly filePath?: string;
	readonly expectedRunId?: string;
	readonly observedRunId?: string;

	constructor(
		code: StreamWriterErrorCode,
		message: string,
		context: {
			agentId?: string;
			filePath?: string;
			expectedRunId?: string;
			observedRunId?: string;
		} = {},
		options?: ErrorOptions,
	) {
		super(message, options);
		this.name = "StreamWriterError";
		this.code = code;
		this.agentId = context.agentId;
		this.filePath = context.filePath;
		this.expectedRunId = context.expectedRunId;
		this.observedRunId = context.observedRunId;
	}
}

function assertAgentId(agentId: string): void {
	if (!AGENT_ID_PATTERN.test(agentId)
		|| TEXT_ENCODER.encode(agentId).byteLength > MAX_AGENT_ID_BYTES) {
		throw new StreamWriterError(
			"invalid_agent_id",
			`invalid stream agent ID: ${JSON.stringify(agentId)}`,
			{ agentId },
		);
	}
}

export function streamDirectory(): string {
	const configured = process.env.NORTH_STREAM_DIR;
	const home = process.env.HOME;
	const raw = configured ?? (home === undefined ? undefined : path.join(home, "code/agent-data"));
	if (raw === undefined || raw.length === 0 || raw.includes("\0") || !path.isAbsolute(raw)) {
		throw new StreamWriterError(
			"invalid_stream_directory",
			"NORTH_STREAM_DIR must be a non-empty absolute filesystem path",
		);
	}
	const resolved = path.resolve(raw);
	if (resolved === path.parse(resolved).root) {
		throw new StreamWriterError(
			"invalid_stream_directory",
			"NORTH_STREAM_DIR cannot be a filesystem root",
		);
	}
	return resolved;
}

function streamPaths(agentId: string): {
	directory: string;
	filePath: string;
	archivePath: string;
	archivePrefix: string;
} {
	assertAgentId(agentId);
	const directory = streamDirectory();
	const fileName = `agent-${agentId}.stream.jsonl`;
	const archivePrefix = `agent-${agentId}.archive-`;
	const archiveName = `${archivePrefix}${Date.now().toString(36)}-${crypto.randomUUID()}.stream.jsonl`;
	const filePath = path.resolve(directory, fileName);
	const archivePath = path.resolve(directory, archiveName);
	if (path.dirname(filePath) !== directory || path.dirname(archivePath) !== directory) {
		throw new StreamWriterError(
			"invalid_stream_directory",
			"resolved stream path escapes NORTH_STREAM_DIR",
			{ agentId, filePath },
		);
	}
	return { directory, filePath, archivePath, archivePrefix };
}

async function pruneStreamArchives(directory: string, archivePrefix: string): Promise<void> {
	const entries = await fs.readdir(directory, { withFileTypes: true });
	const archives = entries
		.filter((entry) => entry.isFile()
			&& entry.name.startsWith(archivePrefix)
			&& entry.name.endsWith(".stream.jsonl"))
		.map((entry) => entry.name)
		.sort();
	for (const archive of archives.slice(0, Math.max(0, archives.length - MAX_STREAM_ARCHIVES))) {
		const archivePath = path.join(directory, archive);
		const replay = await readWireJsonl(archivePath);
		const lifecycle = replay.snapshot?.lifecycle;
		if (lifecycle !== "completed" && lifecycle !== "failed"
			&& lifecycle !== "cancelled" && lifecycle !== "blocked") {
			throw new Error(`stream archive is not a complete wire run: ${archivePath}`);
		}
		await fs.unlink(archivePath);
	}
}

export class StreamWriter {
	readonly filePath: string;
	readonly lockPath: string;
	readonly agentId: string;
	#writer: WireJsonlWriter;
	#runId?: string;
	#closed = false;

	constructor(agentId: string, writer: WireJsonlWriter) {
		this.agentId = agentId;
		this.filePath = writer.filePath;
		this.lockPath = writer.lockPath;
		this.#writer = writer;
	}

	static async open(agentId: string): Promise<StreamWriter> {
		const paths = streamPaths(agentId);
		const writer = await openWireJsonlWriter(paths.filePath, {
			rotateExistingTo: paths.archivePath,
		});
		try {
			await pruneStreamArchives(paths.directory, paths.archivePrefix);
		} catch (error) {
			let cause: unknown = error;
			try {
				await writer.close();
			} catch (closeError) {
				cause = new AggregateError([error, closeError]);
			}
			throw new StreamWriterError(
				"archive_prune_failed",
				`failed to bound stream archives for agent ${agentId}`,
				{ agentId, filePath: paths.filePath },
				{ cause },
			);
		}
		return new StreamWriter(agentId, writer);
	}

	async writeWireEvent(event: WireEvent): Promise<WireEvent> {
		if (this.#closed) {
			throw new StreamWriterError(
				"writer_closed",
				`stream writer is closed for agent ${this.agentId}`,
				{ agentId: this.agentId, filePath: this.filePath },
			);
		}
		if (this.#runId !== undefined && event.runId !== this.#runId) {
			throw new StreamWriterError(
				"run_mismatch",
				`wire event run ${event.runId} does not match stream run ${this.#runId}`,
				{
					agentId: this.agentId,
					filePath: this.filePath,
					expectedRunId: this.#runId,
					observedRunId: event.runId,
				},
			);
		}
		const persisted = await this.#writer.append(event);
		this.#runId ??= persisted.runId;
		return persisted;
	}

	async close(): Promise<void> {
		if (this.#closed) return;
		await this.#writer.close();
		this.#closed = true;
	}

	async [Symbol.asyncDispose](): Promise<void> {
		await this.close();
	}
}

/** Serializes Store-acknowledged canonical suffixes before projecting them to JSONL. */
export class SerializedWireEventCommitter implements WireEventCommitBarrier {
	readonly #canonical: WireEventWriter;
	readonly #publisher: WireEventStorePublisher;
	readonly #stream: StreamWriter;
	#nextSequence = 0;
	#tail: Promise<void> = Promise.resolve();

	constructor(
		canonical: WireEventWriter,
		publisher: WireEventStorePublisher,
		stream: StreamWriter,
	) {
		this.#canonical = canonical;
		this.#publisher = publisher;
		this.#stream = stream;
	}

	commitThrough(event: WireEvent): Promise<void> {
		const targetSequence = event.sequence;
		const commit = this.#tail.then(async () => {
			this.#assertCanonical(event);
			const suffix: WireEvent[] = [];
			for (let sequence = this.#nextSequence; sequence <= targetSequence; sequence += 1) {
				const canonical = this.#canonical.events()[sequence];
				if (!canonical || canonical.sequence !== sequence) {
					throw new Error("wire writer persistence sequence diverged");
				}
				suffix.push(canonical);
			}
			if (suffix.length === 0) return;
			await this.#publisher.publish(Object.freeze(suffix));
			for (const canonical of suffix) await this.#stream.writeWireEvent(canonical);
			this.#nextSequence = targetSequence + 1;
		});
		// A gap, conflicting digest, unavailable Store acknowledgement, or append
		// failure poisons the queue. Later callers observe the same failure; no
		// uncertain suffix is retried behind a fabricated terminal.
		this.#tail = commit;
		void commit.catch(() => {});
		return commit;
	}

	async commitAll(): Promise<void> {
		const events = this.#canonical.events();
		const last = events.at(-1);
		if (last) await this.commitThrough(last);
		else await this.#tail;
	}

	#assertCanonical(event: WireEvent): void {
		const canonical = this.#canonical.events()[event.sequence];
		let matches = canonical === event;
		if (!matches && canonical) {
			try { matches = encodeWireJsonlLine(canonical) === encodeWireJsonlLine(event); }
			catch { /* Invalid values cannot equal the writer-owned canonical event. */ }
		}
		if (!canonical || !matches) {
			throw new Error("wire persistence target differs from its shared writer canonical event");
		}
	}
}
