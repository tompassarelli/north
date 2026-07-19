// Pure tests for the auto-clock contract — no side effects, no live coordinator.
// clockCommand()/finalizeAction() are the single source of "what the SDK auto-clock
// issues"; asserting their shape here locks per-agent semantics: a dispatched worker
// clocks its OWN thread as its OWN id, and a crash orphan-closes while a clean/capped
// exit stops normally. Mirrors death.test.ts (deathCommands is the death contract).
import { test, expect, describe } from "bun:test";
import { chmodSync, mkdtempSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import {
  admitBillableClock, BillableClockPreflightError, clientTicketForBranch, clockCommand,
  finalizeAction, trustedGitExecutable,
} from "../src/clock";
import {
  gitOracleEnvironment, TrustedGitOracleError, trustedGitProjectRoot,
  trustedManagedCodexExecutable,
} from "../src/trusted-runtime";

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

  test("clean and turn-capped runs close normally (real time bills)", () => {
    expect(finalizeAction("ran")).toBe("stop");
    expect(finalizeAction("max_turns")).toBe("stop");
    expect(finalizeAction("capped")).toBe("stop");
  });
});

describe("required client clock admission", () => {
  const base = {
    agentId: "lane-clock-proof",
    capabilities: ["filesystem.write"],
    cwd: "/workspace",
  };
  const projectRoot = () => "/home/tom/code/client/msa/kea";
  const branchName = () => "msa-242-clock-admission";
  const thread = () => [
    { predicate: "owner", value: "msa" },
    { predicate: "linear", value: "MSA-242" },
  ];
  const absent = "not clocked in (agent lane-clock-proof)";
  const failureCode = (
    run: () => unknown,
    code: string,
  ) => {
    let caught: unknown;
    try { run(); } catch (error) { caught = error; }
    expect(caught).toBeInstanceOf(BillableClockPreflightError);
    expect(caught).toMatchObject({ code });
  };

  test("write-capable client work requires a bound thread before provider work", () => {
    failureCode(() => admitBillableClock(base, { projectRoot }),
      "billable_thread_required");
  });

  test("thread owner must be readable and exactly match the canonical client", () => {
    failureCode(() => admitBillableClock(
      { ...base, threadId: "thread-clock-proof" },
      {
        projectRoot,
        branchName,
        readThreadFacts: () => { throw new Error("offline"); },
      },
    ), "billable_thread_owner_unavailable");
    failureCode(() => admitBillableClock(
      { ...base, threadId: "thread-clock-proof" },
      {
        projectRoot,
        branchName,
        readThreadFacts: () => [{ predicate: "owner", value: "other" }],
      },
    ), "billable_thread_owner_mismatch");
  });

  test("branch must carry one client ticket and thread linear must match it exactly", () => {
    failureCode(() => admitBillableClock(
      { ...base, threadId: "thread-clock-proof" },
      {
        projectRoot,
        branchName: () => "main",
        readThreadFacts: thread,
      },
    ), "billable_ticket_required");
    failureCode(() => admitBillableClock(
      { ...base, threadId: "thread-clock-proof" },
      {
        projectRoot,
        branchName,
        readThreadFacts: () => [
          { predicate: "owner", value: "msa" },
          { predicate: "linear", value: "MSA-241" },
        ],
      },
    ), "billable_thread_linear_mismatch");
  });

  test("failed start blocks without accepting a status-only preexisting clock", () => {
    const calls: string[][] = [];
    failureCode(() => admitBillableClock(
      { ...base, threadId: "thread-clock-proof" },
      {
        projectRoot,
        branchName,
        readThreadFacts: thread,
        execute: ({ args }) => {
          calls.push(args);
          return args[1] === "status"
            ? absent
            : "already clocked in on thread-clock-proof";
        },
      },
    ), "billable_clock_start_failed");
    expect(calls).toEqual([
      ["clock", "status"],
      ["clock", "start", "thread-clock-proof"],
    ]);
  });

  test("ambiguous start reads back and closes only an exact newly visible clock", () => {
    const calls: string[][] = [];
    let statusCalls = 0;
    failureCode(() => admitBillableClock(
      { ...base, threadId: "thread-clock-proof" },
      {
        projectRoot,
        branchName,
        readThreadFacts: thread,
        execute: ({ args }) => {
          calls.push(args);
          if (args[1] === "start") throw new Error("transport dropped response");
          if (args[1] === "status") {
            statusCalls++;
            return statusCalls === 1 ? absent
              : "clocked in on thread-clock-proof  Exact title  (agent lane-clock-proof)";
          }
          return "clock stopped";
        },
      },
    ), "billable_clock_start_failed");
    expect(calls).toEqual([
      ["clock", "status"],
      ["clock", "start", "thread-clock-proof"],
      ["clock", "status"],
      ["clock", "stop"],
    ]);
  });

  test("failed-record response is ambiguous because the final commit ack may be lost", () => {
    const calls: string[][] = [];
    let statusCalls = 0;
    failureCode(() => admitBillableClock(
      { ...base, threadId: "thread-clock-proof" },
      {
        projectRoot,
        branchName,
        readThreadFacts: thread,
        execute: ({ args }) => {
          calls.push(args);
          if (args[1] === "start")
            return "clock start FAILED to record (ok:/ok:/coordinator unavailable) — retry";
          if (args[1] === "status") {
            statusCalls++;
            return statusCalls === 1 ? absent
              : "clocked in on thread-clock-proof  Exact title  (agent lane-clock-proof)";
          }
          return "clock stopped";
        },
      },
    ), "billable_clock_start_failed");
    expect(calls).toEqual([
      ["clock", "status"],
      ["clock", "start", "thread-clock-proof"],
      ["clock", "status"],
      ["clock", "stop"],
    ]);
  });

  test("failed readback closes only the clock this admission opened", () => {
    const calls: string[][] = [];
    failureCode(() => admitBillableClock(
      { ...base, threadId: "thread-clock-proof" },
      {
        projectRoot,
        branchName,
        readThreadFacts: thread,
        execute: ({ args }) => {
          calls.push(args);
          if (args[1] === "start")
            return "clocked in on thread-clock-proof at now  (session s1, agent lane-clock-proof)";
          if (args[1] === "status") return "not clocked in (agent lane-clock-proof)";
          return "clock stopped";
        },
      },
    ), "billable_clock_readback_failed");
    expect(calls).toEqual([
      ["clock", "status"],
      ["clock", "start", "thread-clock-proof"],
      ["clock", "status"],
      ["clock", "stop"],
    ]);
  });

  test("success requires exact start and exact live agent/thread readback", () => {
    const calls: string[][] = [];
    let statusCalls = 0;
    expect(admitBillableClock(
      { ...base, threadId: "thread-clock-proof" },
      {
        projectRoot,
        branchName,
        readThreadFacts: thread,
        execute: ({ args }) => {
          calls.push(args);
          if (args[1] === "start")
            return "clocked in on thread-clock-proof at now  (session s1, agent lane-clock-proof)\n";
          statusCalls++;
          return statusCalls === 1 ? absent
            : "clocked in on thread-clock-proof  Exact title  (agent lane-clock-proof)\n  since now";
        },
      },
    )).toEqual({
      kind: "opened",
      agentId: "lane-clock-proof",
      client: "msa",
      threadId: "thread-clock-proof",
    });
    expect(calls).toEqual([
      ["clock", "status"],
      ["clock", "start", "thread-clock-proof"],
      ["clock", "status"],
    ]);
  });

  test("a preexisting clock is never adopted or closed by admission", () => {
    const calls: string[][] = [];
    failureCode(() => admitBillableClock(
      { ...base, threadId: "thread-clock-proof" },
      {
        projectRoot,
        branchName,
        readThreadFacts: thread,
        execute: ({ args }) => {
          calls.push(args);
          return "clocked in on thread-clock-proof  Existing  (agent lane-clock-proof)";
        },
      },
    ), "billable_clock_readback_failed");
    expect(calls).toEqual([["clock", "status"]]);
  });

  test("non-client and read-only work remain non-blocking without a thread", () => {
    expect(admitBillableClock(
      { ...base, cwd: "/workspace", capabilities: ["filesystem.read"] },
      { projectRoot },
    )).toEqual({ kind: "not-required" });
    expect(admitBillableClock(
      base,
      { projectRoot: () => "/home/tom/code/north" },
    )).toEqual({ kind: "not-required" });
  });
});

