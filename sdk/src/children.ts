// Early-exit-with-live-children — the graph-side half of the never-die-with-live-work
// fix (thread 019f4ed2). Half (a) (bgtasks.ts) stops a lane exiting while its OWN
// in-process background Bash tasks run. This half covers the other orphaning path:
// a lane that SPAWNED child agents (each records `coordinator <this-lane>` on
// @agent:<child>) and then truly finalizes while those children have not yet reported
// an outcome — exactly specimen sdk-524a451b (orchestrator said "turn ends here",
// exited, its two workers completed later into a dead inbox; the next wave never fired).
//
// The reactor's died-unreported sweep eventually catches this (lapsed >30min), but that
// is a 30-minute-late signal. This fires it IMMEDIATELY, at the moment of exit, so the
// coordinator learns "I am leaving children behind" now, loudly, with the ids named.
//
// A child is SETTLED (not orphaned) only by a committed lifecycle signal:
// a digest-marked modern lane terminal (or a true pre-process_outcome legacy
// lane), or a tagged run whose last-write kind=run marker landed. Everything
// here is explicit: settlement is not parent reduction, and graph
// unavailability is not the same state as no children.
import { execFileSync } from "node:child_process";
import { resolve } from "node:path";
import { normalizeNorthEntityId, type Fact } from "./north-client";
import type { RoutingRequest } from "./routing-metadata";
import { parseStrictJson } from "./strict-json";
import { laneResolvedByFacts } from "./terminal-projection";

const REPO = resolve(import.meta.dir, "..", "..");
const MSG_CLI = `${REPO}/cli/msg-cli.clj`;
const northBin = () => process.env.NORTH_BIN ?? `${REPO}/bin/north`;
const port = () => process.env.NORTH_PORT ?? "7977";

export const CHILD_SETTLEMENT_MAX_CHILDREN = 128;
export const CHILD_SETTLEMENT_MAX_RUNS = 512;
export const CHILD_SETTLEMENT_MAX_FACT_ROWS = 32_768;
export const CHILD_SETTLEMENT_DEADLINE_MS = 5_000;
const CHILD_SETTLEMENT_MAX_COMMAND_BYTES = 2 * 1024 * 1024;
const CHILD_SETTLEMENT_PROTOCOL = "north.child-settlement";
const CHILD_SETTLEMENT_VERSION = 1;

interface ChildSettlementCommandOptions {
  timeoutMs: number;
  maxBuffer: number;
}

export interface ChildSettlementBulkDependencies {
  run: (
    command: string,
    args: string[],
    options: ChildSettlementCommandOptions,
  ) => string | Uint8Array;
  now?: () => number;
  /** Tests may tighten, never widen, the production wall-clock budget. */
  deadlineMs?: number;
}

function productionChildSettlementCommand(
  command: string,
  args: string[],
  options: ChildSettlementCommandOptions,
): Uint8Array {
  return execFileSync(command, args, {
    timeout: options.timeoutMs,
    maxBuffer: options.maxBuffer,
    stdio: ["ignore", "pipe", "ignore"],
  });
}

function decodedOutput(value: string | Uint8Array, label: string): string {
  const bytes = typeof value === "string"
    ? Buffer.from(value, "utf8")
    : Buffer.from(value.buffer, value.byteOffset, value.byteLength);
  if (bytes.byteLength > CHILD_SETTLEMENT_MAX_COMMAND_BYTES)
    throw new Error(`${label} exceeded its output bound`);
  let text: string;
  try { text = new TextDecoder("utf-8", { fatal: true }).decode(bytes); }
  catch { throw new Error(`${label} returned invalid UTF-8`); }
  return text;
}

interface SubjectFact extends Fact {
  subject: string;
}

function subjectFactRows(parsed: unknown, label: string): SubjectFact[] {
  if (!Array.isArray(parsed) || parsed.length > CHILD_SETTLEMENT_MAX_FACT_ROWS)
    throw new Error(`${label} exceeded its row bound`);
  const rows: SubjectFact[] = [];
  const observed = new Set<string>();
  for (const row of parsed) {
    if (typeof row !== "object" || row === null || Array.isArray(row)
        || Object.keys(row).sort().join("\0") !== "predicate\0subject\0value") {
      throw new Error(`${label} returned an invalid fact row`);
    }
    const fact = row as Record<string, unknown>;
    if (typeof fact.subject !== "string" || typeof fact.predicate !== "string"
        || typeof fact.value !== "string") {
      throw new Error(`${label} returned an invalid fact row`);
    }
    const signature = `${fact.subject}\0${fact.predicate}\0${fact.value}`;
    if (observed.has(signature))
      throw new Error(`${label} returned a duplicate fact row`);
    observed.add(signature);
    rows.push({
      subject: fact.subject,
      predicate: fact.predicate,
      value: fact.value,
    });
  }
  return rows;
}

