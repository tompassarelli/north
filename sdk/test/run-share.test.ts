import { afterEach, expect, test } from "bun:test";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

import { BridgeWireJournal } from "../src/bridge/journal";
import {
	RUN_SHARE_AES_GCM_AAD,
	RUN_SHARE_MAX_EVENTS,
	RUN_SHARE_MAX_PLAINTEXT_BYTES,
	RUN_SHARE_MAX_SEALED_BYTES,
	RUN_SHARE_NONCE_BYTES,
	RUN_SHARE_SEALED_HEADER,
} from "../src/run-share-contract";
import {
	RunShareCliError,
	exportBridgeRunShare,
} from "../src/run-share-cli";
import { renderRunShareViewer } from "../src/run-share-viewer";
import {
	RunShareError,
	buildBridgeRunShareBundle,
	decodeRunShareFragment,
	openRunShareBundle,
	openRunShareFragment,
	redactWireRun,
	sealRunShareBundle,
	type RunShareErrorCode,
} from "../src/run-share";
import {
	WIRE_PROVIDER_JOIN_VERSION,
	WireEventWriter,
	reduceWireEvents,
	wireArtifactId,
	wireEventId,
	wireMessageId,
	wireModelCallId,
	wireResourceId,
	wireRunId,
	wireToolCallId,
	type WireEvent,
} from "../src/wire";

const EXECUTION_ID = "123e4567-e89b-42d3-a456-426614174000";
const RAW_RUN_ID = wireRunId(`bridge:${EXECUTION_ID}`);
const RAW_PARENT_ID = wireRunId("run:private-parent-canary");
const RAW_MESSAGE_ID = wireMessageId("message:private-canary");
const RAW_MODEL_CALL_ID = wireModelCallId("model-call:private-canary");
const RAW_TOOL_CALL_ID = wireToolCallId("tool-call:private-canary");
const RAW_ARTIFACT_ID = wireArtifactId("artifact:private-canary");
const RAW_RESOURCE_ID = wireResourceId("resource:private-canary");
const CONTENT_SECRET = "RUN_SHARE_CONTENT_SECRET_7f8d";
const OWNER_SECRET = "RUN_SHARE_OWNER_SECRET_5a42";
const TOOL_SECRET = "RUN_SHARE_TOOL_SECRET_91ac";
const SCHEMA_SECRET = "RUN_SHARE_SCHEMA_SECRET_1dde";
const RESOURCE_SECRET = "RUN_SHARE_RESOURCE_SECRET_e6b0";
const ARTIFACT_SECRET = "RUN_SHARE_ARTIFACT_SECRET_cb31";
const BRANCH_SECRET = "RUN_SHARE_BRANCH_SECRET_092a";
const ABORT_SECRET = "RUN_SHARE_ABORT_SECRET_417c";
const TOOL_ERROR_SECRET = "private_tool_error_canary";
const MODEL_ERROR_SECRET = "private_model_error_canary";
const HEADER_BYTES = new TextEncoder().encode(RUN_SHARE_SEALED_HEADER);
const AAD_BYTES = new TextEncoder().encode(RUN_SHARE_AES_GCM_AAD);
const roots: string[] = [];

interface SourceFixture {
	readonly root: string;
	readonly stateRoot: string;
	readonly journalRoot: string;
	readonly streamRoot: string;
	readonly events: readonly WireEvent[];
	readonly wirePath: string;
	readonly unreadArtifact: string;
}

afterEach(() => {
	for (const root of roots.splice(0)) fs.rmSync(root, { recursive: true, force: true });
});

