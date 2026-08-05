// The authoritative terminal marker is a CROSS-LANGUAGE contract: the SDK
// declares the writer's process timeout and its lease TTL as env, and
// cli/agent-fact-internal.clj require-write-lease-policy! (see the `>` test
// there) refuses to touch the wire unless the lease strictly outlives the
// timeout. A hardcoded 60s lease against the ~72s timeout that the default
// publication budget produces for publicationTimeout(1) made EVERY lane's
// terminal publication indeterminate (thread 019f9c3b) while auxiliary writes,
// whose smaller stage slices happened to land under 60s, survived.
//
// So this pins the invariant across the FULL range of timeouts the budget can
// produce — every stage count, every elapsed position, the 300s
// NORTH_TERMINAL_PUBLICATION_BUDGET_MS ceiling and every env override shape —
// rather than the one arithmetic accident that broke.
import { expect, test } from "bun:test";
import {
  type ManagedWriterRuntime,
  internalWriteLeaseTtlMs,
  WRITE_LEASE_SAFETY_MARGIN_MS,
  writeAgentTerminal,
} from "../src/identity";
import {
  TerminalPublicationBudget,
  terminalPublicationBudgetMs,
} from "../src/terminal-notification";

/**
 * Mirror of cli/agent-fact-internal.clj require-write-lease-policy! — the
 * predicate the writer actually fails closed on, kept here in the SDK's own
 * terms so a break shows up before a lane pays for it.
 */
function cljWriteLeasePolicyHolds(writerTimeoutMs: number, leaseTtlMs: number): boolean {
  return Number.isSafeInteger(writerTimeoutMs)
    && writerTimeoutMs > 0
    && Number.isSafeInteger(leaseTtlMs)
    && leaseTtlMs > writerTimeoutMs;
}

// Every budget the knob can yield: unset, the clamped floor/ceiling, junk
// (which falls back to the default), and the values around the ceiling.
const BUDGET_RAWS = [
  undefined, "", "0", "-1", "not-a-number", "1.5",
  "1", "100", "10000", "90000", "120000", "299999", "300000", "600000",
];
const STAGE_COUNTS = [0, 1, 2, 3, 4, 5, 12, 202];
// Fractions of the budget already burned when the stage timeout is taken,
// including an overrun (a slow lane can finish past its own budget).
const ELAPSED_FRACTIONS = [0, 0.1, 0.5, 0.8, 0.99, 1, 1.5];

test("write lease outlives the writer timeout for every publication-budget timeout", () => {
  const checked: number[] = [];
  for (const raw of BUDGET_RAWS) {
    const totalMs = terminalPublicationBudgetMs(raw);
    expect(totalMs).toBeGreaterThanOrEqual(100);
    expect(totalMs).toBeLessThanOrEqual(300_000);
    for (const fraction of ELAPSED_FRACTIONS) {
      let now = 0;
      const budget = new TerminalPublicationBudget(totalMs, () => now);
      now = Math.floor(totalMs * fraction);
      for (const stages of STAGE_COUNTS) {
        const timeoutMs = budget.publicationTimeout(stages);
        const leaseTtlMs = internalWriteLeaseTtlMs(timeoutMs);
        expect(cljWriteLeasePolicyHolds(timeoutMs, leaseTtlMs)).toBe(true);
        checked.push(timeoutMs);
      }
    }
  }
  // The exact shape that broke: default budget, authoritative marker alone.
  const authoritative = new TerminalPublicationBudget(
    terminalPublicationBudgetMs(undefined), () => 0,
  ).publicationTimeout(1);
  expect(authoritative).toBe(72_000);
  expect(internalWriteLeaseTtlMs(authoritative)).toBe(72_000 + WRITE_LEASE_SAFETY_MARGIN_MS);
  expect(cljWriteLeasePolicyHolds(authoritative, 60_000)).toBe(false); // the bug
  expect(checked.length).toBe(
    BUDGET_RAWS.length * ELAPSED_FRACTIONS.length * STAGE_COUNTS.length,
  );
});

test("lease margin is additive, so a stuck writer's recovery latency never scales with the budget", () => {
  // A hard-dead writer keeps its per-subject lease until expiry, and
  // acquire-write-lease! waits at most min(5s, timeout/2) before failing a
  // successor. The excess of the lease over the timeout the caller already
  // chose to wait is therefore the whole cost, and it must stay CONSTANT rather
  // than grow with the publication budget.
  for (const timeoutMs of [1, 1_000, 10_000, 72_000, 240_000, 300_000]) {
    expect(internalWriteLeaseTtlMs(timeoutMs) - timeoutMs)
      .toBe(WRITE_LEASE_SAFETY_MARGIN_MS);
  }
  // Nonsense timeouts still produce a legal, positive-integer lease.
  for (const timeoutMs of [0, -5, 0.5, 1.9]) {
    const leaseTtlMs = internalWriteLeaseTtlMs(timeoutMs);
    expect(cljWriteLeasePolicyHolds(Math.max(1, Math.floor(timeoutMs)), leaseTtlMs)).toBe(true);
  }
});

test("the terminal writer hands the clj guard a lease that outlives the timeout it declares", () => {
  const budgets = [
    terminalPublicationBudgetMs(undefined),
    terminalPublicationBudgetMs("300000"),
    terminalPublicationBudgetMs("100"),
  ];
  for (const [index, totalMs] of budgets.entries()) {
    // spawn.ts writes the authoritative marker with publicationTimeout(1).
    const timeoutMs = new TerminalPublicationBudget(totalMs, () => 0).publicationTimeout(1);
    const seen: Array<NodeJS.ProcessEnv> = [];
    const runtime: ManagedWriterRuntime = {
      now: () => 0,
      execute: (args, _timeoutMs, env) => {
        seen.push(env);
        return JSON.stringify({
          ok: true,
          result: { status: "committed", operation_id: args[6] },
        });
      },
    };
    const agentId = `lease-invariant-${index}-${process.pid}`;
    const status = writeAgentTerminal(
      agentId,
      {
        processOutcome: "ran",
        deliveryOutcome: "reported",
        deliveryReason: "delivery_reported",
      } as const,
      timeoutMs,
      runtime,
      undefined,
      { resource: `session:${agentId}`, holder: agentId, epoch: 19 },
    );
    expect(status).toBe("recorded");
    expect(seen).toHaveLength(1);
    const declaredTimeout = Number(seen[0]?.NORTH_IDENTITY_WRITER_TIMEOUT_MS);
    const declaredLease = Number(seen[0]?.NORTH_IDENTITY_WRITE_LEASE_TTL_MS);
    expect(declaredTimeout).toBe(timeoutMs);
    expect(cljWriteLeasePolicyHolds(declaredTimeout, declaredLease)).toBe(true);
  }
});
