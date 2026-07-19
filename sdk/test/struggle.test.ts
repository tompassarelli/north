import { expect, test } from "bun:test";
import {
  checkStruggle, makeStruggleObserver, makeStruggleState,
  resolveStrugglePolicy, updateStruggle, type StruggleTrigger,
} from "../src/struggle";

function toolResult(
  state: ReturnType<typeof makeStruggleState>,
  tool: string,
  id: string,
  isError = false,
  input: unknown = { id },
): void {
  updateStruggle({ type: "assistant", message: { content: [
    { type: "tool_use", id, name: tool, input },
  ] } }, state);
  updateStruggle({ type: "user", message: { content: [
    { type: "tool_result", tool_use_id: id, is_error: isError },
  ] } }, state);
}

test("topology policy defaults and strict bounded overrides are deterministic", () => {
  const workerPolicy = resolveStrugglePolicy("worker", {});
  expect(workerPolicy).toEqual({
    version: "north:struggle-observer:v1",
    topology: "worker",
    errorStreak: 3,
    loopRepeat: 3,
    loopWindow: 20,
    noProgressTurns: 6,
  });
  expect(Object.isFrozen(workerPolicy)).toBe(true);
  expect(() => { (workerPolicy as any).errorStreak = 99; }).toThrow(TypeError);
  expect(workerPolicy.errorStreak).toBe(3);
  expect(resolveStrugglePolicy("orchestrator", {})).toMatchObject({
    topology: "orchestrator", noProgressTurns: 12,
  });
  expect(resolveStrugglePolicy("orchestrator", {
    STRUGGLE_ERROR_STREAK: "5",
    STRUGGLE_LOOP_REPEAT: "4",
    STRUGGLE_LOOP_WINDOW: "30",
    STRUGGLE_STALL_TURNS: "9",
    STRUGGLE_STALL_TURNS_ORCHESTRATOR: "18",
  })).toMatchObject({
    errorStreak: 5, loopRepeat: 4, loopWindow: 30, noProgressTurns: 18,
  });

  for (const [name, value] of [
    ["STRUGGLE_ERROR_STREAK", "0"],
    ["STRUGGLE_LOOP_REPEAT", "-1"],
    ["STRUGGLE_LOOP_WINDOW", "3.5"],
    ["STRUGGLE_STALL_TURNS", " 6"],
    ["STRUGGLE_STALL_TURNS_ORCHESTRATOR", "1001"],
  ]) {
    expect(() => resolveStrugglePolicy("worker", { [name]: value }))
      .toThrow("positive integer between 1 and 1000");
  }
  expect(() => resolveStrugglePolicy("worker", {
    STRUGGLE_LOOP_REPEAT: "5", STRUGGLE_LOOP_WINDOW: "4",
  })).toThrow("less than or equal");
  expect(() => resolveStrugglePolicy("worker", {
    STRUGGLE_STALL_TURNS: "20", STRUGGLE_STALL_TURNS_ORCHESTRATOR: "10",
  })).toThrow("greater than or equal");
});

test("successful research, implementation, evidence, and coordination tools are progress", () => {
  const progressTools = [
    "Read", "Grep", "Glob", "Bash", "WebSearch", "WebFetch",
    "Edit", "Write", "NotebookEdit",
    "mcp__north__show", "mcp__north__ready", "mcp__north__next",
    "mcp__north__board", "mcp__north__plate", "mcp__north__blocked",
    "mcp__north__agenda", "mcp__north__leverage", "mcp__north__needs_review",
    "mcp__north__validate", "mcp__north__clock_status",
    "mcp__north__capture", "mcp__north__tell", "mcp__north__retract",
    "mcp__north__evidence_record", "mcp__north__spawn", "mcp__north__dispatch",
  ];
  for (const [index, tool] of progressTools.entries()) {
    const state = makeStruggleState(resolveStrugglePolicy("worker", {}));
    toolResult(state, tool, `success-${index}`);
    expect(state.lastProgressTurn).toBe(1);
    expect(checkStruggle(state)).toBeNull();
  }
});

test("failed progress tools stay negative and identical successful retries still loop", () => {
  for (const tool of ["Read", "Bash", "WebSearch", "mcp__north__spawn", "mcp__north__evidence_record"]) {
    const state = makeStruggleState(resolveStrugglePolicy("worker", {}));
    toolResult(state, tool, `failed-${tool}`, true);
    expect(state.lastProgressTurn).toBe(0);
    expect(state.totalErrors).toBe(1);
  }

  const repeated = makeStruggleState(resolveStrugglePolicy("worker", {}));
  for (let i = 0; i < 3; i++) toolResult(repeated, "Read", `read-${i}`, false, { file: "same" });
  expect(repeated.lastProgressTurn).toBe(3);
  expect(checkStruggle(repeated)).toBe("tool_loop");
});

test("no-progress is topology-bound and the observer snapshots the full policy", () => {
  for (const [topology, threshold] of [["worker", 6], ["orchestrator", 12]] as const) {
    const policy = resolveStrugglePolicy(topology, {});
    const observer = makeStruggleObserver(policy);
    for (let turn = 1; turn < threshold; turn++) {
      toolResult(observer.state, "UnknownTool", `${topology}-${turn}`);
      expect(observer.observe({ type: "system", subtype: "heartbeat" })).toBeNull();
      expect(checkStruggle(observer.state)).toBeNull();
    }
    toolResult(observer.state, "UnknownTool", `${topology}-${threshold}`);
    expect(checkStruggle(observer.state)).toBe("no_progress");
    // observe records the first distinct trigger in the terminal snapshot.
    expect(observer.observe({ type: "system", subtype: "heartbeat" })).toBe("no_progress");
    expect(observer.snapshot()).toEqual({
      policyVersion: "north:struggle-observer:v1",
      topology,
      errorStreakThreshold: 3,
      loopRepeatThreshold: 3,
      loopWindow: 20,
      noProgressTurnThreshold: threshold,
      errorCount: 0,
      triggers: ["no_progress"],
    });
    const snapshot = observer.snapshot();
    expect(Object.isFrozen(snapshot)).toBe(true);
    expect(Object.isFrozen(snapshot.triggers)).toBe(true);
    expect(() => { (snapshot.triggers as StruggleTrigger[]).push("tool_loop"); }).toThrow(TypeError);
  }
});
