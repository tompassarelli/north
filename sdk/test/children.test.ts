// Pure test for the early-exit-with-live-children notification contract (thread
// 019f4ed2, half b) — mirrors death.test.ts / watchdog.test.ts: lock the command specs
// without a live coordinator. The impure liveChildren() (which queries the engine) is
// exercised end-to-end by the E2E probe, not here.
import { test, expect, describe } from "bun:test";
import { earlyExitCommands } from "../src/children";

describe("earlyExitCommands", () => {
  const TS = "2026-07-11T00:00:00.000Z";

  test("bare: one early_exit_children fact on @agent:<id> naming the orphans", () => {
    const cmds = earlyExitCommands("W1", ["c1", "c2"], {}, TS);
    expect(cmds).toHaveLength(1);
    expect(cmds[0].args).toEqual([
      "tell", "agent:W1", "early_exit_children", "W1 | orphaned: c1,c2 | " + TS,
    ]);
  });

  test("with coordinator: adds a loud EARLY EXIT WITH LIVE CHILDREN peer ping", () => {
    const cmds = earlyExitCommands("W1", ["c1", "c2"], { coordinator: "coord" }, TS);
    expect(cmds).toHaveLength(2);
    expect(cmds[1].cmd).toBe("bb");
    expect(cmds[1].args).toContain("send");
    expect(cmds[1].args).toContain("W1"); // from
    expect(cmds[1].args).toContain("coord"); // to
    expect(cmds[1].args).toContain("EARLY EXIT WITH LIVE CHILDREN");
    expect(cmds[1].args[cmds[1].args.length - 1]).toContain("c1,c2");
    expect(cmds[1].args[cmds[1].args.length - 1]).toContain("2 live child");
  });
});
