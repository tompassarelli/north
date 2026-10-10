import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export {
  LIFECYCLE_EVIDENCE,
  PROFILE_FACT_VALUES,
  PROJECT_EXPOSURE_PROFILE_SCHEMA_ID,
  PROJECT_EXPOSURE_PROFILE_VERSION,
  ENGINEERING_CONTEXTS,
  defaultProjectExposureProfile,
  deriveEngineeringContext,
  resolveProjectExposureProfile,
  validateProjectExposureProfile,
} from "./scripts/project-exposure-profile.mjs";
export { validateContract } from "./scripts/contracts.mjs";

export const packageRoot = dirname(fileURLToPath(import.meta.url));
export const catalogPath = resolve(packageRoot, "catalog.json");
export const CATALOG_SCHEMA_ID = "urn:agent-machinery:schema:catalog:v1";

export function loadExportCatalog(path = catalogPath) {
  return JSON.parse(readFileSync(path, "utf8"));
}

export function assetPath(relativePath) {
  if (typeof relativePath !== "string" || !relativePath ||
      relativePath.startsWith("/") || relativePath.split("/").includes(".."))
    throw new Error("asset path must be a contained package-relative path");
  return resolve(packageRoot, relativePath);
}

export function schemaPath(schemaId, catalog = loadExportCatalog()) {
  const candidates = new Set([
    ...catalog.assets.map(({ path }) => path),
    ...catalog.contracts.map(({ schema }) => schema),
  ]);
  for (const relativePath of candidates) {
    if (!relativePath.endsWith(".schema.json")) continue;
    const path = assetPath(relativePath);
    if (JSON.parse(readFileSync(path, "utf8")).$id === schemaId) return path;
  }
  throw new Error(`unknown schema ID: ${schemaId}`);
}
