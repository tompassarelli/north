// Pure + hermetic tests for the stream watchdog (thread 019f4d54).
//   - withStallWatchdog: passes messages through untouched, fires onStall then onAbort
//     on a silent stream, and propagates a source rejection (so death still routes).
//   - stallCommands/turnCapCommands: lock the notification contract without a live
//     coordinator (mirrors death.test.ts).
import { test, expect, describe } from "bun:test";
import { withStallWatchdog, stallCommands, turnCapCommands } from "../src/watchdog";

describe("withStallWatchdog — liveness", () => {
  test("messages flow through untouched, no stall on an active stream", async () => {
    async function* src() { yield 1; yield 2; yield 3; }
    const seen: number[] = [];
    let stalls = 0, aborts = 0;
    for await (const v of withStallWatchdog(src(), {
      stallMs: 10_000, onStall: () => stalls++, onAbort: () => aborts++,
    })) seen.push(v);
    expect(seen).toEqual([1, 2, 3]);
    expect(stalls).toBe(0);
    expect(aborts).toBe(0);
  });

  test("silent stream -> onStall (once) then onAbort, generator returns", async () => {
    // A source whose next() never resolves — the exact specimen condition (iterator
    // neither yields nor throws). stallMs=20 -> stall at 20ms, abort at 40ms.
    const hanging: AsyncIterator<number> = { next: () => new Promise<never>(() => {}) };
    const order: string[] = [];
    const seen: number[] = [];
    for await (const v of withStallWatchdog(hanging, {
      stallMs: 20,
      onStall: (m) => order.push(`stall:${m}`),
      onAbort: () => order.push("abort"),
    })) seen.push(v);
    expect(seen).toEqual([]);            // nothing yielded
    expect(order).toEqual(["stall:1", "abort"]); // surfaced once, then terminal
  });

  test("stall then a late message recovers — no abort", async () => {
    // One message arrives at ~30ms: past the 20ms stall ping but before the 40ms abort.
    // The watchdog must surface the stall yet still deliver the message and continue.
    let delivered = false;
    const src: AsyncIterator<number> = {
      next: () => new Promise((r) => {
        if (delivered) return; // second call hangs -> eventual abort
        delivered = true;
        setTimeout(() => r({ value: 7, done: false }), 30);
      }),
    };
    const order: string[] = [];
    const seen: number[] = [];
    for await (const v of withStallWatchdog(src, {
      stallMs: 20,
      onStall: () => order.push("stall"),
      onAbort: () => order.push("abort"),
    })) seen.push(v);
    expect(seen).toEqual([7]); // the late message was delivered despite the stall ping
    expect(order[order.length - 1]).toBe("abort"); // second silence -> terminal
    expect(order.filter((x) => x === "stall").length).toBeGreaterThanOrEqual(1);
  });

  test("a source rejection propagates out (death path still fires)", async () => {
    async function* boom() { yield 1; throw new Error("Claude Code process terminated by signal 9"); }
    const seen: number[] = [];
    let msg = "";
    try {
      for await (const v of withStallWatchdog(boom(), {
        stallMs: 10_000, onStall: () => {}, onAbort: () => {},
      })) seen.push(v);
    } catch (e: any) { msg = e.message; }
    expect(seen).toEqual([1]);
    expect(msg).toContain("signal 9");
  });
});

describe("stallCommands", () => {
  const TS = "2026-07-11T00:00:00.000Z";
  test("bare: one `stalled` fact on @agent:<id>", () => {
    const cmds = stallCommands("W7", 10, {}, TS);
    expect(cmds).toHaveLength(1);
    expect(cmds[0].args).toEqual(["tell", "agent:W7", "stalled", "W7 | no SDK output 10min | " + TS]);
  });
  test("with coordinator: adds an AGENT STALLED peer ping", () => {
    const cmds = stallCommands("W7", 10, { coordinator: "coord" }, TS);
    expect(cmds).toHaveLength(2);
    expect(cmds[1].cmd).toBe("bb");
    expect(cmds[1].args).toContain("send");
    expect(cmds[1].args).toContain("W7");        // from
    expect(cmds[1].args).toContain("coord");     // to
    expect(cmds[1].args).toContain("AGENT STALLED");
  });
});

describe("turnCapCommands", () => {
  const TS = "2026-07-11T00:00:00.000Z";
  test("bare: a `turn_capped` fact on @agent:<id>", () => {
    const cmds = turnCapCommands("W8", "error_max_turns — partial: x", {}, TS);
    expect(cmds).toHaveLength(1);
    expect(cmds[0].args).toEqual(["tell", "agent:W8", "turn_capped", "W8 | " + TS]);
  });
  test("with coordinator: adds a TURN CAP ping carrying the partial note", () => {
    const cmds = turnCapCommands("W8", "error_max_turns — partial: x", { coordinator: "coord" }, TS);
    expect(cmds).toHaveLength(2);
    expect(cmds[1].cmd).toBe("bb");
    expect(cmds[1].args).toContain("TURN CAP");
    expect(cmds[1].args[cmds[1].args.length - 1]).toContain("error_max_turns");
  });
});
