import { spawnSync } from "node:child_process";
import { resolve } from "node:path";

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
  constructor(readonly threadId: string) {
    super(`could not establish the active driver for thread @${threadId}`);
    this.name = "DispatchDriverUnavailableError";
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
): { release(): void } {
  const command = options.command ?? commandAt(options.port ?? process.env.NORTH_PORT ?? "7977");
  const verb = options.preclaimed ?? process.env.NORTH_DISPATCH_DRIVER_PRECLAIMED === "1" ? "verify" : "claim";
  const result = command(verb, threadId, agentId);
  if (result.status === 3) throw new DispatchAlreadyActiveError(threadId);
  if (result.status !== 0) throw new DispatchDriverUnavailableError(threadId);
  let released = false;
  return {
    release: () => {
      if (released) return;
      released = true;
      try { command("release", threadId, agentId); } catch { /* best-effort terminal cleanup */ }
    },
  };
}
