// Guard-denial telemetry — the missing trail. A PreToolUse authoring guard that
// DENIES a worker's Edit/Write/Bash leaves NO durable record: north-clock-guard and
// code-upstream-guard emit only the deny JSON (no log), stream-writer.ts drops the
// hook output (it forwards only assistant/result/system), and north-mine.clj mines
// denials from INTERACTIVE Claude Code transcripts (~/.claude/projects) — which SDK
// workers never write. So the exact population the guard bridge exists for (worker
// edits — the ~30% of edit volume that ran with ZERO guards before the parity fix)
// is invisible after the fact: "was this worker blocked, how often, on what" cannot
// be learned from the graph.
//
// Fix: on every worker deny, emit a `kind guard_denial` fact — attribution (agent),
// timestamp (at), what was blocked (tool + target), which guard, and the reason.
// Titleless `@denial:<agent>-<ts>` subject (the @run/@mine pattern): queryable via
// fram `ask`/`show`, invisible to the work board. Fire-and-forget + fully swallowed:
// denial telemetry must NEVER add latency to, or break, a tool call.
import { execFile } from "node:child_process";

// Classify a deny reason to the guard that produced it, so analytics can group by
// guard without threading the guard name through the hook protocol (the guards emit
// only a reason string). Substrings are matched against the REAL guard outputs:
//   north-clock-guard.sh -> "Billable client edit blocked …"
//   code-upstream-guard  -> "… GRAPH-OWNED …"
//   firn-guard           -> "BLOCKED: …"
//   tripwire-guard       -> exit-2 stderr (reason carries "tripwire" or the policy line)
// Unknown reasons fall to "other" rather than being dropped — an unclassified deny is
// still a deny worth counting, and a new "other" mass is the signal to add a label.
export function classifyGuard(reason: string): string {
  const r = reason.toLowerCase();
  if (r.includes("billable client edit blocked") || (r.includes("clock") && r.includes("client")))
    return "north-clock-guard";
  if (r.includes("graph-owned") || r.includes("code-upstream")) return "code-upstream-guard";
  if (r.includes("blocked:") || r.includes("firn")) return "firn-guard";
  if (r.includes("tripwire") || r.includes("safe-push") || r.includes("allowlist")) return "tripwire-guard";
  return "other";
}

// PURE: the (subject, facts) a denial emits — split out like death.ts's deathCommands
// and telemetry.ts's recordRun so the contract is testable without shelling out.
// `hookInput` is the CLI hook payload the guard saw: {tool_name, tool_input:{file_path
// |command,...}}. target = the file_path (edits) or the command (Bash), truncated —
// enough to see WHAT was blocked, never a full command dump on the graph.
// NOTE: subject carries NO leading @ — `north tell` prepends it (as telemetry.ts's
// `run-<agent>-<ts>` does), so `denial:…` here lands as `@denial:…` in the log.
export function denialFacts(
  agentId: string,
  reason: string,
  hookInput: unknown,
  ts: string = new Date().toISOString(),
  nowMs: number = Date.now(),
): { subject: string; facts: Array<[string, string]> } {
  const inp = (hookInput ?? {}) as any;
  const tool = String(inp.tool_name ?? "unknown");
  const ti = inp.tool_input ?? {};
  const rawTarget = String(ti.file_path ?? ti.command ?? "");
  const target = rawTarget.replace(/\s+/g, " ").trim().slice(0, 200);
  const subject = `denial:${agentId}-${nowMs.toString(36)}`;
  const facts: Array<[string, string]> = [
    ["kind", "guard_denial"],
    ["agent", agentId],
    ["guard", classifyGuard(reason)],
    ["tool", tool],
    ["at", ts],
  ];
  if (target) facts.push(["target", target]);
  const cleanReason = reason.replace(/\s+/g, " ").trim().slice(0, 200);
  if (cleanReason) facts.push(["reason", cleanReason]);
  return { subject, facts };
}

// Emit the denial telemetry. Fire-and-forget (async execFile, errors swallowed): a
// failure to record a denial must never delay or break the tool call the guard already
// decided. NORTH_BIN override mirrors telemetry.ts/death.ts so tests point at a fake.
export function recordDenial(agentId: string, reason: string, hookInput: unknown): void {
  const north = process.env.NORTH_BIN ?? "north";
  const { subject, facts } = denialFacts(agentId, reason, hookInput);
  for (const [p, v] of facts) {
    try {
      execFile(north, ["tell", subject, p, v], () => {});
    } catch {
      /* swallow — telemetry never breaks a run */
    }
  }
}
