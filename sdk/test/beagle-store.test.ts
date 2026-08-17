import { expect, test } from "bun:test";
import { join } from "node:path";
import {
  BEAGLE_STORE_RUNTIME_HOME,
  MIN_BEAGLE_STORE_COORDINATOR_CHILD_TIMEOUT_MS,
  beagleStoreBabashkaArguments,
  beagleStoreCoordinatorChildTimeout,
  beagleStoreEnvironment,
  beagleStoreSelection,
  beagleStoreExecutable,
  settleBeagleStoreCoordinatorChild,
} from "../src/beagle-store";

test("Beagle Store engine selectors honor explicit environment independently", () => {
  const env = {
    BEAGLE_STORE_HOME: "/explicit/home",
    BEAGLE_STORE_BIN: "/independent/bin",
    BEAGLE_STORE_OUT: "/independent/out",
    KEEP_ME: "yes",
  };
  expect(beagleStoreSelection(env)).toEqual({
    home: "/explicit/home",
    bin: "/independent/bin",
    out: "/independent/out",
  });
  expect(beagleStoreEnvironment(env)).toMatchObject({
    ...env,
    BEAGLE_STORE_HOME: "/explicit/home",
    BEAGLE_STORE_BIN: "/independent/bin",
    BEAGLE_STORE_OUT: "/independent/out",
  });
  expect(beagleStoreBabashkaArguments(["writer.clj", "7977"], env)).toEqual([
    "-cp", "/independent/out", "writer.clj", "7977",
  ]);
});

test("unset Beagle Store engine selectors default independently to the promoted runtime", () => {
  const selection = beagleStoreSelection({ HOME: "/different/home" });
  expect(selection).toEqual({
    home: BEAGLE_STORE_RUNTIME_HOME,
    bin: join(BEAGLE_STORE_RUNTIME_HOME, "bin"),
    out: join(BEAGLE_STORE_RUNTIME_HOME, "out"),
  });
  expect(beagleStoreSelection({ HOME: "/home/tom", BEAGLE_STORE_HOME: "/only/home" })).toEqual({
    home: "/only/home",
    bin: join(BEAGLE_STORE_RUNTIME_HOME, "bin"),
    out: join(BEAGLE_STORE_RUNTIME_HOME, "out"),
  });
  expect(beagleStoreSelection({
    HOME: "/home/tom",
    BEAGLE_STORE_HOME: "   ",
    BEAGLE_STORE_BIN: " ",
    BEAGLE_STORE_OUT: "\t",
  })).toEqual({ home: "   ", bin: " ", out: "\t" });
  expect(beagleStoreExecutable({ BEAGLE_STORE_HOME: "/exact/beagle/store" }))
    .toBe("/exact/beagle/bin/beagle");
});

test("coordinator children retain at least thirty seconds for fencing stalls", () => {
  expect(beagleStoreCoordinatorChildTimeout()).toBe(MIN_BEAGLE_STORE_COORDINATOR_CHILD_TIMEOUT_MS);
  expect(beagleStoreCoordinatorChildTimeout(15_000)).toBe(MIN_BEAGLE_STORE_COORDINATOR_CHILD_TIMEOUT_MS);
  expect(beagleStoreCoordinatorChildTimeout(45_000)).toBe(45_000);
});

test("coordinator child timeout escalates and returns after a bounded reap", async () => {
  const neverExits = Promise.withResolvers<number>();
  const signals: string[] = [];
  const sleeps: number[] = [];
  const outcome = await settleBeagleStoreCoordinatorChild({
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
  const outcome = await settleBeagleStoreCoordinatorChild({
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
