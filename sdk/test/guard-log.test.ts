// Unit tests for guard-denial telemetry (guard-log.ts). Hermetic: exercises the PURE
// builders (classifyGuard, denialFacts) — no coordinator, no shell-out. recordDenial's
// fire-and-forget exec path is intentionally untested (it swallows all errors by design;
// the contract worth pinning is the fact SHAPE the pure builder produces).
import { test, expect, describe } from "bun:test";
import { classifyGuard, denialFacts } from "../src/guard-log";

describe("classifyGuard", () => {
  test("maps each real guard's reason to its label", () => {
    expect(classifyGuard("Billable client edit blocked — no north clock running for client 'msa'")).toBe("north-clock-guard");
    expect(classifyGuard("This file is GRAPH-OWNED — author via graph edit")).toBe("code-upstream-guard");
    expect(classifyGuard("BLOCKED: edit .nix directly — write the .bnix")).toBe("firn-guard");
    expect(classifyGuard("tripwire: recursive delete outside safe roots")).toBe("tripwire-guard");
    expect(classifyGuard("raw 'git push' — house policy: use safe-push")).toBe("tripwire-guard");
  });
  test("unclassified reason falls to 'other', never dropped", () => {
    expect(classifyGuard("some new guard nobody labeled yet")).toBe("other");
  });
});

describe("denialFacts", () => {
  const TS = "2026-07-16T12:00:00.000Z";
  const MS = 1_700_000_000_000;

  test("edit denial: attribution + timestamp + target + guard + reason", () => {
    const { subject, facts } = denialFacts(
      "sdk-abc123",
      "Billable client edit blocked — no north clock running for client 'msa'",
      { tool_name: "Edit", tool_input: { file_path: "/home/tom/code/client/msa/kea/x.ts" } },
      TS, MS,
    );
    // NO leading @ — north tell prepends it (subject lands as @denial:… in the log).
    expect(subject).toBe(`denial:sdk-abc123-${MS.toString(36)}`);
    const m = new Map(facts);
    expect(m.get("kind")).toBe("guard_denial");
    expect(m.get("agent")).toBe("sdk-abc123");
    expect(m.get("guard")).toBe("north-clock-guard");
    expect(m.get("tool")).toBe("Edit");
    expect(m.get("at")).toBe(TS);
    expect(m.get("target")).toBe("/home/tom/code/client/msa/kea/x.ts");
    expect(m.get("reason")).toContain("Billable client edit blocked");
  });

  test("bash denial: command becomes the (whitespace-collapsed, truncated) target", () => {
    const cmd = "git   push\n  --force origin main " + "x".repeat(300);
    const { facts } = denialFacts("lane-9", "tripwire: raw push", { tool_name: "Bash", tool_input: { command: cmd } }, TS, MS);
    const m = new Map(facts);
    expect(m.get("tool")).toBe("Bash");
    expect(m.get("guard")).toBe("tripwire-guard");
    const target = m.get("target")!;
    expect(target.length).toBeLessThanOrEqual(200);
    expect(target.startsWith("git push --force origin main")).toBe(true);
  });

  test("missing target is omitted, not an empty fact", () => {
    const { facts } = denialFacts("lane-1", "other reason", { tool_name: "Bash", tool_input: {} }, TS, MS);
    const keys = facts.map(([p]) => p);
    expect(keys).not.toContain("target");
    expect(keys).toContain("kind");
  });
});
