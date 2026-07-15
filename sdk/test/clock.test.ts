// Pure tests for the auto-clock contract — no side effects, no live coordinator.
// clockCommand()/finalizeAction() are the single source of "what the SDK auto-clock
// issues"; asserting their shape here locks per-agent semantics: a dispatched worker
// clocks its OWN thread as its OWN id, and a crash orphan-closes while a clean/capped
// exit stops normally. Mirrors death.test.ts (deathCommands is the death contract).
import { test, expect, describe } from "bun:test";
import { clockCommand, finalizeAction } from "../src/clock";

describe("clockCommand", () => {
  test("start: north clock start <thread>, pinning the worker as NORTH_AGENT_ID", () => {
    const c = clockCommand("start", "w1", "2026-07-14-101500");
    expect(c.args).toEqual(["clock", "start", "2026-07-14-101500"]);
    expect(c.agentEnv).toBe("w1"); // -> session carries clocked_by=w1
  });

  test("stop: north clock stop, pinned to THIS agent so it closes only its own", () => {
    const c = clockCommand("stop", "w1");
    expect(c.args).toEqual(["clock", "stop"]);
    expect(c.agentEnv).toBe("w1");
  });

  test("orphan: agent is EXPLICIT in argv, no env pin (a reaper closes a dead agent)", () => {
    const c = clockCommand("orphan", "w1");
    expect(c.args).toEqual(["clock", "orphan", "w1"]);
    expect(c.agentEnv).toBeUndefined();
  });
});

describe("finalizeAction", () => {
  test("crash outcomes orphan-close (flag the untrustworthy tail)", () => {
    expect(finalizeAction("died")).toBe("orphan");
    expect(finalizeAction("stalled")).toBe("orphan");
  });

  test("clean / turn-capped / budget-stopped close normally (real time bills)", () => {
    expect(finalizeAction("ran")).toBe("stop");
    expect(finalizeAction("max_turns")).toBe("stop");
    expect(finalizeAction("capped")).toBe("stop");
    expect(finalizeAction("budget_exceeded")).toBe("stop");
  });
});
