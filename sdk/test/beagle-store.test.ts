import { expect, test } from "bun:test";
import {
  MIN_BEAGLE_STORE_COORDINATOR_CHILD_TIMEOUT_MS,
  beagleStoreBabashkaArguments,
  beagleStoreCoordinatorChildTimeout,
  beagleStoreEnvironment,
  beagleStoreSelection,
  beagleStoreExecutable,
  settleBeagleStoreCoordinatorChild,
} from "../src/beagle-store";

test("Beagle Store uses one explicit home, bin, and out selection", () => {
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

test("the Store selection matrix has explicit, installed, and absent cases", () => {
  const matrix = [
    {
      name: "explicit",
      env: {
        BEAGLE_STORE_HOME: "/explicit/home",
        BEAGLE_STORE_BIN: "/explicit/home/bin",
        BEAGLE_STORE_OUT: "/explicit/home/out",
      },
      expected: {
        home: "/explicit/home", bin: "/explicit/home/bin", out: "/explicit/home/out",
      },
    },
    {
      name: "installed",
      env: {
        BEAGLE_STORE_HOME: "/nix/store/beagle/store",
        BEAGLE_STORE_BIN: "/nix/store/beagle/store/bin",
        BEAGLE_STORE_OUT: "/nix/store/beagle/store/out",
      },
      expected: {
        home: "/nix/store/beagle/store", bin: "/nix/store/beagle/store/bin",
        out: "/nix/store/beagle/store/out",
      },
    },
  ] as const;
  for (const current of matrix) {
    expect(beagleStoreSelection(current.env), current.name).toEqual(current.expected);
  }
  expect(() => beagleStoreSelection({ HOME: "/home/tom" }), "absent")
    .toThrow("BEAGLE_STORE_HOME, BEAGLE_STORE_BIN, BEAGLE_STORE_OUT");
  expect(() => beagleStoreSelection({
    BEAGLE_STORE_HOME: " ", BEAGLE_STORE_BIN: "/bin", BEAGLE_STORE_OUT: "/out",
  })).toThrow("BEAGLE_STORE_HOME");
  expect(() => beagleStoreExecutable({ BEAGLE_STORE_HOME: "/exact/beagle/store" }))
    .toThrow("BEAGLE_STORE_BIN");
  expect(beagleStoreExecutable({
    BEAGLE_STORE_HOME: "/exact/beagle/store",
    BEAGLE_STORE_BIN: "/exact/beagle/bin",
    BEAGLE_STORE_OUT: "/exact/beagle/out",
  })).toBe("/exact/beagle/bin/beagle");
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
