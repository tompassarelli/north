import { isAbsolute, relative, resolve, sep } from "node:path";

export const ROLE_ID_PATTERN_SOURCE = "^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$";
export const ROLE_ID_PATTERN = new RegExp(ROLE_ID_PATTERN_SOURCE);
export const RETIRED_ROLE_IDS = Object.freeze(["researcher"]);

export function canonicalRoleId(value, label = "role") {
  if (typeof value !== "string" || !ROLE_ID_PATTERN.test(value))
    throw new Error(`${label} must be a lowercase kebab-case ID matching ${ROLE_ID_PATTERN_SOURCE}`);
  if (RETIRED_ROLE_IDS.includes(value))
    throw new Error("role 'researcher' is retired because it was ambiguous; use scout for source gathering, analyst for deep mechanism research, or research-scientist for cutting-edge inquiry");
  return value;
}

// ID validation is the first boundary; containment is independent defense in
// depth for every generated filename. Callers pass one leaf, never a path.
export function containedLeaf(root, leaf, label = "generated output") {
  const base = resolve(root);
  const target = resolve(base, leaf);
  const rel = relative(base, target);
  if (!rel || isAbsolute(rel) || rel === ".." || rel.startsWith(`..${sep}`) || rel.includes(sep))
    throw new Error(`${label} must remain one file directly beneath ${base}`);
  return target;
}
