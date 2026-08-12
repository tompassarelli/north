import { expect, test } from "bun:test";
import { join } from "node:path";
import {
  FRAM_RUNTIME_HOME,
  MIN_FRAM_COORDINATOR_CHILD_TIMEOUT_MS,
  framBabashkaArguments,
  framCoordinatorChildTimeout,
  framEngineEnvironment,
  framEngineSelection,
  framExecutable,
  settleFramCoordinatorChild,
} from "../src/fram-engine";

test("Fram engine selectors honor explicit environment independently", () => {
  const env = {
    FRAM_HOME: "/explicit/home",
    FRAM_BIN: "/independent/bin",
    FRAM_OUT: "/independent/out",
    KEEP_ME: "yes",
  };
  expect(framEngineSelection(env)).toEqual({
    home: "/explicit/home",
    bin: "/independent/bin",
    out: "/independent/out",
  });
  expect(framEngineEnvironment(env)).toMatchObject({
    ...env,
    FRAM_HOME: "/explicit/home",
    FRAM_BIN: "/independent/bin",
    FRAM_OUT: "/independent/out",
  });
  expect(framBabashkaArguments(["writer.clj", "7977"], env)).toEqual([
    "-cp", "/independent/out", "writer.clj", "7977",
  ]);
});

test("unset Fram engine selectors default independently to the promoted runtime", () => {
  const selection = framEngineSelection({ HOME: "/different/home" });
  expect(selection).toEqual({
    home: FRAM_RUNTIME_HOME,
    bin: join(FRAM_RUNTIME_HOME, "bin"),
    out: join(FRAM_RUNTIME_HOME, "out"),
  });
  expect(framEngineSelection({ HOME: "/home/tom", FRAM_HOME: "/only/home" })).toEqual({
    home: "/only/home",
    bin: join(FRAM_RUNTIME_HOME, "bin"),
    out: join(FRAM_RUNTIME_HOME, "out"),
  });
  expect(framEngineSelection({
    HOME: "/home/tom",
    FRAM_HOME: "   ",
    FRAM_BIN: " ",
    FRAM_OUT: "\t",
  })).toEqual({ home: "   ", bin: " ", out: "\t" });
  expect(framExecutable({ FRAM_BIN: "/explicit/bin" })).toBe("/explicit/bin/fram");
});

test("coordinator children retain at least thirty seconds for fencing stalls", () => {
  expect(framCoordinatorChildTimeout()).toBe(MIN_FRAM_COORDINATOR_CHILD_TIMEOUT_MS);
  expect(framCoordinatorChildTimeout(15_000)).toBe(MIN_FRAM_COORDINATOR_CHILD_TIMEOUT_MS);
  expect(framCoordinatorChildTimeout(45_000)).toBe(45_000);
});

test("coordinator child timeout escalates and returns after a bounded reap", async () => {
  const neverExits = Promise.withResolvers<number>();
  const signals: string[] = [];
  const sleeps: number[] = [];
  const outcome = await settleFramCoordinatorChild({
    exited: neverExits.promise,
    kill: (signal) => { signals.push(signal); },
  }, 45_000, {
    termGraceMs: 200,
    killGraceMs: 300,
    sleep: async (milliseconds) => { sleeps.push(milliseconds); },
  });

  expect(outcome).toEqual({ timedOut: true });
  expect(signals).toEqual(["SIGTERM", "SIGKILL"]);
  expect(sleeps).toEqual([45_000, 200, 300]);
});

test("coordinator child abort immediately escalates and reaps", async () => {
  const neverExits = Promise.withResolvers<number>();
  const signals: string[] = [];
  const sleeps: number[] = [];
  const abort = new AbortController();
  abort.abort(new Error("host stopped"));
  const outcome = await settleFramCoordinatorChild({
    exited: neverExits.promise,
    kill: (signal) => { signals.push(signal); },
  }, 45_000, {
    signal: abort.signal,
    termGraceMs: 200,
    killGraceMs: 300,
    sleep: async (milliseconds) => { sleeps.push(milliseconds); },
  });

  expect(outcome).toEqual({ timedOut: true });
  expect(signals).toEqual(["SIGTERM", "SIGKILL"]);
  expect(sleeps).toEqual([45_000, 200, 300]);
});
