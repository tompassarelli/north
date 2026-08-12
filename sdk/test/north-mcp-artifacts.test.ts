import { afterEach, expect, test } from "bun:test";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { spawnSync } from "node:child_process";
import { RunArtifactStore } from "../src/run-artifacts";
import { wireArtifactId, wireRunId, type WireArtifactId } from "../src/wire";

const server = path.resolve(import.meta.dir, "../../bin/north-mcp");
const originalStreamDirectory = process.env.NORTH_STREAM_DIR;
const temporary: string[] = [];

interface TextContent {
  type: string;
  text: string;
}

interface ToolDescriptor {
  name: string;
  annotations?: Record<string, boolean>;
  inputSchema: {
    properties: Record<string, unknown>;
    required?: string[];
    additionalProperties?: boolean;
  };
}

interface McpResponse {
  result: {
    isError?: boolean;
    content?: TextContent[];
    tools?: ToolDescriptor[];
  };
}

interface ArtifactPage {
  protocol: string;
  version: number;
  artifactId: string;
  snapshot: string;
  offset: number;
  limit: number;
  total: number;
  nextOffset: number | null;
  complete: boolean;
  state: string;
  content: string;
}

afterEach(() => {
  if (originalStreamDirectory === undefined) delete process.env.NORTH_STREAM_DIR;
  else process.env.NORTH_STREAM_DIR = originalStreamDirectory;
  for (const directory of temporary.splice(0))
    fs.rmSync(directory, { recursive: true, force: true });
});

function call(id: number, args: Record<string, unknown>) {
  return {
    jsonrpc: "2.0",
    id,
    method: "tools/call",
    params: { name: "artifact_read", arguments: args },
  };
}

function rpc(
  requests: Record<string, unknown>[],
  artifactDirectory?: string,
): McpResponse[] {
  const environment = {
    ...process.env,
    NORTH_BIN: "/bin/true",
    NORTH_MCP_BUN: process.execPath,
  };
  if (artifactDirectory === undefined) delete environment.NORTH_RUN_ARTIFACT_DIR;
  else environment.NORTH_RUN_ARTIFACT_DIR = artifactDirectory;
  const result = spawnSync("bb", [server], {
    input: `${requests.map((request) => JSON.stringify(request)).join("\n")}\n`,
    encoding: "utf8",
    env: environment,
  });
  expect(result.status, result.stderr).toBe(0);
  expect(result.stderr).toBe("");
  return result.stdout.trim().split("\n").filter(Boolean)
    .map((line) => JSON.parse(line) as McpResponse);
}

function contentText(response: McpResponse): string {
  const content = response.result.content;
  expect(content).toHaveLength(1);
  if (!content) throw new Error("MCP response omitted content");
  return content[0].text;
}

function text(response: McpResponse): unknown {
  return JSON.parse(contentText(response)) as unknown;
}

function retainedArtifact(content: string): {
  artifactId: WireArtifactId;
  digest: string;
  directory: string;
} {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "north-mcp-artifacts-"));
  temporary.push(root);
  process.env.NORTH_STREAM_DIR = root;
  const store = new RunArtifactStore(wireRunId("run:mcp-artifact-test"));
  const artifactId = wireArtifactId("artifact:test:mcp-output");
  const digest = new Bun.CryptoHasher("sha256").update(content).digest("hex");
  expect(store.persist({
    artifactId,
    mediaType: "text/plain; charset=utf-8",
    content,
    digest,
    label: "MCP artifact paging contract",
  })).toEqual({ artifactId, digest });
  return { artifactId, digest, directory: store.directory };
}

