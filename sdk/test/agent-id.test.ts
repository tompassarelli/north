import { expect, test } from "bun:test";
import { createDispatchAgentId } from "../src/dispatch";
import { createSpawnAgentId } from "../src/spawn";

test("spawn and direct-dispatch IDs remain unique when many agents start in one millisecond", () => {
  const now = 1_752_750_000_000;
  const spawnIds = Array.from({ length: 1_000 }, () => createSpawnAgentId(now));
  const dispatchIds = Array.from({ length: 1_000 }, () => createDispatchAgentId("019f-thread", now));
  expect(new Set(spawnIds).size).toBe(spawnIds.length);
  expect(new Set(dispatchIds).size).toBe(dispatchIds.length);
  expect(new Set([...spawnIds, ...dispatchIds]).size).toBe(spawnIds.length + dispatchIds.length);
});

test("generated IDs retain readable provenance plus the full UUID entropy", () => {
  const uuid = "123e4567-e89b-12d3-a456-426614174000";
  expect(createSpawnAgentId(35, uuid)).toBe(`lane-z-${uuid}`);
  expect(createDispatchAgentId("thread/ABC-123", 35, uuid)).toBe(`sdk-threadABC123-z-${uuid}`);
});
