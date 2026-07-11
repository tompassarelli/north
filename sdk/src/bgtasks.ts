// Background-task liveness tracker — the missing signal that kept lanes dying with
// live work (thread 019f4ed2). A lane runs the Claude Agent SDK query() in
// streaming-input mode; when the model calls Bash with run_in_background:true (or
// backgrounds a foreground task with Ctrl+B semantics), the SDK emits a
// `task_started` system message with a task_id, returns the tool_result immediately,
// and the turn continues. The model then ends its turn and the SDK emits a `result`.
//
// THE BUG this kills: spawn.ts/dispatch.ts BREAK the message loop on that first
// `result` (end the channel, unwind the query, exit the process) — so the backgrounded
// task's child processes die with the process. The model ASSUMED interactive-session
// semantics (the background task re-invokes it on exit); in a lane that semantic did
// not survive our early break.
//
// GROUND TRUTH (observed, sdk/scratch-probe-bg.ts against the real SDK): when the
// channel is held OPEN past the first result, the SDK delivers `task_updated`
// {status:completed} + `task_notification` {status:completed} when the task settles,
// then AUTO-CONTINUES the model (fresh init + new turn). The SDK already does the right
// thing; our early break is what threw it away. This tracker lets the loop KNOW when
// tasks are live so it can refuse to break.
//
// Settlement surfaces on EITHER `task_notification` (status completed|failed|stopped)
// OR `task_updated` (patch.status completed|failed|killed) — both were observed to
// carry the terminal signal across two probe runs, so we honor both, idempotently.
// `is_backgrounded` is NOT settlement (the task is still running, just detached).

export type BgEvent = "started" | "settled" | null;

const NOTIF_TERMINAL = new Set(["completed", "failed", "stopped"]);
const UPDATE_TERMINAL = new Set(["completed", "failed", "killed"]);

export interface BgTracker {
  // Fold one SDK message into the live-task set. Returns "started" when a new task
  // entered the set, "settled" when a tracked task left it, null otherwise.
  observe(msg: any): BgEvent;
  live(): string[]; // task_ids still running (insertion order)
  size(): number;
}

export function makeBgTracker(): BgTracker {
  // Map (not Set) so we retain the human-readable command/description for the
  // continuation message — the id alone is opaque to the model being asked to wait.
  const tasks = new Map<string, string>();

  return {
    observe(msg: any): BgEvent {
      if (!msg || msg.type !== "system" || typeof msg.subtype !== "string") return null;
      const id = msg.task_id;
      if (!id) return null;

      if (msg.subtype === "task_started") {
        if (tasks.has(id)) return null; // idempotent — a re-announced start is not new work
        tasks.set(id, msg.description ?? msg.prompt ?? id);
        return "started";
      }
      if (msg.subtype === "task_notification") {
        if (NOTIF_TERMINAL.has(msg.status) && tasks.delete(id)) return "settled";
        return null;
      }
      if (msg.subtype === "task_updated") {
        const st = msg.patch?.status;
        if (typeof st === "string" && UPDATE_TERMINAL.has(st) && tasks.delete(id)) return "settled";
        return null;
      }
      return null;
    },
    live(): string[] {
      return [...tasks.keys()];
    },
    size(): number {
      return tasks.size;
    },
  };
}

// The continuation user-message injected when a lane tries to end its turn with live
// background tasks. It (1) tells the model exactly why it was re-woken and what to do,
// and (2) is itself stream activity — the model's response resets the stall watchdog,
// so a legitimately long wait never reads as a silent hang. Kept terse + imperative.
export function bgContinuationMessage(ids: string[]): string {
  const list = ids.length ? ids.join(", ") : "unknown";
  return (
    `[harness] background task(s) still live (${list}) — a lane cannot exit with ` +
    `tracked work running; sleep-poll until done, consume the result, or kill it.`
  );
}

// Backstop against a stuck lane (model that keeps ending its turn without actually
// waiting, or a task that never settles). Reset to zero on forward progress (a
// settlement), so this counts CONSECUTIVE no-progress refusals, not total waits — a
// long, healthy multi-task run is never capped, a truly wedged one always is.
// Env-tunable; default 5 (thread 019f4ed2 design intent).
export function maxBgContinuations(): number {
  const raw = Number(process.env.NORTH_BG_MAX_CONTINUATIONS);
  return Number.isFinite(raw) && raw > 0 ? Math.floor(raw) : 5;
}
