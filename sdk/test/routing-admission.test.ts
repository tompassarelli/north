import { afterEach, expect, test } from "bun:test";
import { dispatch } from "../src/dispatch";
import { discover } from "../src/discover";
import { bindDispatchTestRuntime, bindSpawnTestRuntime } from "../src/internal/test-runtime";
import { spawn } from "../src/spawn";
import { harnessOptions } from "../src/harness";
import {
  admitResolvedRoutingRequest, type ResolvedProjectExposureProfile,
} from "../src/routing-admission";
import { presetRequest, projectProfileFixtures } from "./routing-fixtures";

const inheritedTopology = process.env.AGENT_TOPOLOGY;

afterEach(() => {
  if (inheritedTopology === undefined) delete process.env.AGENT_TOPOLOGY;
  else process.env.AGENT_TOPOLOGY = inheritedTopology;
});

const invalidProfile = projectProfileFixtures.invalid[0]!.profile;
const invalidProfileError = projectProfileFixtures.invalid[0]!.errorContains;

function expectDefaultResearchProfile(projectProfile: ResolvedProjectExposureProfile): void {
  expect(projectProfile.facts).toEqual({
    consumer: "unknown",
    state: "none",
    effect: "reversible",
    correctness: "exact-bounded-claim",
    boundaries: [],
    stage: "exploratory",
    explicitLifecycleEscalation: false,
  });
  expect(projectProfile.engineeringContext).toBe("volatile-owner-controlled-research");
  expect(projectProfile.lifecycleBudget).toEqual([]);
}

test("spawn defaults an omitted project profile, rejects invalid escalation, and admits explicit profiles", async () => {
  delete process.env.AGENT_TOPOLOGY;
  {
    const admitted = new Error("spawn-default-profile-admitted");
    const request = {
      prompt: "stop immediately after routing admission",
      routingMetadata: presetRequest("integrator"),
    };
    bindSpawnTestRuntime(request, {
      admitDispatchAuthority: () => { throw admitted; },
    });
    await expect(spawn(request)).rejects.toBe(admitted);
  }
  {
    const request = {
      prompt: "reject before dispatch authority",
      routingMetadata: presetRequest("integrator"),
      projectProfile: invalidProfile,
    };
    let boundaryCalls = 0;
    bindSpawnTestRuntime(request, {
      admitDispatchAuthority: () => { boundaryCalls++; },
    });
    await expect(spawn(request)).rejects.toThrow(invalidProfileError);
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

test("dispatch defaults an omitted project profile, rejects invalid escalation, and admits explicit profiles", async () => {
  delete process.env.AGENT_TOPOLOGY;
  {
    const admitted = new Error("dispatch-default-profile-admitted");
    const request = { routingMetadata: presetRequest("integrator") };
    bindDispatchTestRuntime(request, {
      admitDispatchAuthority: () => { throw admitted; },
    });
    await expect(dispatch("profile-admission-probe", request)).rejects.toBe(admitted);
  }
  {
    const request = {
      routingMetadata: presetRequest("integrator"),
      projectProfile: invalidProfile,
    };
    let boundaryCalls = 0;
    bindDispatchTestRuntime(request, {
      admitDispatchAuthority: () => { boundaryCalls++; },
    });
    await expect(dispatch("profile-admission-probe", request))
      .rejects.toThrow(invalidProfileError);
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

test("discover dispatches the resolved default, rejects invalid escalation, and admits explicit profiles", async () => {
  let dispatchedProfile: ResolvedProjectExposureProfile | undefined;
  const dependencies = {
    readyThreads: () => [{ id: "profile-probe", title: "profile probe", condition: "ready" }],
    dispatch: async (_thread: string, _request: unknown, projectProfile: unknown) => {
      dispatchedProfile = projectProfile as ResolvedProjectExposureProfile;
    },
    sleep: async () => {},
    random: () => 0.5,
  };
  expect(await discover("profile-admission-probe", {
    routingRequest: presetRequest("integrator"),
    maxTasks: 1,
  }, dependencies)).toEqual(["profile-probe"]);
  expectDefaultResearchProfile(dispatchedProfile!);

  await expect(discover("profile-admission-probe", {
    routingRequest: presetRequest("integrator"),
    projectProfile: invalidProfile,
    maxTasks: 0,
  }, dependencies)).rejects.toThrow(invalidProfileError);

  for (const { profile } of projectProfileFixtures.valid) {
    expect(await discover("profile-admission-probe", {
      routingRequest: presetRequest("integrator"),
      projectProfile: structuredClone(profile),
      maxTasks: 0,
    }, dependencies)).toEqual([]);
  }
});

test("resolved exposure context stays beside the eight-field request and enters the worker brief", () => {
  const admission = admitResolvedRoutingRequest(
    presetRequest("integrator"),
    "routing admission fixture",
    { projectProfile: undefined },
  );
  expect(Object.keys(admission.routingRequest).sort()).toEqual([
    "composition", "domainRequirements", "posture", "reasoning",
    "role", "taskGrade", "tier", "topology",
  ]);
  expectDefaultResearchProfile(admission.projectProfile);

  const options = harnessOptions({
    self: "resolved-profile-prompt-probe",
    routingMetadata: admission.routingRequest,
    projectProfile: admission.projectProfile,
    presenceRegistrar: false,
  });
  expect(options.systemPrompt).toContain("## Resolved project exposure context");
  expect(options.systemPrompt).toContain('"consumer":"unknown"');
  expect(options.systemPrompt).toContain('"engineeringContext":"volatile-owner-controlled-research"');
  expect(options.systemPrompt).toContain('"lifecycleBudget":[]');
  expect((options.env as Record<string, string>).AGENT_PROJECT_PROFILE)
    .toBe(JSON.stringify(admission.projectProfile));
});