function groupedSubjectFacts(rows: SubjectFact[]): Map<string, Fact[]> {
  const grouped = new Map<string, Fact[]>();
  for (const row of rows) {
    const facts = grouped.get(row.subject) ?? [];
    facts.push({ predicate: row.predicate, value: row.value });
    grouped.set(row.subject, facts);
  }
  return grouped;
}

interface ChildIdentity {
  subject: string;
  graphId: string;
  agentId: string;
}

function childIdentity(value: string): ChildIdentity {
  if (!value.startsWith("agent:"))
    throw new Error("child settlement projection returned a non-agent child");
  const graphId = normalizeNorthEntityId(value);
  if (graphId !== value || graphId.length === "agent:".length)
    throw new Error("child settlement projection returned a noncanonical child");
  return {
    subject: `@${graphId}`,
    graphId,
    agentId: graphId.slice("agent:".length),
  };
}

function runIdentity(value: string): string {
  if (!value.startsWith("run:"))
    throw new Error("child settlement projection returned a non-run subject");
  let graphId: string;
  try {
    graphId = normalizeNorthEntityId(value);
  } catch {
    throw new Error("child settlement projection returned a noncanonical run");
  }
  if (graphId !== value || !/^run:[a-z0-9][a-z0-9._:-]*$/i.test(graphId))
    throw new Error("child settlement projection returned a noncanonical run");
  return graphId;
}

function exactFactValue(facts: readonly Fact[], predicate: string): string | undefined {
  const values = facts.filter((fact) => fact.predicate === predicate).map((fact) => fact.value);
  return values.length === 1 ? values[0] : undefined;
}

export type ChildSettlement =
  | { kind: "settled"; children: string[] }
  | { kind: "live"; children: string[]; live: string[] }
  | { kind: "unavailable"; reason: string };

export interface ChildContinuationState {
  requiredChildren: number;
  observedChildren: string[];
  liveSignature?: string;
  obligationSignature?: string;
  noProgress: number;
  pendingSettledSignature?: string;
  acknowledgedSettledSignature?: string;
}

export type ChildTurnEndDecision =
  | { action: "finish"; state: ChildContinuationState }
  | {
    action: "continue";
    reason: "children_live";
    state: ChildContinuationState;
    live: string[];
    attempt: number;
    cap: number;
  }
  | {
    action: "continue";
    reason: "child_dispatch_required";
    state: ChildContinuationState;
    children: string[];
    required: number;
    attempt: number;
    cap: number;
  }
  | {
    action: "continue";
    reason: "child_reduction_required";
    state: ChildContinuationState;
    children: string[];
  }
  | {
    action: "block";
    state: ChildContinuationState;
    reason:
      | "children_live_at_continuation_cap"
      | "child_dispatch_continuation_cap"
      | "child_reconciliation_unavailable"
      | "child_set_regressed";
    live?: string[];
    children?: string[];
    required?: number;
    missing?: string[];
  };

export type ChildFinalizationDecision =
  | { ok: true }
  | {
    ok: false;
    outcome:
      | "orchestrator_children_incomplete"
      | "orchestrator_child_obligation_unmet"
      | "child_reconciliation_unavailable"
      | "orchestrator_reduction_incomplete"
      | "orchestrator_child_set_inconsistent";
    live?: string[];
    children?: string[];
    required?: number;
    missing?: string[];
    reason?: string;
  };

export function requiredDirectChildCount(routing: RoutingRequest): number {
  if (routing.topology !== "orchestrator") return 0;
  return routing.composition.kind === "preset"
      && routing.composition.id === "director"
    ? 2
    : 1;
}

export function initialChildContinuationState(
  requiredChildren: number,
): ChildContinuationState {
  if (!Number.isSafeInteger(requiredChildren) || requiredChildren < 0) {
    throw new Error("required child count must be a non-negative safe integer");
  }
  return { requiredChildren, observedChildren: [], noProgress: 0 };
}

function canonicalChildren(children: string[]): string[] {
  return [...new Set(children)].sort();
}

function setSignature(children: string[]): string {
  return canonicalChildren(children).join("\u0000");
}

function observeChildren(
  previous: ChildContinuationState,
  children: string[],
): { state: ChildContinuationState; missing: string[] } {
  const current = new Set(canonicalChildren(children));
  const missing = previous.observedChildren.filter((child) => !current.has(child));
  if (missing.length > 0) return { state: previous, missing };
  return {
    state: {
      ...previous,
      observedChildren: canonicalChildren([
        ...previous.observedChildren,
        ...children,
      ]),
    },
    missing: [],
  };
}

