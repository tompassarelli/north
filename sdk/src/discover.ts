// P4 — self-organized discovery. An IDLE agent finds its own work with no human
// and no command: pull ready threads off the fact graph, ATOMICALLY acquire the
// driver (fram-1's P3 lease — race-proof, so N agents never double-drive the same
// thread), dispatch it through the harness, release. Jittered exponential backoff
// on empty/contended rounds so agents desynchronize instead of thundering an empty queue.
//
// This is the pull side of the loop: P1 (reactor) reacts to commands addressed to
// an agent; discover lets an idle agent SELECT its own work. Together: a thread is
// dropped, and whichever agent is free grabs it — leaderless.
import { execSync, execFileSync } from "node:child_process";
import { resolve } from "node:path";
import { dispatch } from "./dispatch";
import { ProviderSelectionError } from "./provider-routing";

const REPO = resolve(import.meta.dir, "../..");
const ACQUIRE_CLI = `${REPO}/cli/acquire-cli.clj`;
const PORT = process.env.NORTH_PORT ?? "7977";

export interface ReadyThread {
  id: string;
  title: string;
  condition: string;
}

// Unblocked, committed, undriven work off the fact graph.
function readyThreads(): ReadyThread[] {
  try {
    // --all: discovery needs the FULL ready set to pick from, not the curated
    // top-15 slice the JSON default now returns (parity with the MCP/CLI edge).
    const rows = JSON.parse(execSync("north json ready --all", { encoding: "utf8", timeout: 8000 }).trim());
    return Array.isArray(rows) ? rows : [];
  } catch {
    return [];
  }
}

// Atomic driver acquire (fram-1's P3 lease). True iff THIS agent won; false = a peer
// holds it (acquire-cli exits 1 + prints DENIED). The race agentchat never closed.
function acquireDriver(thread: string, holder: string): boolean {
  try {
    execFileSync("bb", [ACQUIRE_CLI, PORT, "acquire", thread, holder], { stdio: "pipe" });
    return true;
  } catch {
    return false;
  }
}

function releaseDriver(thread: string, holder: string): void {
  try {
    execFileSync("bb", [ACQUIRE_CLI, PORT, "release", thread, holder], { stdio: "pipe" });
  } catch {}
}

export interface DiscoverOpts {
  maxTasks?: number; // stop after N completed (default: unbounded)
  maxEmptyRounds?: number; // stop after N consecutive unsuccessful rounds (default 5)
}

export interface DiscoverDependencies {
  readyThreads: () => ReadyThread[];
  acquireDriver: (thread: string, holder: string) => boolean;
  releaseDriver: (thread: string, holder: string) => void;
  dispatch: (thread: string) => Promise<unknown>;
  sleep: (ms: number) => Promise<void>;
  random: () => number;
}

const defaultDependencies: DiscoverDependencies = {
  readyThreads,
  acquireDriver,
  releaseDriver,
  dispatch,
  sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)),
  random: Math.random,
};

function dispatchFailureReason(error: unknown): string {
  if (error instanceof ProviderSelectionError)
    return `provider routing ${error.kind.replaceAll("_", " ")} before side effects`;
  return "dispatch failed";
}

function errorSummary(error: unknown): string {
  return String(error instanceof Error ? error.message : error)
    .replace(/\s+/g, " ")
    .trim()
    .slice(0, 500) || "unknown error";
}

// The pull loop. Returns the thread ids it drove to completion.
export async function discover(
  self: string,
  opts: DiscoverOpts = {},
  overrides: Partial<DiscoverDependencies> = {},
): Promise<string[]> {
  const dependencies = { ...defaultDependencies, ...overrides };
  const maxTasks = opts.maxTasks ?? Infinity;
  const maxEmpty = opts.maxEmptyRounds ?? 5;
  const done: string[] = [];
  let unsuccessfulRounds = 0;

  const backoff = async (why: string) => {
    unsuccessfulRounds++;
    const base = Math.min(30_000, 1_000 * 2 ** unsuccessfulRounds);
    const ms = Math.round(base * (0.5 + dependencies.random()));
    console.log(`[discover] ${self} ${why} (${unsuccessfulRounds}/${maxEmpty}) — backoff ${ms}ms`);
    await dependencies.sleep(ms);
  };

  while (done.length < maxTasks && unsuccessfulRounds < maxEmpty) {
    let acquired = false;
    for (const t of dependencies.readyThreads()) {
      if (!dependencies.acquireDriver(t.id, self)) continue; // peer got it — try the next ready thread
      acquired = true;
      console.log(`[discover] ${self} acquired ${t.id} — ${t.title}`);
      let failure: unknown;
      let failed = false;
      try {
        await dependencies.dispatch(t.id);
        done.push(t.id);
        unsuccessfulRounds = 0;
        console.log(`[discover] ${self} finished ${t.id} (${done.length} total)`);
      } catch (e) {
        failed = true;
        failure = e;
        console.error(`[discover] ${self} dispatch of ${t.id} failed: ${errorSummary(e)}`);
      } finally {
        dependencies.releaseDriver(t.id, self);
      }
      if (failed) await backoff(dispatchFailureReason(failure));
      break; // re-poll fresh — the graph moved
    }
    if (!acquired) await backoff("no acquirable work");
  }
  console.log(`[discover] ${self} exiting — drove ${done.length} thread(s)`);
  return done;
}

if (import.meta.main) {
  const self = process.env.AGENT_ID ?? `sdk-disc-${Date.now().toString(36).slice(-6)}`;
  const maxTasks = process.env.DISCOVER_MAX ? Number(process.env.DISCOVER_MAX) : undefined;
  discover(self, { maxTasks }).catch((e) => {
    console.error(e);
    process.exit(1);
  });
}
