import { expect, test } from "bun:test";
import { discover, type DiscoverDependencies } from "../src/discover";
import { ProviderSelectionError } from "../src/provider-routing";

function dependencies(dispatch: DiscoverDependencies["dispatch"]) {
  const observations = { acquired: 0, released: 0, sleeps: [] as number[] };
  const value: DiscoverDependencies = {
    readyThreads: () => [{ id: "thread-1", title: "ready", condition: "ready" }],
    acquireDriver: () => { observations.acquired++; return true; },
    releaseDriver: () => { observations.released++; },
    dispatch,
    sleep: async (ms) => {
      expect(observations.released).toBe(observations.sleeps.length + 1);
      observations.sleeps.push(ms);
    },
    random: () => 0.5,
  };
  return { value, observations };
}

test("subscription exhaustion backs off and terminates instead of hot-looping", async () => {
  const { value, observations } = dependencies(async () => {
    throw new ProviderSelectionError("no_provider_available",
      "no agent provider available: anthropic=ready/exhausted, openai=ready/exhausted");
  });

  expect(await discover("test-discover", { maxEmptyRounds: 3 }, value)).toEqual([]);
  expect(observations).toEqual({ acquired: 3, released: 3, sleeps: [2_000, 4_000, 8_000] });
});

test("repeated generic dispatch failures also consume maxEmptyRounds", async () => {
  const { value, observations } = dependencies(async () => { throw new Error("broken thread"); });

  expect(await discover("test-discover", { maxEmptyRounds: 2 }, value)).toEqual([]);
  expect(observations).toEqual({ acquired: 2, released: 2, sleeps: [2_000, 4_000] });
});