function afterSuccessfulProviderResult(
  previous: ChildContinuationState,
): ChildContinuationState {
  if (!previous.pendingSettledSignature) return previous;
  return {
    ...previous,
    acknowledgedSettledSignature: previous.pendingSettledSignature,
    pendingSettledSignature: undefined,
  };
}

export function decideChildTurnEnd(
  previous: ChildContinuationState,
  settlement: ChildSettlement,
  cap: number,
): ChildTurnEndDecision {
  if (!Number.isSafeInteger(cap) || cap < 0) {
    throw new Error("child continuation cap must be a non-negative safe integer");
  }
  let observed = previous;
  if (settlement.kind !== "unavailable") {
    const observation = observeChildren(previous, settlement.children);
    if (observation.missing.length > 0) {
      return {
        action: "block",
        state: observation.state,
        reason: "child_set_regressed",
        missing: observation.missing,
      };
    }
    observed = observation.state;
  }
  // This function is called only after a successful provider result. Therefore
  // a pending settled signature can be acknowledged now: the provider has
  // completed the continuation that North injected for that exact child set.
  // The child observation above MUST happen first: a disappeared coordinator
  // edge cannot acknowledge the reduction that was pending for that child.
  const acknowledged = afterSuccessfulProviderResult(observed);
  if (settlement.kind === "settled") {
    const children = canonicalChildren(settlement.children);
    if (children.length < acknowledged.requiredChildren) {
      const obligationSignature = `${acknowledged.requiredChildren}\u0001${setSignature(children)}`;
      const noProgress = acknowledged.obligationSignature === obligationSignature
        ? acknowledged.noProgress + 1
        : 1;
      const state = {
        ...acknowledged,
        liveSignature: undefined,
        obligationSignature,
        noProgress,
        pendingSettledSignature: undefined,
      };
      if (noProgress > cap) {
        return {
          action: "block",
          state,
          reason: "child_dispatch_continuation_cap",
          children,
          required: acknowledged.requiredChildren,
        };
      }
      return {
        action: "continue",
        reason: "child_dispatch_required",
        state,
        children,
        required: acknowledged.requiredChildren,
        attempt: noProgress,
        cap,
      };
    }
    if (children.length === 0) {
      return {
        action: "finish",
        state: {
          ...acknowledged,
          liveSignature: undefined,
          obligationSignature: undefined,
          noProgress: 0,
        },
      };
    }
    const signature = setSignature(children);
    if (acknowledged.acknowledgedSettledSignature === signature) {
      return {
        action: "finish",
        state: {
          ...acknowledged,
          liveSignature: undefined,
          obligationSignature: undefined,
          noProgress: 0,
        },
      };
    }
    return {
      action: "continue",
      reason: "child_reduction_required",
      state: {
        ...acknowledged,
        liveSignature: undefined,
        obligationSignature: undefined,
        noProgress: 0,
        pendingSettledSignature: signature,
      },
      children: settlement.children,
    };
  }
  if (settlement.kind === "unavailable") {
    return {
      action: "block",
      state: acknowledged,
      reason: "child_reconciliation_unavailable",
    };
  }
  const liveSignature = `${setSignature(settlement.children)}\u0001${setSignature(settlement.live)}`;
  const noProgress = acknowledged.liveSignature === liveSignature
    ? acknowledged.noProgress + 1
    : 1;
  const state = {
    ...acknowledged,
    liveSignature,
    obligationSignature: undefined,
    noProgress,
    pendingSettledSignature: undefined,
  };
  if (noProgress > cap) {
    return {
      action: "block",
      state,
      reason: "children_live_at_continuation_cap",
      live: settlement.live,
    };
  }
  return {
    action: "continue",
    reason: "children_live",
    state,
    live: settlement.live,
    attempt: noProgress,
    cap,
  };
}

