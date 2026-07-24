import { applyOrchestrationStaffing } from "../src/orchestration-staffing";
import type { RoutingRequest } from "../src/routing-metadata";

/** Canonical complete preset request for managed-boundary tests. */
export function presetRequest(role: string): RoutingRequest {
  return applyOrchestrationStaffing({ role });
}
