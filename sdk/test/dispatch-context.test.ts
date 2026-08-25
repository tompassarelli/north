import { afterEach, expect, test } from "bun:test";
import { mkdtempSync, mkdirSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { resolveDispatchWorkingDirectory } from "../src/dispatch-context";

const temporary: string[] = [];
afterEach(() => {
  for (const path of temporary.splice(0)) rmSync(path, { recursive: true, force: true });
});

function workspace(): { home: string; north: string; machinery: string } {
  const home = mkdtempSync(join(tmpdir(), "north-dispatch-context-"));
  temporary.push(home);
  const north = join(home, "code/north/main");
  const machinery = join(home, "code/agent-machinery/main");
  mkdirSync(north, { recursive: true });
  mkdirSync(machinery, { recursive: true });
  return { home, north, machinery };
}

test("a thread repo fact overrides the MCP server cwd", () => {
  const { home, north, machinery } = workspace();
  expect(resolveDispatchWorkingDirectory([
    { predicate: "title", value: "Orchestration repair" },
    { predicate: "repo", value: "~/code/agent-machinery/main" },
  ], { home, cwd: north })).toBe(machinery);
});

test("parallel-safe resolution disambiguates multi-repo threads without process.chdir", () => {
  const { home, north, machinery } = workspace();
  const facts = [
    { predicate: "repo", value: "~/code/north/main" },
    { predicate: "repo", value: "~/code/agent-machinery/main" },
  ];
  expect(resolveDispatchWorkingDirectory(facts, { home, cwd: machinery })).toBe(machinery);
  expect(resolveDispatchWorkingDirectory(facts, { home, cwd: north })).toBe(north);
});

test("ambiguous, relative, missing, non-directory, and escaping repo facts fail before execution", () => {
  const { home, north } = workspace();
  const file = join(home, "not-a-repo");
  writeFileSync(file, "x");
  const outside = mkdtempSync(join(tmpdir(), "north-dispatch-outside-"));
  temporary.push(outside);
  symlinkSync(outside, join(home, "escape"));

  expect(() => resolveDispatchWorkingDirectory([
    { predicate: "repo", value: "~/code/north/main" },
    { predicate: "repo", value: "~/code/agent-machinery/main" },
  ], { home, cwd: home })).toThrow("multiple repository facts");
  expect(() => resolveDispatchWorkingDirectory([{ predicate: "repo", value: "code/north" }], { home, cwd: north }))
    .toThrow("must be absolute or ~-anchored");
  expect(() => resolveDispatchWorkingDirectory([{ predicate: "repo", value: "~/missing" }], { home, cwd: north }))
    .toThrow("does not resolve");
  expect(() => resolveDispatchWorkingDirectory([{ predicate: "repo", value: "~/not-a-repo" }], { home, cwd: north }))
    .toThrow("not a directory");
  expect(() => resolveDispatchWorkingDirectory([{ predicate: "repo", value: "~/escape" }], { home, cwd: north }))
    .toThrow("escapes the home directory");
  expect(() => resolveDispatchWorkingDirectory([{ predicate: "repo", value: outside }], { home, cwd: north }))
    .toThrow("escapes the home directory");
});

test("absolute and home-anchored spellings share the same trusted-root boundary", () => {
  const { home, north } = workspace();
  expect(resolveDispatchWorkingDirectory([
    { predicate: "repo", value: north },
  ], { home, cwd: home })).toBe(north);
});