export function assessChildFinalization(
  state: ChildContinuationState,
  settlement: ChildSettlement,
): ChildFinalizationDecision {
  if (settlement.kind === "unavailable") {
    return {
      ok: false,
      outcome: "child_reconciliation_unavailable",
      reason: settlement.reason,
    };
  }
  const current = new Set(canonicalChildren(settlement.children));
  const missing = state.observedChildren.filter((child) => !current.has(child));
  if (missing.length > 0) {
    return {
      ok: false,
      outcome: "orchestrator_child_set_inconsistent",
      missing,
      reason: "previously observed coordinator relation disappeared",
    };
  }
  if (settlement.kind === "live") {
    return {
      ok: false,
      outcome: "orchestrator_children_incomplete",
      live: settlement.live,
    };
  }
  const children = canonicalChildren(settlement.children);
  if (children.length < state.requiredChildren) {
    return {
      ok: false,
      outcome: "orchestrator_child_obligation_unmet",
      children,
      required: state.requiredChildren,
      reason: "minimum direct-child count was not met",
    };
  }
  if (children.length === 0) return { ok: true };
  const signature = setSignature(children);
  if (state.acknowledgedSettledSignature === signature
      && state.pendingSettledSignature === undefined) {
    return { ok: true };
  }
  return {
    ok: false,
    outcome: "orchestrator_reduction_incomplete",
    children: settlement.children,
  };
}

export function resolveChildLifecycle(
  laneFacts: Fact[],
  readTaggedRuns: () => Fact[][],
): boolean {
  if (laneResolvedByFacts(laneFacts, [])) return true;
  return laneResolvedByFacts([], readTaggedRuns());
}

// Classify every child under one snapshot attempt. An empty or fully-terminal
// set is `settled`; this says nothing yet about parent reduction. A read failure
// remains `unavailable`.
export function gatherChildSettlement(
  coordId: string,
  readChildren: (id: string) => string[],
  resolved: (child: string) => boolean,
): ChildSettlement {
  try {
    if (!coordId) return { kind: "settled", children: [] };
    const kids = readChildren(coordId);
    if (!kids.length) return { kind: "settled", children: [] };
    const live = kids.filter((child) => !resolved(child));
    return live.length
      ? { kind: "live", children: kids, live }
      : { kind: "settled", children: kids };
  } catch (error) {
    return {
      kind: "unavailable",
      reason: error instanceof Error ? error.message : "unknown child settlement failure",
    };
  }
}

/**
 * One-command child projection. North derives direct children, their complete
 * fact sets, and every tagged run for those children from one `live-facts`
 * vector and emits a closed, versioned envelope. This is one actual snapshot:
 * child growth/shrink and run commit cannot split across reads. The SDK still
 * independently validates the complete envelope, identities, authority facts,
 * cardinality and lifecycle markers before classifying anything.
 */
export function settleChildrenBounded(
  coordId: string,
  dependencies: ChildSettlementBulkDependencies,
): ChildSettlement {
  try {
    if (!coordId) return { kind: "settled", children: [] };
    const canonicalCoordId = normalizeNorthEntityId(coordId);
    const deadlineMs = dependencies.deadlineMs ?? CHILD_SETTLEMENT_DEADLINE_MS;
    if (!Number.isSafeInteger(deadlineMs) || deadlineMs <= 0
        || deadlineMs > CHILD_SETTLEMENT_DEADLINE_MS) {
      throw new Error("child settlement deadline is invalid");
    }
    const now = dependencies.now ?? (() => performance.now());
    const deadline = now() + deadlineMs;
    const remaining = Math.floor(deadline - now());
    if (remaining <= 0) throw new Error("child settlement aggregate deadline exceeded");
    const output = decodedOutput(
      dependencies.run(
        northBin(),
        ["json", "child-settlement", canonicalCoordId],
        {
          timeoutMs: Math.max(1, remaining),
          maxBuffer: CHILD_SETTLEMENT_MAX_COMMAND_BYTES,
        },
      ),
      "child settlement projection",
    );
    if (now() > deadline) throw new Error("child settlement aggregate deadline exceeded");
    const parsed = parseStrictJson(output, "child settlement projection", {
      maxBytes: CHILD_SETTLEMENT_MAX_COMMAND_BYTES,
      maxDepth: 32,
      maxNodes: CHILD_SETTLEMENT_MAX_FACT_ROWS * 8 + 16,
    });
    if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)
        || Object.keys(parsed).sort().join("\0")
          !== "children\0coordinator\0protocol\0runs\0version") {
      throw new Error("child settlement projection returned an invalid envelope");
    }
    const envelope = parsed as Record<string, unknown>;
    if (envelope.protocol !== CHILD_SETTLEMENT_PROTOCOL
        || envelope.version !== CHILD_SETTLEMENT_VERSION
        || envelope.coordinator !== canonicalCoordId) {
      throw new Error("child settlement projection returned an incompatible envelope");
    }
    const childRows = subjectFactRows(
      envelope.children,
      "child settlement child projection",
    );
    const runRows = subjectFactRows(
      envelope.runs,
      "child settlement run projection",
    );
    if (childRows.length + runRows.length > CHILD_SETTLEMENT_MAX_FACT_ROWS)
      throw new Error("child settlement projection exceeded its cumulative row bound");
    const childFacts = groupedSubjectFacts(childRows);
    if (childFacts.size > CHILD_SETTLEMENT_MAX_CHILDREN)
      throw new Error("child settlement child projection exceeded its subject bound");
    const children = [...childFacts.keys()].map(childIdentity)
      .sort((left, right) => left.subject < right.subject ? -1 : left.subject > right.subject ? 1 : 0);
    for (const child of children) {
      const facts = childFacts.get(child.graphId)!;
      if (exactFactValue(facts, "coordinator") !== canonicalCoordId)
        throw new Error("child settlement projection returned invalid child authority");
    }
    const runFacts = groupedSubjectFacts(runRows);
    if (runFacts.size > CHILD_SETTLEMENT_MAX_RUNS)
      throw new Error("child settlement run projection exceeded its subject bound");
    const runsByAgent = new Map(children.map((child) => [child.agentId, [] as Fact[][]]));
    for (const [rawRunId, facts] of runFacts) {
      runIdentity(rawRunId);
      const agent = exactFactValue(facts, "agent");
      if (exactFactValue(facts, "kind") !== "run" || !agent || !runsByAgent.has(agent))
        throw new Error("child settlement projection returned invalid run authority");
      runsByAgent.get(agent)!.push(facts);
    }

    const childSubjects = children.map((child) => child.subject);
    const live = children.filter((child) =>
      !resolveChildLifecycle(
        childFacts.get(child.graphId)!,
        () => runsByAgent.get(child.agentId)!,
      )).map((child) => child.subject);
    return live.length
      ? { kind: "live", children: childSubjects, live }
      : { kind: "settled", children: childSubjects };
  } catch (error) {
    return {
      kind: "unavailable",
      reason: error instanceof Error ? error.message : "unknown child settlement failure",
    };
  }
}

