import { expect, test } from "bun:test";
import {
  parseInvocationObservationReceipt, serializeInvocationObservation,
  type InvocationObservation,
} from "../src/invocation-observation";

const observation: InvocationObservation = {
  schema: "InvocationObservation/v1",
  hook: "firn-system-policy",
  operation: "functions.get_goal",
  classification: "empty-object",
  decision: "pass",
};

test("the exact canonical Firn receipt round-trips", () => {
  const raw = serializeInvocationObservation(observation);
  expect(parseInvocationObservationReceipt(raw)).toEqual({ raw, observation });
});

test("only fixed operation/classification/decision relations are observations", () => {
  for (const value of [
    { ...observation, operation: "functions.exec" },
    { ...observation, classification: "target-message" },
    { ...observation, decision: "deny" },
    { ...observation, classification: "invalid-arguments", decision: "pass" },
  ]) expect(parseInvocationObservationReceipt(JSON.stringify(value))).toBeUndefined();

  const denied = {
    ...observation,
    classification: "invalid-arguments" as const,
    decision: "deny" as const,
  };
  expect(parseInvocationObservationReceipt(JSON.stringify(denied))?.observation).toEqual(denied);
});

test("noncanonical, duplicate, oversized, and raw-payload contexts are ignored", () => {
  const canonical = serializeInvocationObservation(observation);
  expect(parseInvocationObservationReceipt(` ${canonical}`)).toBeUndefined();
  expect(parseInvocationObservationReceipt(canonical.replace(
    '"schema":"InvocationObservation/v1"',
    '"schema":"InvocationObservation/v1","schema":"InvocationObservation/v1"',
  ))).toBeUndefined();
  expect(parseInvocationObservationReceipt(JSON.stringify({
    ...observation,
    tool_input: { target: "secret" },
  }))).toBeUndefined();
  expect(parseInvocationObservationReceipt(`{"schema":"${"x".repeat(9_000)}"}`)).toBeUndefined();
});
