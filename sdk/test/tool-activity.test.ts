import { expect, test } from "bun:test";
import {
  McpActivityAccumulator, normalizeCodexMcpIdentity, parseAnthropicMcpName,
} from "../src/tool-activity";

test("unfinished and legacy-unavailable activity remains unknown, never exact zero", () => {
  const activity = new McpActivityAccumulator("fixture");
  expect(activity.snapshot()).toEqual({ source: "fixture", coverage: "unknown", tools: [] });
});

test("actual calls dedupe provider retries and retain only normalized server/tool/count", () => {
  const activity = new McpActivityAccumulator("fixture");
  const identity = parseAnthropicMcpName("mcp__North__Tell");
  activity.observe("turn-1:call-1", identity);
  activity.observe("turn-1:call-1", identity);
  activity.observe("turn-2:call-1", identity);
  activity.complete();
  expect(activity.snapshot()).toEqual({
    source: "fixture", coverage: "exact", totalCalls: 2,
    tools: [{ server: "north", tool: "tell", count: 2 }],
  });
  expect(JSON.stringify(activity.snapshot())).not.toContain("argument");
});

test("a completed MCP item with lost identity is partial rather than inferred zero", () => {
  const activity = new McpActivityAccumulator("fixture");
  activity.observe("call-1", normalizeCodexMcpIdentity("north", { hostile: true }));
  activity.complete();
  expect(activity.snapshot()).toEqual({
    source: "fixture", coverage: "partial", totalCalls: 1, tools: [],
  });
});
