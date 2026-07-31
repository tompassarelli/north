import { expect, test } from "bun:test";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  DEFAULT_MANAGED_SESSION_HARD_CAP_MS,
  ManagedQueryTermination,
  SessionHardCapError,
  writeSessionHardCapHandoff,
} from "../src/query-lifecycle";
import type { HostTerminationParticipant } from "../src/host-termination";
import { classifyExecutionTerminal } from "../src/execution-outcome";

function inertParticipant(): HostTerminationParticipant {
  return {
    signal: () => undefined,
    publicationSettled: () => {},
    cleanupSettled: () => {},
    release: () => {},
  };
}

test("managed sessions hard-cap at exactly 60 minutes and write one handoff before teardown", () => {
  let fire!: () => void;
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

  fire();
  fire();

  expect(events).toEqual([
    "handoff",
    "abort",
    "input-closed",
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
  });
  termination.publicationSettled();
  termination.cleanupSettled();
  termination.release();
  expect(cancelled).toEqual(["deadline"]);
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

test("the terminal handoff artifact is atomic, self-contained, and indexed once", () => {
  const directory = mkdtempSync(join(tmpdir(), "north-session-hard-cap-"));
  const indexed: Array<[string, string]> = [];
  try {
    const result = writeSessionHardCapHandoff({
      agentId: "lane-artifact",
      threadId: "thread-artifact",
      goal: "one deliverable",
      repo: "/home/tom/code/north",
      worktree: "/home/tom/code/north/wt-artifact",
      branch: "lane-artifact",
    }, DEFAULT_MANAGED_SESSION_HARD_CAP_MS, {
      stateDirectory: directory,
      now: () => new Date("2026-07-31T01:02:03.000Z"),
      indexHandoff: (thread, value) => { indexed.push([thread, value]); },
    });

    expect(result.indexed).toBe(true);
    expect(indexed).toHaveLength(1);
    expect(indexed[0]?.[0]).toBe("thread-artifact");
    expect(indexed[0]?.[1]).toContain(result.path);
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
      worktree: "/home/tom/code/north/wt-artifact",
      branch: "lane-artifact",
      nextAction: "Resume only this deliverable; inspect the named thread, worktree, branch, and session transcript before editing.",
      completionClaimed: false,
    });
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("the hard cap is a typed blocked terminal, not a provider death", () => {
  expect(classifyExecutionTerminal("session_hard_cap")).toEqual({
    processOutcome: "session_hard_cap",
    deliveryOutcome: "blocked",
    deliveryReason: "north_managed_session_hard_cap",
  });
});
