import { afterEach, expect, test } from "bun:test";
import * as fs from "node:fs/promises";
import * as os from "node:os";
import * as path from "node:path";

import {
	StreamWriter, StreamWriterError,
} from "../src/stream-writer";
import {
	WireEventWriter,
	WireJsonlError,
	encodeWireJsonlLine,
	readWireJsonl,
	wireEventId,
	wireModelCallId,
	wireRunId,
	wireToolCallId,
	type WireEvent,
} from "../src/wire";

const roots: string[] = [];
const originalStreamDirectory = process.env.NORTH_STREAM_DIR;

afterEach(async () => {
	if (originalStreamDirectory === undefined) delete process.env.NORTH_STREAM_DIR;
	else process.env.NORTH_STREAM_DIR = originalStreamDirectory;
	await Promise.all(roots.splice(0).map((root) => fs.rm(root, { recursive: true, force: true })));
});

async function streamDirectory(): Promise<string> {
	const root = await fs.mkdtemp(path.join(os.tmpdir(), "north-stream-writer-"));
	roots.push(root);
	process.env.NORTH_STREAM_DIR = root;
	return root;
}

function runEvents(label: string): readonly WireEvent[] {
	let tick = 0;
	const source = new WireEventWriter({
		runId: wireRunId(`run:stream:${label}`),
		eventId: (sequence) => wireEventId(`event:stream:${label}:${sequence}`),
		now: () => new Date(Date.UTC(2026, 7, 10, 4, 0, tick++)).toISOString(),
	});
	return [
		source.append({ kind: "run.started", lifecycle: "running", owner: `agent-${label}` }),
		source.append({
			kind: "run.progress",
			lifecycle: "running",
			progress: { currentAction: `work-${label}` },
		}),
		source.append({
			kind: "run.terminated",
			lifecycle: "completed",
			reason: { code: "completed" },
		}),
	];
}

async function expectWireJsonlError(
	action: () => Promise<unknown>,
	code: WireJsonlError["code"],
): Promise<WireJsonlError> {
	try {
		await action();
	} catch (error) {
		expect(error).toBeInstanceOf(WireJsonlError);
		if (!(error instanceof WireJsonlError)) throw error;
		expect(error.code).toBe(code);
		return error;
	}
	throw new Error(`expected wire JSONL error ${code}`);
}

test("persists exact canonical events and rotates a completed run without truncation or mixing", async () => {
	const directory = await streamDirectory();
	const agentId = "stream-lane-1";
	const firstEvents = runEvents("first");
	const first = await StreamWriter.open(agentId);
	for (const event of firstEvents) await first.writeWireEvent(event);
	await first.close();

	const stablePath = path.join(directory, `agent-${agentId}.stream.jsonl`);
	const firstText = firstEvents.map((event) => encodeWireJsonlLine(event)).join("");
	expect(await Bun.file(stablePath).text()).toBe(firstText);
	expect((await readWireJsonl(stablePath)).events).toEqual(firstEvents);
	expect(await Bun.file(`${stablePath}.lock`).exists()).toBe(false);

	const secondEvents = runEvents("second");
	const second = await StreamWriter.open(agentId);
	expect(await Bun.file(stablePath).text()).toBe("");
	const archives = (await fs.readdir(directory))
		.filter((name) => name.startsWith(`agent-${agentId}.archive-`));
	expect(archives).toHaveLength(1);
	const archivePath = path.join(directory, archives[0]);
	expect(await Bun.file(archivePath).text()).toBe(firstText);
	for (const event of secondEvents) await second.writeWireEvent(event);
	await second.close();

	expect((await readWireJsonl(archivePath)).events).toEqual(firstEvents);
	expect((await readWireJsonl(stablePath)).events).toEqual(secondEvents);
	expect(await Bun.file(stablePath).text()).toBe(
		secondEvents.map((event) => encodeWireJsonlLine(event)).join(""),
	);
});

test("a commit barrier reopens the exact durable prefix after tool admission", async () => {
	const directory = await streamDirectory();
	const agentId = "stream-crash-prefix";
	const child = Bun.spawn([
		process.execPath,
		path.join(import.meta.dir, "fixtures/stream-writer-kill-after-admitted.ts"),
		agentId,
	], {
		env: { ...process.env, NORTH_STREAM_DIR: directory },
		stdout: "pipe",
		stderr: "inherit",
	});
	try {
		const output = (child.stdout as ReadableStream<Uint8Array>).getReader();
		const ready = await output.read();
		output.releaseLock();
		expect(new TextDecoder().decode(ready.value)).toContain("ready");
		child.kill("SIGKILL");
		await child.exited;
	} finally {
		if (child.exitCode === null) {
			child.kill("SIGKILL");
			await child.exited;
		}
	}

	const writer = new WireEventWriter({
		runId: wireRunId("run:stream:crash-prefix"),
		eventId: (sequence) => wireEventId(`event:stream:crash-prefix:${sequence}`),
		now: () => "2026-08-12T00:00:00.000Z",
	});
	writer.append({ kind: "run.started", lifecycle: "running" });
	const modelCallId = wireModelCallId("model-call:stream:crash-prefix");
	writer.append({
		kind: "model-call.started",
		modelCallId,
		model: { provider: "openai", capabilityClass: "authoring" },
		attempt: 1,
	});
	writer.append({
		kind: "tool.admitted",
		toolCallId: wireToolCallId("tool:stream:crash-prefix"),
		modelCallId,
		name: "command",
		schema: { status: "unavailable", reason: "provider schema unavailable" },
	});

	const replay = await readWireJsonl(
		path.join(directory, `agent-${agentId}.stream.jsonl`),
	);
	expect(replay.events).toEqual(writer.events());
	expect(replay.events.map((event) => event.kind)).toEqual([
		"run.started", "model-call.started", "tool.admitted",
	]);
	expect(replay.snapshot?.lifecycle).toBe("running");
	expect(replay.events).not.toContainEqual(expect.objectContaining({ kind: "tool.terminal" }));
	expect(replay.events).not.toContainEqual(expect.objectContaining({ kind: "model-call.completed" }));
	expect(replay.events).not.toContainEqual(expect.objectContaining({ kind: "run.terminated" }));
});

