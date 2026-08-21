import { expect, test } from "bun:test";
import {
  "cleanup-suspend!" as cleanupSuspend,
  "suspend-runtime!" as suspendRuntime,
} from "../src/bridge/generated/north/bridge/app.js";

type Listener = () => void;

function fakeProcess(log: string[], stopError?: Error) {
  let listener: Listener | undefined;
  return {
    once(signal: string, next: Listener) {
      log.push(`once:${signal}`);
      listener = next;
    },
    removeListener(signal: string, next: Listener) {
      log.push(`remove:${signal}:${String(next === listener)}`);
      if (next === listener) listener = undefined;
    },
    kill(pid: number, signal: string) {
      log.push(`kill:${pid}:${signal}`);
      if (stopError) throw stopError;
      return true;
    },
    continue() { listener?.(); },
    listener() { return listener; },
  };
}

function runtime(log: string[], suspendError?: Error) {
  return {
    disposed: false,
    rendererSuspended: false,
    suspendResume: null as Listener | null,
    suspendError: "",
    renderer: {
      suspend() {
        log.push("renderer:suspend");
        if (suspendError) throw suspendError;
      },
      resume() { log.push("renderer:resume"); },
    },
    render() { log.push("render"); },
  };
}

test("POSIX suspend installs continuation recovery before stopping the foreground group", () => {
  const log: string[] = [];
  const processApi = fakeProcess(log);
  const target = runtime(log);

  expect(suspendRuntime(target, "linux", processApi)).toBe(true);
  expect(log).toEqual(["once:SIGCONT", "renderer:suspend", "kill:0:SIGSTOP"]);
  expect(target.rendererSuspended).toBe(true);
  expect(target.suspendResume).toBe(processApi.listener());

  processApi.continue();
  expect(log).toEqual([
    "once:SIGCONT", "renderer:suspend", "kill:0:SIGSTOP",
    "remove:SIGCONT:true", "renderer:resume", "render",
  ]);
  expect(target.rendererSuspended).toBe(false);
  expect(target.suspendResume).toBeNull();
});

test("Windows Ctrl-Z is a no-op", () => {
  const log: string[] = [];
  const processApi = fakeProcess(log);
  const target = runtime(log);
  expect(suspendRuntime(target, "win32", processApi)).toBe(false);
  expect(log).toEqual([]);
  expect(target.suspendResume).toBeNull();
});

test("a failed POSIX stop removes the exact handler and restores OpenTUI", () => {
  const log: string[] = [];
  const processApi = fakeProcess(log, new Error("stop denied"));
  const target = runtime(log);

  expect(suspendRuntime(target, "darwin", processApi)).toBe(false);
  expect(log).toEqual([
    "once:SIGCONT", "renderer:suspend", "kill:0:SIGSTOP",
    "remove:SIGCONT:true", "renderer:resume", "render",
  ]);
  expect(target.suspendError).toBe("stop denied");
  expect(target.rendererSuspended).toBe(false);
  expect(target.suspendResume).toBeNull();
  expect(processApi.listener()).toBeUndefined();
});

test("an OpenTUI suspend failure still attempts terminal recovery", () => {
  const log: string[] = [];
  const processApi = fakeProcess(log);
  const target = runtime(log, new Error("terminal suspend failed"));

  expect(suspendRuntime(target, "linux", processApi)).toBe(false);
  expect(log).toEqual([
    "once:SIGCONT", "renderer:suspend", "remove:SIGCONT:true", "renderer:resume", "render",
  ]);
  expect(target.suspendError).toBe("terminal suspend failed");
  expect(target.rendererSuspended).toBe(false);
  expect(processApi.listener()).toBeUndefined();
});

test("teardown clears pending continuation recovery and resumes a suspended renderer", () => {
  const log: string[] = [];
  const processApi = fakeProcess(log);
  const target = runtime(log);
  expect(suspendRuntime(target, "linux", processApi)).toBe(true);
  const stale = processApi.listener();

  target.disposed = true;
  expect(cleanupSuspend(target, processApi)).toBe(true);
  expect(log).toEqual([
    "once:SIGCONT", "renderer:suspend", "kill:0:SIGSTOP",
    "remove:SIGCONT:true", "renderer:resume",
  ]);
  stale?.();
  expect(log.at(-1)).toBe("renderer:resume");
  expect(cleanupSuspend(target, processApi)).toBe(false);
});