describe("clientTicketForBranch", () => {
  test("extracts only a boundary-delimited ticket for the exact client", () => {
    expect(clientTicketForBranch("feature/msa-242-clock-proof", "msa")).toBe("MSA-242");
    expect(clientTicketForBranch("MSA-7", "msa")).toBe("MSA-7");
    expect(clientTicketForBranch("feature/notmsa-242", "msa")).toBeUndefined();
    expect(clientTicketForBranch("feature/other-242", "msa")).toBeUndefined();
    expect(clientTicketForBranch("msa-242-msa-999", "msa")).toBeUndefined();
  });
});

test("clock admission rejects a mutable PATH-shadow Git as a trust root", () => {
  const directory = mkdtempSync(join(tmpdir(), "north-shadow-git-"));
  const shadow = join(directory, "git");
  try {
    writeFileSync(shadow, "#!/bin/sh\nprintf forged\n");
    chmodSync(shadow, 0o755);
    expect(() => trustedGitExecutable([shadow]))
      .toThrow("trusted Nix-store Git executable unavailable");
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("Git root authority ignores every ambient repository/config redirect", () => {
  const saved = Object.fromEntries(Object.entries(process.env)
    .filter(([key]) => key.startsWith("GIT_")));
  try {
    process.env.GIT_DIR = "/tmp/forged-git-dir";
    process.env.GIT_WORK_TREE = "/tmp/forged-work-tree";
    process.env.GIT_CEILING_DIRECTORIES = resolve(import.meta.dir, "..");
    process.env.GIT_CONFIG_GLOBAL = "/tmp/forged-global-config";
    process.env.GIT_CONFIG_SYSTEM = "/tmp/forged-system-config";
    process.env.GIT_CONFIG_COUNT = "1";
    process.env.GIT_CONFIG_KEY_0 = "include.path";
    process.env.GIT_CONFIG_VALUE_0 = "/tmp/forged-include";
    const root = resolve(import.meta.dir, "../..");
    expect(trustedGitProjectRoot(join(root, "sdk", "src"))).toBe(root);
    expect(gitOracleEnvironment()).toEqual({
      HOME: "/homeless-shelter",
      PATH: "",
      LC_ALL: "C",
      GIT_CONFIG_NOSYSTEM: "1",
      GIT_CONFIG_GLOBAL: "/dev/null",
      GIT_CONFIG_SYSTEM: "/dev/null",
      GIT_CEILING_DIRECTORIES: "/",
    });
  } finally {
    for (const key of Object.keys(process.env)) {
      if (key.startsWith("GIT_")) delete process.env[key];
    }
    Object.assign(process.env, saved);
  }
});

test("Git root authority treats only Git's exact C-locale non-repository result as absence", () => {
  const directory = mkdtempSync(join(tmpdir(), "north-no-git-root-"));
  try {
    expect(trustedGitProjectRoot(directory)).toBe(realpathSync(directory));
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("Git root authority keeps every other fatal result fail-closed", () => {
  const directory = mkdtempSync(join(tmpdir(), "north-fatal-git-root-"));
  const fakeGit = join(directory, "git");
  try {
    writeFileSync(fakeGit, "#!/bin/sh\nprintf '%s\\n' 'fatal: unsafe repository authority' >&2\nexit 128\n");
    chmodSync(fakeGit, 0o755);
    let caught: unknown;
    try { trustedGitProjectRoot(directory, fakeGit); } catch (error) { caught = error; }
    expect(caught).toBeInstanceOf(TrustedGitOracleError);
    expect(caught).toMatchObject({ code: "execution_failed" });
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("managed Codex authority rejects mutable PATH and profile executables", () => {
  const directory = mkdtempSync(join(tmpdir(), "north-shadow-codex-"));
  const shadow = join(directory, "codex");
  try {
    writeFileSync(shadow, "#!/bin/sh\nexit 0\n");
    chmodSync(shadow, 0o755);
    expect(() => trustedManagedCodexExecutable([shadow]))
      .toThrow("trusted Nix-store Codex executable unavailable");
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});
