// Per-agent billing clocks have two deliberately different reliability contracts.
// Required client admission is synchronous and fail-closed before provider/identity
// work: the branch ticket, North thread, owner, start acknowledgement, and exact
// live readback must all agree. Terminal stop/orphan cleanup remains synchronous
// best-effort: it must flush before exit but must never replace the run outcome.
// Every command pins NORTH_AGENT_ID so parallel workers are attributed separately.
import { execFileSync } from "node:child_process";
import { resolve } from "node:path";
import { getThreadFacts, type Fact } from "./north-client";
import {
  trustedGitBranchName, trustedGitExecutable, trustedGitProjectRoot,
} from "./trusted-runtime";
export { trustedGitExecutable } from "./trusted-runtime";

const REPO = resolve(import.meta.dir, "..", "..");
// NORTH_BIN override mirrors death.ts, so tests can point at a fake.
const northBin = () => process.env.NORTH_BIN ?? `${REPO}/bin/north`;

export type ClockAction = "start" | "stop" | "orphan";
export type BillableClockPreflightCode =
  | "billable_thread_required"
  | "billable_ticket_required"
  | "billable_thread_owner_unavailable"
  | "billable_thread_owner_mismatch"
  | "billable_thread_linear_mismatch"
  | "billable_clock_start_failed"
  | "billable_clock_readback_failed";

export class BillableClockPreflightError extends Error {
  readonly preSideEffect = true;

  constructor(readonly code: BillableClockPreflightCode) {
    super(code);
    this.name = "BillableClockPreflightError";
  }
}

export type BillableClockAdmission =
  | { kind: "not-required" }
  | {
      kind: "opened";
      agentId: string;
      client?: string;
      threadId: string;
    };

export interface BillableClockRuntime {
  projectRoot?: (cwd: string) => string;
  branchName?: (projectRoot: string) => string;
  gitExecutable?: () => string;
  readThreadFacts?: (threadId: string) => Fact[];
  execute?: (
    command: { args: string[]; agentEnv?: string },
  ) => string;
}

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

function runNorthCapture(
  cmd: { args: string[]; agentEnv?: string },
): string {
  return execFileSync(northBin(), cmd.args, {
    encoding: "utf8",
    timeout: 10_000,
    maxBuffer: 64 * 1024,
    stdio: ["ignore", "pipe", "pipe"],
    env: cmd.agentEnv
      ? { ...process.env, NORTH_AGENT_ID: cmd.agentEnv }
      : process.env,
  });
}

function runNorth(cmd: { args: string[]; agentEnv?: string }): void {
  try {
    runNorthCapture(cmd);
  } catch {
    /* best-effort: never break the run on a clock write */
  }
}

function defaultProjectRoot(cwd: string, gitExecutable: string): string {
  return trustedGitProjectRoot(cwd, gitExecutable);
}

/** Exact client identity encoded by a canonical project root, or undefined. */
export function clientOwnerForProjectRoot(projectRoot: string): string | undefined {
  const match = /(?:^|\/)code\/client\/([^/]+)(?:\/|$)/.exec(projectRoot);
  return match?.[1] || undefined;
}

function defaultBranchName(projectRoot: string, gitExecutable: string): string {
  return trustedGitBranchName(projectRoot, gitExecutable);
}

/** Canonical Linear ticket carried by a client branch, e.g. msa-242-x -> MSA-242. */
export function clientTicketForBranch(
  branchName: string,
  client: string,
): string | undefined {
  const escapedClient = client.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const candidates = [...branchName.matchAll(
    new RegExp(`${escapedClient}-[0-9]+`, "ig"),
  )].filter((match) => {
    const start = match.index ?? -1;
    const end = start + match[0].length;
    return start >= 0
      && (start === 0 || /[/_-]/.test(branchName[start - 1]!))
      && (end === branchName.length || /[/_-]/.test(branchName[end]!));
  });
  return candidates.length === 1 ? candidates[0]![0].toUpperCase() : undefined;
}

function exactStartProof(output: string, threadId: string, agentId: string): boolean {
  const line = output.split(/\r?\n/, 1)[0] ?? "";
  return line.startsWith(`clocked in on ${threadId} at `)
    && line.endsWith(`, agent ${agentId})`);
}

function exactStatusProof(output: string, threadId: string, agentId: string): boolean {
  const line = output.split(/\r?\n/, 1)[0] ?? "";
  return line.startsWith(`clocked in on ${threadId}  `)
    && line.endsWith(`  (agent ${agentId})`);
}