export function settleChildren(coordId: string): ChildSettlement {
  return settleChildrenBounded(coordId, { run: productionChildSettlementCommand });
}

// The three orchestrator continuation shapes North injects at turn-end. A
// continuation asks the provider for ANOTHER genuine turn; the value doubles as
// the `decideChildTurnEnd` continue-reason so the harness can remember which
// obligation is outstanding while the next turn runs.
export type OrchestratorContinuationKind =
  | "children_live"
  | "child_dispatch_required"
  | "child_reduction_required";

// Orchestrator continuation race (thread 019f8ec5): a continuation can be
// "answered" by a degenerate empty-success terminal when the Anthropic session
// tears down after its final message — the injected continuation lands on a
// closing stream. Such a terminal leaves the obligation UNMET, so map the
// outstanding continuation to the same explicit blocked outcome the final child
// gate would record. This keeps the terminal loud and truthful (never a
// ran_empty masquerade, never a false reduction acknowledgement).
export function continuationRaceOutcome(kind: OrchestratorContinuationKind): string {
  switch (kind) {
    case "children_live":
      return "orchestrator_children_incomplete";
    case "child_dispatch_required":
      return "orchestrator_child_obligation_unmet";
    case "child_reduction_required":
      return "orchestrator_reduction_incomplete";
  }
}

export function childContinuationMessage(liveIds: string[]): string {
  return [
    `North refuses orchestrator turn-end: ${liveIds.length} child lane(s) remain live (${liveIds.join(", ")}).`,
    "Keep this turn active, consume the North listener/peer results, reconcile completed work into the prebound thread,",
    "and return a later terminal result only after every child has a committed lifecycle terminal.",
  ].join(" ");
}

export function childDispatchMessage(
  observedIds: string[],
  required: number,
): string {
  const remaining = Math.max(0, required - observedIds.length);
  return [
    `North refuses orchestrator turn-end: the direct-child obligation is unmet (${observedIds.length}/${required} observed).`,
    `Use North coordination now to dispatch at least ${remaining} more direct child lane(s); do not perform their terminal work inline.`,
    "Return a later terminal result only after the required direct coordinator relations exist; live-child settlement and parent reduction are enforced separately.",
  ].join(" ");
}

export function childReductionMessage(settledIds: string[]): string {
  return [
    `North requires a post-settlement reduction turn: ${settledIds.length} child lane(s) are terminal (${settledIds.join(", ")}).`,
    "Consume their completion pings/reports, inspect the child results as needed, and reduce those results into the prebound parent thread.",
    "Return a new terminal result only after that reduction; a changed settled child set requires another reduction turn.",
  ].join(" ");
}

export interface EarlyExitCtx {
  coordinator?: string;
}

type Cmd = { cmd: string; args: string[] };

