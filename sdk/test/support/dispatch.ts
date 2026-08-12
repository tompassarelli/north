import {
  createDispatchAgentId,
  dispatch as productionDispatch,
  dispatchParallel as productionDispatchParallel,
  selectDispatchAgentId,
  type DispatchDependencies,
} from "../../src/dispatch";
import { bindDispatchTestRuntime } from "../../src/internal/test-runtime";

const RUNTIME_FIELDS = new Set([
  "claimDriver", "driverOptions", "queryFn", "loadThreadFacts", "loadChildren",
  "deliveryRuntime", "threadFactsLoadOptions", "childSettlementReader", "feedSubscriber",
  "registerTermination", "refreshAccountUsages", "refreshCodexEntitlements",
  "admitResourceEnvelope", "completeResourceEnvelope",
  "releaseDriver", "admitDispatchAuthority",
  "publishLearningAssignment",
  "loadShadowReviewerConfig", "shadowReviewRunner", "publishShadowReviewerNote",
]);

// Dispatch-side twin of the pin in ./spawn.ts — same subprocess, same stubbed PATH.
const pinnedDispatchAuthority = () => {};

function split(
  value: DispatchDependencies & Record<string, unknown>,
): DispatchDependencies {
  const request: Record<string, unknown> = {};
  const runtime: Record<string, unknown> = {
    admitDispatchAuthority: pinnedDispatchAuthority,
  };
  for (const [field, fieldValue] of Object.entries(value))
    (RUNTIME_FIELDS.has(field) ? runtime : request)[field] = fieldValue;
  bindDispatchTestRuntime(request, runtime);
  return request as unknown as DispatchDependencies;
}

export function dispatch(
  threadId: string,
  value: DispatchDependencies & Record<string, unknown>,
) {
  return productionDispatch(threadId, split(value));
}

export function dispatchParallel(
  threadIds: string[],
  value?: DispatchDependencies & Record<string, unknown>,
) {
  if (value === undefined)
    return productionDispatchParallel(threadIds, value as any);
  // A bound runtime is consumed on take, so one dependencies object cannot serve
  // N children — rebind per thread.
  if (value.agentId && threadIds.length > 1)
    throw new Error("dispatchParallel cannot reuse one explicit agentId across multiple children");
  return Promise.all(threadIds.map((threadId) =>
    productionDispatch(threadId, split(value))));
}

export { createDispatchAgentId, selectDispatchAgentId };