test("refuses concurrent and stale locks, incomplete streams, and torn streams", async () => {
	const directory = await streamDirectory();
	const agentId = "stream-lane-2";
	const stablePath = path.join(directory, `agent-${agentId}.stream.jsonl`);
	const events = runEvents("refusal");

	const live = await StreamWriter.open(agentId);
	const lockEvidence = await Bun.file(live.lockPath).text();
	await expectWireJsonlError(() => StreamWriter.open(agentId), "writer_locked");
	await live.writeWireEvent(events[0]);
	await live.close();
	const incomplete = await Bun.file(stablePath).text();
	await expectWireJsonlError(() => StreamWriter.open(agentId), "rotation_refused");
	expect(await Bun.file(stablePath).text()).toBe(incomplete);

	await Bun.write(`${stablePath}.lock`, lockEvidence);
	await expectWireJsonlError(() => StreamWriter.open(agentId), "writer_locked");
	expect(await Bun.file(`${stablePath}.lock`).text()).toBe(lockEvidence);
	await fs.unlink(`${stablePath}.lock`);

	await Bun.write(stablePath, encodeWireJsonlLine(events[0]).slice(0, -1));
	await expectWireJsonlError(() => StreamWriter.open(agentId), "torn");
	expect(await Bun.file(`${stablePath}.lock`).exists()).toBe(false);
});

test("binds the first event run, rejects later run changes, and rejects writes after close", async () => {
	await streamDirectory();
	const writer = await StreamWriter.open("stream-lane-3");
	const first = runEvents("bound");
	const other = runEvents("other");
	await writer.writeWireEvent(first[0]);

	try {
		await writer.writeWireEvent(other[1]);
		throw new Error("expected run mismatch");
	} catch (error) {
		expect(error).toBeInstanceOf(StreamWriterError);
		if (!(error instanceof StreamWriterError)) throw error;
		expect(error.code).toBe("run_mismatch");
		expect(error.expectedRunId).toBe(first[0].runId);
		expect(error.observedRunId).toBe(other[1].runId);
	}

	await writer.close();
	await writer.close();
	try {
		await writer.writeWireEvent(first[1]);
		throw new Error("expected closed writer");
	} catch (error) {
		expect(error).toBeInstanceOf(StreamWriterError);
		if (!(error instanceof StreamWriterError)) throw error;
		expect(error.code).toBe("writer_closed");
	}
});

test("rejects path traversal IDs and keeps only the newest bounded archive set", async () => {
	const directory = await streamDirectory();
	for (const invalid of ["../escape", "/absolute", "lane/child", ".hidden", "lane\nchild"]) {
		try {
			await StreamWriter.open(invalid);
			throw new Error("expected invalid agent ID");
		} catch (error) {
			expect(error).toBeInstanceOf(StreamWriterError);
			if (!(error instanceof StreamWriterError)) throw error;
			expect(error.code).toBe("invalid_agent_id");
		}
	}
	for (const invalidDirectory of ["relative/streams", path.parse(directory).root]) {
		process.env.NORTH_STREAM_DIR = invalidDirectory;
		try {
			await StreamWriter.open("valid-lane");
			throw new Error("expected invalid stream directory");
		} catch (error) {
			expect(error).toBeInstanceOf(StreamWriterError);
			if (!(error instanceof StreamWriterError)) throw error;
			expect(error.code).toBe("invalid_stream_directory");
		}
	}
	process.env.NORTH_STREAM_DIR = directory;

	const agentId = "stream-lane-4";
	const archiveText = runEvents("archive-fixture")
		.map((event) => encodeWireJsonlLine(event))
		.join("");
	for (let index = 0; index < 35; index += 1) {
		const ordinal = index.toString().padStart(3, "0");
		await Bun.write(
			path.join(directory, `agent-${agentId}.archive-${ordinal}.stream.jsonl`),
			archiveText,
		);
	}
	const writer = await StreamWriter.open(agentId);
	await writer.close();
	const remaining = (await fs.readdir(directory))
		.filter((name) => name.startsWith(`agent-${agentId}.archive-`))
		.sort();
	expect(remaining).toHaveLength(32);
	expect(remaining[0]).toBe(`agent-${agentId}.archive-003.stream.jsonl`);

	const corrupt = path.join(
		directory,
		`agent-${agentId}.archive-000-corrupt.stream.jsonl`,
	);
	await Bun.write(corrupt, "not wire JSONL\n");
	await Bun.write(
		path.join(directory, `agent-${agentId}.archive-999.stream.jsonl`),
		archiveText,
	);
	try {
		await StreamWriter.open(agentId);
		throw new Error("expected corrupt archive refusal");
	} catch (error) {
		expect(error).toBeInstanceOf(StreamWriterError);
		if (!(error instanceof StreamWriterError)) throw error;
		expect(error.code).toBe("archive_prune_failed");
	}
	expect(await Bun.file(corrupt).text()).toBe("not wire JSONL\n");
	expect(await Bun.file(path.join(directory, `agent-${agentId}.stream.jsonl.lock`)).exists()).toBe(false);
});
