import { join } from "node:path";

export const BEAGLE_STORE_SELECTION_KEYS = [
  "BEAGLE_STORE_HOME", "BEAGLE_STORE_BIN", "BEAGLE_STORE_OUT",
] as const;

export const MIN_BEAGLE_STORE_COORDINATOR_CHILD_TIMEOUT_MS = 30_000;

export interface BeagleStoreSelection {
  home: string;
  bin: string;
  out: string;
}

export function beagleStoreSelection(
  env: NodeJS.ProcessEnv = process.env,
): BeagleStoreSelection {
  const missing = BEAGLE_STORE_SELECTION_KEYS.filter((key) => {
    const value = env[key];
    return value === undefined || value.trim().length === 0;
  });
  if (missing.length > 0) {
    throw new Error(
      `canonical Beagle Store selection is incomplete; missing ${missing.join(", ")}`,
    );
  }
  return {
    home: env.BEAGLE_STORE_HOME!,
    bin: env.BEAGLE_STORE_BIN!,
    out: env.BEAGLE_STORE_OUT!,
  };
}

export function beagleStoreExecutable(
  env: NodeJS.ProcessEnv = process.env,
): string {
  return join(beagleStoreSelection(env).home, "..", "bin", "beagle");
}

export function beagleStoreEnvironment(
  env: NodeJS.ProcessEnv = process.env,
): NodeJS.ProcessEnv {
  const selection = beagleStoreSelection(env);
  return {
    ...env,
    BEAGLE_STORE_HOME: selection.home,
    BEAGLE_STORE_BIN: selection.bin,
    BEAGLE_STORE_OUT: selection.out,
  };
}

export function beagleStoreBabashkaArguments(
  args: readonly string[],
  env: NodeJS.ProcessEnv = process.env,
): string[] {
  return ["-cp", beagleStoreSelection(env).out, ...args];
}

export function beagleStoreCoordinatorChildTimeout(timeoutMs?: number): number {
  if (timeoutMs === undefined || !Number.isFinite(timeoutMs))
    return MIN_BEAGLE_STORE_COORDINATOR_CHILD_TIMEOUT_MS;
  return Math.max(
    MIN_BEAGLE_STORE_COORDINATOR_CHILD_TIMEOUT_MS,
    Math.floor(timeoutMs),
  );
}

export interface BeagleStoreCoordinatorChild {
  readonly exited: Promise<number>;
  kill(signal: "SIGTERM" | "SIGKILL"): unknown;
}

export interface BeagleStoreCoordinatorSettlementOptions {
  readonly termGraceMs?: number;
  readonly killGraceMs?: number;
  readonly sleep?: (milliseconds: number) => Promise<unknown>;
  readonly signal?: AbortSignal;
}

export type BeagleStoreCoordinatorChildOutcome =
  | Readonly<{ timedOut: false; exitCode: number }>
  | Readonly<{ timedOut: true }>;

/** Bound timeout escalation and reaping even when a child ignores both signals. */
export async function settleBeagleStoreCoordinatorChild(
  child: BeagleStoreCoordinatorChild,
  timeoutMs?: number,
  options: BeagleStoreCoordinatorSettlementOptions = {},
): Promise<BeagleStoreCoordinatorChildOutcome> {
  const sleep = options.sleep ?? Bun.sleep;
  const exited = child.exited.then((exitCode) => ({ kind: "exited" as const, exitCode }));
  const aborted = Promise.withResolvers<{ readonly kind: "aborted" }>();
  const abort = () => aborted.resolve({ kind: "aborted" });
  if (options.signal?.aborted) abort();
  else options.signal?.addEventListener("abort", abort, { once: true });
  const initial = await Promise.race([
    exited,
    sleep(beagleStoreCoordinatorChildTimeout(timeoutMs)).then(() => ({ kind: "timeout" as const })),
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
