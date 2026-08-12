import { afterEach, expect, test } from "bun:test";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { spawnSync } from "node:child_process";

const server = path.resolve(import.meta.dir, "../../bin/north-mcp");
const temporary: string[] = [];

interface TextContent {
  type: string;
  text: string;
}

interface ToolDescriptor {
  name: string;
  inputSchema: {
    properties: Record<string, unknown>;
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

interface PageItemFragment {
  index: number;
  encoding: string;
  byteOffset: number;
  totalBytes: number;
  content: string;
}

interface PageResponse {
  protocol: string;
  version: number;
  snapshot: string;
  offset: number;
  limit: number;
  maxResponseBytes: number;
  total: number;
  nextOffset: number | null;
  nextItemByteOffset?: number | null;
  complete: boolean;
  state: string;
  items: unknown[];
  itemFragment?: PageItemFragment;
}

afterEach(() => {
  for (const directory of temporary.splice(0))
    fs.rmSync(directory, { recursive: true, force: true });
});

function call(id: number, name: string, args: Record<string, unknown>) {
  return { jsonrpc: "2.0", id, method: "tools/call", params: { name, arguments: args } };
}

function rpc(projections: unknown[][], requests: Record<string, unknown>[]): McpResponse[] {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "north-mcp-pages-"));
  temporary.push(directory);
  const fake = path.join(directory, "north");
  const projectionFile = path.join(directory, "projections.json");
  const countFile = path.join(directory, "count");
  fs.writeFileSync(projectionFile, JSON.stringify(projections));
  fs.writeFileSync(fake, `#!/usr/bin/env bun
const projections = await Bun.file(process.env.FAKE_PROJECTION_FILE).json();
let count = 0;
try { count = Number(await Bun.file(process.env.FAKE_INVOCATION_FILE).text()); } catch {}
await Bun.write(process.env.FAKE_INVOCATION_FILE, String(count + 1));
const selected = projections[Math.min(count, projections.length - 1)];
process.stdout.write(JSON.stringify(selected));
`);
  fs.chmodSync(fake, 0o755);
  const result = spawnSync("bb", [server], {
    input: `${requests.map((request) => JSON.stringify(request)).join("\n")}\n`,
    encoding: "utf8",
    env: {
      ...process.env,
      NORTH_BIN: fake,
      FAKE_PROJECTION_FILE: projectionFile,
      FAKE_INVOCATION_FILE: countFile,
    },
  });
  expect(result.status, result.stderr).toBe(0);
  expect(result.stderr).toBe("");
  return result.stdout.trim().split("\n").filter(Boolean)
    .map((line) => JSON.parse(line) as McpResponse);
}

function sha256(value: unknown): string {
  return new Bun.CryptoHasher("sha256").update(JSON.stringify(value)).digest("hex");
}

function contentText(response: McpResponse): string {
  const content = response.result.content;
  expect(content).toHaveLength(1);
  if (!content) throw new Error("MCP response omitted content");
  return content[0].text;
}

function text(response: McpResponse): unknown {
  return JSON.parse(contentText(response));
}

test("show returns coherent bounded pages without changing its complete projection snapshot", () => {
  const facts = [
    { predicate: "a", value: "one" },
    { predicate: "b", value: "two" },
    { predicate: "c", value: "three" },
    { predicate: "d", value: "four" },
    { predicate: "e", value: "five" },
  ];
  const snapshot = sha256(facts);
  const responses = rpc([facts], [
    call(1, "show", { id: "thread-1", limit: 2 }),
    call(2, "show", { id: "thread-1", offset: 2, limit: 2, snapshot }),
    call(3, "show", { id: "thread-1", offset: 5, limit: 2, snapshot }),
  ]);

  expect(text(responses[0])).toEqual({
    protocol: "north.page", version: 1, snapshot, offset: 0, limit: 2,
    maxResponseBytes: 65_536, total: 5, nextOffset: 2, complete: false,
    state: "incomplete", items: facts.slice(0, 2),
  });
  expect(text(responses[1])).toEqual({
    protocol: "north.page", version: 1, snapshot, offset: 2, limit: 2,
    maxResponseBytes: 65_536, total: 5, nextOffset: 4, complete: false,
    state: "incomplete", items: facts.slice(2, 4),
  });
  expect(text(responses[2])).toEqual({
    protocol: "north.page", version: 1, snapshot, offset: 5, limit: 2,
    maxResponseBytes: 65_536, total: 5, nextOffset: null, complete: true,
    state: "exhausted", items: [],
  });
});

