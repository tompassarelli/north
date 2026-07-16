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
// A child is RESOLVED (not orphaned) if it carries a completion signal, mirroring the
// reactor's join (north.reap): recordRun lands `outcome` on a @run-<h>-<ts> subject
// (`agent <h>`), so a clean child's @agent:<id> outcome is usually empty — we must check
// BOTH the child's own outcome fact AND any @run tagged with it. Everything here is
// FAIL-OPEN: a query hiccup must never block or throw out of a finalizing lane.
import { execFileSync } from "node:child_process";
import { resolve } from "node:path";
import { getThreadFacts } from "./north-client";

const REPO = resolve(import.meta.dir, "..", "..");
const MSG_CLI = `${REPO}/cli/msg-cli.clj`;
const northBin = () => process.env.NORTH_BIN ?? `${REPO}/bin/north`;
const port = () => process.env.NORTH_PORT ?? "7977";

// Run a single-column rules query against the engine; return the bare string values
// (rows arrive as JSON arrays like ["@agent:x"]). Fail-open to [].
function queryCol(rules: unknown): string[] {
  try {
    const out = execFileSync(northBin(), ["query", JSON.stringify(rules)], {
      encoding: "utf8",
      timeout: 5000,
      stdio: ["ignore", "pipe", "ignore"],
    });
    return out
      .split("\n")
      .map((l) => l.trim())
      .filter((l) => l.startsWith("["))
      .map((l) => {
        try {
          const row = JSON.parse(l);
          return Array.isArray(row) ? String(row[0]) : "";
        } catch {
          return "";
        }
      })
      .filter(Boolean);
  } catch {
    return [];
  }
}

const oneColRule = (bindPred: string, subj: string, pred: string, val: string) => ({
  find: bindPred,
  rules: [
    {
      head: { rel: bindPred, args: [{ var: bindPred }] },
      body: [{ rel: "triple", args: [subj === "?" ? { var: bindPred } : subj, pred, val === "?" ? { var: "_v" } : val] }],
    },
  ],
});

// Agents whose `coordinator` fact points at this lane.
function childrenOf(coordId: string): string[] {
  return queryCol(oneColRule("c", "?", "coordinator", coordId));
}

// Does this child (subject literal, e.g. "@agent:x") carry a completion signal?
function childResolved(childSubject: string): boolean {
  const bare = childSubject.replace(/^@?agent:/, "");
  // (a) the child's own outcome fact (reactor writes died-unreported here; a lane may
  //     also carry an explicit terminal outcome).
  try {
    if (getThreadFacts(`agent:${bare}`).some((f) => f.predicate === "outcome")) return true;
  } catch {
    /* fall through to the run check */
  }
  // (b) a @run tagged with this agent that recorded an outcome (the normal clean path).
  const runs = queryCol({
    find: "r",
    rules: [
      {
        head: { rel: "r", args: [{ var: "r" }] },
        body: [
          { rel: "triple", args: [{ var: "r" }, "agent", bare] },
          { rel: "triple", args: [{ var: "r" }, "outcome", { var: "_o" }] },
        ],
      },
    ],
  });
  return runs.length > 0;
}

// Children of this lane that have NOT reported an outcome — the orphans left behind if
// the lane exits now. Fail-open: any error yields [] (no false alarm).
export function liveChildren(coordId: string): string[] {
  if (!coordId) return [];
  const kids = childrenOf(coordId);
  if (!kids.length) return [];
  return kids.filter((c) => !childResolved(c));
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
export function notifyEarlyExitChildren(agentId: string, liveIds: string[], ctx: EarlyExitCtx = {}): void {
  if (!liveIds.length) return;
  for (const { cmd, args } of earlyExitCommands(agentId, liveIds, ctx)) {
    try {
      execFileSync(cmd, args, { encoding: "utf8", timeout: 10_000, stdio: ["ignore", "ignore", "ignore"] });
    } catch {
      /* best-effort */
    }
  }
  console.error(`[early-exit] @agent:${agentId} EXITING WITH ${liveIds.length} LIVE CHILD(REN): ${liveIds.join(", ")}`);
}
