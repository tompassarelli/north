import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { applyOrchestrationStaffing } from "../src/orchestration-staffing";
import type { RoutingRequest } from "../src/routing-metadata";

interface ProjectProfileFixtures {
  valid: Array<{ name: string; profile: Record<string, unknown> }>;
  invalid: Array<{ name: string; errorContains: string; profile: Record<string, unknown> }>;
}

const agentMachinery = resolve(import.meta.dir, "..", "..", "agent-machinery");

export const projectProfileFixtures = JSON.parse(readFileSync(resolve(
  agentMachinery,
  "contracts/project-exposure-profile.fixtures.json",
), "utf8")) as ProjectProfileFixtures;

export function researchProjectProfile(): Record<string, unknown> {
  return structuredClone(projectProfileFixtures.valid[0]!.profile);
}

/** Canonical complete preset request for managed-boundary tests. */
export function presetRequest(role: string): RoutingRequest {
  return applyOrchestrationStaffing({ role });
}
