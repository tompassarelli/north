import { afterEach, expect, test } from "bun:test";
import { mkdtempSync, mkdirSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { resolveDispatchWorkingDirectory } from "../src/dispatch-context";

const temporary: string[] = [];
afterEach(() => {
  for (const path of temporary.splice(0)) rmSync(path, { recursive: true, force: true });
});

function workspace(): { home: string; north: string; orchestration: string } {
  const home = mkdtempSync(join(tmpdir(), "north-dispatch-context-"));
  temporary.push(home);
  const north = join(home, "code/north");
  const orchestration = join(north, "orchestration");
  mkdirSync(north, { recursive: true });
  mkdirSync(orchestration, { recursive: true });
  return { home, north, orchestration };
}

test("a thread repo fact overrides the MCP server cwd", () => {
  const { home, north, orchestration } = workspace();
  expect(resolveDispatchWorkingDirectory([
    { predicate: "title", value: "Orchestration repair" },
    { predicate: "repo", value: "~/code/north/orchestration" },
  ], { home, cwd: north })).toBe(orchestration);
});

test("parallel-safe resolution disambiguates multi-repo threads without process.chdir", () => {
  const { home, north, orchestration } = workspace();
  const facts = [
    { predicate: "repo", value: "~/code/north" },
    { predicate: "repo", value: "~/code/north/orchestration" },
  ];
  expect(resolveDispatchWorkingDirectory(facts, { home, cwd: orchestration })).toBe(orchestration);
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
    { predicate: "repo", value: "~/code/north" },
    { predicate: "repo", value: "~/code/north/orchestration" },
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
