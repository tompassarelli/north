export const ORCHESTRATION_ROLE_ID_PATTERN = /^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$/;
export const RETIRED_ORCHESTRATION_ROLE_IDS = new Set(["researcher"]);

export function requireOrchestrationRoleId(value: unknown, label = "role"): string {
  if (value === "researcher") {
    throw new Error(
      "role researcher is retired because it was ambiguous; use scout, analyst, or research-scientist",
    );
  }
  if (typeof value !== "string" || !ORCHESTRATION_ROLE_ID_PATTERN.test(value)
      || RETIRED_ORCHESTRATION_ROLE_IDS.has(value)) {
    throw new Error(label + " must be a lowercase kebab-case Orchestration role id");
  }
  return value;
}
