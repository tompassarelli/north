import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import Ajv2020 from "ajv/dist/2020.js";
import { validateProjectExposureProfile } from "./project-exposure-profile.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const ajv = new Ajv2020({ allErrors: true, strict: false });

const definitions = new Map([
  ["project-exposure-v1", {
    schema: "contracts/project-exposure-profile.schema.json",
    semantic: validateProjectExposureProfile,
  }],
]);

for (const definition of definitions.values()) {
  const schema = JSON.parse(readFileSync(resolve(ROOT, definition.schema), "utf8"));
  definition.structural = ajv.compile(schema);
}

export function validateContract(contractId, value) {
  const definition = definitions.get(contractId);
  if (!definition) throw new Error(`unknown contract ID: ${contractId}`);
  if (!definition.structural(value)) {
    throw new Error(`${contractId} is structurally invalid: ${ajv.errorsText(definition.structural.errors)}`);
  }
  return definition.semantic(value);
}
