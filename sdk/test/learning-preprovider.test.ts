import { afterEach, expect, test } from "bun:test";
import { spawn } from "./support/spawn";
import { presetRequest } from "./routing-fixtures";

const priorPolicy = process.env.NORTH_LEARNING_POLICY;
afterEach(() => {
  if (priorPolicy === undefined) delete process.env.NORTH_LEARNING_POLICY;
  else process.env.NORTH_LEARNING_POLICY = priorPolicy;
});

test("a failed assignment recorder aborts before provider selection or query", async () => {
  process.env.NORTH_LEARNING_POLICY = "/tmp/north-learning-policy-intentionally-absent.json";
  const events: string[] = [];
  let providerCalls = 0;
  const queryFn = () => {
    providerCalls++;
    events.push("provider");
    return {
      async *[Symbol.asyncIterator]() {
        yield { type: "result", subtype: "success", result: "must-not-run", num_turns: 1 };
      },
    } as any;
  };

  await expect(spawn({
    prompt: "read-only recorder ordering probe",
    agentId: "learning-recorder-order",
    worktree: false,
    routingMetadata: presetRequest("scout"),
    queryFn,
    admitBillableClock: () => {},
    admitResourceEnvelope: async () => undefined,
    completeResourceEnvelope: async () => {},
    publishLearningAssignment: async () => {
      events.push("assignment");
      throw new Error("fixture recorder unavailable");
    },
  })).rejects.toThrow("fixture recorder unavailable");

  expect(events).toEqual(["assignment"]);
  expect(providerCalls).toBe(0);
});
