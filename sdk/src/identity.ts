// Agent identity facts — ids stay meaningless + immutable; everything meaningful
// is a FACT on @agent:<id> in the coordination log (design: thread 019f40f8).
// Predicates (single-valued, declared via the schema-write gate): kind role model
// provider provider_target effort composition_kind composition_id goal spawned_at display_handle
// display_name; repo stays multi (threads span repos).
// Writes shell to the installed `north tell` (the proven serialized OCC path) and
// are NON-FATAL: a facts failure must never kill a spawn.
import { execFileSync } from "node:child_process";
import { resolve } from "node:path";

// NORTH_BIN override mirrors death.ts/clock.ts/children.ts/watchdog.ts, so the whole
// coordinator-writing surface resolves the SAME engine — and a hermetic test that points
// NORTH_BIN at a fake redirects identity writes too. A bare `north` on PATH ignored that
// seam, so identity tells escaped the fake, hit the real CLI (~3.7s/call against a dead
// port) and wrote test agents into the production graph.
const REPO = resolve(import.meta.dir, "..", "..");
const northBin = () => process.env.NORTH_BIN ?? `${REPO}/bin/north`;

export interface AgentIdentity {
  kind: "lane" | "session" | "cron";
  role?: string;
  model?: string; // tier name as spawned (opus|sonnet|haiku); SDK resolves the full id
  provider?: string;
  providerTarget?: string;
  effort?: string;
  compositionKind?: "preset" | "bespoke" | "none";
  compositionId?: string;
  repo?: string;
  goal?: string;
  // spawning coordinator handle. Persisted (not just held at ping time) so it survives
  // the spawning session: the reactor's died-unreported sweep reads it to ping on a
  // silent hard-kill (sweep-lanes! in north-reactor.clj), and `north health` folds it to
  // compute ping-loss (lanes that carried a coordinator but landed no COMPLETE/DEATH).
  coordinator?: string;
}

const component = (value?: string) => {
  const normalized = value?.trim().toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");
  return normalized || "unknown";
};
const shortModel = (m?: string) => {
  const normalized = component(m);
  for (const family of ["opus", "sonnet", "haiku", "fable", "sol", "terra", "luna"])
    if (normalized.split("-").includes(family)) return family;
  return normalized;
};
const idSuffix = (id: string) => component(id.split("-").at(-1));

export function gafferProvenance(f: AgentIdentity): string {
  if (f.compositionKind === "preset") return `gaffer:${f.compositionId ? component(f.compositionId) : "unknown"}`;
  if (f.compositionKind === "bespoke") return `gaffer:bespoke(${f.compositionId ? component(f.compositionId) : "unknown"})`;
  if (f.compositionKind === "none" || f.kind === "session") return "gaffer:none";
  return "gaffer:unknown";
}

/** Preserve an unknown native session as provider-only; managed routes always supply providerTarget. */
export function providerTargetLabel(f: AgentIdentity): string {
  const provider = f.provider?.trim() || "unknown";
  const target = f.providerTarget?.trim();
  if (!target) return provider;
  return `${provider}:${target === provider || target === "ambient" ? "ambient" : target}`;
}

export function semanticHandle(id: string, f: AgentIdentity): string {
  const composition = f.compositionKind === "none" || f.kind === "session"
    ? "native"
    : component(f.compositionId);
  return [component(providerTargetLabel(f)), shortModel(f.model), component(f.effort), composition, idSuffix(id)].join("-");
}

export function renderDisplayName(id: string, f: AgentIdentity): string {
  const goal = f.goal ? ` — ${f.goal.length > 40 ? f.goal.slice(0, 37) + "…" : f.goal}` : "";
  if (f.providerTarget) {
    const task = f.goal ? (f.goal.length > 40 ? f.goal.slice(0, 37) + "…" : f.goal) : "unknown";
    return `${providerTargetLabel(f)} · ${shortModel(f.model)} · ${component(f.effort)} · ${gafferProvenance(f)} · ${task}`;
  }
  return `${semanticHandle(id, f)}${goal}`;
}

export function agentRouteFacts(agentId: string, f: AgentIdentity): Array<[string, string | undefined]> {
  return [
    ["provider", f.provider],
    ["provider_target", f.providerTarget],
    ["model", f.model],
    ["effort", f.effort],
    ["display_handle", semanticHandle(agentId, f)],
    ["display_name", renderDisplayName(agentId, f)],
  ];
}

