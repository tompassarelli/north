// Auto-clock lifecycle for dispatched work — the billing side of per-agent clocks.
// A worker's time is logged against ITS thread as ITS agent id (clocked_by), so N
// parallel workers each get their own session instead of one global clock soaking
// up every hour (the failure that collapsed per-ticket attribution: 15 tickets
// touched, one thread absorbed 5h23m). The north CLI resolves the agent from
// NORTH_AGENT_ID, so we pin it per call.
//
// Best-effort + SYNCHRONOUS (execFileSync): a clock write must never reject the run
// nor throw out of a finally, and a death path must flush before the process exits.
// All output swallowed; a missing coordinator is handled by the north CLI itself.
import { execFileSync } from "node:child_process";
import { resolve } from "node:path";

const REPO = resolve(import.meta.dir, "..", "..");
// NORTH_BIN override mirrors death.ts, so tests can point at a fake.
const northBin = () => process.env.NORTH_BIN ?? `${REPO}/bin/north`;

export type ClockAction = "start" | "stop" | "orphan";

// PURE: the north argv (+ the agent-id to pin as NORTH_AGENT_ID, if any) a clock
// action issues. Split out like death.ts's deathCommands so the contract is testable
// without shelling out. `start`/`stop` pin NORTH_AGENT_ID so the CLI resolves the
// caller's identity; `orphan` names the agent EXPLICITLY in argv (a reaper closes on
// behalf of a dead agent, so its own identity must not leak in via env).
export function clockCommand(
  action: ClockAction,
  agentId: string,
  thread?: string,
): { args: string[]; agentEnv?: string } {
  switch (action) {
    case "start":
      return { args: ["clock", "start", thread ?? ""], agentEnv: agentId };
    case "stop":
      return { args: ["clock", "stop"], agentEnv: agentId };
    case "orphan":
      return { args: ["clock", "orphan", agentId] };
  }
}

// PURE: the terminal clock action for a finalized run — crash outcomes orphan-close
// (flag the untrustworthy tail), everything else (clean or turn-capped)
// closes normally so real time still bills.
export function finalizeAction(outcome: string): ClockAction {
  return outcome === "died" || outcome === "stalled" ? "orphan" : "stop";
}

function runNorth(cmd: { args: string[]; agentEnv?: string }): void {
  try {
    execFileSync(northBin(), cmd.args, {
      encoding: "utf8",
      timeout: 10_000,
      stdio: ["ignore", "ignore", "ignore"],
      env: cmd.agentEnv ? { ...process.env, NORTH_AGENT_ID: cmd.agentEnv } : process.env,
    });
  } catch {
    /* best-effort: never break the run on a clock write */
  }
}

// Open a per-agent session on `thread`, clocked_by=agentId. The CLI denies only if
// THIS agent already has an open session, so it never blocks across agents. `thread`
// is the bare id (the CLI adds the @).
export function clockStart(agentId: string, thread: string): void {
  runNorth(clockCommand("start", agentId, thread));
}

// Close THIS agent's open session normally (clean exit / turn-cap — the time is real).
export function clockStop(agentId: string): void {
  runNorth(clockCommand("stop", agentId));
}

// Crash-honesty: close the agent's orphan session, stamping end_time at detection +
// clock_orphaned so audits distrust the untimed tail without dropping the work. No-op
// if the agent has no open session; idempotent, so a death path and a reaper may both
// fire it.
export function clockOrphan(agentId: string): void {
  runNorth(clockCommand("orphan", agentId));
}

// The terminal clock action for a finalized run (see finalizeAction).
export function clockFinalize(agentId: string, outcome: string): void {
  runNorth(clockCommand(finalizeAction(outcome), agentId));
}
