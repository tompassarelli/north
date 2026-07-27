import { spawnSync } from "node:child_process";
import { resolve } from "node:path";
import { normalizeNorthEntityId, northEntitySubject } from "./north-client";

const REPO_ROOT = resolve(import.meta.dir, "..", "..");
const ACQUIRE_CLI = resolve(REPO_ROOT, "cli/acquire-cli.clj");

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

function commandAt(port: string): DispatchDriverCommand {
  return (verb, threadId, agentId) => spawnSync(
    "bb", [ACQUIRE_CLI, port, verb, threadId, agentId],
    { encoding: "utf8", stdio: "pipe", timeout: 8_000 },
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
  if (result.status !== 0)
    throw new DispatchDriverUnavailableError(canonicalThreadId, port);
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
