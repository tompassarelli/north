import { expect, test } from "bun:test";
import {
  attestDelivery,
  attestationActorFromEnv,
  normalizedAgentId,
} from "../src/delivery-attest";

test("delivery attest accepts one safe target spelling", () => {
  expect(normalizedAgentId("lane-123")).toBe("lane-123");
  expect(normalizedAgentId("@agent:lane-123")).toBe("lane-123");
  expect(normalizedAgentId("a".repeat(249))).toBe("a".repeat(249));
  expect(() => normalizedAgentId("a".repeat(250))).toThrow("valid target agent id");
  expect(() => normalizedAgentId("../peer")).toThrow("valid target agent id");
});

test("same-UID verifier promotion fails closed", async () => {
  expect(attestDelivery("lane-123")).rejects.toThrow(
    "independent delivery attestation is unavailable",
  );
});

test("attestation authority comes only from the managed caller environment", () => {
  expect(attestationActorFromEnv({ AGENT_ID: "verifier-1" })).toBe("agent:verifier-1");
  expect(attestationActorFromEnv({ AGENT_ID: "@agent:judge-1" })).toBe("agent:judge-1");
  expect(() => attestationActorFromEnv({})).toThrow("managed verifier or judge lane");
  expect(() => attestationActorFromEnv({ AGENT_ID: "../forged" }))
    .toThrow("managed verifier or judge lane");
});
