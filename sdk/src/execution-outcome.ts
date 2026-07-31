import type { DeliveryAssessment, DeliveryProof } from "./delivery-verification";

export type DeliveryOutcome = "unverified" | "reported" | "verified" | "blocked";

export interface ExecutionTerminal {
  /** Did the adapter/process reach a terminal state, and which one? */
  processOutcome: string;
  /** Was the requested delivery established independently of model prose? */
  deliveryOutcome: DeliveryOutcome;
  /** Stable machine reason; never copied from provider prose. */
  deliveryReason: string;
  /** Self-contained fact snapshot; required for reported. `verified` is legacy/reserved. */
  deliveryProof?: DeliveryProof;
}

/**
 * A provider "success" terminal whose result text is empty (0b) is a DEGENERATE
 * completion, not a delivery. Opus-high extended-thinking turns that exhaust the
 * output-token ceiling truncate before committing any final text (the final
 * assistant block is an unanswered tool_use or a terminal thinking block), yet
 * the SDK still yields subtype=success/result="". Recording that as process=ran
 * makes a zero-deliverable lane read as a clean completion (thread 019f8300).
 * This distinct outcome makes the empty terminal LOUD and non-clean.
 */
export const EMPTY_RESULT_OUTCOME = "ran_empty";

/**
 * The provider-process-level death class: the SDK subprocess itself died
 * (OOM SIGKILL / parent SIGTERM / idle Transport-closed / openai_provider_execution_failed),
 * classified below as processOutcome "provider_process_died". Bounded auto-retry
 * (spawn.ts, thread 019f8f81) gates eligibility against this exact constant rather
 * than a repeated string literal, so the retry gate and this classification can
 * never drift apart. Distinct from blocked_preflight/watchdog_aborted/resource_envelope_*,
 * which are NOT provider-process deaths and are never retried by that policy.
 */
export const PROVIDER_PROCESS_DEATH_OUTCOME = "died";

/** True when a provider success terminal carried no committed deliverable text. */
export function isEmptyResultTerminal(outcome: string, result: string): boolean {
  return outcome === "ran" && result.trim() === "";
}

/** Ceiling for the rendered provider_error detail: a lane-log line and a fact value. */
export const PROVIDER_ERROR_DETAIL_MAX_LEN = 1200;

/** What a provider_error terminal means when the stream never produced one at all. */
export const NO_PROVIDER_TERMINAL_DETAIL =
  "provider stream closed without a terminal result message";

/** No error payload accompanied the error terminal — itself the diagnosis. */
export const EMPTY_PROVIDER_ERROR_DETAIL =
  "provider error terminal carried no error payload";

function oneLine(value: unknown, maxLen: number): string {
  const raw = typeof value === "string"
    ? value
    : (() => { try { return JSON.stringify(value) ?? String(value); } catch { return String(value); } })();
  return String(raw).replace(/\s+/g, " ").trim().slice(0, maxLen);
}

/**
 * `provider_error` is a CLASSIFICATION, not a diagnosis.
 *
 * On 2026-07-26 three managed Codex lanes settled process=provider_error /
 * delivery=blocked / turns=0 after producing real first-turn text, and NOTHING
 * durable named the provider failure: the adapter had the cause chain in hand,
 * folded its summary into the terminal frame, and the harness read exactly one
 * boolean off that frame (`is_error`) before `break`ing the message loop — which
 * discards both the frame and the adapter's pending throw. The empty managed
 * home is disposed at teardown, so the graph was the only possible witness and
 * it recorded nothing.
 *
 * This renders a terminal frame's own error evidence into one bounded,
 * whitespace-collapsed line for the lane log, the peer ping, and the
 * `provider_error_detail` run fact. It reads ONLY diagnostic fields — never
 * `result` (model prose, recorded separately and never classification input) —
 * so a provider cannot smuggle prose into a machine reason through this seam.
 */