test("show and search losslessly fragment arbitrarily large graph values within the byte bound", () => {
  const hugeValue = '🙂"\\'.repeat(12_000);
  const itemsByTool = {
    show: [{ predicate: "note", value: hugeValue }],
    search: [{ subject: "thread-1", title: "Large", predicate: "note", value: hugeValue }],
  };

  for (const [name, items] of Object.entries(itemsByTool)) {
    const snapshot = sha256(items);
    const fragments: string[] = [];
    let offset = 0;
    let itemByteOffset = 0;
    let complete = false;

    for (let pageNumber = 0; pageNumber < 20 && !complete; pageNumber += 1) {
      const args: Record<string, unknown> = name === "show"
        ? { id: "thread-1", offset, limit: 100 }
        : { query: "large", offset, limit: 100 };
      if (itemByteOffset > 0) {
        args.itemByteOffset = itemByteOffset;
        args.snapshot = snapshot;
      }
      const response = rpc([items], [call(pageNumber + 1, name, args)])[0];
      expect(response.result.isError).toBe(false);
      const page = text(response) as PageResponse;
      expect(page.maxResponseBytes).toBe(65_536);
      expect(Buffer.byteLength(JSON.stringify(response), "utf8"))
        .toBeLessThanOrEqual(page.maxResponseBytes);
      expect(page.snapshot).toBe(snapshot);
      expect(page.items).toEqual([]);
      expect(page.itemFragment).toMatchObject({
        index: 0,
        encoding: "json",
        byteOffset: itemByteOffset,
      });
      if (!page.itemFragment) throw new Error("oversized item omitted its fragment");
      fragments.push(page.itemFragment.content);
      complete = page.complete;
      if (!complete) {
        expect(page.nextOffset).toBe(0);
        expect(page.nextItemByteOffset).toBeGreaterThan(itemByteOffset);
        offset = page.nextOffset ?? 0;
        itemByteOffset = page.nextItemByteOffset ?? 0;
      }
    }

    expect(complete).toBe(true);
    expect(JSON.parse(fragments.join(""))).toEqual(items[0]);
  }
});

test("byte packing continues from whole items through a fragmented item to the next item", () => {
  const facts = [
    { predicate: "a", value: "small first" },
    { predicate: "b", value: '🙂"\\'.repeat(12_000) },
    { predicate: "c", value: "small last" },
  ];
  const snapshot = sha256(facts);
  const first = rpc([facts], [call(1, "show", { id: "thread-1", limit: 100 })])[0];
  const firstPage = text(first) as PageResponse;
  expect(Buffer.byteLength(JSON.stringify(first), "utf8")).toBeLessThanOrEqual(65_536);
  expect(firstPage).toMatchObject({
    snapshot,
    offset: 0,
    nextOffset: 1,
    complete: false,
    items: facts.slice(0, 1),
  });

  const fragments: string[] = [];
  let itemByteOffset = 0;
  let afterFragment: PageResponse | undefined;
  for (let pageNumber = 0; pageNumber < 20 && !afterFragment; pageNumber += 1) {
    const args: Record<string, unknown> = {
      id: "thread-1", offset: 1, limit: 100, snapshot,
    };
    if (itemByteOffset > 0) args.itemByteOffset = itemByteOffset;
    const response = rpc([facts], [call(pageNumber + 2, "show", args)])[0];
    expect(Buffer.byteLength(JSON.stringify(response), "utf8")).toBeLessThanOrEqual(65_536);
    const page = text(response) as PageResponse;
    expect(page.itemFragment).toMatchObject({ index: 1, byteOffset: itemByteOffset });
    if (!page.itemFragment) throw new Error("oversized middle item omitted its fragment");
    fragments.push(page.itemFragment.content);
    if (page.nextItemByteOffset == null) afterFragment = page;
    else itemByteOffset = page.nextItemByteOffset;
  }
  expect(JSON.parse(fragments.join(""))).toEqual(facts[1]);
  expect(afterFragment).toMatchObject({
    offset: 1, nextOffset: 2, complete: false, items: [],
  });

  const last = rpc([facts], [call(20, "show", {
    id: "thread-1", offset: 2, limit: 100, snapshot,
  })])[0];
  expect(Buffer.byteLength(JSON.stringify(last), "utf8")).toBeLessThanOrEqual(65_536);
  expect(text(last)).toMatchObject({
    offset: 2, nextOffset: null, complete: true, items: facts.slice(2),
  });
});