// PURE: the command specs an early-exit-with-live-children emits — a durable
// `early_exit_children` fact on @agent:<id> (queryable, like agent_death/stalled) + a
// loud "EARLY EXIT WITH LIVE CHILDREN" peer ping naming the orphans. Pure so the
// contract is unit-testable without a live coordinator (mirrors death/watchdog).
export function earlyExitCommands(
  agentId: string,
  liveIds: string[],
  ctx: EarlyExitCtx = {},
  ts: string = new Date().toISOString(),
): Cmd[] {
  const ids = liveIds.join(",");
  const line = `${agentId} | orphaned: ${ids} | ${ts}`;
  const cmds: Cmd[] = [
    { cmd: northBin(), args: ["tell", `agent:${agentId}`, "early_exit_children", line] },
  ];
  if (ctx.coordinator) {
    cmds.push({
      cmd: "bb",
      args: [MSG_CLI, port(), "send", agentId, ctx.coordinator, "EARLY EXIT WITH LIVE CHILDREN",
        `${liveIds.length} live child(ren): ${ids} (${ts})`],
    });
  }
  return cmds;
}

// Emit the early-exit notification. Synchronous + fully swallowed (a finalizing lane
// must never throw out of this), and a loud stderr line so it shows in the lane log.
export function notifyEarlyExitChildren(
  agentId: string,
  liveIds: string[],
  ctx: EarlyExitCtx = {},
  timeoutMs = 10_000,
): void {
  if (!liveIds.length) return;
  const startedAt = performance.now();
  for (const { cmd, args } of earlyExitCommands(agentId, liveIds, ctx)) {
    try {
      const remaining = Math.max(
        1,
        Math.floor(timeoutMs - (performance.now() - startedAt)),
      );
      execFileSync(cmd, args, {
        encoding: "utf8",
        timeout: remaining,
        stdio: ["ignore", "ignore", "ignore"],
      });
    } catch {
      /* best-effort */
    }
  }
  console.error(`[early-exit] @agent:${agentId} EXITING WITH ${liveIds.length} LIVE CHILD(REN): ${liveIds.join(", ")}`);
}

// ── Orchestrator park-and-resume (thread 019f9599) ──────────────────────────────
// The defect this closes: a parent with live children had only two turn-end paths —
// spin genuine provider turns (burning tokens to "wait"), or end the turn and be
// recorded orchestrator_children_incomplete (three standing seats died this way in
// one evening). Neither is WAITING. Park is the third path. A parent DECLARES it is
// dormant-waiting on named children via a bounded `orchestrator_park` fact on
// @agent:<id>, keeps its provider subprocess idle (zero tokens — the harness simply
// stops pulling the next message, so the stall watchdog is never armed), and is
// resumed for reduction when those children settle. The declaration is load-bearing:
//   • it makes the parent OBSERVABLE (`north agents` shows the awaited children) and
//     durably REAP-EXEMPT until expiry (cli/reap.clj park-active?), so a parked parent
//     is neither false-live nor silently reaped;
//   • it is the ONLY thing separating a legitimate park from genuine abandonment. An
//     UNDECLARED exit with live children still fires the loud early_exit_children fact
//     + peer ping exactly as before, and a park that EXPIRES with children still live
//     (or whose awaited child vanishes) is treated as abandonment and goes loud too —
//     park can never silently drop children.

export const ORCHESTRATOR_PARK_VERSION = 1;
// A park is bounded: a parent may wait a long time on slow children, but never
// forever — an unbounded park is indistinguishable from a wedged process. Expiry
// restores normal reaping and the loud abandonment path.
export const ORCHESTRATOR_PARK_TTL_CEILING_MS = 24 * 60 * 60 * 1000; // 24h hard ceiling
export const ORCHESTRATOR_PARK_TTL_DEFAULT_MS = 4 * 60 * 60 * 1000; //  4h
export const ORCHESTRATOR_PARK_POLL_CEILING_MS = 5 * 60 * 1000; //      5min
export const ORCHESTRATOR_PARK_POLL_DEFAULT_MS = 15 * 1000; //         15s
const ORCHESTRATOR_PARK_MAX_VALUE_BYTES = 64 * 1024;
const ORCHESTRATOR_PARK_MAX_CHILD_ID = 512;

function boundedEnvMs(name: string, fallback: number, ceiling: number): number {
  const raw = process.env[name];
  if (raw === undefined) return fallback;
  const value = Number(raw);
  if (!Number.isSafeInteger(value) || value <= 0 || value > ceiling) return fallback;
  return value;
}

export function orchestratorParkTtlMs(): number {
  return boundedEnvMs(
    "NORTH_ORCHESTRATOR_PARK_TTL_MS",
    ORCHESTRATOR_PARK_TTL_DEFAULT_MS,
    ORCHESTRATOR_PARK_TTL_CEILING_MS,
  );
}