export function describeProviderErrorTerminal(
  message: unknown,
  maxLen = PROVIDER_ERROR_DETAIL_MAX_LEN,
): string {
  if (!message || typeof message !== "object") return NO_PROVIDER_TERMINAL_DETAIL;
  const msg = message as Record<string, any>;
  const parts: string[] = [];
  if (typeof msg.subtype === "string" && msg.subtype !== "success")
    parts.push(`subtype=${oneLine(msg.subtype, 120)}`);
  if (msg.is_error === true) parts.push("is_error=true");
  if (Array.isArray(msg.errors) && msg.errors.length) {
    const shown = msg.errors.slice(0, 4)
      .map((entry: unknown) => oneLine((entry as any)?.message ?? entry, 240));
    const overflow = msg.errors.length > 4 ? ` | +${msg.errors.length - 4} more` : "";
    parts.push(`errors=[${shown.join(" | ")}${overflow}]`);
  }
  // The managed Codex harvest frame is the adapter's own post-mortem: its
  // `failure` field is the full nested cause chain of the throw the message
  // loop is about to discard. This is the single most diagnostic field here.
  const harvest = msg._north_harvest;
  if (harvest && typeof harvest === "object") {
    if (harvest.failure) parts.push(`failure=${oneLine(harvest.failure, 800)}`);
    const landed = [
      `${Number(harvest.completedTurns ?? 0)} completed turn(s)`,
      harvest.mcp?.totalCalls !== undefined ? `${harvest.mcp.totalCalls} MCP call(s)` : undefined,
      harvest.nativeCommands?.totalCommands !== undefined
        ? `${harvest.nativeCommands.totalCommands} native command(s)` : undefined,
    ].filter(Boolean).join(", ");
    parts.push(`landed=[${landed}]`);
    if (harvest.threadId) parts.push(`provider_thread=${oneLine(harvest.threadId, 128)}`);
  }
  if (!parts.length) return EMPTY_PROVIDER_ERROR_DETAIL;
  return `provider error terminal: ${parts.join(" ")}`.slice(0, maxLen);
}

const BLOCKED_REASON: Record<string, string> = {
  blocked_preflight: "execution_preflight_blocked",
  blocked_spend_guard: "spend_guard_budget_incomplete",
  ran_empty: "provider_terminal_empty_result",
  provider_error: "provider_terminal_error",
  died: "provider_process_died",
  stalled: "provider_process_stalled",
  watchdog_aborted: "north_watchdog_execution_inactivity",
  session_hard_cap: "north_managed_session_hard_cap",
  max_turns: "provider_turn_cap",
  capped: "provider_cap",
  resource_envelope_exceeded: "resource_envelope_exceeded",
  provider_escalation_unsupported: "provider_escalation_unsupported",
  max_tier: "escalation_ladder_exhausted",
  orchestrator_children_incomplete: "orchestrator_children_live_at_terminal",
  orchestrator_child_obligation_unmet: "orchestrator_minimum_children_not_dispatched",
  child_reconciliation_unavailable: "orchestrator_child_reconciliation_unavailable",
  orchestrator_reduction_incomplete: "orchestrator_child_results_unreconciled",
  orchestrator_child_set_inconsistent: "orchestrator_child_relation_regressed",
};

/**
 * A successful provider terminal proves only that the process ran. Delivery is
 * intentionally unverified until an external bar/evidence seam proves it.
 */
export function classifyExecutionTerminal(
  processOutcome: string,
  delivery?: DeliveryAssessment,
): ExecutionTerminal {
  if (processOutcome === "ran") {
    if (delivery?.deliveryOutcome === "reported") {
      return {
        processOutcome,
        deliveryOutcome: delivery.deliveryOutcome,
        deliveryReason: delivery.deliveryReason,
        deliveryProof: delivery.proof,
      };
    }
    return {
      processOutcome,
      deliveryOutcome: delivery?.deliveryOutcome ?? "unverified",
      deliveryReason: delivery?.deliveryReason
        ?? "provider_terminal_success_without_external_verification",
    };
  }
  return {
    processOutcome,
    deliveryOutcome: "blocked",
    deliveryReason: BLOCKED_REASON[processOutcome] ?? "execution_did_not_reach_success_terminal",
  };
}
