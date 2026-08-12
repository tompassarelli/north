import { afterEach, expect, test } from "bun:test";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

import {
	readRunArtifactPage,
	RUN_ARTIFACT_PAGE_MAX_ENCODED_BYTES,
	RunArtifactError,
	RunArtifactStore,
} from "../src/run-artifacts";
import { wireArtifactId, wireRunId } from "../src/wire";

const temporary: string[] = [];
const originalStreamDirectory = process.env.NORTH_STREAM_DIR;

afterEach(() => {
	if (originalStreamDirectory === undefined) delete process.env.NORTH_STREAM_DIR;
	else process.env.NORTH_STREAM_DIR = originalStreamDirectory;
	for (const directory of temporary.splice(0)) fs.rmSync(directory, { recursive: true, force: true });
});

function store(label: string): RunArtifactStore {
	const root = fs.mkdtempSync(path.join(os.tmpdir(), "north-run-artifacts-"));
	temporary.push(root);
	process.env.NORTH_STREAM_DIR = root;
	return new RunArtifactStore(wireRunId(`run:test:${label}`));
}

function persist(target: RunArtifactStore, id: string, content: string) {
	const artifactId = wireArtifactId(id);
	const digest = new Bun.CryptoHasher("sha256").update(content).digest("hex");
	return {
		artifactId,
		digest,
		receipt: target.persist({ artifactId, mediaType: "text/plain; charset=utf-8", content, digest }),
	};
}

test("run artifacts are hash-confined, private, immutable, and exactly idempotent", () => {
	const target = store("confined");
	const saved = persist(target, "artifact:hostile/../../still-confined", "durable bytes");
	expect(saved.receipt).toEqual({ artifactId: saved.artifactId, digest: saved.digest });
	expect(path.basename(target.directory)).toMatch(/^run-[a-f0-9]{64}$/);
	expect(fs.statSync(target.directory).mode & 0o777).toBe(0o700);
	const files = fs.readdirSync(target.directory).filter((name) => name.startsWith("artifact-"));
	expect(files).toHaveLength(1);
	expect(files[0]).toMatch(/^artifact-[a-f0-9]{64}\.json$/);
	expect(fs.statSync(path.join(target.directory, files[0]!)).mode & 0o777).toBe(0o600);
	expect(persist(target, saved.artifactId, "durable bytes").receipt).toEqual(saved.receipt);
	expect(() => persist(target, saved.artifactId, "different bytes"))
		.toThrow("different material");
});

test("the store preserves its shared root permissions and rejects non-round-trippable text", () => {
	const root = fs.mkdtempSync(path.join(os.tmpdir(), "north-run-artifact-root-"));
	temporary.push(root);
	fs.chmodSync(root, 0o755);
	process.env.NORTH_STREAM_DIR = root;
	const target = new RunArtifactStore(wireRunId("run:test:shared-root"));
	expect(fs.statSync(root).mode & 0o777).toBe(0o755);
	const content = "\ud800";
	const artifactId = wireArtifactId("artifact:test:ill-formed");
	const digest = new Bun.CryptoHasher("sha256").update(content).digest("hex");
	expect(() => target.persist({ artifactId, mediaType: "text/plain", content, digest }))
		.toThrow("invalid shape");
});

test("artifact pages reconstruct UTF-8 content with digest fencing and exhaustion", () => {
	const target = store("paging");
	const content = "head\n第二行🙂\ntail";
	const saved = persist(target, "artifact:test:paging", content);
	const chunks: string[] = [];
	let offset = 0;
	while (true) {
		const page = readRunArtifactPage(target.directory, {
			artifactId: saved.artifactId,
			offset,
			limit: 7,
			...(offset === 0 ? {} : { snapshot: saved.digest }),
		});
		chunks.push(page.content);
		if (page.complete) break;
		if (page.nextOffset === null) throw new Error("incomplete artifact page omitted continuation");
		offset = page.nextOffset;
	}
	expect(chunks.join("")).toBe(content);
	expect(readRunArtifactPage(target.directory, {
		artifactId: saved.artifactId,
		offset: Buffer.byteLength(content) + 5,
		snapshot: saved.digest,
	})).toMatchObject({ state: "exhausted", content: "", complete: true, nextOffset: null });
	expect(() => readRunArtifactPage(target.directory, {
		artifactId: saved.artifactId,
		snapshot: "a".repeat(64),
	})).toThrow(RunArtifactError);
});

test("retrieval refuses split UTF-8 offsets, symbolic records, and expansion beyond its hard response cap", () => {
	const target = store("unsafe");
	const saved = persist(target, "artifact:test:utf8", "🙂safe");
	expect(() => readRunArtifactPage(target.directory, {
		artifactId: saved.artifactId,
		offset: 1,
	})).toThrow("splits a UTF-8 sequence");

	const expanded = persist(target, "artifact:test:expanded", "\u0001".repeat(65_536));
	const page = readRunArtifactPage(target.directory, {
		artifactId: expanded.artifactId,
		limit: 65_536,
	});
	expect(Buffer.byteLength(JSON.stringify(page))).toBeLessThanOrEqual(RUN_ARTIFACT_PAGE_MAX_ENCODED_BYTES);

	const source = fs.readdirSync(target.directory)
		.find((name) => name.startsWith("artifact-") && fs.readFileSync(path.join(target.directory, name), "utf8")
			.includes(saved.artifactId));
	if (!source) throw new Error("fixture artifact record missing");
	fs.unlinkSync(path.join(target.directory, source));
	fs.symlinkSync("/etc/passwd", path.join(target.directory, source));
	expect(() => readRunArtifactPage(target.directory, { artifactId: saved.artifactId }))
		.toThrow("unsafe");
});
