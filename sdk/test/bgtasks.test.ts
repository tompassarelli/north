// Pure + hermetic tests for the background-task tracker (thread 019f4ed2). No SDK, no
// network — feed synthetic system messages shaped exactly like the real SDK's
// task_started / task_notification / task_updated (observed in sdk/scratch-probe-bg.ts)
// and lock the live-set + settlement-event contract that spawn.ts/dispatch.ts rely on.
import { test, expect, describe } from "bun:test";
import { makeBgTracker, bgContinuationMessage, maxBgContinuations } from "../src/bgtasks";

const started = (id: string, description = "sleep 20 && touch x") =>
  ({ type: "system", subtype: "task_started", task_id: id, description });
const notif = (id: string, status: string) =>
  ({ type: "system", subtype: "task_notification", task_id: id, status });
const updated = (id: string, status: string, extra: any = {}) =>
  ({ type: "system", subtype: "task_updated", task_id: id, patch: { status, ...extra } });

describe("makeBgTracker — lifecycle", () => {
  test("task_started adds to the live set and returns 'started'", () => {
    const t = makeBgTracker();
    expect(t.observe(started("a"))).toBe("started");
    expect(t.size()).toBe(1);
    expect(t.live()).toEqual(["a"]);
  });

  test("task_notification completed settles it (returns 'settled', leaves the set)", () => {
    const t = makeBgTracker();
    t.observe(started("a"));
    expect(t.observe(notif("a", "completed"))).toBe("settled");
    expect(t.size()).toBe(0);
  });

  test("task_updated completed ALSO settles — both terminal signals honored", () => {
    // Probe #2 (scratch-probe-fix.ts) observed settlement arriving as task_updated, NOT
    // task_notification — so either must clear the task, idempotently.
    const t = makeBgTracker();
    t.observe(started("a"));
    expect(t.observe(updated("a", "completed", { end_time: 123 }))).toBe("settled");
    expect(t.size()).toBe(0);
  });

  test("failed / stopped / killed all settle", () => {
    for (const [mk, st] of [
      [notif, "failed"], [notif, "stopped"], [updated, "killed"], [updated, "failed"],
    ] as const) {
      const t = makeBgTracker();
      t.observe(started("a"));
      expect(t.observe((mk as any)("a", st))).toBe("settled");
      expect(t.size()).toBe(0);
    }
  });

  test("is_backgrounded is NOT settlement — a detached task is still live", () => {
    const t = makeBgTracker();
    t.observe(started("a"));
    expect(t.observe(updated("a", "running", { is_backgrounded: true }))).toBe(null);
    expect(t.size()).toBe(1);
  });

  test("a settlement for an unknown / already-settled id is a no-op (null)", () => {
    const t = makeBgTracker();
    expect(t.observe(notif("ghost", "completed"))).toBe(null);
    t.observe(started("a"));
    t.observe(notif("a", "completed"));
    expect(t.observe(notif("a", "completed"))).toBe(null); // second settle: no double-count
  });

  test("a re-announced start is not new work (idempotent add)", () => {
    const t = makeBgTracker();
    expect(t.observe(started("a"))).toBe("started");
    expect(t.observe(started("a"))).toBe(null);
    expect(t.size()).toBe(1);
  });

  test("tracks multiple concurrent tasks independently", () => {
    const t = makeBgTracker();
    t.observe(started("a")); t.observe(started("b")); t.observe(started("c"));
    expect(t.size()).toBe(3);
    t.observe(notif("b", "completed"));
    expect(t.size()).toBe(2);
    expect(t.live()).toEqual(["a", "c"]);
  });

  test("non-task messages are ignored (assistant/result/other system subtypes)", () => {
    const t = makeBgTracker();
    for (const m of [
      { type: "assistant", message: { content: [] } },
      { type: "result", subtype: "success", result: "done" },
      { type: "system", subtype: "task_progress", task_id: "a" }, // liveness pulse, not add/remove
      { type: "system", subtype: "init" },
      null, undefined, {},
    ]) {
      expect(t.observe(m)).toBe(null);
    }
    expect(t.size()).toBe(0);
  });
});

describe("bgContinuationMessage", () => {
  test("names the live ids and states the harness rule", () => {
    const m = bgContinuationMessage(["a", "b"]);
    expect(m).toContain("a, b");
    expect(m).toContain("[harness]");
    expect(m).toContain("cannot exit");
    expect(m.toLowerCase()).toContain("sleep-poll");
  });
  test("degrades gracefully with no ids", () => {
    expect(bgContinuationMessage([])).toContain("unknown");
  });
});

describe("maxBgContinuations", () => {
  test("defaults to 5", () => {
    const prev = process.env.NORTH_BG_MAX_CONTINUATIONS;
    delete process.env.NORTH_BG_MAX_CONTINUATIONS;
    expect(maxBgContinuations()).toBe(5);
    if (prev !== undefined) process.env.NORTH_BG_MAX_CONTINUATIONS = prev;
  });
  test("honors a positive env override", () => {
    const prev = process.env.NORTH_BG_MAX_CONTINUATIONS;
    process.env.NORTH_BG_MAX_CONTINUATIONS = "3";
    expect(maxBgContinuations()).toBe(3);
    if (prev === undefined) delete process.env.NORTH_BG_MAX_CONTINUATIONS;
    else process.env.NORTH_BG_MAX_CONTINUATIONS = prev;
  });
  test("ignores garbage / non-positive, falling back to 5", () => {
    const prev = process.env.NORTH_BG_MAX_CONTINUATIONS;
    for (const bad of ["0", "-2", "abc", ""]) {
      process.env.NORTH_BG_MAX_CONTINUATIONS = bad;
      expect(maxBgContinuations()).toBe(5);
    }
    if (prev === undefined) delete process.env.NORTH_BG_MAX_CONTINUATIONS;
    else process.env.NORTH_BG_MAX_CONTINUATIONS = prev;
  });
});