function sourceEvents(): readonly WireEvent[] {
	let tick = 0;
	const writer = new WireEventWriter({
		runId: RAW_RUN_ID,
		eventId: (sequence) => wireEventId(`event:private-canary:${sequence}`),
		now: () => new Date(Date.UTC(2026, 7, 12, 4, 0, tick++)).toISOString(),
	});
	const digest = new Bun.CryptoHasher("sha256").update(ARTIFACT_SECRET).digest("hex");
	const providerSession = "a".repeat(64);
	const providerTurn = "b".repeat(64);
	return Object.freeze([
		writer.append({
			kind: "run.started",
			lifecycle: "running",
			parentRunId: RAW_PARENT_ID,
			owner: OWNER_SECRET,
		}),
		writer.append({
			kind: "artifact.published",
			artifactId: RAW_ARTIFACT_ID,
			resourceId: RAW_RESOURCE_ID,
			mediaType: "text/plain",
			bytes: Buffer.byteLength(ARTIFACT_SECRET),
			digest,
			label: ARTIFACT_SECRET,
		}),
		writer.append({
			kind: "model-call.started",
			modelCallId: RAW_MODEL_CALL_ID,
			model: { provider: "openai", tier: "frontier", capabilityClass: "authoring" },
			effort: "high",
			attempt: 1,
		}),
		writer.append({
			kind: "message.recorded",
			messageId: RAW_MESSAGE_ID,
			modelCallId: RAW_MODEL_CALL_ID,
			stage: "started",
			role: "assistant",
		}),
		writer.append({
			kind: "message.recorded",
			messageId: RAW_MESSAGE_ID,
			modelCallId: RAW_MODEL_CALL_ID,
			stage: "delta",
			role: "assistant",
			content: { secret: CONTENT_SECRET },
		}),
		writer.append({
			kind: "message.recorded",
			messageId: RAW_MESSAGE_ID,
			modelCallId: RAW_MODEL_CALL_ID,
			stage: "completed",
			role: "assistant",
			content: CONTENT_SECRET,
		}),
		writer.append({
			kind: "tool.admitted",
			toolCallId: RAW_TOOL_CALL_ID,
			messageId: RAW_MESSAGE_ID,
			modelCallId: RAW_MODEL_CALL_ID,
			name: TOOL_SECRET,
			schema: { status: "valid", source: SCHEMA_SECRET, digest: "c".repeat(64) },
			argumentDigest: "d".repeat(64),
			argumentPreview: CONTENT_SECRET,
			argumentArtifactId: RAW_ARTIFACT_ID,
		}),
		writer.append({
			kind: "tool.progress",
			toolCallId: RAW_TOOL_CALL_ID,
			progress: { secret: CONTENT_SECRET },
			outputArtifactId: RAW_ARTIFACT_ID,
		}),
		writer.append({
			kind: "run.progress",
			lifecycle: "running",
			progress: {
				currentAction: CONTENT_SECRET,
				outputReferences: [
					{ kind: "artifact", artifactId: RAW_ARTIFACT_ID },
					{ kind: "resource", resourceId: RAW_RESOURCE_ID },
				],
				retry: { attempt: 1, maxAttempts: 3, delayMs: 10, reason: CONTENT_SECRET },
				nested: [{ runId: wireRunId("run:nested-private-canary"), lifecycle: "running", currentAction: CONTENT_SECRET }],
				patch: { artifactId: RAW_ARTIFACT_ID, filesChanged: 2 },
				branch: { name: BRANCH_SECRET, base: BRANCH_SECRET },
				abort: { requestedAt: "2026-08-12T04:00:00.000Z", source: "operator", reason: ABORT_SECRET },
			},
		}),
		writer.append({
			kind: "tool.terminal",
			toolCallId: RAW_TOOL_CALL_ID,
			status: "failed",
			origin: "provider",
			resultPreview: CONTENT_SECRET,
			resultArtifactId: RAW_ARTIFACT_ID,
			resultArtifactDigest: digest,
			errorCode: TOOL_ERROR_SECRET,
		}),
		writer.append({
			kind: "model-call.completed",
			modelCallId: RAW_MODEL_CALL_ID,
			status: "failed",
			origin: "provider",
			usage: {
				lifetime: {
					inputTokens: 10,
					outputTokens: 20,
					cacheReadTokens: 2,
					cacheWriteTokens: 1,
					reasoningTokens: 3,
					modelCalls: 1,
				},
				context: { tokens: 30, window: 128_000 },
			},
			usageCoverage: "exact",
			errorCode: MODEL_ERROR_SECRET,
			evidence: {
				providerJoin: {
					version: WIRE_PROVIDER_JOIN_VERSION,
					sessionKey: providerSession,
					turnKeys: [providerTurn],
					sessionPersistence: "persisted",
					coverage: "exact",
				},
				turns: { unit: "assistant-turn", count: 1, comparable: true },
				failure: {
					detail: MODEL_ERROR_SECRET,
					landed: { completedTurns: 0, toolItems: 1 },
				},
			},
		}),
		writer.append({
			kind: "resource.pressure",
			resourceId: RAW_RESOURCE_ID,
			scope: RESOURCE_SECRET,
			resource: RESOURCE_SECRET,
			used: 4,
			reserved: 2,
			limit: 10,
			advisory: true,
		}),
		writer.append({
			kind: "run.terminated",
			lifecycle: "completed",
			reason: { code: "completed", detail: "completed" },
		}),
	]);
}