function exactAbsentStatusProof(output: string, agentId: string): boolean {
  return (output.split(/\r?\n/, 1)[0] ?? "")
    === `not clocked in (agent ${agentId})`;
}

/**
 * Required pre-provider billing admission. Non-client or read-only work is
 * unchanged. A write-capable client lane must bind a same-owner thread, open
 * its own clock, and read that exact agent/thread session back before any model
 * or identity side effect. If readback fails after a successful start, close
 * only the clock this attempt opened before rejecting.
 */
export function admitBillableClock(
  input: {
    agentId: string;
    capabilities: readonly string[];
    cwd: string;
    threadId?: string;
  },
  runtime: BillableClockRuntime = {},
): BillableClockAdmission {
  let resolvedGit: string | undefined;
  const gitExecutable = () => {
    if (!resolvedGit) {
      resolvedGit = runtime.gitExecutable
        ? trustedGitExecutable([runtime.gitExecutable()])
        : trustedGitExecutable();
    }
    return resolvedGit;
  };
  const projectRoot = runtime.projectRoot
    ? runtime.projectRoot(input.cwd)
    : defaultProjectRoot(input.cwd, gitExecutable());
  const client = clientOwnerForProjectRoot(projectRoot);
  const required = input.capabilities.includes("filesystem.write")
    && client !== undefined;
  if (!input.threadId) {
    if (required)
      throw new BillableClockPreflightError("billable_thread_required");
    return { kind: "not-required" };
  }
  if (required) {
    let ticket: string | undefined;
    try {
      ticket = clientTicketForBranch(
        runtime.branchName
          ? runtime.branchName(projectRoot)
          : defaultBranchName(projectRoot, gitExecutable()),
        client,
      );
    } catch {
      ticket = undefined;
    }
    if (!ticket)
      throw new BillableClockPreflightError("billable_ticket_required");
    let facts: Fact[];
    try {
      facts = (runtime.readThreadFacts ?? getThreadFacts)(input.threadId);
    } catch {
      throw new BillableClockPreflightError(
        "billable_thread_owner_unavailable",
      );
    }
    const owners = facts
      .filter(({ predicate }) => predicate === "owner")
      .map(({ value }) => value);
    if (owners.length !== 1 || owners[0] !== client)
      throw new BillableClockPreflightError("billable_thread_owner_mismatch");
    const linearTickets = facts
      .filter(({ predicate }) => predicate === "linear")
      .map(({ value }) => value);
    if (linearTickets.length !== 1 || linearTickets[0] !== ticket)
      throw new BillableClockPreflightError("billable_thread_linear_mismatch");
  }

  const execute = runtime.execute ?? runNorthCapture;
  const status = { args: ["clock", "status"], agentEnv: input.agentId };
  try {
    if (!exactAbsentStatusProof(execute(status), input.agentId)) {
      if (required)
        throw new BillableClockPreflightError("billable_clock_readback_failed");
      return { kind: "not-required" };
    }
  } catch (error) {
    if (error instanceof BillableClockPreflightError) throw error;
    if (required)
      throw new BillableClockPreflightError("billable_clock_readback_failed");
    return { kind: "not-required" };
  }
  const start = clockCommand("start", input.agentId, input.threadId);
  let started = false;
  let ambiguousStart = false;
  try {
    const output = execute(start);
    started = exactStartProof(output, input.threadId, input.agentId);
    ambiguousStart = !started
      && !/^(?:already clocked in|no such thread:)/.test(output);
  } catch {
    ambiguousStart = true;
  }
  if (!started) {
    if (ambiguousStart) {
      try {
        const current = execute(status);
        if (exactStatusProof(current, input.threadId, input.agentId))
          execute(clockCommand("stop", input.agentId));
      } catch {
        // Without exact readback this attempt cannot safely claim or close a clock.
      }
    }
    if (required)
      throw new BillableClockPreflightError("billable_clock_start_failed");
    return { kind: "not-required" };
  }

  try {
    if (exactStatusProof(
      execute(status),
      input.threadId,
      input.agentId,
    )) {
      return {
        kind: "opened",
        agentId: input.agentId,
        ...(client ? { client } : {}),
        threadId: input.threadId,
      };
    }
  } catch {
    // Cleanup below owns the clock this attempt positively started.
  }
  try { execute(clockCommand("stop", input.agentId)); } catch { /* best effort */ }
  if (required)
    throw new BillableClockPreflightError("billable_clock_readback_failed");
  return { kind: "not-required" };
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