function tell(subject: string, pred: string, value: string) {
  execFileSync(northBin(), ["tell", subject, pred, value], { stdio: "ignore", timeout: 10_000 });
}

/**
 * A deterministic lane id may be reused sequentially (dispatch ids are derived
 * from thread ids). Remove the prior generation's terminal marker before
 * publishing the new identity, otherwise a hard death in the new generation is
 * hidden forever by the old outcome. Simultaneous reuse remains unsupported:
 * callers must not run two live generations under one agent id concurrently.
 */
export function clearAgentOutcome(agentId: string): void {
  const subject = `agent:${agentId}`;
  try {
    const raw = execFileSync(northBin(), ["json", "show", subject], {
      encoding: "utf8", stdio: ["ignore", "pipe", "ignore"], timeout: 10_000,
    });
    const facts = JSON.parse(raw) as Array<{ predicate?: string; value?: string }>;
    for (const fact of facts) {
      if (fact.predicate === "outcome" && fact.value)
        execFileSync(northBin(), ["retract", subject, "outcome", fact.value], { stdio: "ignore", timeout: 10_000 });
    }
  } catch {
    // Non-fatal like identity writes. A failed clear leaves the conservative
    // terminal marker in place rather than blocking the worker from starting.
  }
}

export function writeAgentFacts(agentId: string, f: AgentIdentity): void {
  const subject = `agent:${agentId}`; // north tell @-prefixes bare ids
  clearAgentOutcome(agentId);
  const facts: Array<[string, string | undefined]> = [
    ["kind", f.kind],
    ["display_handle", semanticHandle(agentId, f)],
    ["role", f.role],
    ["model", f.model],
    ["provider", f.provider],
    ["provider_target", f.providerTarget],
    ["effort", f.effort],
    ["composition_kind", f.compositionKind],
    ["composition_id", f.compositionId],
    ["repo", f.repo],
    ["goal", f.goal],
    ["coordinator", f.coordinator],
    ["spawned_at", new Date().toISOString()],
    ["display_name", renderDisplayName(agentId, f)],
  ];
  for (const [p, v] of facts) {
    if (!v) continue;
    try {
      tell(subject, p, v);
    } catch {
      // non-fatal by design; presence falls back to the bare id
    }
  }
}

// Refresh the route projection without resetting generation identity. This is
// used when a pre-side-effect provider fallback activates or an in-flight
// escalation changes model/effort. The control key and spawned_at stay stable.
export function updateAgentRoute(agentId: string, f: AgentIdentity): void {
  for (const [predicate, value] of agentRouteFacts(agentId, f)) {
    if (!value) continue;
    try { tell(`agent:${agentId}`, predicate, value); }
    catch { /* identity telemetry is non-fatal */ }
  }
}

// Terminal-outcome fact on the agent entity ITSELF (@agent:<id>). The reactor's
// died-unreported sweep (cli/reap.clj reap-lane?) reaps a lane whose @agent outcome is
// EMPTY and whose presence lapsed >30min. A lane that reaches its finalize KNOWS its
// outcome, so writing it here is the exact line between a REPORTED terminal (any outcome —
// ran/died/stalled/capped) and a SILENT hard-kill (finally never ran → no outcome anywhere
// → correctly reaped). recordRun's @run write is async fire-and-forget and races process
// exit, so it cannot carry liveness; this write is SYNCHRONOUS (execFileSync) + NORTH_BIN-
// honoring like writeAgentFacts, so it lands before the process exits. Non-fatal by
// design: a failed outcome write must never break the finalize (a lane with no outcome is
// still correctly reaped — fail-safe).
export function writeAgentOutcome(agentId: string, outcome: string): void {
  if (!outcome) return;
  try {
    tell(`agent:${agentId}`, "outcome", outcome); // north tell @-prefixes the bare id -> @agent:<id>
  } catch {
    // non-fatal; presence-lapse reap still catches a truly silent death
  }
}

// First sentence (or first 100 chars) of a spawn prompt — the goal fact seed.
export function goalFromPrompt(prompt: string): string {
  const delegated = prompt.match(/(?:^|\n)DELEGATE TASK:\s*([^\n]+)/)?.[1]?.trim();
  const firstLine = delegated ?? prompt.split("\n", 1)[0] ?? "";
  const sentence = firstLine.split(/(?<=[.!?])\s/, 1)[0] ?? firstLine;
  return sentence.length > 100 ? sentence.slice(0, 97) + "…" : sentence;
}
