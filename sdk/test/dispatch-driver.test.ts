import { expect, test } from "bun:test";
import {
  claimDispatchDriver,
  DispatchAlreadyActiveError,
  DispatchDriverReleaseError,
  DispatchDriverUnavailableError,
  type DispatchDriverCommand,
} from "../src/dispatch-driver";
import { InvalidNorthEntityIdError } from "../src/north-client";

test("canonical dispatch claims once and releases exactly once", () => {
  const calls: string[] = [];
  const command: DispatchDriverCommand = (verb, thread, agent) => {
    calls.push(`${verb}:${thread}:${agent}`);
    return { status: 0 };
  };
  const lease = claimDispatchDriver("thread-1", "agent-1", { command });
  lease.release();
  lease.release();
  expect(calls).toEqual([
    "claim:@thread-1:agent-1",
    "release:@thread-1:agent-1",
  ]);
});

test("an MCP-preclaimed dispatch verifies the same holder instead of reacquiring", () => {
  const calls: string[] = [];
  const command: DispatchDriverCommand = (verb) => { calls.push(verb); return { status: 0 }; };
  claimDispatchDriver("thread-1", "agent-1", { command, preclaimed: true }).release();
  expect(calls).toEqual(["verify", "release"]);
});

test("release reports coordinator failure and remains retryable", () => {
  let attempts = 0;
  const command: DispatchDriverCommand = (verb) => {
    if (verb !== "release") return { status: 0 };
    attempts++;
    return { status: attempts === 1 ? 5 : 0 };
  };
  const lease = claimDispatchDriver("thread-1", "agent-1", { command });
  expect(lease.release()).toBe(false);
  expect(lease.release()).toBe(true);
  expect(lease.release()).toBe(true);
  expect(attempts).toBe(2);
  expect(new DispatchDriverReleaseError("thread-1")).toMatchObject({
    preSideEffect: false,
    retrySafe: false,
    threadId: "thread-1",
  });
});

test("driver claim canonicalizes one sigil and rejects hostile ids before the command", () => {
  const calls: string[] = [];
  const command: DispatchDriverCommand = (verb, thread) => {
    calls.push(`${verb}:${thread}`);
    return { status: 0 };
  };
  claimDispatchDriver("@thread-1", "agent-1", { command }).release();
  expect(calls).toEqual(["claim:@thread-1", "release:@thread-1"]);

  for (const invalid of ["", "@", "@@thread-1", " thread-1", "thread-1;touch-owned"]) {
    expect(() => claimDispatchDriver(invalid, "agent-1", { command }))
      .toThrow(InvalidNorthEntityIdError);
  }
  expect(calls).toHaveLength(2);
});

test("contention and coordinator failure are distinct fixed pre-side-effect errors", () => {
  const hostile = "CANARY coordinator stderr must never cross boundary";
  const contended: DispatchDriverCommand = () => ({ status: 3, stderr: hostile } as any);
  const unavailable: DispatchDriverCommand = () => ({ status: 1, stderr: hostile } as any);

  let contention: unknown;
  try { claimDispatchDriver("thread-1", "agent-1", { command: contended }); }
  catch (error) { contention = error; }
  expect(contention).toBeInstanceOf(DispatchAlreadyActiveError);
  expect(contention).toMatchObject({ preSideEffect: true, threadId: "thread-1" });
  expect((contention as Error).message).not.toContain(hostile);

  let failure: unknown;
  try { claimDispatchDriver("thread-1", "agent-1", { command: unavailable }); }
  catch (error) { failure = error; }
  expect(failure).toBeInstanceOf(DispatchDriverUnavailableError);
  expect(failure).toMatchObject({ preSideEffect: true, threadId: "thread-1" });
  expect((failure as Error).message).not.toContain(hostile);
});
