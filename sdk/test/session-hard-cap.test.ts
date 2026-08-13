import { expect, test } from "bun:test";
import { mkdtempSync, readdirSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  DEFAULT_MANAGED_SESSION_HARD_CAP_MS,
  ManagedQueryTermination,
  replaySessionHardCapHandoffs,
  SessionHardCapError,
  writeSessionHardCapHandoff,
} from "../src/query-lifecycle";
import type { HostTerminationParticipant } from "../src/host-termination";
import { classifyExecutionTerminal } from "../src/execution-outcome";
import { dispatch } from "./support/dispatch";
import { spawn } from "./support/spawn";

function inertParticipant(): HostTerminationParticipant {
  return {
    signal: () => undefined,
    publicationSettled: () => {},
    cleanupSettled: () => {},
    release: () => {},
  };
}

test("managed sessions hard-cap at exactly 60 minutes and write one handoff before teardown", async () => {
  let fire!: () => void | Promise<void>;
  const cancelled: unknown[] = [];
  const events: string[] = [];
  const termination = new ManagedQueryTermination(
    () => inertParticipant(),
    {
      agentId: "lane-hard-cap",
      threadId: "thread-hard-cap",
      goal: "ship one bounded deliverable",
      repo: "/home/tom/code/north",
      schedule: (callback, delayMs) => {
        expect(delayMs).toBe(DEFAULT_MANAGED_SESSION_HARD_CAP_MS);
        fire = callback;
        return "deadline";
      },
      cancel: (timer) => { cancelled.push(timer); },
      writeHandoff: (document) => {
        events.push("handoff");
        expect(document.reason).toBe("session_hard_cap");
        expect(document.hardCapMs).toBe(60 * 60_000);
        return {
          path: "/state/session-handoffs/lane-hard-cap.json",
          indexed: true,
          spooled: false,
        };
      },
    },
  );
  termination.signal.addEventListener("abort", () => events.push("abort"));
  termination.attachInput(() => events.push("input-closed"));
  termination.attachQuery({
    forceClose: () => { events.push("query-killed"); },
    async *[Symbol.asyncIterator]() {},
  });

  await fire();
  await fire();

  expect(events).toEqual([
    "input-closed",
    "handoff",
    "abort",
    "query-killed",
  ]);
  expect(termination.signal.reason).toBeInstanceOf(SessionHardCapError);
  expect(() => termination.throwIfTerminated()).toThrow(
    "managed session reached its 3600000ms hard cap",
  );
  expect(termination.hardCapStatus()).toEqual({
    hardCapMs: 3_600_000,
    handoffPath: "/state/session-handoffs/lane-hard-cap.json",
    indexed: true,
    spooled: false,
  });
  termination.publicationSettled();
  termination.cleanupSettled();
  termination.release();
  expect(cancelled).toEqual(["deadline"]);
});

