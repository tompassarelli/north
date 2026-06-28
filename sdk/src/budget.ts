// Token budget — the declarative spend cap (replaces the concurrency cap). Set it
// once: `lodestar tell swarm-budget budget_total 500000`. Every agent run charges
// its token usage to `swarm-budget budget_spent`; executors stop dispatching once
// spent >= total. This bounds the REAL resource — spend — regardless of how the
// swarm fans out. (A concurrency cap bounds count, not cost, and fights the
// parallelism we actually want.) No budget_total set => unbounded (opt-in).
import { execSync } from "node:child_process";

const SUBJECT = process.env.LODESTAR_BUDGET ?? "swarm-budget";

function readNum(pred: string): number | null {
  try {
    const claims = JSON.parse(
      execSync(`lodestar json show ${SUBJECT}`, { encoding: "utf8", timeout: 5000 }).trim()
    ) as { predicate: string; value: string }[];
    const c = claims.find((x) => x.predicate === pred);
    return c && c.value !== "" ? Number(c.value) : null;
  } catch {
    return null;
  }
}

// Tokens left, or Infinity if no budget is set (the unbounded, opt-in default).
export function remaining(): number {
  const total = readNum("budget_total");
  if (total == null || Number.isNaN(total)) return Infinity;
  return total - (readNum("budget_spent") ?? 0);
}

// Total tokens for one agent run (input + output + cache) from the SDK result msg.
export function tokensOf(resultMsg: any): number {
  const u = resultMsg?.usage ?? {};
  return (
    (u.input_tokens ?? 0) +
    (u.output_tokens ?? 0) +
    (u.cache_creation_input_tokens ?? 0) +
    (u.cache_read_input_tokens ?? 0)
  );
}

// Add `tokens` to budget_spent. Read-modify-write — approximate under concurrent
// finishes; fram-1's atomic coord add-verb makes it exact. The budget is a guard,
// not accounting, so minor drift is fine. No-op when no budget is set.
export function charge(tokens: number): void {
  if (tokens <= 0) return;
  if (readNum("budget_total") == null) return; // no budget => don't track
  const spent = readNum("budget_spent") ?? 0;
  try {
    execSync(`lodestar tell ${SUBJECT} budget_spent ${spent + tokens}`, { encoding: "utf8", timeout: 5000 });
  } catch {}
}
