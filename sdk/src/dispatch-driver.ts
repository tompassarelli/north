import { spawnSync } from "node:child_process";
import { resolve } from "node:path";
import {
  beagleStoreBabashkaArguments,
  beagleStoreCoordinatorChildTimeout,
  beagleStoreEnvironment,
} from "./beagle-store";
import { normalizeNorthEntityId, northEntitySubject } from "./north-client";

const REPO_ROOT = resolve(import.meta.dir, "..", "..");
const ACQUIRE_CLI = resolve(REPO_ROOT, "cli/acquire-cli.clj");

/** Named so the failure report can cite the budget it actually blew. */
const DRIVER_TIMEOUT_MS = 30_000;

export class DispatchAlreadyActiveError extends Error {
  readonly preSideEffect = true;
  constructor(readonly threadId: string) {
    super(`thread @${threadId} already has an active driver`);
    this.name = "DispatchAlreadyActiveError";
  }
}

export class DispatchDriverUnavailableError extends Error {
  readonly preSideEffect = true;
  constructor(readonly threadId: string, readonly port = "7977") {
    super(
      `North coordinator unavailable or mismatched at port ${port} ` +
      `while establishing the active driver for thread @${threadId}`,
    );
    this.name = "DispatchDriverUnavailableError";
  }
}

export class DispatchDriverPreclaimAbsentError extends Error {
  readonly preSideEffect = true;
  constructor(readonly threadId: string) {
    super(`MCP-preclaimed driver for thread @${threadId} is absent during SDK startup`);
    this.name = "DispatchDriverPreclaimAbsentError";
  }
}

export class DispatchDriverPreclaimMismatchError extends Error {
  readonly preSideEffect = true;
  constructor(readonly threadId: string) {
    super(`MCP-preclaimed driver for thread @${threadId} is held by a different adapter`);
    this.name = "DispatchDriverPreclaimMismatchError";
  }
}

export class DispatchDriverReleaseError extends Error {
  readonly preSideEffect = false;
  readonly retrySafe = false;
  constructor(readonly threadId: string) {
    super(`could not safely release the active driver for thread @${threadId}`);
    this.name = "DispatchDriverReleaseError";
  }
}

export interface DispatchDriverCommandResult { status: number | null }
export type DispatchDriverCommand = (verb: "claim" | "verify" | "release", threadId: string, agentId: string) => DispatchDriverCommandResult;

export interface DispatchDriverOptions {
  preclaimed?: boolean;
  command?: DispatchDriverCommand;
  port?: string;
}

/**
 * Why a driver claim failed, on stderr — the lane's durable log.
 *
 * A lane died on 2026-07-29 showing the operator exactly this and nothing more:
 *
 *     [death] @agent:lane-… died: spawnSync /nix/store/…/bb ETIMEDOUT
 *
 * The real cause was that a `shell.readonly` template gets a read-only sandbox,
 * which blocks `:7977`, so the claim HUNG until the 8s budget expired. Confirmed
 * by controlled experiment: the same probe as `scout` (read-only) died, as
 * `implementer` (workspace-write) ran. The budget was never the issue — that
 * call measures 63-64ms against a healthy coordinator, 125x headroom.
 *
 * ETIMEDOUT therefore gets an explicit hypothesis rather than a bare errno.
 * Every orchestrator template carries `shell.readonly`, so this is the default
 * experience of anything that coordinates.
 *
 * Never throws: this runs on an already-failing path, and a reporter that can
 * fail is worse than the silence it replaces.
 */
function reportDriverFailure(
  verb: string,
  port: string,
  result: { status?: number | null; error?: Error; stderr?: string },
): void {
  try {
    // Deliberately NOT result.stderr. The existing canary — "coordinator stderr
    // must never cross boundary" — asserts only that it stays out of the thrown
    // message, but its intent is broader, and the diagnostic value here never
    // came from the coordinator's own text. It came from the errno and from
    // knowing what an ETIMEDOUT implies. Report harness facts, not corpus
    // content: status and errno are produced by the spawn, not the coordinator.
    const code = (result.error as NodeJS.ErrnoException | undefined)?.code;
    const parts = [
      `[north] dispatch driver ${verb} failed on :${port}`,
      result.status != null ? `status=${result.status}` : undefined,
      code ? `errno=${code}` : undefined,
    ].filter(Boolean);
    process.stderr.write(`${parts.join(" ")}\n`);
    if (code === "ETIMEDOUT")
      process.stderr.write(
        `[north] the claim HUNG rather than being refused. On a healthy ` +
        `coordinator it takes ~64ms, so a ${DRIVER_TIMEOUT_MS}ms timeout means ` +
        `the socket never answered. Most likely this lane's template carries ` +
        `shell.readonly, whose read-only sandbox blocks :${port} — every ` +
        `orchestrator template does. Check the "sandbox=" line in this log.\n`,
      );
  } catch {
    // A diagnostic must never become the failure.
  }
}

function commandAt(port: string): DispatchDriverCommand {
  return (verb, threadId, agentId) => spawnSync(
    "bb", beagleStoreBabashkaArguments([ACQUIRE_CLI, port, verb, threadId, agentId]),
    {
      encoding: "utf8",
      env: beagleStoreEnvironment(),
      stdio: "pipe",
      timeout: beagleStoreCoordinatorChildTimeout(DRIVER_TIMEOUT_MS),
    },
  );
}

/**
 * Atomically become the sole active driver, or verify an MCP-owned handoff.
 * No command output crosses this boundary: driver failures remain fixed and
 * cannot leak coordinator diagnostics into model-visible errors.
 */
export function claimDispatchDriver(
  threadId: string,
  agentId: string,
  options: DispatchDriverOptions = {},
): { release(): boolean } {
  const canonicalThreadId = normalizeNorthEntityId(threadId);
  const threadSubject = northEntitySubject(canonicalThreadId);
  const port = options.port ?? process.env.NORTH_PORT ?? "7977";
  const command = options.command ?? commandAt(port);
  const preclaimed =
    options.preclaimed ?? process.env.NORTH_DISPATCH_DRIVER_PRECLAIMED === "1";
  const verb = preclaimed ? "verify" : "claim";
  const result = command(verb, threadSubject, agentId);
  if (!preclaimed && result.status === 3)
    throw new DispatchAlreadyActiveError(canonicalThreadId);
  if (preclaimed && result.status === 6)
    throw new DispatchDriverPreclaimAbsentError(canonicalThreadId);
  if (preclaimed && (result.status === 3 || result.status === 7))
    throw new DispatchDriverPreclaimMismatchError(canonicalThreadId);
  if (result.status !== 0) {
    // The THROWN error stays fixed — coordinator diagnostics must not reach the
    // model. But the OPERATOR needs them, and suppressing both is what made this
    // failure mode opaque for so long. Split the boundary: fixed for the model,
    // explicit on stderr, which is the lane's durable log.
    reportDriverFailure(verb, port, result);
    throw new DispatchDriverUnavailableError(canonicalThreadId, port);
  }
  let released = false;
  return {
    release: () => {
      if (released) return true;
      try {
        const release = command("release", threadSubject, agentId);
        released = release.status === 0;
        return released;
      } catch {
        return false;
      }
    },
  };
}
