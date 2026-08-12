import { afterEach, expect, test } from "bun:test";
import { createHash } from "node:crypto";
import {
  mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import {
  distillOneTier1,
  tier1ProjectIdentity,
  tier1ProjectIdentityFromRoot,
  type Tier1DistillationOptions,
  type Tier1ModelRequest,
  type Tier1ModelResult,
} from "../src/tier1-distiller";

const temporary: string[] = [];

afterEach(() => {
  for (const directory of temporary.splice(0)) {
    rmSync(directory, { recursive: true, force: true });
  }
});

function sha256(value: string): string {
  return createHash("sha256").update(value).digest("hex");
}

interface Fixture {
  root: string;
  rawDirectory: string;
  distillationsDirectory: string;
  stateDirectory: string;
  repository: string;
  lineageDigest: string;
  transcript: string;
  options(runner: (request: Tier1ModelRequest) => Promise<Tier1ModelResult>): Tier1DistillationOptions;
}

function fixture(): Fixture {
  const root = mkdtempSync(join(tmpdir(), "north-tier1-distiller-test-"));
  temporary.push(root);
  const rawDirectory = join(root, "streams", "raw");
  const distillationsDirectory = join(root, "streams", "distillations");
  const stateDirectory = join(root, "state");
  mkdirSync(rawDirectory, { recursive: true });
  const repository = "https://example.test/team/project.git";
  const namespace = "managed-launch-fixture";
  const sessionId = "019f-test-session";
  const lineage = `2026/08/12/rollout-2026-08-12T00-00-00-${sessionId}`;
  const lineageDigest = sha256(`north-stream-source-v4\0openai\0${namespace}\0${lineage}`);
  const transcript = [
    JSON.stringify({
      timestamp: "2026-08-12T00:00:00Z",
      type: "session_meta",
      payload: {
        session_id: sessionId,
        cwd: "/removed/worktree",
        git: { repository_url: repository, commit_hash: "a".repeat(40) },
      },
    }),
    JSON.stringify({
      timestamp: "2026-08-12T00:01:00Z",
      type: "response_item",
      payload: { role: "assistant", content: "Decision: keep exact provenance." },
    }),
  ].join("\n") + "\n";
  const rawDigest = sha256(transcript);
  const rawBasename = `2026-08-12-openai-managed.${lineageDigest}.jsonl`;
  writeFileSync(join(rawDirectory, rawBasename), transcript);
  const scope = "openai-managed-launch-0123456789abcdef";
  const cursor = [
    "v4",
    String(Buffer.byteLength(transcript)),
    `/preserved/managed/sessions/${lineage}.jsonl`,
    "openai",
    namespace,
    lineage,
    lineageDigest,
    rawBasename,
    rawDigest,
    "b".repeat(64),
  ].join("\t");
  writeFileSync(join(rawDirectory, `.cursors.v4.${scope}`), `${cursor}\n`);
  writeFileSync(join(rawDirectory, `source-receipt.${scope}.json`), `${JSON.stringify({
    accountHome: "/private/account",
    agentId: "source-agent",
    runId: "run:source-agent-1",
    threadId: "source-work-thread",
    settledAt: "2026-08-12T00:02:00.000Z",
  })}\n`);
  return {
    root,
    rawDirectory,
    distillationsDirectory,
    stateDirectory,
    repository,
    lineageDigest,
    transcript,
    options: (runner) => ({
      rawDirectory,
      distillationsDirectory,
      stateDirectory,
      project: tier1ProjectIdentity(repository),
      streamThread: "stream-thread",
      lineageDigest,
      runner,
      runtime: { verifyStreamThread: () => {} },
    }),
  };
}

function modelResult(body: string, suffix = "1"): Tier1ModelResult {
  return {
    body,
    execution: {
      provider: "anthropic",
      wireRunId: `run:tier1-test-${suffix}`,
      wirePromptSha256: "c".repeat(64),
      promptManifestSha256: "d".repeat(64),
      environmentReceiptSha256: "e".repeat(64),
      availableSkillCatalogSha256: "f".repeat(64),
      activatedResourceClosureSha256: "0".repeat(64),
    },
  };
}

test("one settled mirror produces one provenance-complete write-once artifact", async () => {
  const f = fixture();
  let calls = 0;
  let observedInput: Record<string, unknown> | undefined;
  const runner = async (request: Tier1ModelRequest): Promise<Tier1ModelResult> => {
    calls += 1;
    observedInput = JSON.parse(request.input) as Record<string, unknown>;
    return modelResult("## Decisions\n\n- Keep exact provenance.");
  };

  const created = await distillOneTier1(f.options(runner));
  const source = readFileSync(created.artifactPath, "utf8");
  expect(created.status).toBe("created");
  expect(source).toContain('north-tier1: "north:tier1:v1"');
  expect(source).toContain(`raw-lineage-sha256: "${f.lineageDigest}"`);
  expect(source).toContain(`raw-snapshot-sha256: "${sha256(f.transcript)}"`);
  expect(source).toContain('cursor: "streams/raw/.cursors.v4.openai-managed-launch-0123456789abcdef"');
  expect(source).toContain('source-receipt: "streams/raw/source-receipt.openai-managed-launch-0123456789abcdef.json"');
  expect(source).toContain('source-provider: "openai"');
  expect(source).toContain('source-namespace: "managed-launch-fixture"');
  expect(source).toContain('source-session: "019f-test-session"');
  expect(source).toContain('source-agent: "@source-agent"');
  expect(source).toContain('source-run: "@run:source-agent-1"');
  expect(source).toContain('source-thread: "@source-work-thread"');
  expect(source).toContain('stream-thread: "@stream-thread"');
  expect(source).toContain('distiller-tier: "economy"');
  expect(source).toContain(`distiller-input-sha256: "${created.artifactPath
    ? sha256(JSON.stringify(observedInput)) : "unreachable"}"`);
  expect(source).toContain(`distiller-environment-receipt-sha256: "${"e".repeat(64)}"`);
  expect(source).toContain(`distiller-available-skills-sha256: "${"f".repeat(64)}"`);
  expect(source).toContain(`distiller-activated-resources-sha256: "${"0".repeat(64)}"`);
  expect(source).toContain("## Decisions\n\n- Keep exact provenance.");
  expect(source).not.toContain("/private/account");
  expect(readdirSync(f.distillationsDirectory)).toEqual([
    `2026-08-12-project.${f.lineageDigest}.tier1.md`,
  ]);
  expect(observedInput?.schema).toBe("north:tier1-distillation-input:v1");
  expect(observedInput?.transcript).toBe(f.transcript);
  expect(JSON.stringify(observedInput)).not.toContain("/private/account");
  expect(Object.keys(observedInput ?? {}).sort()).toEqual([
    "projectSha256", "provenance", "schema", "streamThread", "transcript",
  ]);

  const repeated = await distillOneTier1(f.options(runner));
  expect(repeated.status).toBe("already_complete");
  expect(repeated.artifactSha256).toBe(created.artifactSha256);
  expect(readFileSync(repeated.artifactPath, "utf8")).toBe(source);
  expect(calls).toBe(1);
});

test("a crash after artifact publication recovers without another model call", async () => {
  const f = fixture();
  let calls = 0;
  let crash = true;
  const runner = async (): Promise<Tier1ModelResult> => {
    calls += 1;
    return modelResult("## Principles\n\n- Publication is write-once.");
  };
  const options = {
    ...f.options(runner),
    runtime: {
      verifyStreamThread: () => {},
      afterArtifactWrite: () => {
        if (crash) {
          crash = false;
          throw new Error("simulated crash after durable artifact");
        }
      },
    },
  };

  await expect(distillOneTier1(options)).rejects.toThrow("simulated crash");
  expect(readdirSync(f.distillationsDirectory)).toHaveLength(1);
  const recovered = await distillOneTier1(options);
  expect(recovered.status).toBe("recovered");
  expect(calls).toBe(1);
  expect(readdirSync(f.distillationsDirectory)).toHaveLength(1);
});

test("an expired claim rejects late output after a retry takes ownership", async () => {
  const f = fixture();
  let nowMs = Date.parse("2026-08-12T00:03:00.000Z");
  const ids = ["attempt-one", "attempt-two"];
  const firstStarted = Promise.withResolvers<void>();
  const firstResult = Promise.withResolvers<Tier1ModelResult>();
  const runtime = {
    now: () => new Date(nowMs),
    uuid: () => ids.shift()!,
    claimLeaseMs: 100,
    verifyStreamThread: () => {},
  };
  const first = distillOneTier1({
    ...f.options(async () => {
      firstStarted.resolve();
      return firstResult.promise;
    }),
    runtime,
  });
  await firstStarted.promise;
  nowMs += 101;
  const second = await distillOneTier1({
    ...f.options(async () => modelResult("## Decisions\n\n- Retry owns publication.", "2")),
    runtime,
  });
  expect(second.status).toBe("created");

  firstResult.resolve(modelResult("## Decisions\n\n- Stale output.", "1"));
  await expect(first).rejects.toThrow("lost claim ownership and is stale");
  const artifact = readFileSync(second.artifactPath, "utf8");
  expect(artifact).toContain("Retry owns publication.");
  expect(artifact).not.toContain("Stale output.");
  expect(readdirSync(f.distillationsDirectory)).toHaveLength(1);
});

test("a project cannot claim another repository's mirrored session", async () => {
  const f = fixture();
  let called = false;
  const options = f.options(async () => {
    called = true;
    return modelResult("should not run");
  });
  const foreign = tier1ProjectIdentity("https://example.test/team/foreign.git");
  await expect(distillOneTier1({ ...options, project: foreign })).rejects.toThrow(
    "no settled mirrored session matches project and lineage",
  );
  expect(called).toBe(false);
  expect(readdirSync(f.root)).toEqual(["streams"]);
});

test("an unrelated malformed managed receipt cannot poison project discovery", async () => {
  const f = fixture();
  const foreignNamespace = "managed-foreign-fixture";
  const foreignSession = "foreign-session";
  const foreignLineage = `2026/08/12/rollout-${foreignSession}`;
  const foreignDigest = sha256(
    `north-stream-source-v4\0openai\0${foreignNamespace}\0${foreignLineage}`,
  );
  const foreignTranscript = `${JSON.stringify({
    type: "session_meta",
    payload: {
      session_id: foreignSession,
      git: { repository_url: "https://example.test/team/foreign.git" },
    },
  })}\n`;
  const foreignBasename = `2026-08-12-foreign.${foreignDigest}.jsonl`;
  writeFileSync(join(f.rawDirectory, foreignBasename), foreignTranscript);
  const foreignScope = "openai-managed-foreign-fedcba9876543210";
  writeFileSync(join(f.rawDirectory, `.cursors.v4.${foreignScope}`), `${[
    "v4", Buffer.byteLength(foreignTranscript), "/foreign/source.jsonl", "openai",
    foreignNamespace, foreignLineage, foreignDigest, foreignBasename,
    sha256(foreignTranscript), "1".repeat(64),
  ].join("\t")}\n`);
  writeFileSync(join(f.rawDirectory, `source-receipt.${foreignScope}.json`), "{}\n");

  const result = await distillOneTier1(f.options(
    async () => modelResult("## Decisions\n\n- Ignore unrelated scopes."),
  ));
  expect(result.status).toBe("created");
});

test("project identity ignores ambient Git config overrides", async () => {
  const root = mkdtempSync(join(tmpdir(), "north-tier1-project-identity-test-"));
  temporary.push(root);
  const init = Bun.spawnSync(["git", "init", "--quiet", root]);
  expect(init.exitCode).toBe(0);
  const canonical = "https://example.test/team/canonical.git";
  const configured = Bun.spawnSync([
    "git", "-C", root, "config", "--local", "remote.origin.url", canonical,
  ]);
  expect(configured.exitCode).toBe(0);
  const keys = ["GIT_CONFIG_COUNT", "GIT_CONFIG_KEY_0", "GIT_CONFIG_VALUE_0"] as const;
  const inherited = Object.fromEntries(keys.map((key) => [key, process.env[key]]));
  try {
    process.env.GIT_CONFIG_COUNT = "1";
    process.env.GIT_CONFIG_KEY_0 = "remote.origin.url";
    process.env.GIT_CONFIG_VALUE_0 = "https://attacker.test/forged.git";
    const identity = await tier1ProjectIdentityFromRoot(root);
    expect(identity.repository).toBe(canonical);
    expect(identity).toEqual(tier1ProjectIdentity(canonical));
  } finally {
    for (const key of keys) {
      const value = inherited[key];
      if (value === undefined) delete process.env[key];
      else process.env[key] = value;
    }
  }
});

test("crash recovery rejects a published artifact whose provenance changed", async () => {
  const f = fixture();
  let artifactPath = "";
  const options = {
    ...f.options(async () => modelResult("## Decisions\n\n- Preserve provenance.")),
    runtime: {
      verifyStreamThread: () => {},
      afterArtifactWrite: (path: string) => {
        artifactPath = path;
        throw new Error("crash before completion");
      },
    },
  };
  await expect(distillOneTier1(options)).rejects.toThrow("crash before completion");
  const source = readFileSync(artifactPath, "utf8");
  writeFileSync(artifactPath, source.replace(
    `distiller-environment-receipt-sha256: "${"e".repeat(64)}"`,
    `distiller-environment-receipt-sha256: "${"a".repeat(64)}"`,
  ));
  await expect(distillOneTier1(options)).rejects.toThrow(
    "tier-1 artifact changed after publication planning",
  );
});
