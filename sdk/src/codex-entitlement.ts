import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import type { ChildProcessWithoutNullStreams } from "node:child_process";
import type { ProviderUsageObservation, ProviderUsageWindow } from "./providers/types";
import { writeProviderUsageObservations } from "./provider-observation-store";
import { DEFAULT_PROVIDER_OBSERVATIONS_PATH, loadProviderUsageObservations, loadResourcePolicy } from "./resource-policy";
import type { ResourcePolicy, RoutingPreference, RoutingTarget } from "./providers/types";
import { withFileLease } from "./file-lease";
import { codexConfigArguments, providerEnvironmentForTarget } from "./accounts";

const DEFAULT_TIMEOUT_MS = 3_000;
export const CODEX_OBSERVATION_TTL_MS = 5 * 60 * 1000;
const MAX_LINE_BYTES = 1024 * 1024;

interface RpcResponse {
  id?: number;
  result?: unknown;
  error?: { code?: number; message?: string };
}

interface AppServerOptions {
  command?: string;
  /** Arguments inserted before `app-server --stdio`; useful for wrappers and tests. */
  commandArgs?: string[];
  timeoutMs?: number;
  targetId?: string;
  target?: RoutingTarget;
  env?: NodeJS.ProcessEnv;
  now?: Date;
  spawnProcess?: typeof spawn;
}

interface ObserveOptions extends AppServerOptions {
  storePath?: string;
}

interface RefreshOptions extends ObserveOptions {
  observe?: (options: ObserveOptions) => Promise<ProviderUsageObservation>;
  onDiagnostic?: (message: string) => void;
}

const refreshes = new Map<string, Promise<ProviderUsageObservation | undefined>>();

interface RateLimitWindow {
  usedPercent?: unknown;
  resetsAt?: unknown;
}

interface RateLimitSnapshot {
  limitId?: unknown;
  primary?: RateLimitWindow | null;
  secondary?: RateLimitWindow | null;
  rateLimitReachedType?: unknown;
}