export function orchestratorParkPollMs(): number {
  return boundedEnvMs(
    "NORTH_ORCHESTRATOR_PARK_POLL_MS",
    ORCHESTRATOR_PARK_POLL_DEFAULT_MS,
    ORCHESTRATOR_PARK_POLL_CEILING_MS,
  );
}

export interface OrchestratorPark {
  children: string[]; // canonical live children the parent is waiting on
  parkedAt: string; //  ISO-8601
  expiresAt: string; // ISO-8601 (parkedAt + TTL)
}

export function orchestratorPark(
  live: string[],
  parkedAtMs: number,
  ttlMs: number,
): OrchestratorPark {
  if (!Number.isSafeInteger(parkedAtMs) || parkedAtMs < 0) {
    throw new Error("park timestamp must be a non-negative safe integer");
  }
  if (!Number.isSafeInteger(ttlMs) || ttlMs <= 0) {
    throw new Error("park ttl must be a positive safe integer");
  }
  const children = canonicalChildren(live);
  if (children.length === 0) throw new Error("a park must name at least one live child");
  if (children.length > CHILD_SETTLEMENT_MAX_CHILDREN) {
    throw new Error("park child set exceeds its bound");
  }
  return {
    children,
    parkedAt: new Date(parkedAtMs).toISOString(),
    expiresAt: new Date(parkedAtMs + ttlMs).toISOString(),
  };
}

export function serializeOrchestratorPark(park: OrchestratorPark): string {
  return JSON.stringify({
    v: ORCHESTRATOR_PARK_VERSION,
    children: park.children,
    parkedAt: park.parkedAt,
    expiresAt: park.expiresAt,
  });
}

// Fail closed: any malformed / oversized / wrong-version value is "no valid park"
// (null), never a partially-trusted park. The reaper treats null as "not exempt".
export function parseOrchestratorPark(value: string): OrchestratorPark | null {
  if (typeof value !== "string" || value.length > ORCHESTRATOR_PARK_MAX_VALUE_BYTES) {
    return null;
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(value);
  } catch {
    return null;
  }
  if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) return null;
  const obj = parsed as Record<string, unknown>;
  if (obj.v !== ORCHESTRATOR_PARK_VERSION) return null;
  if (!Array.isArray(obj.children) || obj.children.length === 0
      || obj.children.length > CHILD_SETTLEMENT_MAX_CHILDREN) {
    return null;
  }
  const children: string[] = [];
  for (const child of obj.children) {
    if (typeof child !== "string" || child.length === 0
        || child.length > ORCHESTRATOR_PARK_MAX_CHILD_ID) {
      return null;
    }
    children.push(child);
  }
  if (typeof obj.parkedAt !== "string" || typeof obj.expiresAt !== "string") return null;
  if (!Number.isFinite(Date.parse(obj.parkedAt))
      || !Number.isFinite(Date.parse(obj.expiresAt))) {
    return null;
  }
  return {
    children: canonicalChildren(children),
    parkedAt: obj.parkedAt,
    expiresAt: obj.expiresAt,
  };
}

export function parkDeclareCommands(agentId: string, park: OrchestratorPark): Cmd[] {
  return [{
    cmd: northBin(),
    args: ["tell", `agent:${agentId}`, "orchestrator_park", serializeOrchestratorPark(park)],
  }];
}

export function parkClearCommands(agentId: string, park: OrchestratorPark): Cmd[] {
  return [{
    cmd: northBin(),
    args: ["retract", `agent:${agentId}`, "orchestrator_park", serializeOrchestratorPark(park)],
  }];
}

// PURE resume verdict, evaluated each poll against a fresh settlement snapshot.
//   resume    — every awaited child is terminal; hand the settled snapshot back to
//               the normal reduction/finish machine.
//   wait      — children still live (or a transient unavailable read) and unexpired.
//   expired   — the park's TTL elapsed with children still live; abandonment, go loud.
//   abandoned — an awaited coordinator edge disappeared (child-set regression); loud.
export type ParkResumeDecision =
  | { action: "resume"; settlement: ChildSettlement & { kind: "settled" } }
  | { action: "wait" }
  | { action: "expired"; live: string[] }
  | { action: "abandoned"; missing: string[] };

