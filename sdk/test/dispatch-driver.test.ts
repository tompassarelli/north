import { expect, test } from "bun:test";
import {
  claimDispatchDriver,
  DispatchAlreadyActiveError,
  DispatchDriverPreclaimAbsentError,
  DispatchDriverPreclaimMismatchError,
  DispatchDriverReleaseError,
  DispatchDriverUnavailableError,
  type DispatchDriverCommand,
} from "../src/dispatch-driver";
import { InvalidNorthEntityIdError } from "../src/north-client";
import { MIN_BEAGLE_STORE_COORDINATOR_CHILD_TIMEOUT_MS } from "../src/beagle-store";

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

test("competing, missing, mismatched, and coordinator failures stay distinct", () => {
  const hostile = "CANARY coordinator stderr must never cross boundary";
  const contended: DispatchDriverCommand = () => ({ status: 3, stderr: hostile } as any);
  const absent: DispatchDriverCommand = () => ({ status: 6, stderr: hostile } as any);
  const mismatched: DispatchDriverCommand = () => ({ status: 7, stderr: hostile } as any);
  const unavailable: DispatchDriverCommand = () => ({ status: 1, stderr: hostile } as any);

  let contention: unknown;
  try { claimDispatchDriver("thread-1", "agent-1", { command: contended }); }
  catch (error) { contention = error; }
  expect(contention).toBeInstanceOf(DispatchAlreadyActiveError);
  expect(contention).toMatchObject({ preSideEffect: true, threadId: "thread-1" });
  expect((contention as Error).message).not.toContain(hostile);

  let missingHandoff: unknown;
  try {
    claimDispatchDriver("thread-1", "agent-1", {
      command: absent,
      preclaimed: true,
    });
  } catch (error) { missingHandoff = error; }
  expect(missingHandoff).toBeInstanceOf(DispatchDriverPreclaimAbsentError);
  expect((missingHandoff as Error).message).toContain("is absent during SDK startup");
  expect((missingHandoff as Error).message).not.toContain(hostile);

  let wrongHandoff: unknown;
  try {
    claimDispatchDriver("thread-1", "agent-1", {
      command: mismatched,
      preclaimed: true,
    });
  } catch (error) { wrongHandoff = error; }
  expect(wrongHandoff).toBeInstanceOf(DispatchDriverPreclaimMismatchError);
  expect((wrongHandoff as Error).message).toContain("held by a different adapter");
  expect((wrongHandoff as Error).message).not.toContain(hostile);

  let failure: unknown;
  try {
    claimDispatchDriver("thread-1", "agent-1", {
      command: unavailable,
      port: "17977",
    });
  }
  catch (error) { failure = error; }
  expect(failure).toBeInstanceOf(DispatchDriverUnavailableError);
  expect(failure).toMatchObject({
    preSideEffect: true,
    threadId: "thread-1",
    port: "17977",
  });
  expect((failure as Error).message).toContain(
    "North coordinator unavailable or mismatched at port 17977",
  );
  expect((failure as Error).message).not.toContain(hostile);
});

// A driver failure must be OPAQUE TO THE MODEL and EXPLICIT TO THE OPERATOR.
// Suppressing both is what made this the longest-lived silent failure in the
// harness: a lane died showing only
//     [death] @agent:lane-… died: spawnSync /nix/store/…/bb ETIMEDOUT
// while the real cause was a shell.readonly template whose read-only sandbox
// blocks :7977, so the claim HUNG until the 8s budget expired. Confirmed by
// controlled experiment 2026-07-29: the same probe as `scout` (read-only) died,
// as `implementer` (workspace-write) ran.
function captureStderr(run: () => void): string {
  const chunks: string[] = [];
  const original = process.stderr.write;
  (process.stderr as any).write = (chunk: any) => { chunks.push(String(chunk)); return true; };
  try { run(); } finally { (process.stderr as any).write = original; }
  return chunks.join("");
}

test("a driver failure is explained on stderr without leaking coordinator output", () => {
  const hostile = "CANARY coordinator stderr must never cross boundary";
  const timedOut: DispatchDriverCommand = () => ({
    status: null,
    error: Object.assign(new Error("spawnSync bb ETIMEDOUT"), { code: "ETIMEDOUT" }),
    stderr: hostile,
  } as any);

  let thrown: unknown;
  const emitted = captureStderr(() => {
    try { claimDispatchDriver("thread-1", "agent-1", { command: timedOut, port: "7977" }); }
    catch (error) { thrown = error; }
  });

  // Opaque to the model: the thrown error stays fixed.
  expect(thrown).toBeInstanceOf(DispatchDriverUnavailableError);
  expect((thrown as Error).message).not.toContain(hostile);
  expect((thrown as Error).message).not.toContain("ETIMEDOUT");

  // Explicit to the operator: errno, and what a hang actually implies.
  expect(emitted).toContain("dispatch driver claim failed on :7977");
  expect(emitted).toContain("errno=ETIMEDOUT");
  expect(emitted).toContain("shell.readonly");
  expect(emitted).toContain(`${MIN_BEAGLE_STORE_COORDINATOR_CHILD_TIMEOUT_MS}ms`);

  // The boundary still holds — coordinator stderr reaches NEITHER surface.
  expect(emitted).not.toContain(hostile);
});

test("a refused claim reports its status without the ETIMEDOUT hypothesis", () => {
  // status=1 is a REFUSAL, not a hang. Offering the sandbox explanation here
  // would send the reader at the wrong cause.
  const refused: DispatchDriverCommand = () => ({ status: 1, stderr: "noise" } as any);
  const emitted = captureStderr(() => {
    try { claimDispatchDriver("thread-1", "agent-1", { command: refused }); } catch {}
  });
  expect(emitted).toContain("status=1");
  expect(emitted).not.toContain("shell.readonly");
  expect(emitted).not.toContain("noise");
});

test("the failure reporter never throws on a malformed result", () => {
  const malformed: DispatchDriverCommand = () => (null as any);
  expect(() => {
    try { claimDispatchDriver("thread-1", "agent-1", { command: malformed }); } catch {}
  }).not.toThrow();
});
