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

export interface FramCoordinatorChild {
  readonly exited: Promise<number>;
  kill(signal: "SIGTERM" | "SIGKILL"): unknown;
}

export interface FramCoordinatorSettlementOptions {
  readonly termGraceMs?: number;
  readonly killGraceMs?: number;
  readonly sleep?: (milliseconds: number) => Promise<unknown>;
  readonly signal?: AbortSignal;
}

export type FramCoordinatorChildOutcome =
  | Readonly<{ timedOut: false; exitCode: number }>
  | Readonly<{ timedOut: true }>;

/** Bound timeout escalation and reaping even when a child ignores both signals. */
export async function settleFramCoordinatorChild(
  child: FramCoordinatorChild,
  timeoutMs?: number,
  options: FramCoordinatorSettlementOptions = {},
): Promise<FramCoordinatorChildOutcome> {
  const sleep = options.sleep ?? Bun.sleep;
  const exited = child.exited.then((exitCode) => ({ kind: "exited" as const, exitCode }));
  const aborted = Promise.withResolvers<{ readonly kind: "aborted" }>();
  const abort = () => aborted.resolve({ kind: "aborted" });
  if (options.signal?.aborted) abort();
  else options.signal?.addEventListener("abort", abort, { once: true });
  const initial = await Promise.race([
    exited,
    sleep(framCoordinatorChildTimeout(timeoutMs)).then(() => ({ kind: "timeout" as const })),
    aborted.promise,
  ]);
  options.signal?.removeEventListener("abort", abort);
  if (initial.kind === "exited") return { timedOut: false, exitCode: initial.exitCode };

  try { child.kill("SIGTERM"); } catch { /* bounded escalation remains authoritative */ }
  const terminated = await Promise.race([
    exited.then(() => true),
    sleep(options.termGraceMs ?? 500).then(() => false),
  ]);
  if (!terminated) {
    try { child.kill("SIGKILL"); } catch { /* bounded reap remains authoritative */ }
    await Promise.race([
      exited,
      sleep(options.killGraceMs ?? 500),
    ]);
  }
  return { timedOut: true };
}
