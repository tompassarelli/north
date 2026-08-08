import { homedir } from "node:os";
import { join } from "node:path";

// The runtime selector is stable while promotions replace its current target.
// Keep each selector independently overrideable so explicit caller environment wins.
export const FRAM_RUNTIME_HOME = join(
  homedir(), ".local/state/north/fram-runtime/active/current",
);

export const MIN_FRAM_COORDINATOR_CHILD_TIMEOUT_MS = 30_000;

export interface FramEngineSelection {
  home: string;
  bin: string;
  out: string;
}

function selected(value: string | undefined, fallback: string): string {
  return value === undefined || value.length === 0 ? fallback : value;
}

export function framEngineSelection(
  env: NodeJS.ProcessEnv = process.env,
): FramEngineSelection {
  return {
    home: selected(env.FRAM_HOME, FRAM_RUNTIME_HOME),
    bin: selected(env.FRAM_BIN, join(FRAM_RUNTIME_HOME, "bin")),
    out: selected(env.FRAM_OUT, join(FRAM_RUNTIME_HOME, "out")),
  };
}

export function framExecutable(
  env: NodeJS.ProcessEnv = process.env,
): string {
  return join(framEngineSelection(env).bin, "fram");
}

export function framEngineEnvironment(
  env: NodeJS.ProcessEnv = process.env,
): NodeJS.ProcessEnv {
  const selection = framEngineSelection(env);
  return {
    ...env,
    FRAM_HOME: selection.home,
    FRAM_BIN: selection.bin,
    FRAM_OUT: selection.out,
  };
}

export function framBabashkaArguments(
  args: readonly string[],
  env: NodeJS.ProcessEnv = process.env,
): string[] {
  return ["-cp", framEngineSelection(env).out, ...args];
}

export function framCoordinatorChildTimeout(timeoutMs?: number): number {
  if (timeoutMs === undefined || !Number.isFinite(timeoutMs))
    return MIN_FRAM_COORDINATOR_CHILD_TIMEOUT_MS;
  return Math.max(
    MIN_FRAM_COORDINATOR_CHILD_TIMEOUT_MS,
    Math.floor(timeoutMs),
  );
}
