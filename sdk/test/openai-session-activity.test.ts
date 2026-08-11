import { afterEach, expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, rmSync, utimesSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { readOpenAISessionActivity } from "../src/openai-session-activity";

const roots: string[] = [];
const now = new Date("2026-08-02T12:00:00.000Z");

afterEach(() => {
  for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true });
});

function rollout(name: string, ageSeconds: number, records: unknown[], finalNewline = true): string {
  const root = roots[0] ?? (() => {
    const created = mkdtempSync(join(tmpdir(), "north-openai-activity-"));
    roots.push(created);
    return created;
  })();
  const directory = join(root, "sessions", "2026", "08", "02");
  mkdirSync(directory, { recursive: true });
  const path = join(directory, `rollout-${name}.jsonl`);
  writeFileSync(path, `${records.map((record) => JSON.stringify(record)).join("\n")}${finalNewline ? "\n" : ""}`);
  const modified = new Date(now.getTime() - ageSeconds * 1_000);
  utimesSync(path, modified, modified);
  return root;
}

function usage(outputTokens: number): unknown {
  return { payload: { info: { total_token_usage: { input_tokens: 1, output_tokens: outputTokens } } } };
}

test("sums only the last total_token_usage entry in each rollout", async () => {
  const root = rollout("one", 300, [usage(10), { payload: { event: "noise" } }, usage(25)]);
  rollout("two", 600, [usage(7)]);

  expect(await readOpenAISessionActivity({ accountRoot: root, now })).toMatchObject({
    hours: 24, sessions: 2, live: 0, outputTokens: 32,
  });
});

test("finds the newest valid usage before large trailing records", async () => {
  const root = rollout("reverse", 300, [
    usage(10),
    usage(25),
    { payload: "x".repeat(128 * 1024) },
    { note: "invalid total_token_usage marker" },
  ], false);

  expect(await readOpenAISessionActivity({ accountRoot: root, now })).toMatchObject({
    sessions: 1, outputTokens: 25,
  });
});

test("filters rollout fixtures by the requested hour window", async () => {
  const root = rollout("recent", 30 * 60, [usage(11)]);
  rollout("old", 2 * 60 * 60, [usage(99)]);

  expect(await readOpenAISessionActivity({ accountRoot: root, hours: 1, now })).toEqual({
    hours: 1,
    sessions: 1,
    live: 0,
    outputTokens: 11,
    lastActivityAt: new Date(now.getTime() - 30 * 60 * 1_000),
  });
});

test("counts live sessions from rollout mtime within 120 seconds", async () => {
  const root = rollout("live", 119, [usage(5)]);
  rollout("not-live", 120, [usage(6)]);

  expect(await readOpenAISessionActivity({ accountRoot: root, now })).toMatchObject({
    sessions: 2, live: 1, outputTokens: 11,
  });
});