function record(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function responseError(response: RpcResponse, method: string): Error {
  const detail = response.error?.message ?? "missing result";
  return new Error(`Codex app-server ${method} failed: ${detail}`);
}

function request(
  child: ChildProcessWithoutNullStreams,
  pending: Map<number, { method: string; resolve(value: unknown): void; reject(error: Error): void }>,
  id: number,
  method: string,
  params: unknown,
): Promise<unknown> {
  return new Promise((resolve, reject) => {
    pending.set(id, { method, resolve, reject });
    child.stdin.write(`${JSON.stringify({ id, method, params })}\n`, (error) => {
      if (!error) return;
      pending.delete(id);
      reject(new Error(`Codex app-server ${method} write failed: ${error.message}`));
    });
  });
}

function normalizedWindow(limitId: string, name: "primary" | "secondary", value: RateLimitWindow | null | undefined): ProviderUsageWindow | undefined {
  if (!value || typeof value.usedPercent !== "number" || !Number.isFinite(value.usedPercent) || value.usedPercent < 0)
    return undefined;
  if (typeof value.resetsAt !== "number" || !Number.isFinite(value.resetsAt) || value.resetsAt <= 0)
    return undefined;
  return {
    limitId: `${limitId}:${name}`,
    usedPercent: value.usedPercent,
    resetsAt: new Date(value.resetsAt * 1000).toISOString(),
  };
}

/**
 * Select the shared Codex subscription bucket. Model-specific buckets are not
 * provider headroom: exhausting one must not disable every OpenAI model.
 */
export function normalizeCodexRateLimits(value: unknown): ProviderUsageWindow[] {
  if (!record(value)) throw new Error("Codex app-server returned an invalid rate-limit response");
  let snapshot: RateLimitSnapshot | undefined;
  const byId = value.rateLimitsByLimitId;
  if (record(byId) && record(byId.codex)) snapshot = byId.codex as RateLimitSnapshot;
  if (!snapshot && record(value.rateLimits)) {
    const fallback = value.rateLimits as RateLimitSnapshot;
    if (fallback.limitId == null || fallback.limitId === "codex") snapshot = fallback;
  }
  if (!snapshot) throw new Error("Codex app-server returned no shared codex rate-limit bucket");
  const limitId = typeof snapshot.limitId === "string" && snapshot.limitId ? snapshot.limitId : "codex";
  return [normalizedWindow(limitId, "primary", snapshot.primary), normalizedWindow(limitId, "secondary", snapshot.secondary)]
    .filter((window): window is ProviderUsageWindow => window !== undefined);
}

/** Read ChatGPT/Codex subscription headroom without sending a model turn. */
export async function readCodexEntitlementObservation(options: AppServerOptions = {}): Promise<ProviderUsageObservation> {
  if (options.target && options.targetId && options.target.id !== options.targetId)
    throw new Error(`Codex entitlement target mismatch: ${options.target.id} != ${options.targetId}`);
  const env = providerEnvironmentForTarget("openai", options.target, { env: options.env });
  const command = options.command ?? env.NORTH_CODEX_BIN ?? "codex";
  const child = (options.spawnProcess ?? spawn)(command, [
    ...(options.commandArgs ?? []),
    "app-server",
    ...codexConfigArguments(env),
    "--stdio",
  ], {
    env,
    stdio: ["pipe", "pipe", "pipe"],
  });
  const pending = new Map<number, { method: string; resolve(value: unknown): void; reject(error: Error): void }>();
  let buffer = "";
  let stderr = "";
  let terminalError: Error | undefined;

  const rejectAll = (error: Error) => {
    terminalError = error;
    for (const waiter of pending.values()) waiter.reject(error);
    pending.clear();
  };
  child.stdout.setEncoding("utf8");
  child.stdout.on("data", (chunk: string) => {
    buffer += chunk;
    if (Buffer.byteLength(buffer) > MAX_LINE_BYTES) {
      rejectAll(new Error("Codex app-server response exceeded 1 MiB"));
      child.kill("SIGKILL");
      return;
    }
    for (;;) {
      const newline = buffer.indexOf("\n");
      if (newline < 0) break;
      const line = buffer.slice(0, newline).trim();
      buffer = buffer.slice(newline + 1);
      if (!line) continue;
      let response: RpcResponse;
      try { response = JSON.parse(line); }
      catch { continue; }
      if (typeof response.id !== "number") continue;
      const waiter = pending.get(response.id);
      if (!waiter) continue;
      pending.delete(response.id);
      if (response.error || response.result === undefined) waiter.reject(responseError(response, waiter.method));
      else waiter.resolve(response.result);
    }
  });
  child.stderr.setEncoding("utf8");
  child.stderr.on("data", (chunk: string) => { stderr = (stderr + chunk).slice(-4096); });
  child.on("error", (error) => rejectAll(new Error(`could not start Codex app-server: ${error.message}`)));
  child.on("exit", (code, signal) => {
    if (pending.size && !terminalError)
      rejectAll(new Error(`Codex app-server exited before replying (${signal ?? code ?? "unknown"})${stderr.trim() ? `: ${stderr.trim()}` : ""}`));
  });

  const timeoutMs = options.timeoutMs ?? DEFAULT_TIMEOUT_MS;
  const timeout = setTimeout(() => {
    rejectAll(new Error(`Codex app-server entitlement probe timed out after ${timeoutMs}ms`));
    child.kill("SIGKILL");
  }, timeoutMs);
  timeout.unref?.();
  try {
    await request(child, pending, 1, "initialize", { clientInfo: { name: "north", version: "1" } });
    const account = await request(child, pending, 2, "account/read", {});
    if (!record(account) || !record(account.account) || account.account.type !== "chatgpt")
      throw new Error("Codex is not authenticated through a ChatGPT subscription");
    const limits = await request(child, pending, 3, "account/rateLimits/read", null);
    const windows = normalizeCodexRateLimits(limits);
    if (!windows.length) throw new Error("Codex shared subscription bucket has no usable reset windows");
    return {
      targetId: options.target?.id ?? options.targetId ?? "openai",
      provider: "openai",
      observedAt: (options.now ?? new Date()).toISOString(),
      windows,
    };
  } finally {
    clearTimeout(timeout);
    for (const waiter of pending.values()) waiter.reject(new Error("Codex app-server entitlement probe closed"));
    pending.clear();
    child.kill("SIGTERM");
    const force = setTimeout(() => child.kill("SIGKILL"), 250);
    force.unref?.();
  }
}

/** Probe and atomically merge the observation into North's shared provider store. */
export async function observeCodexEntitlement(options: ObserveOptions = {}): Promise<ProviderUsageObservation> {
  const observation = await readCodexEntitlementObservation(options);
  await writeProviderUsageObservations(observation, options.storePath);
  return observation;
}

function orderedCodexTargets(policy: ResourcePolicy | undefined): RoutingTarget[] {
  const targets = policy?.targets?.length
    ? policy.targets
    : [{ id: "openai", provider: "openai", authMode: "ambient" } as const];
  const byId = new Map(targets.map((target) => [target.id, target]));
  const order = [...new Set([...(policy?.targetOrder ?? []), ...targets.map(({ id }) => id)])];
  return order.map((id) => byId.get(id)).filter((target): target is RoutingTarget => target?.provider === "openai");
}

export function shouldRefreshCodexEntitlement(requested: RoutingPreference | undefined): boolean {
  const provider = typeof requested === "string" ? requested : requested?.provider;
  return (provider ?? "auto") !== "anthropic";
}

function cachedCodexObservation(storePath: string | undefined, targetId: string): ProviderUsageObservation | undefined {
  return loadProviderUsageObservations(storePath)?.observations
    .filter((entry) => entry.provider === "openai" && entry.targetId === targetId)
    .sort((left, right) => Date.parse(right.observedAt) - Date.parse(left.observedAt))[0];
}

function observationFresh(cached: ProviderUsageObservation | undefined, now: Date): boolean {
  const freshByAge = cached && now.getTime() - Date.parse(cached.observedAt) <= CODEX_OBSERVATION_TTL_MS;
  const hasLiveWindow = cached?.windows?.some(({ resetsAt }) => Date.parse(resetsAt) > now.getTime()) ?? cached?.state !== undefined;
  return Boolean(freshByAge && hasLiveWindow);
}

async function refreshOneCodexEntitlement(options: RefreshOptions): Promise<ProviderUsageObservation | undefined> {
  const now = options.now ?? new Date();
  const configuredTarget = options.target ?? (options.targetId
    ? orderedCodexTargets(loadResourcePolicy()).find(({ id }) => id === options.targetId)
    : undefined);
  const targetId = configuredTarget?.id ?? options.targetId ?? "openai";
  const storePath = options.storePath ?? process.env.NORTH_PROVIDER_OBSERVATIONS ?? DEFAULT_PROVIDER_OBSERVATIONS_PATH;
  const key = `${storePath}\u0000${targetId}`;
  const running = refreshes.get(key);
  if (running) return running;
  const refresh = (async () => {
    let cached: ProviderUsageObservation | undefined;
    try {
      cached = cachedCodexObservation(storePath, targetId);
      if (observationFresh(cached, now)) return cached;
      const targetLock = createHash("sha256").update(targetId).digest("hex").slice(0, 16);
      const lockPath = `${storePath}.refresh.${targetLock}.lock`;
      return await withFileLease(lockPath, async () => {
        const afterWait = cachedCodexObservation(storePath, targetId);
        if (observationFresh(afterWait, now)) return afterWait;
        return (options.observe ?? observeCodexEntitlement)({
          ...options, storePath, target: configuredTarget, targetId, now,
        });
      });
    } catch (error) {
      (options.onDiagnostic ?? console.warn)(
        `[north] Codex subscription headroom refresh unavailable; using ${cached ? "cached observation" : "unknown pressure"}: ${error instanceof Error ? error.message : String(error)}`,
      );
      return cached;
    }
  })();
  refreshes.set(key, refresh);
  try { return await refresh; }
  finally { if (refreshes.get(key) === refresh) refreshes.delete(key); }
}

interface RefreshAllOptions extends Omit<RefreshOptions, "target" | "targetId"> {
  requested?: RoutingPreference;
  policy?: ResourcePolicy;
}

/** Refresh every candidate Codex account concurrently; each target retains its own cache/single-flight key. */
export async function refreshCodexEntitlementsIfStale(
  options: RefreshAllOptions = {},
): Promise<Array<ProviderUsageObservation | undefined>> {
  const request = typeof options.requested === "string" ? { provider: options.requested } : options.requested ?? {};
  if (request.provider === "anthropic") return [];
  const targets = orderedCodexTargets(options.policy ?? loadResourcePolicy())
    .filter((target) => request.target === undefined || target.id === request.target);
  return Promise.all(targets.map((target) => refreshOneCodexEntitlement({ ...options, target, targetId: target.id })));
}

/** Refresh at most once per five minutes; an unpinned call refreshes every configured Codex target. */
export async function refreshCodexEntitlementIfStale(options: RefreshOptions = {}): Promise<ProviderUsageObservation | undefined> {
  if (options.target || options.targetId) return refreshOneCodexEntitlement(options);
  return (await refreshCodexEntitlementsIfStale(options))[0];
}