test("artifact_read advertises a local read-only bounded continuation contract", () => {
  const [listed, invalidId, invalidLimit, unknown] = rpc([
    { jsonrpc: "2.0", id: 1, method: "tools/list", params: {} },
    call(2, { artifactId: "../outside" }),
    call(3, { artifactId: "artifact:valid", limit: 65_537 }),
    call(4, { artifactId: "artifact:valid", runId: "run:forbidden" }),
  ]);
  const tools = listed.result.tools;
  if (!tools) throw new Error("tools/list response omitted tools");
  const descriptor = tools.find((tool) => tool.name === "artifact_read");
  expect(descriptor).toMatchObject({
    annotations: {
      readOnlyHint: true,
      destructiveHint: false,
      idempotentHint: true,
      openWorldHint: false,
    },
    inputSchema: {
      properties: {
        artifactId: {
          type: "string",
          pattern: "^[A-Za-z0-9@][A-Za-z0-9@_.:/-]{0,255}$",
        },
        offset: { type: "integer", minimum: 0, maximum: 2147483647, default: 0 },
        limit: { type: "integer", minimum: 1, maximum: 65536, default: 16384 },
        snapshot: { type: "string", pattern: "^[0-9a-f]{64}$" },
      },
      required: ["artifactId"],
      additionalProperties: false,
    },
  });
  expect(invalidId.result.isError).toBe(true);
  expect(contentText(invalidId)).toBe("artifactId must be a valid Wire artifact ID");
  expect(invalidLimit.result.isError).toBe(true);
  expect(contentText(invalidLimit)).toBe("limit must be an integer from 1 through 65536");
  expect(unknown.result.isError).toBe(true);
  expect(contentText(unknown)).toBe("unknown artifact_read arguments: runId");
});

test("artifact_read retrieves one run-confined artifact through coherent UTF-8 pages", () => {
  const content = "first line\n第二行🙂\nlast line";
  const artifact = retainedArtifact(content);
  const first = rpc([call(1, { artifactId: artifact.artifactId, limit: 12 })], artifact.directory)[0];
  const firstPage = text(first) as ArtifactPage;
  expect(first.result.isError).toBe(false);
  expect(firstPage).toMatchObject({
    protocol: "north.page",
    version: 1,
    artifactId: artifact.artifactId,
    snapshot: artifact.digest,
    offset: 0,
    limit: 12,
    total: Buffer.byteLength(content, "utf8"),
    complete: false,
    state: "incomplete",
  });
  expect(Buffer.byteLength(firstPage.content, "utf8")).toBeLessThanOrEqual(12);
  expect(firstPage.content).not.toContain("�");

  const pages = [firstPage];
  while (!pages.at(-1).complete) {
    const previous = pages.at(-1);
    const response = rpc([call(pages.length + 1, {
      artifactId: artifact.artifactId,
      offset: previous.nextOffset,
      limit: 12,
      snapshot: artifact.digest,
    })], artifact.directory)[0];
    expect(response.result.isError).toBe(false);
    pages.push(text(response) as ArtifactPage);
  }
  expect(pages.map((page) => page.content).join("")).toBe(content);
  expect(pages.at(-1)).toMatchObject({
    snapshot: artifact.digest,
    complete: true,
    state: "complete",
    nextOffset: null,
  });
});

test("artifact_read fails closed for stale continuations and absent run authority", () => {
  const artifact = retainedArtifact("retained material");
  const stale = "a".repeat(64);
  const [staleResponse] = rpc([call(1, {
    artifactId: artifact.artifactId,
    offset: 1,
    limit: 4,
    snapshot: stale,
  })], artifact.directory);
  expect(staleResponse.result.isError).toBe(true);
  expect(text(staleResponse)).toEqual({
    error: "stale_snapshot",
    expected: stale,
    actual: artifact.digest,
  });

  const [missing] = rpc([call(3, { artifactId: "artifact:test:missing" })], artifact.directory);
  expect(missing.result.isError).toBe(true);
  expect(text(missing)).toEqual({ error: "artifact_not_found" });

  const [unavailable] = rpc([call(2, { artifactId: artifact.artifactId })]);
  expect(unavailable.result.isError).toBe(true);
  expect(text(unavailable)).toEqual({ error: "artifact_directory_unavailable" });
});
