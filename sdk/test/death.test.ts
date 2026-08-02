// Pure tests for the death-notification contract — no side effects, no live coordinator.
// deathCommands() is the single source of "what a death emits"; asserting its shape here
// locks the contract (agent_death fact on @swarm + thread; peer ping to the coordinator).
import { test, expect, describe } from "bun:test";
import { deathReason, deathCommands, causeChain } from "../src/death";

describe("deathReason", () => {
  test("Error -> its message", () => {
    expect(deathReason(new Error("Claude Code process terminated by signal 9"))).toBe(
      "Claude Code process terminated by signal 9",
    );
  });
  test("string passthrough", () => {
    expect(deathReason("Transport is closed")).toBe("Transport is closed");
  });
  test("collapses whitespace and bounds length", () => {
    const r = deathReason(new Error("a\n\n  b\t c".padEnd(500, "x")));
    expect(r).not.toContain("\n");
    expect(r.length).toBeLessThanOrEqual(300);
  });
  test("nullish -> 'unknown'", () => {
    expect(deathReason(undefined)).toBe("unknown");
    expect(deathReason(null)).toBe("unknown");
  });
});

describe("causeChain", () => {
  test("walks err.cause -> cause.cause and joins every nested message", () => {
    // The exact staged-preflight shape: outer umbrella wraps the real RPC failure.
    const inner = new Error("rpc handshake refused: connection reset");
    const mid = new Error("openai_codex_app_server_unreachable", { cause: inner });
    const outer = new Error("openai_codex_authority_preflight_failed", { cause: mid });
    const chain = causeChain(outer);
    expect(chain).toBe(
      "openai_codex_authority_preflight_failed <- cause: " +
      "openai_codex_app_server_unreachable <- cause: " +
      "rpc handshake refused: connection reset",
    );
  });
  test("string cause and bare error pass through", () => {
    expect(causeChain(new Error("plain"))).toBe("plain");
    expect(causeChain("just a string")).toBe("just a string");
  });
  test("a cyclic cause chain terminates and is bounded", () => {
    const a: any = new Error("a");
    const b: any = new Error("b", { cause: a });
    a.cause = b; // cycle
    const chain = causeChain(a);
    expect(chain).toContain("[cyclic cause]");
    expect(chain.length).toBeLessThanOrEqual(2000);
  });
});

describe("deathCommands", () => {
  const TS = "2026-07-04T00:00:00.000Z";

  test("bare: one fact on @swarm, carrying agent | reason | ts", () => {
    const cmds = deathCommands("W3", "exited with code 1", {}, TS);
    expect(cmds).toHaveLength(1);
    expect(cmds[0].args).toEqual(["tell", "@swarm", "agent_death", "W3 | exited with code 1 | " + TS]);
  });

  test("with thread: a second identical fact on the driven thread", () => {
    const cmds = deathCommands("P1", "signal 9", { thread: "019f2800" }, TS);
    expect(cmds).toHaveLength(2);
    expect(cmds[0].args[1]).toBe("@swarm"); // @swarm first (roster), thread second
    expect(cmds[1].args).toEqual(["tell", "019f2800", "agent_death", "P1 | signal 9 | " + TS]);
  });

  test("with coordinator: adds an msg-cli peer ping", () => {
    const cmds = deathCommands("P2", "Transport is closed", { coordinator: "fram-1" }, TS);
    expect(cmds).toHaveLength(2);
    const ping = cmds[1];
    expect(ping.cmd).toBe("bb");
    expect(ping.args).toContain("send");
    expect(ping.args).toContain("P2"); // from
    expect(ping.args).toContain("fram-1"); // to
    expect(ping.args).toContain("AGENT DEATH"); // subject
  });

  test("the peer ping carries the full cause chain while the facts stay short", () => {
    // The 2026-07-26 diagnosability hole: every death surface rendered only the
    // outermost message, so `openai_provider_execution_failed` reached the
    // operator with its real cause swallowed. The FACT stays legible in
    // `north show`; the transient ping carries the chain.
    const inner = new Error("Codex thread runtime workspace roots does not match");
    const outer = new Error("openai_provider_execution_failed", { cause: inner });
    const cmds = deathCommands(
      "P6", deathReason(outer), { thread: "T", coordinator: "coord" }, TS, causeChain(outer),
    );
    expect(cmds[0].args[3]).toBe("P6 | openai_provider_execution_failed | " + TS);
    expect(cmds[1].args[3]).toBe("P6 | openai_provider_execution_failed | " + TS);
    const body = cmds[2].args[cmds[2].args.length - 1];
    expect(body).toContain("openai_provider_execution_failed <- cause: ");
    expect(body).toContain("Codex thread runtime workspace roots does not match");
  });

  test("full context: @swarm fact, thread fact, then coordinator ping — in order", () => {
    const cmds = deathCommands("P5", "signal 15", { thread: "T", coordinator: "coord" }, TS);
    expect(cmds).toHaveLength(3);
    expect(cmds[0].args[1]).toBe("@swarm");
    expect(cmds[1].args[1]).toBe("T");
    expect(cmds[2].cmd).toBe("bb");
    expect(cmds[2].args[0]).toBe("-cp");
    expect(cmds[2].args[2]).toContain("msg-cli"); // the peer ping is last
  });
});
