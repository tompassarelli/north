// Stream watchdog — the missing liveness signal (thread 019f4d54).
//
// THE BUG it kills: `for await (const message of q)` in spawn.ts/dispatch.ts awaits
// the SDK's async iterator with NO timeout. When the underlying turn STALLS without
// dying — context-window exhaustion mid-turn, an API 529/overload retried below the
// message layer, or a turn that simply never completes — the iterator neither yields
// nor throws. The error boundary (try/catch) only catches THROWS; a hang is not a
// throw, so finally/recordRun/notifyDeath never fire and the lane goes silent for
// hours, alive but terminal-invisible (specimens sdk-a63f2676, sdk-e30a4d6f: logs
// carry only "[spawn] starting").
//
// THE FIX: race each message against a stall timer. Liveness = ANY SDK message (a
// working lane emits assistant/tool_result/status messages steadily, so it never
// trips — BOUNDED: we never abort a lane that is producing output). Total message
// silence for N minutes -> onStall (surface, non-destructive). Silence for 2N ->
// onAbort (terminal). N is NORTH_STALL_MS (default 10min) so it is testable with a
// tiny override and tunable per workload.
//
// Known trade-off: a SINGLE tool call that runs >2N minutes emitting nothing (a very
// long silent build) would trip the abort — but such a call is pathological and the
// abort is visible + recoverable, not data loss. Default N=10min gives 20min of
// abort headroom, past which "silent" is indistinguishable from "hung".
import { execFileSync } from "node:child_process";
import { resolve } from "node:path";
import type {
  ExecutionActivityEvidence, ExecutionActivitySnapshot, ExecutionActivitySource,
} from "./execution-activity";
import {
  framBabashkaArguments,
  framCoordinatorChildTimeout,
  framEngineEnvironment,
} from "./fram-engine";

const REPO = resolve(import.meta.dir, "..", "..");
const MSG_CLI = `${REPO}/cli/msg-cli.clj`;
const northBin = () => process.env.NORTH_BIN ?? `${REPO}/bin/north`;
const port = () => process.env.NORTH_PORT ?? "7977";
const peerBb = () => process.env.NORTH_PEER_BB ?? "bb";

// Default stall window: 10 minutes. NORTH_STALL_MS overrides (ms) — the test seam.
export const DEFAULT_STALL_MS = 10 * 60_000;
export function stallMs(): number {
  const raw = Number(process.env.NORTH_STALL_MS);
  return Number.isFinite(raw) && raw > 0 ? raw : DEFAULT_STALL_MS;
}

export interface WatchdogHooks {
  stallMs: number;
  onStall: (mins: number, evidence: ExecutionActivitySnapshot) => void;
  onAbort: (evidence: WatchdogAbortEvidence) => void;
  activitySources?: readonly ExecutionActivitySource[];
}

export interface WatchdogAbortEvidence extends ExecutionActivitySnapshot {
  reason: "north_watchdog_execution_inactivity";
  silenceMs: number;
}

export function describeWatchdogAbortEvidence(evidence: WatchdogAbortEvidence): string {
  const render = (activity: ExecutionActivityEvidence | undefined) =>
    activity ? `${activity.kind}@${activity.observedAt}` : "none";
  return `${evidence.reason} silence_ms=${evidence.silenceMs} `
    + `last_outer=${render(evidence.lastOuter)} `
    + `last_provider=${render(evidence.lastProvider)}`;
}

function latestEvidence(
  sources: readonly ExecutionActivitySource[],
): ExecutionActivitySnapshot {
  let lastOuter: ExecutionActivityEvidence | undefined;
  let lastProvider: ExecutionActivityEvidence | undefined;
  for (const source of sources) {
    const snapshot = source.snapshot();
    if (snapshot.lastOuter
        && (!lastOuter || snapshot.lastOuter.observedAt > lastOuter.observedAt))
      lastOuter = snapshot.lastOuter;
    if (snapshot.lastProvider
        && (!lastProvider || snapshot.lastProvider.observedAt > lastProvider.observedAt))
      lastProvider = snapshot.lastProvider;
  }
  return {
    ...(lastOuter ? { lastOuter } : {}),
    ...(lastProvider ? { lastProvider } : {}),
  };
}