async function fixture(): Promise<SourceFixture> {
	const root = fs.mkdtempSync(path.join(os.tmpdir(), "north-run-share-"));
	roots.push(root);
	const stateRoot = path.join(root, "bridge");
	const journalRoot = path.join(stateRoot, "journal");
	const streamRoot = path.join(root, "streams");
	fs.mkdirSync(journalRoot, { recursive: true });
	fs.mkdirSync(path.join(streamRoot, "run-artifacts"), { recursive: true });
	const events = sourceEvents();
	const journal = await BridgeWireJournal.open(journalRoot, EXECUTION_ID);
	try {
		for (const event of events) await journal.append(event);
	} finally {
		await journal.close();
	}
	const unreadArtifact = path.join(streamRoot, "run-artifacts", "opaque-secret-source");
	fs.writeFileSync(unreadArtifact, ARTIFACT_SECRET, { mode: 0o600 });
	fs.chmodSync(unreadArtifact, 0o000);
	return {
		root,
		stateRoot,
		journalRoot,
		streamRoot,
		events,
		wirePath: path.join(journalRoot, EXECUTION_ID, "wire.jsonl"),
		unreadArtifact,
	};
}

async function expectRunShareError(
	operation: Promise<unknown>,
	code: RunShareErrorCode,
): Promise<RunShareError> {
	try { await operation; }
	catch (error) {
		expect(error).toBeInstanceOf(RunShareError);
		if (!(error instanceof RunShareError)) throw error;
		expect(error.code).toBe(code);
		return error;
	}
	throw new Error(`expected run-share error ${code}`);
}

async function authenticatedEnvelope(plaintext: Uint8Array): Promise<{
	readonly sealed: Uint8Array;
	readonly key: Uint8Array;
}> {
	const key = crypto.getRandomValues(new Uint8Array(32));
	const nonce = crypto.getRandomValues(new Uint8Array(RUN_SHARE_NONCE_BYTES));
	const imported = await crypto.subtle.importKey("raw", key, "AES-GCM", false, ["encrypt"]);
	const compressed = Bun.gzipSync(plaintext);
	const ciphertext = new Uint8Array(await crypto.subtle.encrypt(
		{ name: "AES-GCM", iv: nonce, additionalData: AAD_BYTES },
		imported,
		compressed,
	));
	const sealed = new Uint8Array(HEADER_BYTES.byteLength + nonce.byteLength + ciphertext.byteLength);
	sealed.set(HEADER_BYTES);
	sealed.set(nonce, HEADER_BYTES.byteLength);
	sealed.set(ciphertext, HEADER_BYTES.byteLength + nonce.byteLength);
	return { sealed, key };
}

test("typed redaction is deterministic, structural, and drops every tainted field", () => {
	const source = sourceEvents();
	const first = redactWireRun(source);
	const second = redactWireRun(source);
	expect(first).toEqual(second);
	expect(reduceWireEvents(first.events).lifecycle).toBe("completed");
	expect(first.events.map((event) => event.sequence)).toEqual(source.map((event) => event.sequence));
	expect(first.events.map((event) => event.kind)).toEqual(source.map((event) => event.kind));
	const serialized = JSON.stringify(first);
	for (const secret of [
		CONTENT_SECRET, OWNER_SECRET, TOOL_SECRET, SCHEMA_SECRET, RESOURCE_SECRET,
		ARTIFACT_SECRET, BRANCH_SECRET, ABORT_SECRET, TOOL_ERROR_SECRET, MODEL_ERROR_SECRET,
		RAW_RUN_ID, RAW_PARENT_ID,
		RAW_MESSAGE_ID, RAW_MODEL_CALL_ID, RAW_TOOL_CALL_ID, RAW_ARTIFACT_ID, RAW_RESOURCE_ID,
		"2026-08-12T04:00:00.000Z", "a".repeat(64), "b".repeat(64), "c".repeat(64), "d".repeat(64),
	]) expect(serialized).not.toContain(secret);
	const terminal = first.events.find((event) => event.kind === "run.terminated");
	expect(terminal).toMatchObject({ reason: { code: "completed" } });
	expect(terminal && "reason" in terminal && "detail" in terminal.reason).toBe(false);
	expect(first.artifacts).toEqual([{
		artifactId: "artifact:share:1",
		mediaType: "application/vnd.north.redacted+json",
		bytes: 17,
		digest: new Bun.CryptoHasher("sha256").update('{"redacted":true}').digest("hex"),
		content: '{"redacted":true}',
	}]);
});

