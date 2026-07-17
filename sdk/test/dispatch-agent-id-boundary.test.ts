import { afterEach, expect, test } from "bun:test";
import { selectDispatchAgentId } from "../src/dispatch";

const priorId = process.env.AGENT_ID;
const priorPreclaim = process.env.NORTH_DISPATCH_DRIVER_PRECLAIMED;
afterEach(() => {
  if (priorId === undefined) delete process.env.AGENT_ID;
  else process.env.AGENT_ID = priorId;
  if (priorPreclaim === undefined) delete process.env.NORTH_DISPATCH_DRIVER_PRECLAIMED;
  else process.env.NORTH_DISPATCH_DRIVER_PRECLAIMED = priorPreclaim;
});

test("programmatic and parallel dispatch identities never reuse ambient parent identity", () => {
  process.env.AGENT_ID = "director-parent-poison";
  delete process.env.NORTH_DISPATCH_DRIVER_PRECLAIMED;
  const ids = Array.from({ length: 100 }, (_, index) => selectDispatchAgentId(`thread-${index}`));
  expect(new Set(ids).size).toBe(ids.length);
  expect(ids).not.toContain("director-parent-poison");
});

test("only an explicit/preclaimed handoff may retain the adapter-minted identity", () => {
  process.env.AGENT_ID = "sdk-preclaimed-child";
  process.env.NORTH_DISPATCH_DRIVER_PRECLAIMED = "1";
  expect(selectDispatchAgentId("thread-a")).toBe("sdk-preclaimed-child");
  expect(selectDispatchAgentId("thread-b", { driverOptions: { preclaimed: false } }))
    .not.toBe("sdk-preclaimed-child");
  expect(selectDispatchAgentId("thread-c", { agentId: "explicit-child" })).toBe("explicit-child");
});
