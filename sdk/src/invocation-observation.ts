import { parseStrictJson } from "./strict-json";

export const INVOCATION_OBSERVATION_SCHEMA = "InvocationObservation/v1" as const;
export const INVOCATION_OBSERVATION_HOOK = "firn-system-policy" as const;
export const INVOCATION_OBSERVATION_MAX_BYTES = 8 * 1024;

const OPERATIONS = [
  "Bash",
  "Edit",
  "Write",
  "MultiEdit",
  "apply_patch",
  "functions.create_goal",
  "functions.get_goal",
  "functions.update_goal",
  "collaboration.followup_task",
  "collaboration.interrupt_agent",
  "collaboration.list_agents",
  "collaboration.send_message",
  "collaboration.spawn_agent",
  "collaboration.wait_agent",
] as const;

const CLASSIFICATIONS = [
  "shell-command",
  "file-path",
  "patch",
  "goal-create",
  "empty-object",
  "goal-update",
  "target-message",
  "target",
  "list",
  "agent-spawn",
  "wait",
  "invalid-arguments",
] as const;

export type InvocationObservationOperation = typeof OPERATIONS[number];
export type InvocationObservationClassification = typeof CLASSIFICATIONS[number];
export type InvocationObservationDecision = "pass" | "deny";

export interface InvocationObservation {
  schema: typeof INVOCATION_OBSERVATION_SCHEMA;
  hook: typeof INVOCATION_OBSERVATION_HOOK;
  operation: InvocationObservationOperation;
  classification: InvocationObservationClassification;
  decision: InvocationObservationDecision;
}

export interface InvocationObservationReceipt {
  /** Exact canonical bytes supplied to the model as PreToolUse additionalContext. */
  raw: string;
  observation: InvocationObservation;
}

const operations = new Set<string>(OPERATIONS);
const classifications = new Set<string>(CLASSIFICATIONS);
const expectedClassification: Readonly<Record<
  InvocationObservationOperation,
  InvocationObservationClassification
>> = {
  Bash: "shell-command",
  Edit: "file-path",
  Write: "file-path",
  MultiEdit: "file-path",
  apply_patch: "patch",
  "functions.create_goal": "goal-create",
  "functions.get_goal": "empty-object",
  "functions.update_goal": "goal-update",
  "collaboration.followup_task": "target-message",
  "collaboration.interrupt_agent": "target",
  "collaboration.list_agents": "list",
  "collaboration.send_message": "target-message",
  "collaboration.spawn_agent": "agent-spawn",
  "collaboration.wait_agent": "wait",
};

export function serializeInvocationObservation(observation: InvocationObservation): string {
  return JSON.stringify({
    schema: observation.schema,
    hook: observation.hook,
    operation: observation.operation,
    classification: observation.classification,
    decision: observation.decision,
  });
}

/**
 * Accept only Firn's compact receipt vocabulary. This is observation, not a
 * ControlExpectation: it carries no intended call and cannot classify a valid
 * emitted call as a mismatch.
 */
export function parseInvocationObservationReceipt(
  raw: string,
): InvocationObservationReceipt | undefined {
  let value: unknown;
  try {
    value = parseStrictJson(raw, "Firn invocation observation", {
      maxBytes: INVOCATION_OBSERVATION_MAX_BYTES,
      maxDepth: 2,
      maxNodes: 8,
    });
  } catch {
    return undefined;
  }
  if (!value || typeof value !== "object" || Array.isArray(value)) return undefined;
  const record = value as Record<string, unknown>;
  if (Object.keys(record).join(",") !== "schema,hook,operation,classification,decision"
      || record.schema !== INVOCATION_OBSERVATION_SCHEMA
      || record.hook !== INVOCATION_OBSERVATION_HOOK
      || typeof record.operation !== "string" || !operations.has(record.operation)
      || typeof record.classification !== "string" || !classifications.has(record.classification)
      || (record.decision !== "pass" && record.decision !== "deny")) return undefined;
  const observation = record as unknown as InvocationObservation;
  const invalid = observation.classification === "invalid-arguments";
  if ((invalid && observation.decision !== "deny")
      || (!invalid && observation.decision !== "pass")
      || (!invalid
        && expectedClassification[observation.operation] !== observation.classification)) {
    return undefined;
  }
  if (serializeInvocationObservation(observation) !== raw) return undefined;
  return { raw, observation };
}

export function invocationObservationKey(observation: InvocationObservation): string {
  return `${observation.operation}\u0000${observation.classification}\u0000${observation.decision}`;
}
