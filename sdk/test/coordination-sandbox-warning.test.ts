import { expect, test } from "bun:test";
import { warnCoordinationUnderReadOnlySandbox } from "../src/providers/authority";

function capture(run: () => void): string {
  const out: string[] = [];
  const ow = console.warn;
  console.warn = (...a: any[]) => { out.push(a.join(" ")); };
  try { run(); } finally { console.warn = ow; }
  return out.join("\n");
}
const surface = (o: any) => o as any;

// The contradiction: holds `coordination`, runs in the sandbox that blocks :7977.
const orchestrator = surface({
  provider: "openai", sandbox: "read-only",
  capabilities: ["filesystem.read", "shell.readonly", "coordination"],
});

test("warns when coordination meets the read-only sandbox", () => {
  const w = capture(() => warnCoordinationUnderReadOnlySandbox("spawn", orchestrator));
  expect(w).toContain("coordination");
  expect(w).toContain("read-only sandbox");
  expect(w).toContain(":7977");
  expect(w).toContain("ETIMEDOUT");
});

test("a read-only worker that does NOT coordinate is not warned", () => {
  expect(capture(() => warnCoordinationUnderReadOnlySandbox("spawn", surface({
    provider: "openai", sandbox: "read-only",
    capabilities: ["filesystem.read", "shell.readonly"],
  })))).toBe("");
});

test("a coordinating workspace-write role is not warned", () => {
  expect(capture(() => warnCoordinationUnderReadOnlySandbox("spawn", surface({
    provider: "openai", sandbox: "workspace-write",
    capabilities: ["filesystem.write", "shell", "coordination"],
  })))).toBe("");
});

// The sandbox is a Codex construct; warning an Anthropic lane would be noise.
test("anthropic lanes are never warned", () => {
  expect(capture(() => warnCoordinationUnderReadOnlySandbox("spawn", surface({
    provider: "anthropic", sandbox: "read-only",
    capabilities: ["shell.readonly", "coordination"],
  })))).toBe("");
});

test("dispatch is labelled distinctly from spawn", () => {
  expect(capture(() => warnCoordinationUnderReadOnlySandbox("dispatch", orchestrator)))
    .toContain("[dispatch]");
});
