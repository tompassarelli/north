import { execFileSync } from "node:child_process";
import { resolve } from "node:path";
import type { ExecutionTerminal } from "./execution-outcome";
import type { TerminalPublicationStatus } from "./identity";
import type { RunPublicationStatus } from "./telemetry";

const REPO = resolve(import.meta.dir, "..", "..");
const MSG_CLI = `${REPO}/cli/msg-cli.clj`;
const port = () => process.env.NORTH_PORT ?? "7977";
const peerBb = () => process.env.NORTH_PEER_BB ?? "bb";
// A run record carries ~200 facts and the writer issues ONE coordinator
// round-trip per fact, so this budget is really "200 sequential writes, minus a
// peer-wake reserve, split across stages". At the old 10s default that left
// ~3.9s for ~202 writes — about 19ms each — which the coordinator cannot meet
// under any write churn. Measured: lane-ms0f3ak0 died with "writer exceeded
// 3876ms budget and was killed; 202 facts = 202 coordinator writes", and
// telemetry was lost on 170 of 765 runs for exactly this reason.
//
// This is terminal FINALIZATION with no interactive consumer waiting, so give it
// room. The right long-term fix is a batch assert so one record is one op rather
// than 200; until the coordinator protocol has one, the budget must match what
// the work actually costs.
const DEFAULT_PUBLICATION_BUDGET_MS = 90_000;
const MIN_PUBLICATION_BUDGET_MS = 100;
const MAX_PUBLICATION_BUDGET_MS = 300_000;

type Command = { cmd: string; args: string[] };

export interface TerminalNotification {
  outcome: string;
  terminal: ExecutionTerminal;
  terminalPublication: TerminalPublicationStatus;
  runPublication: RunPublicationStatus;
  detail?: string;
  subject?: string;
}

export function terminalPublicationBudgetMs(raw = process.env.NORTH_TERMINAL_PUBLICATION_BUDGET_MS): number {
  if (raw === undefined) return DEFAULT_PUBLICATION_BUDGET_MS;
  const value = Number(raw);
  if (!Number.isSafeInteger(value) || value <= 0) return DEFAULT_PUBLICATION_BUDGET_MS;
  return Math.min(MAX_PUBLICATION_BUDGET_MS, Math.max(MIN_PUBLICATION_BUDGET_MS, value));
}

/**
 * One wall-clock budget covers terminal publication, run publication, and the
 * peer wake. Publication stages split the non-peer remainder fairly; a slow
 * first stage therefore cannot consume the wake-up's reserved final slice.
 */
export class TerminalPublicationBudget {
  private readonly startedAt: number;
  private readonly peerReserveMs: number;

  constructor(
    readonly totalMs = terminalPublicationBudgetMs(),
    private readonly now: () => number = () => performance.now(),
  ) {
    this.startedAt = this.now();
    this.peerReserveMs = Math.max(1, Math.floor(totalMs / 5));
  }

  publicationTimeout(stagesRemaining: number): number {
    const elapsed = Math.max(0, this.now() - this.startedAt);
    const available = Math.max(1, this.totalMs - elapsed - this.peerReserveMs);
    return Math.max(1, Math.floor(available / Math.max(1, stagesRemaining)));
  }

  notificationTimeout(): number {
    const elapsed = Math.max(0, this.now() - this.startedAt);
    return Math.max(1, Math.floor(this.totalMs - elapsed));
  }
}

function defaultSubject(outcome: string, terminal: ExecutionTerminal): string {
  if (outcome === "died" || outcome === "stalled" || outcome === "watchdog_aborted")
    return "AGENT DEATH";
  if (outcome === "max_turns" || outcome === "capped") return "TURN CAP";
  if (outcome === "ran_empty") return "AGENT EMPTY RESULT";
  return terminal.deliveryOutcome === "blocked" ? "AGENT BLOCKED" : "AGENT COMPLETE";
}

function boundedDetail(detail?: string): string | undefined {
  const value = detail?.replace(/\s+/g, " ").trim().slice(0, 500);
  return value || undefined;
}

export function terminalNotificationCommand(
  agentId: string,
  coordinator: string | undefined,
  notification: TerminalNotification,
): Command | undefined {
  if (!coordinator) return undefined;
  const detail = boundedDetail(notification.detail);
  const body = [
    detail,
    `process=${notification.terminal.processOutcome}`,
    `delivery=${notification.terminal.deliveryOutcome}`,
    `terminal=${notification.terminalPublication}`,
    `run=${notification.runPublication}`,
  ].filter(Boolean).join(" — ");
  return {
    cmd: peerBb(),
    args: [
      MSG_CLI,
      port(),
      "send",
      agentId,
      coordinator,
      notification.subject ?? defaultSubject(notification.outcome, notification.terminal),
      body,
    ],
  };
}

export function notifyTerminalSettlement(
  agentId: string,
  coordinator: string | undefined,
  notification: TerminalNotification,
  timeoutMs = 10_000,
): void {
  const command = terminalNotificationCommand(agentId, coordinator, notification);
  if (!command) return;
  try {
    execFileSync(command.cmd, command.args, {
      encoding: "utf8",
      timeout: Math.max(1, Math.floor(timeoutMs)),
      stdio: ["ignore", "ignore", "ignore"],
    });
  } catch {
    // Publication settlement remains authoritative. Notification is only a
    // wake-up and never replaces the lane's real execution outcome.
  }
}
