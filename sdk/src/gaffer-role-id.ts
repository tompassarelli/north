export const GAFFER_ROLE_ID_PATTERN = /^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$/;
export const RETIRED_GAFFER_ROLE_IDS = new Set(["researcher"]);

export function requireGafferRoleId(value: unknown, label = "role"): string {
  if (value === "researcher") {
    throw new Error(
      "role researcher is retired because it was ambiguous; use scout, analyst, or research-scientist",
    );
  }
  if (typeof value !== "string" || !GAFFER_ROLE_ID_PATTERN.test(value)
      || RETIRED_GAFFER_ROLE_IDS.has(value)) {
    throw new Error(label + " must be a lowercase kebab-case Gaffer role id");
  }
  return value;
}
