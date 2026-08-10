import { afterEach, describe, expect, test } from "bun:test";
import { $ } from "bun";
import * as fs from "node:fs/promises";
import * as os from "node:os";
import * as path from "node:path";

import {
	WIRE_REQUIRED_SEMANTICS,
	WIRE_VERSION,
	WireEventWriter,
	decodeWireEvent,
	reduceWireEvents,
	wireEventId,
	wireRunId,
	type WireEvent,
	type WireRunId,
} from "../src/wire";
import {
	WireJsonlError,
	decodeWireJsonl,
	decodeWireJsonlLine,
	encodeWireJsonlLine,
	openWireJsonlWriter,
	readWireJsonl,
	type WireJsonlErrorCode,
} from "../src/wire/jsonl";

const RUN_ID = wireRunId("run:wire-jsonl");
const OTHER_RUN_ID = wireRunId("run:wire-jsonl-other");
const roots: string[] = [];

afterEach(async () => {
	await Promise.all(roots.splice(0).map((root) => fs.rm(root, { recursive: true, force: true })));
});

function rawEvent(
	sequence: number,
	runId: WireRunId,
	kind: string,
	payload: Readonly<Record<string, unknown>>,
): WireEvent {
	return decodeWireEvent({
		version: WIRE_VERSION,
		id: `event:jsonl:${runId}:${sequence}`,
		runId,
		sequence,
		at: new Date(Date.UTC(2026, 7, 10, 0, 0, sequence)).toISOString(),
		kind,
		essential: true,
		requiredSemantics: WIRE_REQUIRED_SEMANTICS,
		...(kind === "run.started" ? {} : { parentId: runId }),
		...payload,
	});
}

function started(runId = RUN_ID): WireEvent {
	return rawEvent(0, runId, "run.started", { lifecycle: "running" });
}

function progress(sequence: number, runId = RUN_ID): WireEvent {
	return rawEvent(sequence, runId, "run.progress", {
		lifecycle: "running",
		progress: { currentAction: `step-${sequence}` },
	});
}

function terminated(sequence: number, runId = RUN_ID): WireEvent {
	return rawEvent(sequence, runId, "run.terminated", {
		lifecycle: "completed",
		reason: { code: "completed" },
	});
}

function expectJsonlError(action: () => unknown, code: WireJsonlErrorCode): WireJsonlError {
	try {
		action();
	} catch (error) {
		expect(error).toBeInstanceOf(WireJsonlError);
		if (!(error instanceof WireJsonlError)) throw error;
		expect(error.code).toBe(code);
		return error;
	}
	throw new Error(`expected wire JSONL error ${code}`);
}