test("a changed complete projection rejects an old continuation explicitly", () => {
  const before = [
    { predicate: "title", value: "Before" },
    { predicate: "kind", value: "thread" },
  ];
  const after = [...before, { predicate: "outcome", value: "done" }];
  const oldSnapshot = sha256(before);
  const responses = rpc([before, after], [
    call(1, "show", { id: "thread-1", limit: 1 }),
    call(2, "show", { id: "thread-1", offset: 1, limit: 1, snapshot: oldSnapshot }),
  ]);

  expect(responses[0].result.isError).toBe(false);
  expect(responses[1].result.isError).toBe(true);
  expect(text(responses[1])).toEqual({
    error: "stale_snapshot",
    expected: oldSnapshot,
    actual: sha256(after),
  });
});

test("search distinguishes no matches, incomplete pages, and exhaustion", () => {
  const matches = [
    { subject: "a", title: "Alpha", predicate: "title", value: "Alpha" },
    { subject: "b", title: "Beta", predicate: "note", value: "mentions alpha" },
    { subject: "c", title: "Gamma", predicate: "alpha_flag", value: "yes" },
  ];
  const snapshot = sha256(matches);
  const responses = rpc([[], matches, matches], [
    call(1, "search", { query: "absent", limit: 2 }),
    call(2, "search", { query: "alpha", limit: 2 }),
    call(3, "search", { query: "alpha", offset: 3, limit: 2, snapshot }),
  ]);

  expect(text(responses[0])).toMatchObject({
    total: 0, state: "no_matches", complete: true, nextOffset: null, items: [],
  });
  expect(text(responses[1])).toMatchObject({
    total: 3, state: "incomplete", complete: false, nextOffset: 2,
    items: matches.slice(0, 2),
  });
  expect(text(responses[2])).toMatchObject({
    total: 3, state: "exhausted", complete: true, nextOffset: null, items: [],
  });
});

test("show and search advertise and enforce the bounded continuation contract", () => {
  const [listed, invalid] = rpc([[]], [
    { jsonrpc: "2.0", id: 1, method: "tools/list", params: {} },
    call(2, "search", { query: "x", limit: 101 }),
  ]);
  const tools = listed.result.tools;
  if (!tools) throw new Error("tools/list response omitted tools");
  const byName = Object.fromEntries(tools.map((tool) => [tool.name, tool]));
  for (const name of ["show", "search"]) {
    expect(byName[name].inputSchema.properties).toMatchObject({
      offset: { type: "integer", minimum: 0, maximum: 2147483647, default: 0 },
      limit: { type: "integer", minimum: 1, maximum: 100, default: 50 },
      itemByteOffset: { type: "integer", minimum: 0, maximum: 2147483647, default: 0 },
      snapshot: { type: "string", pattern: "^[0-9a-f]{64}$" },
    });
    expect(byName[name].inputSchema.additionalProperties).toBe(false);
  }
  expect(invalid.result.isError).toBe(true);
  expect(contentText(invalid)).toBe("limit must be an integer from 1 through 100");
});
