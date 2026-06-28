// Telemetry auto-capture — write each agent run's tuple as claims so the system
// has a queryable feedback loop (calibrate estimates against actuals, see who ran
// what at what cost). Records to a dedicated `run-<agent>-<ts>` subject that has
// NO title, so runs never show up as threads on the board — they're queryable via
// fram, invisible to the work views. Fire-and-forget: telemetry must NEVER block
// or fail an agent run, so writes are async and all errors are swallowed.
import { execFile } from "node:child_process";

export interface RunRecord {
  thread: string; // the thread driven, or "(ad-hoc)" for a bare spawn
  agent: string; // agent id / handle
  tokens: number; // total tokens this run (from tokensOf)
  durationMs: number; // SDK result duration_ms
  posture: string; // unplanned | atomic | composite | spawn
  outcome: string; // "ran" | "error"
}

export function recordRun(rec: RunRecord): void {
  // base36 ms suffix keeps the id unique per agent without a clock dependency the
  // board cares about; this is runtime code, not a workflow script, so Date is fine.
  const id = `run-${rec.agent}-${Date.now().toString(36)}`;
  const claims: Array<[string, string]> = [
    ["kind", "run"],
    ["thread", rec.thread],
    ["agent", rec.agent],
    ["tokens", String(Math.round(rec.tokens))],
    ["duration_ms", String(Math.round(rec.durationMs))],
    ["posture", rec.posture],
    ["outcome", rec.outcome],
    ["at", new Date().toISOString()],
  ];
  for (const [p, v] of claims) {
    // async + ignored: never let telemetry add latency to, or break, the run.
    try {
      execFile("lodestar", ["tell", id, p, v], () => {});
    } catch {
      /* swallow */
    }
  }
}