async function expectJsonlErrorAsync(
	action: () => Promise<unknown>,
	code: WireJsonlErrorCode,
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

async function tempFile(): Promise<string> {
	const root = await fs.mkdtemp(path.join(os.tmpdir(), "north-wire-jsonl-"));
	roots.push(root);
	return path.join(root, "run.stream.jsonl");
}

describe("wire-v2 JSONL persistence", () => {
	test("appends one canonical event per line and replays the identical run snapshot", async () => {
		let tick = 0;
		const source = new WireEventWriter({
			runId: RUN_ID,
			eventId: (sequence) => wireEventId(`event:writer-jsonl:${sequence}`),
			now: () => new Date(Date.UTC(2026, 7, 10, 0, 0, tick++)).toISOString(),
		});
		const events = [
			source.append({ kind: "run.started", lifecycle: "running", owner: "jsonl-test" }),
			source.append({
				kind: "run.progress",
				lifecycle: "running",
				progress: { currentAction: "persist canonical events" },
			}),
			source.append({
				kind: "run.terminated",
				lifecycle: "completed",
				reason: { code: "completed" },
			}),
		];

		const file = await tempFile();
		const writer = await openWireJsonlWriter(file);
		try {
			await writer.append(events[0]);
			const firstAppend = await Bun.file(file).text();
			await writer.append(events[1]);
			await writer.append(events[2]);
			const persisted = await Bun.file(file).text();
			expect(persisted.startsWith(firstAppend)).toBe(true);
			expect(persisted).toBe(events.map((event) => encodeWireJsonlLine(event)).join(""));

			const replay = await readWireJsonl(file);
			expect(replay.events).toEqual(events);
			expect(replay.snapshot).toEqual(source.snapshot());
			expect(replay.snapshot).toEqual(reduceWireEvents(replay.events));
			expect(writer.replay()).toEqual(replay);
			expect(persisted.split("\n").slice(0, -1)).toHaveLength(events.length);
		} finally {
			await writer.close();
		}
		expect(await Bun.file(writer.lockPath).exists()).toBe(false);
	});

	test("line helpers require canonical JSON and a complete LF terminator", () => {
		const event = started();
		const line = encodeWireJsonlLine(event);
		expect(decodeWireJsonlLine(line)).toEqual(event);
		expectJsonlError(() => decodeWireJsonlLine(line.slice(0, -1)), "torn");
		expectJsonlError(() => decodeWireJsonlLine(`${JSON.stringify(event)}\n`), "noncanonical");
		expectJsonlError(() => encodeWireJsonlLine(event, { maxLineBytes: 16 }), "oversized");
	});

	test("rejects malformed, torn, oversized, noncontiguous, mixed-run, and post-terminal streams", () => {
		const start = encodeWireJsonlLine(started());
		const terminal = encodeWireJsonlLine(terminated(1));
		const malformed = expectJsonlError(() => decodeWireJsonl("{not-json}\n"), "malformed");
		expect(malformed.line).toBe(1);
		expectJsonlError(() => decodeWireJsonl(start.slice(0, -1)), "torn");
		expectJsonlError(() => decodeWireJsonl(start, { maxStreamBytes: 16 }), "oversized");

		const noncontiguous = expectJsonlError(
			() => decodeWireJsonl(start + encodeWireJsonlLine(progress(2))),
			"noncontiguous",
		);
		expect(noncontiguous.line).toBe(2);
		expectJsonlError(
			() => decodeWireJsonl(start + encodeWireJsonlLine(progress(1, OTHER_RUN_ID))),
			"mixed_run",
		);
		const duplicateId = decodeWireEvent({ ...progress(1), id: started().id });
		expectJsonlError(
			() => decodeWireJsonl(start + encodeWireJsonlLine(duplicateId)),
			"noncontiguous",
		);
		expectJsonlError(
			() => decodeWireJsonl(start + terminal + encodeWireJsonlLine(progress(2))),
			"post_terminal",
		);
	});

	test("holds one exclusive writer lock until close and refuses a second writer before append", async () => {
		const file = await tempFile();
		const first = await openWireJsonlWriter(file);
		try {
			const before = await Bun.file(file).text();
			const locked = await expectJsonlErrorAsync(() => openWireJsonlWriter(file), "writer_locked");
			expect(locked.lockPath).toBe(first.lockPath);
			expect(locked.lockEvidence?.status).toBe("owner");
			if (locked.lockEvidence?.status !== "owner") {
				throw new Error("expected parsed writer lock owner evidence");
			}
			expect(locked.lockEvidence.owner.filePath).toBe(file);
			expect(locked.lockEvidence.owner.pid).toBe(process.pid);
			expect(locked.lockEvidence.owner.process.bootId).toMatch(/^[0-9a-f-]{36}$/i);
			expect(locked.lockEvidence.owner.process.startTicks).toMatch(/^\d+$/);
			expect(await Bun.file(file).text()).toBe(before);
			expect(await Bun.file(first.lockPath).exists()).toBe(true);
			await expectJsonlErrorAsync(
				() => openWireJsonlWriter(file, { recoverDeadOwnerLock: true }),
				"writer_locked",
			);
			await first.append(started());
		} finally {
			await first.close();
		}
		expect(await Bun.file(first.lockPath).exists()).toBe(false);
		await expectJsonlErrorAsync(() => first.append(progress(1)), "writer_closed");

		const reopened = await openWireJsonlWriter(file);
		try {
			await reopened.append(progress(1));
		} finally {
			await reopened.close();
		}
		expect((await readWireJsonl(file)).events).toEqual([started(), progress(1)]);
	});

	test("refuses symlink and special-file stable paths without touching their targets", async () => {
		const symlink = await tempFile();
		const victim = path.join(path.dirname(symlink), "empty-victim.jsonl");
		await Bun.write(victim, "");
		await fs.symlink(victim, symlink);

		await expectJsonlErrorAsync(
			() => openWireJsonlWriter(symlink),
			"concurrent_modification",
		);
		expect((await fs.lstat(symlink)).isSymbolicLink()).toBe(true);
		expect(await Bun.file(victim).text()).toBe("");
		expect(await Bun.file(`${symlink}.lock`).exists()).toBe(false);

		const fifo = path.join(path.dirname(symlink), "stable-fifo.jsonl");
		await $`mkfifo ${fifo}`.quiet();
		await expectJsonlErrorAsync(
			() => openWireJsonlWriter(fifo),
			"concurrent_modification",
		);
		expect((await fs.lstat(fifo)).isFIFO()).toBe(true);
		expect(await Bun.file(`${fifo}.lock`).exists()).toBe(false);

		const directory = path.join(path.dirname(symlink), "stable-directory.jsonl");
		await fs.mkdir(directory);
		await expectJsonlErrorAsync(
			() => openWireJsonlWriter(directory),
			"concurrent_modification",
		);
		expect((await fs.lstat(directory)).isDirectory()).toBe(true);
		expect(await Bun.file(`${directory}.lock`).exists()).toBe(false);
	});

	test("public reads refuse symlinks and nonblocking special-file paths", async () => {
		const symlink = await tempFile();
		const emptyTarget = path.join(path.dirname(symlink), "empty-read-target.jsonl");
		await Bun.write(emptyTarget, "");
		await fs.symlink(emptyTarget, symlink);
		await expectJsonlErrorAsync(() => readWireJsonl(symlink), "concurrent_modification");
		expect((await fs.lstat(symlink)).isSymbolicLink()).toBe(true);
		expect(await Bun.file(emptyTarget).text()).toBe("");

		const fifo = path.join(path.dirname(symlink), "read-fifo.jsonl");
		await $`mkfifo ${fifo}`.quiet();
		await expectJsonlErrorAsync(() => readWireJsonl(fifo), "concurrent_modification");
		expect((await fs.lstat(fifo)).isFIFO()).toBe(true);
	});

	test("public reads reject stable-path inode substitution during a held read", async () => {
		const file = await tempFile();
		const displaced = path.join(path.dirname(file), "displaced-read.jsonl");
		const sparse = await fs.open(file, "wx", 0o600);
		await sparse.truncate(48 * 1_024 * 1_024);
		await sparse.close();

		const reading = readWireJsonl(file);
		await Bun.sleep(2);
		await fs.rename(file, displaced);
		await Bun.write(file, "");
		await expectJsonlErrorAsync(() => reading, "concurrent_modification");
		expect((await fs.lstat(file)).ino).not.toBe((await fs.lstat(displaced)).ino);
	});

	test("refuses a multiply-linked stable file without touching its other name", async () => {
		const file = await tempFile();
		const victim = path.join(path.dirname(file), "hard-link-victim.jsonl");
		await Bun.write(victim, "");
		await fs.link(victim, file);

		await expectJsonlErrorAsync(
			() => openWireJsonlWriter(file),
			"concurrent_modification",
		);
		expect((await fs.lstat(file)).nlink).toBe(2);
		expect(await Bun.file(victim).text()).toBe("");
		expect(await Bun.file(`${file}.lock`).exists()).toBe(false);
	});

	test("refuses stable-path inode substitution before appending", async () => {
		const file = await tempFile();
		const displaced = path.join(path.dirname(file), "displaced.jsonl");
		const writer = await openWireJsonlWriter(file);
		await fs.rename(file, displaced);
		await Bun.write(file, "");

		await expectJsonlErrorAsync(() => writer.append(started()), "concurrent_modification");
		expect(await Bun.file(file).text()).toBe("");
		expect(await Bun.file(displaced).text()).toBe("");
		await writer.close();
		expect(await Bun.file(`${file}.lock`).exists()).toBe(false);
	});

	test("refuses an append after its lock is replaced by a second writer", async () => {
		const file = await tempFile();
		const first = await openWireJsonlWriter(file);
		await first.append(started());
		await fs.unlink(first.lockPath);
		const second = await openWireJsonlWriter(file);

		await expectJsonlErrorAsync(() => first.append(progress(1)), "concurrent_modification");
		await expectJsonlErrorAsync(() => first.close(), "concurrent_modification");
		expect(await Bun.file(second.lockPath).exists()).toBe(true);
		try {
			await second.append(progress(1));
		} finally {
			await second.close();
		}
		expect((await readWireJsonl(file)).events).toEqual([started(), progress(1)]);
	});

	test("an ordinary acquirer yields its new lock to an active reclaim marker", async () => {
		const file = await tempFile();
		const owner = await openWireJsonlWriter(file);
		const markerPath = `${owner.lockPath}.reclaiming`;
		await fs.link(owner.lockPath, markerPath);
		await fs.unlink(owner.lockPath);

		await expectJsonlErrorAsync(() => openWireJsonlWriter(file), "writer_locked");
		expect(await Bun.file(owner.lockPath).exists()).toBe(false);
		expect(await Bun.file(markerPath).exists()).toBe(true);
		await expectJsonlErrorAsync(() => owner.close(), "concurrent_modification");
		await fs.unlink(markerPath);
	});

	test("one Bridge recovery wins after a reclaimer crashes with only its marker", async () => {
		const file = await tempFile();
		const child = Bun.spawn([
			process.execPath,
			path.join(import.meta.dir, "fixtures/wire-jsonl-orphan-marker.ts"),
			file,
		], { stdout: "pipe", stderr: "inherit" });
		try {
			const output = (child.stdout as ReadableStream<Uint8Array>).getReader();
			const ready = await output.read();
			output.releaseLock();
			expect(new TextDecoder().decode(ready.value)).toContain("ready");
			child.kill("SIGKILL");
			await child.exited;

			const attempts = await Promise.allSettled([
				openWireJsonlWriter(file, { recoverDeadOwnerLock: true }),
				openWireJsonlWriter(file, { recoverDeadOwnerLock: true }),
			]);
			const recovered = attempts.flatMap((attempt) =>
				attempt.status === "fulfilled" ? [attempt.value] : []);
			const refused = attempts.flatMap((attempt) =>
				attempt.status === "rejected" ? [attempt.reason] : []);
			expect(recovered).toHaveLength(1);
			expect(refused).toHaveLength(1);
			expect(refused[0]).toBeInstanceOf(WireJsonlError);
			expect(refused[0]?.code).toBe("writer_locked");
			const writer = recovered[0];
			if (!writer) throw new Error("expected one recovered wire JSONL writer");
			await writer.append(started());
			await writer.close();
			expect(await Bun.file(`${file}.lock.reclaiming`).exists()).toBe(false);
			expect((await readWireJsonl(file)).events).toEqual([started()]);
		} finally {
			if (child.exitCode === null) {
				child.kill("SIGKILL");
				await child.exited;
			}
		}
	});

	test("recovers after a replacement winner is killed before retiring the older dead marker", async () => {
		const file = await tempFile();
		const orphan = Bun.spawn([
			process.execPath,
			path.join(import.meta.dir, "fixtures/wire-jsonl-orphan-marker.ts"),
			file,
		], { stdout: "pipe", stderr: "inherit" });
		let winner: Bun.Subprocess | undefined;
		try {
			const orphanOutput = (orphan.stdout as ReadableStream<Uint8Array>).getReader();
			const orphanReady = await orphanOutput.read();
			orphanOutput.releaseLock();
			expect(new TextDecoder().decode(orphanReady.value)).toContain("ready");
			orphan.kill("SIGKILL");
			await orphan.exited;

			winner = Bun.spawn([
				process.execPath,
				path.join(import.meta.dir, "fixtures/wire-jsonl-killed-recovery-winner.ts"),
				file,
			], { stdout: "pipe", stderr: "inherit" });
			const winnerOutput = (winner.stdout as ReadableStream<Uint8Array>).getReader();
			const winnerReady = await winnerOutput.read();
			winnerOutput.releaseLock();
			expect(new TextDecoder().decode(winnerReady.value)).toContain("ready");

			const lockPath = `${file}.lock`;
			const markerPath = `${lockPath}.reclaiming`;
			const [liveWinner, olderMarker, winnerOwner, markerOwner] = await Promise.all([
				fs.lstat(lockPath),
				fs.lstat(markerPath),
				Bun.file(lockPath).json() as Promise<Record<string, unknown>>,
				Bun.file(markerPath).json() as Promise<Record<string, unknown>>,
			]);
			expect(liveWinner.ino).not.toBe(olderMarker.ino);
			expect(winnerOwner.token).not.toBe(markerOwner.token);
			expect(winnerOwner.pid).toBe(winner.pid);
			expect(markerOwner.pid).toBe(orphan.pid);

			winner.kill("SIGKILL");
			await winner.exited;

			const attempts = await Promise.allSettled([
				openWireJsonlWriter(file, { recoverDeadOwnerLock: true }),
				openWireJsonlWriter(file, { recoverDeadOwnerLock: true }),
			]);
			const recovered = attempts.flatMap((attempt) =>
				attempt.status === "fulfilled" ? [attempt.value] : []);
			const refused = attempts.flatMap((attempt) =>
				attempt.status === "rejected" ? [attempt.reason] : []);
			expect(recovered).toHaveLength(1);
			expect(refused).toHaveLength(1);
			expect(refused[0]).toBeInstanceOf(WireJsonlError);
			expect(refused[0]?.code).toBe("writer_locked");
			const writer = recovered[0];
			if (!writer) throw new Error("expected one killed-winner recovery");
			await writer.append(started());
			await writer.close();
			expect(await Bun.file(markerPath).exists()).toBe(false);
			expect((await readWireJsonl(file)).events).toEqual([started()]);
		} finally {
			if (orphan.exitCode === null) {
				orphan.kill("SIGKILL");
				await orphan.exited;
			}
			if (winner?.exitCode === null) {
				winner.kill("SIGKILL");
				await winner.exited;
			}
		}
	});

	test("refuses an append when held data bytes drift from its replay", async () => {
		const file = await tempFile();
		const writer = await openWireJsonlWriter(file);
		await writer.append(started());
		await fs.appendFile(file, encodeWireJsonlLine(progress(1)));

		await expectJsonlErrorAsync(() => writer.append(progress(1)), "concurrent_modification");
		await writer.close();
		expect((await readWireJsonl(file)).events).toEqual([started(), progress(1)]);
	});

	test("rotates a terminal stream under its stable lock before accepting a new run", async () => {
		const file = await tempFile();
		const archive = path.join(path.dirname(file), "run.archive.jsonl");
		const firstRun = await openWireJsonlWriter(file);
		await firstRun.append(started());
		await firstRun.append(terminated(1));
		await firstRun.close();
		const prior = await Bun.file(file).text();

		const secondRun = await openWireJsonlWriter(file, { rotateExistingTo: archive });
		try {
			expect(await Bun.file(archive).text()).toBe(prior);
			expect(await Bun.file(file).text()).toBe("");
			expect(secondRun.replay().events).toEqual([]);
			expect(await Bun.file(secondRun.lockPath).exists()).toBe(true);
			await expectJsonlErrorAsync(() => openWireJsonlWriter(file), "writer_locked");
			await secondRun.append(started(OTHER_RUN_ID));
			await secondRun.append(terminated(1, OTHER_RUN_ID));
		} finally {
			await secondRun.close();
		}

		expect((await readWireJsonl(archive)).events).toEqual([started(), terminated(1)]);
		expect((await readWireJsonl(file)).events).toEqual([
			started(OTHER_RUN_ID),
			terminated(1, OTHER_RUN_ID),
		]);
	});

	test("refuses to rotate an incomplete stream or overwrite an archive", async () => {
		const file = await tempFile();
		const archive = path.join(path.dirname(file), "run.archive.jsonl");
		const incomplete = await openWireJsonlWriter(file);
		await incomplete.append(started());
		await incomplete.close();
		const incompleteText = await Bun.file(file).text();

		await expectJsonlErrorAsync(
			() => openWireJsonlWriter(file, { rotateExistingTo: archive }),
			"rotation_refused",
		);
		expect(await Bun.file(file).text()).toBe(incompleteText);
		expect(await Bun.file(archive).exists()).toBe(false);
		expect(await Bun.file(`${file}.lock`).exists()).toBe(false);

		const resumed = await openWireJsonlWriter(file);
		await resumed.append(terminated(1));
		await resumed.close();
		await Bun.write(archive, "occupied\n");
		const terminalText = await Bun.file(file).text();
		await expectJsonlErrorAsync(
			() => openWireJsonlWriter(file, { rotateExistingTo: archive }),
			"rotation_refused",
		);
		expect(await Bun.file(file).text()).toBe(terminalText);
		expect(await Bun.file(archive).text()).toBe("occupied\n");
		expect(await Bun.file(`${file}.lock`).exists()).toBe(false);
	});

	test("refuses to rotate through a replaced stable-path symlink", async () => {
		const file = await tempFile();
		const target = path.join(path.dirname(file), "run.target.jsonl");
		const archive = path.join(path.dirname(file), "run.archive.jsonl");
		const writer = await openWireJsonlWriter(file);
		await writer.append(started());
		await writer.append(terminated(1));
		await writer.close();
		await fs.rename(file, target);
		await fs.symlink(target, file);
		const prior = await Bun.file(target).text();

		await expectJsonlErrorAsync(
			() => openWireJsonlWriter(file, { rotateExistingTo: archive }),
			"concurrent_modification",
		);
		expect((await fs.lstat(file)).isSymbolicLink()).toBe(true);
		expect(await Bun.file(target).text()).toBe(prior);
		expect(await Bun.file(archive).exists()).toBe(false);
		expect(await Bun.file(`${file}.lock`).exists()).toBe(false);
	});

	test("preserves stale lock evidence and reports a torn crash tail after explicit lock recovery", async () => {
		const file = await tempFile();
		const writer = await openWireJsonlWriter(file);
		const lockPath = writer.lockPath;
		const staleEvidence = await Bun.file(lockPath).text();
		await writer.close();

		const complete = encodeWireJsonlLine(started());
		await Bun.write(file, complete.slice(0, -1));
		await Bun.write(lockPath, staleEvidence);
		const locked = await expectJsonlErrorAsync(() => openWireJsonlWriter(file), "writer_locked");
		expect(locked.lockEvidence?.status).toBe("owner");
		expect(await Bun.file(lockPath).text()).toBe(staleEvidence);

		await fs.unlink(lockPath);
		await expectJsonlErrorAsync(() => openWireJsonlWriter(file), "torn");
		expect(await Bun.file(lockPath).exists()).toBe(false);
	});

	test("never removes a replacement lock it no longer owns", async () => {
		const file = await tempFile();
		const writer = await openWireJsonlWriter(file);
		const original = await Bun.file(writer.lockPath).json();
		await fs.unlink(writer.lockPath);
		const replacement = {
			...original,
			token: "replacement-lock-owner",
			pid: process.pid + 1,
		};
		await Bun.write(writer.lockPath, `${JSON.stringify(replacement)}\n`);

		const changed = await expectJsonlErrorAsync(
			() => writer.close(),
			"concurrent_modification",
		);
		expect(changed.lockPath).toBe(writer.lockPath);
		expect(await Bun.file(writer.lockPath).json()).toEqual(replacement);
	});
});
