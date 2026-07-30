export const ORCHESTRATION_ROLE_ID_PATTERN = /^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$/;
export const RETIRED_ORCHESTRATION_ROLE_IDS = new Set(["researcher", "research-scientist"]);

export function requireOrchestrationRoleId(value: unknown, label = "role"): string {
  if (typeof value === "string" && RETIRED_ORCHESTRATION_ROLE_IDS.has(value)) {
    throw new Error(`role ${value} is retired; use scout, analyst, or cs-researcher`);
  }
  if (typeof value !== "string" || !ORCHESTRATION_ROLE_ID_PATTERN.test(value)) {
    throw new Error(label + " must be a lowercase kebab-case Orchestration role id");
  }
  return value;
}