test("AES-GCM keeps the key fragment-only and rejects missing, wrong, or tampered material", async () => {
	const bundle = redactWireRun(sourceEvents());
	const seal = await sealRunShareBundle(bundle);
	const key = decodeRunShareFragment(`#${seal.fragment}`);
	const viewer = renderRunShareViewer(seal.sealed);
	expect(seal.fragment).toMatch(/^[A-Za-z0-9_-]{43}$/);
	expect(Buffer.from(seal.sealed).includes(Buffer.from(seal.fragment))).toBe(false);
	expect(viewer).not.toContain(seal.fragment);
	expect(await openRunShareBundle(seal.sealed, key)).toEqual(bundle);
	await expectRunShareError(openRunShareFragment(seal.sealed, "#"), "missing_key");
	const wrongKey = new Uint8Array(key);
	wrongKey[0] ^= 1;
	await expectRunShareError(openRunShareBundle(seal.sealed, wrongKey), "authentication_failed");
	const tampered = new Uint8Array(seal.sealed);
	tampered[tampered.byteLength - 1] ^= 1;
	await expectRunShareError(openRunShareBundle(tampered, key), "authentication_failed");
});

test("event, source, envelope, and authenticated decompression bounds fail closed", async () => {
	const source = sourceEvents();
	expect(() => redactWireRun(source, source.length - 1)).toThrow(RunShareError);
	expect(RUN_SHARE_MAX_EVENTS).toBeGreaterThan(source.length);
	const persisted = await fixture();
	await expect(buildBridgeRunShareBundle(EXECUTION_ID, {
		journalRoot: persisted.journalRoot,
		maxSourceBytes: 1,
	})).rejects.toThrow();
	const oversized = new Uint8Array(RUN_SHARE_MAX_SEALED_BYTES + 1);
	oversized.set(HEADER_BYTES);
	await expectRunShareError(openRunShareBundle(oversized, new Uint8Array(32)), "invalid_bundle");
	const bomb = await authenticatedEnvelope(new Uint8Array(RUN_SHARE_MAX_PLAINTEXT_BYTES + 1).fill(0x61));
	expect(bomb.sealed.byteLength).toBeLessThan(RUN_SHARE_MAX_SEALED_BYTES);
	await expectRunShareError(openRunShareBundle(bomb.sealed, bomb.key), "output_limit_exceeded");
});

