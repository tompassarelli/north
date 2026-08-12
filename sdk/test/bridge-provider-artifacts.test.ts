import { afterEach, expect, test } from "bun:test";
import { createHash } from "node:crypto";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  bridgeProviderWithDependenciesForTest,
  type BridgeProviderRouting,
} from "../src/bridge/provider";
import type { AgentProvider, AgentProviderQuery } from "../src/providers/types";
import { readRunArtifactPage, RunArtifactStore } from "../src/run-artifacts";
import {
  WireEventWriter,
  wireArtifactId,
  wireRunId,
  type WireEvent,
  type WireQuery,
} from "../src/wire";

const cleanups: Array<() => void> = [];
afterEach(() => {
  for (const cleanup of cleanups.splice(0).reverse()) cleanup();
});

class EmptyQuery implements WireQuery {
  async interrupt(): Promise<void> {}
  async close(): Promise<void> {}
  async *[Symbol.asyncIterator](): AsyncIterator<WireEvent> {}
}

const routing: BridgeProviderRouting = {
  BOOT_ROUTING_TIMEOUT_MS: 1,
  selectProviderFromCachedState: async () => undefined,
  refreshProviderRoutingInBackground: () => Promise.resolve(),
  selectProviderForExecution: async () => { throw new Error("no routed target"); },
  configuredDefaultTarget: () => undefined,
};

function artifactDirectory(args: AgentProviderQuery): string {
  const north = args.options.mcpServers?.north as {
    env?: Record<string, string>;
  } | undefined;
  const directory = north?.env?.NORTH_RUN_ARTIFACT_DIR;
  if (!directory) throw new Error("Bridge query omitted the run artifact selector");
  return directory;
}

test("Bridge provider executions retain artifacts behind their deterministic MCP selector", async () => {
  const streamRoot = mkdtempSync(join(tmpdir(), "north-bridge-artifacts-"));
  const priorStreamRoot = process.env.NORTH_STREAM_DIR;
  process.env.NORTH_STREAM_DIR = streamRoot;
  cleanups.push(() => {
    if (priorStreamRoot === undefined) delete process.env.NORTH_STREAM_DIR;
    else process.env.NORTH_STREAM_DIR = priorStreamRoot;
  });
  cleanups.push(() => rmSync(streamRoot, { recursive: true, force: true }));

  const queries: AgentProviderQuery[] = [];
  const provider: AgentProvider = {
    id: "openai",
    liveInput: "turn-framed",
    probe: () => ({ provider: "openai", available: true, reason: "ready" }),
    admit: () => {},
    query(args) {
      queries.push(args);
      return new EmptyQuery();
    },
  };
  const bridge = bridgeProviderWithDependenciesForTest(
    { anthropic: provider, openai: provider },
    routing,
  );
  const writer = new WireEventWriter({ runId: wireRunId("run:bridge-artifact-contract") });
  writer.append({ kind: "run.started", lifecycle: "running", owner: "bridge:implementer" });
  const first = await bridge.open({
    executionId: "artifact-contract",
    prompt: "retain the result",
    cwd: streamRoot,
    role: "implementer",
    provider: "openai",
    signal: new AbortController().signal,
    writer,
  });

  const firstQuery = queries[0]!;
  expect(firstQuery.context.artifacts).toBeInstanceOf(RunArtifactStore);
  const directory = artifactDirectory(firstQuery);
  expect(directory).toBe((firstQuery.context.artifacts as RunArtifactStore).directory);
  const content = "Bridge durable output \u4fdd\u7559";
  const digest = createHash("sha256").update(content).digest("hex");
  const artifactId = wireArtifactId("artifact:bridge:retained-output");
  expect(firstQuery.context.artifacts!.persist({
    artifactId,
    mediaType: "text/plain",
    content,
    digest,
    label: "command output",
  })).toEqual({ artifactId, digest });

  const page = await readRunArtifactPage(directory, { artifactId, limit: 1_024 });
  expect(page.artifactId).toBe(artifactId);
  expect(page.snapshot).toBe(digest);
  expect(page.content).toBe(content);
  expect(page.complete).toBe(true);

  const restored = WireEventWriter.restore(writer.events());
  const reopened = await bridge.open({
    executionId: "artifact-contract",
    prompt: "continue",
    cwd: streamRoot,
    role: "implementer",
    provider: "openai",
    signal: new AbortController().signal,
    writer: restored,
  });
  expect(artifactDirectory(queries[1]!)).toBe(directory);

  await reopened.terminateSession();
  await first.terminateSession();
});