test("a recovered lane inherits its original absolute deadline", () => {
  const directory = mkdtempSync(join(tmpdir(), "north-session-deadline-"));
  const scheduled: number[] = [];
  let now = new Date("2026-07-31T00:00:00.000Z");
  const options = {
    agentId: "lane-recovered-deadline",
    threadId: "thread-recovered-deadline",
    goal: "one bounded deliverable",
    repo: "/home/tom/code/north",
    stateDirectory: directory,
    now: () => now,
    schedule: (_callback: () => void, delayMs: number) => {
      scheduled.push(delayMs);
      return `deadline-${scheduled.length}`;
    },
    cancel: () => {},
    writeHandoff: () => {
      throw new Error("deadline was not fired");
    },
  };
  try {
    const original = new ManagedQueryTermination(
      () => inertParticipant(),
      options as any,
    );
    now = new Date("2026-07-31T00:15:00.000Z");
    const recovered = new ManagedQueryTermination(
      () => inertParticipant(),
      options as any,
    );

    expect(scheduled).toEqual([
      DEFAULT_MANAGED_SESSION_HARD_CAP_MS,
      DEFAULT_MANAGED_SESSION_HARD_CAP_MS - 15 * 60_000,
    ]);
    recovered.release();
    original.release();
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("hard-cap expiry awaits Codex-style interrupt when forceClose is absent", async () => {
  let fire!: () => void | Promise<void>;
  let settleInterrupt!: () => void;
  let providerAlive = true;
  let queryUnsettled = true;
  const events: string[] = [];
  const termination = new ManagedQueryTermination(
    () => inertParticipant(),
    {
      agentId: "lane-codex-hard-cap",
      threadId: "thread-codex-hard-cap",
      goal: "terminate the provider",
      repo: "/home/tom/code/north",
      schedule: (callback) => {
        fire = callback;
        return "deadline";
      },
      cancel: () => {},
      writeHandoff: () => {
        events.push("handoff");
        return {
          path: "/state/session-handoffs/lane-codex-hard-cap.json",
          indexed: true,
          spooled: false,
        };
      },
    },
  );
  termination.signal.addEventListener("abort", () => events.push("abort"));
  termination.attachInput(() => events.push("input-closed"));
  termination.attachQuery({
    interrupt: () => new Promise<void>((resolve) => {
      settleInterrupt = () => {
        resolve();
      };
    }).then(() => {
      providerAlive = false;
      queryUnsettled = false;
      events.push("query-interrupted");
    }),
    async *[Symbol.asyncIterator]() {},
  });

  const expiry = Promise.resolve(fire());
  await Promise.resolve();
  expect(termination.hardCapStatus()).toBeUndefined();
  expect(providerAlive).toBe(true);
  expect(queryUnsettled).toBe(true);
  settleInterrupt();
  await expiry;

  expect(events).toEqual([
    "input-closed",
    "handoff",
    "abort",
    "query-interrupted",
  ]);
  expect(providerAlive).toBe(false);
  expect(queryUnsettled).toBe(false);
  expect(termination.hardCapStatus()).toMatchObject({
    hardCapMs: DEFAULT_MANAGED_SESSION_HARD_CAP_MS,
    indexed: true,
  });
  termination.release();
});

test("managed requests and plausible environment overrides cannot change the production cap", async () => {
  const names = [
    "NORTH_MANAGED_SESSION_HARD_CAP_MS",
    "NORTH_SESSION_HARD_CAP_MS",
    "AGENT_SESSION_HARD_CAP_MS",
  ] as const;
  const previous = new Map(names.map((name) => [name, process.env[name]]));
  for (const name of names) process.env[name] = "1";
  let scheduled: number | undefined;
  try {
    await expect(spawn({ hardCapMs: 1 } as any)).rejects.toThrow(
      "managed North spawn request has unknown field hardCapMs",
    );
    await expect(dispatch("thread-hard-cap", { hardCapMs: 1 } as any)).rejects.toThrow(
      "managed North dispatch request has unknown field hardCapMs",
    );
    const termination = new ManagedQueryTermination(
      () => inertParticipant(),
      {
        agentId: "lane-fixed-cap",
        threadId: "thread-fixed-cap",
        goal: "fixed production cap",
        repo: "/home/tom/code/north",
        schedule: (_callback, delayMs) => {
          scheduled = delayMs;
          return "deadline";
        },
        cancel: () => {},
        writeHandoff: () => {
          throw new Error("deadline was not fired");
        },
      },
    );
    expect(scheduled).toBe(DEFAULT_MANAGED_SESSION_HARD_CAP_MS);
    termination.release();
  } finally {
    for (const name of names) {
      const value = previous.get(name);
      if (value === undefined) delete process.env[name];
      else process.env[name] = value;
    }
  }
});

test("a normally settled query cancels the deadline without writing a handoff", async () => {
  let fire!: () => void;
  let cancelled = 0;
  let handoffs = 0;
  const termination = new ManagedQueryTermination(
    () => inertParticipant(),
    {
      agentId: "lane-finished",
      goal: "finish before the cap",
      repo: "/home/tom/code/north",
      hardCapMs: 25,
      schedule: (callback) => {
        fire = callback;
        return "deadline";
      },
      cancel: () => { cancelled++; },
      writeHandoff: () => {
        handoffs++;
        return { path: "/should/not/exist", indexed: false };
      },
    },
  );
  termination.attachQuery({
    close: async () => {},
    async *[Symbol.asyncIterator]() {},
  });

  await termination.close();
  fire();

  expect(cancelled).toBe(1);
  expect(handoffs).toBe(0);
  expect(termination.hardCapStatus()).toBeUndefined();
  termination.publicationSettled();
  termination.cleanupSettled();
  termination.release();
});

test("the terminal handoff artifact and direct index are durable and idempotent", () => {
  const directory = mkdtempSync(join(tmpdir(), "north-session-hard-cap-"));
  const indexed: Array<[string, string]> = [];
  const context = {
    agentId: "lane-artifact",
    threadId: "thread-artifact",
    goal: "one deliverable",
    repo: "/home/tom/code/north",
    worktree: "/home/tom/code/north/worktrees/artifact",
    branch: "lane-artifact",
  };
  try {
    const result = writeSessionHardCapHandoff(context, DEFAULT_MANAGED_SESSION_HARD_CAP_MS, {
      stateDirectory: directory,
      now: () => new Date("2026-07-31T01:02:03.000Z"),
      indexHandoff: (thread, value) => { indexed.push([thread, value]); },
    });
    const duplicate = writeSessionHardCapHandoff(
      context,
      DEFAULT_MANAGED_SESSION_HARD_CAP_MS,
      {
        stateDirectory: directory,
        now: () => new Date("2026-07-31T02:02:03.000Z"),
        indexHandoff: (thread, value) => { indexed.push([thread, value]); },
      },
    );

    expect(result.indexed).toBe(true);
    expect(result.spooled).toBe(false);
    expect(result.idempotencyKey).toMatch(/^[0-9a-f]{64}$/);
    expect(duplicate).toEqual(result);
    expect(indexed).toHaveLength(1);
    expect(indexed[0]?.[0]).toBe("thread-artifact");
    expect(indexed[0]?.[1]).toContain(result.path);
    expect(indexed[0]?.[1]).toContain(`idempotency_key=${result.idempotencyKey}`);
    expect(readdirSync(join(directory, "outbox"))).toEqual([]);
    expect(readdirSync(join(directory, "settled"))).toEqual([
      `${result.idempotencyKey}.json`,
    ]);
    const document = JSON.parse(readFileSync(result.path, "utf8"));
    expect(document).toEqual({
      version: 1,
      reason: "session_hard_cap",
      writtenAt: "2026-07-31T01:02:03.000Z",
      hardCapMs: 3_600_000,
      agentId: "lane-artifact",
      threadId: "thread-artifact",
      goal: "one deliverable",
      repo: "/home/tom/code/north",
      worktree: "/home/tom/code/north/worktrees/artifact",
      branch: "lane-artifact",
      nextAction: "Resume only this deliverable; inspect the named thread, worktree, branch, and session transcript before editing.",
      completionClaimed: false,
    });
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("an explicit writer runtime overrides an unusable NORTH_BIN environment", () => {
  const directory = mkdtempSync(join(tmpdir(), "north-session-hard-cap-runtime-"));
  const previous = process.env.NORTH_BIN;
  process.env.NORTH_BIN = "/must/not/be-executed";
  try {
    const result = writeSessionHardCapHandoff({
      agentId: "lane-runtime",
      threadId: "thread-runtime",
      goal: "runtime override",
      repo: "/home/tom/code/north",
    }, DEFAULT_MANAGED_SESSION_HARD_CAP_MS, {
      stateDirectory: directory,
      indexHandoff: () => {},
    });
    expect(result.indexed).toBe(true);
    expect(result.path.startsWith(directory)).toBe(true);
  } finally {
    if (previous === undefined) delete process.env.NORTH_BIN;
    else process.env.NORTH_BIN = previous;
    rmSync(directory, { recursive: true, force: true });
  }
});

test("forced thread-index failure spools once before provider teardown and replays at settlement", async () => {
  const directory = mkdtempSync(join(tmpdir(), "north-session-hard-cap-failure-"));
  let fire!: () => void | Promise<void>;
  let indexAvailable = false;
  let indexAttempts = 0;
  const events: string[] = [];
  const context = {
    agentId: "lane-index-failure",
    threadId: "thread-index-failure",
    goal: "preserve the terminal handoff",
    repo: "/home/tom/code/north",
  };
  const indexHandoff = () => {
    indexAttempts++;
    if (!indexAvailable) throw new Error("forced index failure");
  };
  try {
    const termination = new ManagedQueryTermination(
      () => inertParticipant(),
      {
        ...context,
        schedule: (callback, delayMs) => {
          expect(delayMs).toBe(DEFAULT_MANAGED_SESSION_HARD_CAP_MS);
          fire = callback;
          return "deadline";
        },
        cancel: () => {},
        writeHandoff: () => {
          events.push("handoff");
          return writeSessionHardCapHandoff(
            context,
            DEFAULT_MANAGED_SESSION_HARD_CAP_MS,
            {
              stateDirectory: directory,
              now: () => new Date("2026-07-31T03:02:03.000Z"),
              indexHandoff,
            },
          );
        },
        replayHandoffs: () => replaySessionHardCapHandoffs({
          stateDirectory: directory,
          indexHandoff,
        }),
      },
    );
    termination.signal.addEventListener("abort", () => events.push("abort"));
    termination.attachInput(() => events.push("input-closed"));
    termination.attachQuery({
      forceClose: () => { events.push("query-killed"); },
      async *[Symbol.asyncIterator]() {},
    });

    await fire();
    await fire();

    expect(events).toEqual([
      "input-closed",
      "handoff",
      "abort",
      "query-killed",
    ]);
    expect(indexAttempts).toBe(1);
    expect(termination.hardCapStatus()).toMatchObject({
      hardCapMs: DEFAULT_MANAGED_SESSION_HARD_CAP_MS,
      indexed: false,
      spooled: true,
    });
    expect(readdirSync(join(directory, "outbox"))).toHaveLength(1);

    indexAvailable = true;
    termination.publicationSettled();
    termination.cleanupSettled();
    termination.release();

    expect(indexAttempts).toBe(2);
    expect(readdirSync(join(directory, "outbox"))).toEqual([]);
    expect(readdirSync(join(directory, "settled"))).toHaveLength(1);
    expect(replaySessionHardCapHandoffs({
      stateDirectory: directory,
      indexHandoff,
    })).toBe(0);
    expect(indexAttempts).toBe(2);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("provider teardown is withheld when neither thread index nor durable outbox exists", () => {
  let fire!: () => void;
  const events: string[] = [];
  const termination = new ManagedQueryTermination(
    () => inertParticipant(),
    {
      agentId: "lane-no-durable-handoff",
      threadId: "thread-no-durable-handoff",
      goal: "fail closed",
      repo: "/home/tom/code/north",
      schedule: (callback) => {
        fire = callback;
        return "deadline";
      },
      cancel: () => {},
      writeHandoff: () => {
        events.push("handoff-unavailable");
        return { path: "/not/durable", indexed: false, spooled: false };
      },
    },
  );
  termination.signal.addEventListener("abort", () => events.push("abort"));
  termination.attachInput(() => events.push("input-closed"));
  termination.attachQuery({
    forceClose: () => { events.push("query-killed"); },
    async *[Symbol.asyncIterator]() {},
  });

  fire();

  expect(events).toEqual(["input-closed", "handoff-unavailable"]);
  expect(termination.signal.aborted).toBe(false);
  expect(termination.hardCapStatus()).toBeUndefined();
  termination.release();
});

test("the hard cap is a typed blocked terminal, not a provider death", () => {
  expect(classifyExecutionTerminal("session_hard_cap")).toEqual({
    processOutcome: "session_hard_cap",
    deliveryOutcome: "blocked",
    deliveryReason: "north_managed_session_hard_cap",
  });
});