export function decideParkResume(
  park: OrchestratorPark,
  settlement: ChildSettlement,
  nowMs: number,
): ParkResumeDecision {
  const expired = nowMs >= Date.parse(park.expiresAt);
  if (settlement.kind === "unavailable") {
    // A transient graph read failure must not collapse a park; keep waiting until
    // TTL. Only expiry ends an unresolvable park (then handled loudly upstream).
    return expired ? { action: "expired", live: park.children } : { action: "wait" };
  }
  const current = new Set(canonicalChildren(settlement.children));
  const missing = park.children.filter((child) => !current.has(child));
  if (missing.length > 0) return { action: "abandoned", missing };
  if (settlement.kind === "settled") return { action: "resume", settlement };
  return expired ? { action: "expired", live: settlement.live } : { action: "wait" };
}

export interface ParkAwaitDependencies {
  settle: () => ChildSettlement;
  sleep: (ms: number) => Promise<void>;
  now: () => number; // epoch ms
  declare: (park: OrchestratorPark) => void;
  clear: (park: OrchestratorPark) => void;
  heartbeat?: () => void;
  aborted?: () => boolean;
  ttlMs?: number;
  pollMs?: number;
}

export type ParkAwaitResult =
  | { kind: "settled"; settlement: ChildSettlement & { kind: "settled" } }
  | { kind: "expired"; live: string[] }
  | { kind: "abandoned"; missing: string[] }
  | { kind: "aborted" };

// The dormant wait loop. Impure but fully INJECTABLE (fake sleep/now/settle) so the
// park lifecycle is unit-testable with no live coordinator and no real clock. The park
// fact is declared once up front and always cleared in `finally` — a resumed, expired,
// abandoned, or aborted park never leaves a stale declaration behind.
export async function awaitParkedChildren(
  live: string[],
  deps: ParkAwaitDependencies,
): Promise<ParkAwaitResult> {
  const ttlMs = deps.ttlMs ?? orchestratorParkTtlMs();
  const pollMs = deps.pollMs ?? orchestratorParkPollMs();
  const park = orchestratorPark(live, deps.now(), ttlMs);
  deps.declare(park);
  try {
    for (;;) {
      if (deps.aborted?.()) return { kind: "aborted" };
      const decision = decideParkResume(park, deps.settle(), deps.now());
      if (decision.action === "resume") {
        return { kind: "settled", settlement: decision.settlement };
      }
      if (decision.action === "expired") return { kind: "expired", live: decision.live };
      if (decision.action === "abandoned") return { kind: "abandoned", missing: decision.missing };
      deps.heartbeat?.(); // renew presence so the lane stays observably alive
      await deps.sleep(pollMs); // dormant until the next poll — no provider turn
    }
  } finally {
    deps.clear(park);
  }
}

function runParkCommands(cmds: Cmd[], timeoutMs = 10_000): void {
  const startedAt = performance.now();
  for (const { cmd, args } of cmds) {
    try {
      const remaining = Math.max(1, Math.floor(timeoutMs - (performance.now() - startedAt)));
      execFileSync(cmd, args, {
        encoding: "utf8",
        timeout: remaining,
        stdio: ["ignore", "ignore", "ignore"],
      });
    } catch {
      /* best-effort: the park fact is advisory for observability + reap-exemption;
         the wait loop still awaits settlement even if the write did not land. */
    }
  }
}

function abortableSleep(ms: number, signal: AbortSignal): Promise<void> {
  return new Promise<void>((resolve) => {
    if (signal.aborted) return resolve();
    const onAbort = () => { clearTimeout(timer); resolve(); };
    const timer = setTimeout(() => { signal.removeEventListener("abort", onAbort); resolve(); }, ms);
    signal.addEventListener("abort", onAbort, { once: true });
  });
}

export interface OrchestratorParkHooks {
  agentId: string;
  settle: () => ChildSettlement;
  signal: AbortSignal;
  renewPresence?: () => void;
  aborted?: () => boolean;
}

// Thin production wiring: bind the injectable wait loop to the real coordinator
// (fact writes), the real clock, and the harness abort signal.
export async function runOrchestratorPark(
  live: string[],
  hooks: OrchestratorParkHooks,
): Promise<ParkAwaitResult> {
  return awaitParkedChildren(live, {
    settle: hooks.settle,
    sleep: (ms) => abortableSleep(ms, hooks.signal),
    now: () => Date.now(),
    declare: (park) => {
      runParkCommands(parkDeclareCommands(hooks.agentId, park));
      console.error(
        `[park] @agent:${hooks.agentId} PARKED on ${park.children.length} live child(ren): `
        + `${park.children.join(", ")} (until ${park.expiresAt})`,
      );
    },
    clear: (park) => runParkCommands(parkClearCommands(hooks.agentId, park)),
    heartbeat: hooks.renewPresence,
    aborted: hooks.aborted ?? (() => hooks.signal.aborted),
  });
}