test("CLI publishes private files once without reading or changing source material", async () => {
	const source = await fixture();
	const output = path.join(source.root, "shared-run");
	const beforeWire = fs.readFileSync(source.wirePath);
	const beforeWireStat = fs.statSync(source.wirePath);
	const previousBridge = process.env.NORTH_BRIDGE_STATE_DIR;
	const previousStream = process.env.NORTH_STREAM_DIR;
	process.env.NORTH_BRIDGE_STATE_DIR = source.stateRoot;
	process.env.NORTH_STREAM_DIR = source.streamRoot;
	try {
		const processResult = Bun.spawn([
			path.resolve(import.meta.dir, "../../bin/north"),
			"run-share", "export", EXECUTION_ID, "--out", output,
		], {
			env: { ...process.env, NORTH_BUN: process.execPath },
			stdout: "pipe",
			stderr: "pipe",
		});
		const [status, stdout, stderr] = await Promise.all([
			processResult.exited,
			new Response(processResult.stdout).text(),
			new Response(processResult.stderr).text(),
		]);
		expect(status).toBe(0);
		expect(stderr).toBe("");
		expect(stdout.match(/\n/g)).toHaveLength(1);
		const link = new URL(stdout.trim());
		expect(link.protocol).toBe("file:");
		expect(link.hash).toMatch(/^#[A-Za-z0-9_-]{43}$/);
		const bundlePath = path.join(output, "run.northshare");
		const viewerPath = path.join(output, "viewer.html");
		expect(fs.statSync(output).mode & 0o777).toBe(0o700);
		expect(fs.statSync(bundlePath).mode & 0o777).toBe(0o600);
		expect(fs.statSync(viewerPath).mode & 0o777).toBe(0o600);
		expect(fs.realpathSync(decodeURIComponent(link.pathname))).toBe(fs.realpathSync(viewerPath));
		const sealed = fs.readFileSync(bundlePath);
		const viewer = fs.readFileSync(viewerPath, "utf8");
		expect(sealed.includes(Buffer.from(link.hash.slice(1)))).toBe(false);
		expect(viewer).not.toContain(link.hash.slice(1));
		const opened = await openRunShareFragment(sealed, link.hash);
		expect(reduceWireEvents(opened.events).lifecycle).toBe("completed");
		for (const secret of [CONTENT_SECRET, ARTIFACT_SECRET, OWNER_SECRET, TOOL_SECRET]) {
			expect(JSON.stringify(opened)).not.toContain(secret);
			expect(viewer).not.toContain(secret);
		}
		expect(fs.readFileSync(source.wirePath)).toEqual(beforeWire);
		const afterWireStat = fs.statSync(source.wirePath);
		expect({ size: afterWireStat.size, mtimeMs: afterWireStat.mtimeMs, mode: afterWireStat.mode })
			.toEqual({ size: beforeWireStat.size, mtimeMs: beforeWireStat.mtimeMs, mode: beforeWireStat.mode });
		fs.chmodSync(source.unreadArtifact, 0o600);
		expect(fs.readFileSync(source.unreadArtifact, "utf8")).toBe(ARTIFACT_SECRET);
		const bundleBefore = fs.readFileSync(bundlePath);
		await expect(exportBridgeRunShare(EXECUTION_ID, output)).rejects.toMatchObject({ code: "output_exists" });
		expect(fs.readFileSync(bundlePath)).toEqual(bundleBefore);
		const victim = path.join(source.root, "victim");
		const symbolicOutput = path.join(source.root, "symbolic-output");
		fs.mkdirSync(victim);
		fs.symlinkSync(victim, symbolicOutput);
		await expect(exportBridgeRunShare(EXECUTION_ID, symbolicOutput)).rejects.toBeInstanceOf(RunShareCliError);
		expect(fs.readdirSync(victim)).toEqual([]);
		await expect(exportBridgeRunShare(
			EXECUTION_ID,
			path.join(source.journalRoot, "forbidden-output"),
		)).rejects.toMatchObject({ code: "output_conflicts_source" });
	} finally {
		if (previousBridge === undefined) delete process.env.NORTH_BRIDGE_STATE_DIR;
		else process.env.NORTH_BRIDGE_STATE_DIR = previousBridge;
		if (previousStream === undefined) delete process.env.NORTH_STREAM_DIR;
		else process.env.NORTH_STREAM_DIR = previousStream;
		try { fs.chmodSync(source.unreadArtifact, 0o600); } catch { /* fixture cleanup remains bounded. */ }
	}
});

test("generated viewer is deterministic and exposes no network or run-control surface", async () => {
	const seal = await sealRunShareBundle(redactWireRun(sourceEvents()));
	const first = renderRunShareViewer(seal.sealed);
	const second = renderRunShareViewer(seal.sealed);
	expect(first).toBe(second);
	expect(first).toContain("connect-src 'none'");
	expect(first).toContain("form-action 'none'");
	expect(first).toContain("default-src 'none'");
	expect(first).not.toMatch(/https?:|wss?:/i);
	expect(first).not.toContain("<form");
	expect(first).not.toContain("<button");
	for (const capability of [
		"WebSocket", "XMLHttpRequest", "fetch(", "prompt(", "submitInput",
		"interruptTurn", "redirectNow", "terminateSession",
	]) expect(first).not.toContain(capability);
});
