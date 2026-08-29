import { afterEach, expect, test } from "bun:test";
import { dispatch } from "../src/dispatch";
import { discover } from "../src/discover";
import { bindDispatchTestRuntime, bindSpawnTestRuntime } from "../src/internal/test-runtime";
import { spawn } from "../src/spawn";
import { presetRequest, projectProfileFixtures } from "./routing-fixtures";

const inheritedTopology = process.env.AGENT_TOPOLOGY;

afterEach(() => {
  if (inheritedTopology === undefined) delete process.env.AGENT_TOPOLOGY;
  else process.env.AGENT_TOPOLOGY = inheritedTopology;
});

function profileError(value: unknown): string {
  return value === undefined
    ? "project exposure profile must be an object"
    : projectProfileFixtures.invalid[0]!.errorContains;
}

test("spawn rejects missing and invalid project profiles, then admits all resolved profiles", async () => {
  delete process.env.AGENT_TOPOLOGY;
  for (const projectProfile of [undefined, projectProfileFixtures.invalid[0]!.profile]) {
    const request = {
      prompt: "reject before dispatch authority",
      routingMetadata: presetRequest("integrator"),
      projectProfile,
    };
    let boundaryCalls = 0;
    bindSpawnTestRuntime(request, {
      admitDispatchAuthority: () => { boundaryCalls++; },
    });
    await expect(spawn(request)).rejects.toThrow(profileError(projectProfile));
    expect(boundaryCalls).toBe(0);
  }

  for (const { profile } of projectProfileFixtures.valid) {
    const admitted = new Error("spawn-profile-admitted");
    const request = {
      prompt: "stop immediately after routing admission",
      routingMetadata: presetRequest("integrator"),
      projectProfile: structuredClone(profile),
    };
    bindSpawnTestRuntime(request, {
      admitDispatchAuthority: () => { throw admitted; },
    });
    await expect(spawn(request)).rejects.toBe(admitted);
  }
});

test("dispatch rejects missing and invalid project profiles, then admits all resolved profiles", async () => {
  delete process.env.AGENT_TOPOLOGY;
  for (const projectProfile of [undefined, projectProfileFixtures.invalid[0]!.profile]) {
    const request = {
      routingMetadata: presetRequest("integrator"),
      projectProfile,
    };
    let boundaryCalls = 0;
    bindDispatchTestRuntime(request, {
      admitDispatchAuthority: () => { boundaryCalls++; },
    });
    await expect(dispatch("profile-admission-probe", request))
      .rejects.toThrow(profileError(projectProfile));
    expect(boundaryCalls).toBe(0);
  }

  for (const { profile } of projectProfileFixtures.valid) {
    const admitted = new Error("dispatch-profile-admitted");
    const request = {
      routingMetadata: presetRequest("integrator"),
      projectProfile: structuredClone(profile),
    };
    bindDispatchTestRuntime(request, {
      admitDispatchAuthority: () => { throw admitted; },
    });
    await expect(dispatch("profile-admission-probe", request)).rejects.toBe(admitted);
  }
});

test("discover rejects missing and invalid project profiles, then admits all resolved profiles", async () => {
  const dependencies = {
    readyThreads: () => { throw new Error("discovery polled before admission"); },
    dispatch: async () => { throw new Error("discovery dispatched before admission"); },
    sleep: async () => {},
    random: () => 0.5,
  };
  for (const projectProfile of [undefined, projectProfileFixtures.invalid[0]!.profile]) {
    await expect(discover("profile-admission-probe", {
      routingRequest: presetRequest("integrator"),
      projectProfile,
      maxTasks: 0,
    }, dependencies)).rejects.toThrow(profileError(projectProfile));
  }

  for (const { profile } of projectProfileFixtures.valid) {
    expect(await discover("profile-admission-probe", {
      routingRequest: presetRequest("integrator"),
      projectProfile: structuredClone(profile),
      maxTasks: 0,
    }, dependencies)).toEqual([]);
  }
});