// One absolute inactivity deadline spans every non-activity outer message.
// Provider-native pulses wake the SAME pending source.next(), so a quiet outer
// iterator can remain healthy without concurrent next() calls.
export async function* withStallWatchdog<T>(
  source: AsyncIterator<T>,
  hooks: WatchdogHooks,
): AsyncGenerator<T> {
  const { stallMs, onStall, onAbort } = hooks;
  const activitySources = hooks.activitySources ?? [];
  const mins = Math.max(1, Math.round(stallMs / 60_000));
  const sequences = new Map(
    activitySources.map((activity) => [activity, activity.snapshot().sequence]),
  );
  let lastActivityAt = performance.now();
  let warned = false;
  while (true) {
    const pending = source.next();
    while (true) {
      let activityObserved = false;
      for (const activity of activitySources) {
        const sequence = activity.snapshot().sequence;
        if (sequence !== sequences.get(activity)) {
          sequences.set(activity, sequence);
          activityObserved = true;
        }
      }
      if (activityObserved) {
        lastActivityAt = performance.now();
        warned = false;
      }
      const elapsed = Math.max(0, performance.now() - lastActivityAt);
      if (!warned && elapsed >= stallMs) {
        warned = true;
        onStall(mins, latestEvidence(activitySources));
      }
      if (elapsed >= stallMs * 2) {
        const evidence: WatchdogAbortEvidence = {
          reason: "north_watchdog_execution_inactivity",
          silenceMs: stallMs * 2,
          ...latestEvidence(activitySources),
        };
        onAbort(evidence);
        void pending.catch(() => undefined);
        return;
      }

      let timer: ReturnType<typeof setTimeout> | undefined;
      const remaining = Math.max(
        1,
        (warned ? stallMs * 2 : stallMs) - elapsed,
      );
      let unsubscribe = () => {};
      const pulse = new Promise<"activity">((resolvePulse) => {
        const removers = activitySources.map((activity) =>
          activity.subscribe(() => resolvePulse("activity"))
        );
        unsubscribe = () => removers.forEach((remove) => remove());
      });
      const timeout = new Promise<"timer">((resolveTimer) => {
        timer = setTimeout(() => resolveTimer("timer"), remaining);
      });
      try {
        const tag = await Promise.race([
          pending.then(() => "message" as const),
          pulse,
          timeout,
        ]);
        if (tag !== "message") continue;
        const result = await pending;
        if (result.done) return;
        yield result.value;
        break;
      } finally {
        unsubscribe();
        clearTimeout(timer);
      }
    }
  }
}

export interface CoordCtx {
  coordinator?: string; // handle that gets the peer ping
}

type Cmd = { cmd: string; args: string[]; framChild?: true };

// PURE: the command specs a stall emits — a durable `stalled` fact on @agent:<id>
// (queryable off the graph, like agent_death) + an "AGENT STALLED" peer ping. Pure so
// the contract is unit-testable without a live coordinator (mirrors death.ts).
export function stallCommands(
  agentId: string,
  mins: number,
  ctx: CoordCtx = {},
  ts: string = new Date().toISOString(),
): Cmd[] {
  const line = `${agentId} | no SDK output ${mins}min | ${ts}`;
  const cmds: Cmd[] = [
    { cmd: northBin(), args: ["tell", `agent:${agentId}`, "stalled", line] },
  ];
  if (ctx.coordinator) {
    cmds.push({ cmd: peerBb(), args: framBabashkaArguments([MSG_CLI, port(), "send", agentId, ctx.coordinator, "AGENT STALLED", `${mins}min — no output (${ts})`]), framChild: true });
  }
  return cmds;
}

// PURE: the command specs a turn-cap emits — a durable `turn_capped` fact + a
// "TURN CAP" peer ping carrying a partial-result note, so a maxTurns stop is VISIBLE
// instead of masquerading as a clean completion.
export function turnCapCommands(
  agentId: string,
  note: string,
  ctx: CoordCtx = {},
  ts: string = new Date().toISOString(),
): Cmd[] {
  const cmds: Cmd[] = [
    { cmd: northBin(), args: ["tell", `agent:${agentId}`, "turn_capped", `${agentId} | ${ts}`] },
  ];
  if (ctx.coordinator) {
    cmds.push({ cmd: peerBb(), args: framBabashkaArguments([MSG_CLI, port(), "send", agentId, ctx.coordinator, "TURN CAP", `${note} (${ts})`]), framChild: true });
  }
  return cmds;
}

// Execute a command spec list. Synchronous + fully swallowed: notifying must never
// throw out of a dying/stalling agent nor mask the original condition (like death.ts).
function emit(cmds: Cmd[], timeoutMs = 10_000): void {
  const startedAt = performance.now();
  for (const { cmd, args, framChild } of cmds) {
    try {
      const remaining = Math.max(
        1,
        Math.floor(timeoutMs - (performance.now() - startedAt)),
      );
      execFileSync(cmd, args, {
        encoding: "utf8",
        ...(framChild ? { env: framEngineEnvironment() } : {}),
        timeout: framChild ? framCoordinatorChildTimeout(remaining) : remaining,
        stdio: ["ignore", "ignore", "ignore"],
      });
    } catch { /* best-effort: telemetry outcome still records the condition */ }
  }
}

export function notifyStall(agentId: string, mins: number, ctx: CoordCtx = {}): void {
  emit(stallCommands(agentId, mins, ctx));
  console.error(`[stall] @agent:${agentId} silent ${mins}min — no SDK output`);
}

export function notifyTurnCap(
  agentId: string,
  note: string,
  ctx: CoordCtx = {},
  timeoutMs = 10_000,
): void {
  emit(turnCapCommands(agentId, note, ctx), timeoutMs);
  console.error(`[turn-cap] @agent:${agentId} hit maxTurns — ${note}`);
}
