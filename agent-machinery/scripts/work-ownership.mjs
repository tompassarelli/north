// Foreign-module boundary only. Ownership semantics live in the typed source
// and its generated JavaScript projection.
import * as generated from "./work-ownership.js";

export const WORK_OWNERSHIP_SCHEMA_ID = generated.WORK_OWNERSHIP_SCHEMA_ID;
export const WORK_OWNERSHIP_VERSION = generated.WORK_OWNERSHIP_VERSION;

export function validateWorkOwnershipTransition(value) {
  if (value == null || typeof value !== "object" || Array.isArray(value))
    throw new Error("work ownership transition must be an object");
  const result = generated["validate-work-ownership-transition-result"](value);
  if (!result[0]) throw new Error(result[2]);
  return result[1];
}
