import { existsSync, readFileSync } from "node:fs";
import { homedir } from "node:os";
import { resolve } from "node:path";
import type {
  AllocationMode,
  EntitlementPressure,
  EnvelopeLimits,
  PressureObservation,
  ProviderId,
  ProviderUsageObservation,
  ProviderUsageObservationStore,
  ProviderUsageCollectionFailureReason,
  ProviderUsageSource,
  ProviderUsageUnavailableComponent,
  ProviderUsageWindow,
  ResourceEnvelopes,
  ResourcePolicy,
  RoutingTarget,
  TargetAuthMode,
} from "./providers/types";

const PROVIDERS: ProviderId[] = ["anthropic", "openai"];
const PRESSURES: EntitlementPressure[] = ["plenty", "normal", "low", "exhausted", "unknown"];
const PRESSURE_RANK: Record<EntitlementPressure, number> = {
  exhausted: 4, low: 3, normal: 2, plenty: 1, unknown: 0,
};
const MODES: AllocationMode[] = ["preferential", "balanced", "reserved"];
const TARGET_AUTH_MODES: TargetAuthMode[] = ["ambient", "isolated"];
const USAGE_SOURCES: ProviderUsageSource[] = [
  "claude-agent-sdk:usage-control-experimental",
  "claude-agent-sdk:rate-limit-event",
  "claude-code:statusline",
  "codex-app-server:account-rate-limits",
];
const COLLECTION_FAILURE_REASONS: ProviderUsageCollectionFailureReason[] = [
  "anthropic_usage_capability_unavailable",
  "anthropic_usage_probe_failed",
  "anthropic_usage_probe_timed_out",
  "anthropic_usage_rate_limits_unavailable",
  "anthropic_usage_response_schema_changed",
  "anthropic_usage_windows_unavailable",
  "codex_usage_command_unavailable",
  "codex_usage_probe_failed",
  "codex_usage_probe_timed_out",
  "codex_usage_response_schema_changed",
  "codex_usage_subscription_auth_required",
  "codex_usage_transport_failed",
  "codex_usage_windows_unavailable",
];
const PORTABLE_PROFILE_SLUG = /^[a-z0-9][a-z0-9_-]{0,63}$/;

/** Pressure observations are trustworthy for one day unless `until` is set. */
export const PRESSURE_TTL_MS = 24 * 60 * 60 * 1000;
export const OBSERVATION_CLOCK_SKEW_MS = 5 * 60 * 1000;
export const COLLECTION_FAILURE_TTL_MS = 5 * 60 * 1000;
export const DEFAULT_ROUTING_POLICY_PATH = resolve(homedir(), ".config/north/routing-policy.json");
export const DEFAULT_PROVIDER_OBSERVATIONS_PATH = resolve(homedir(), ".local/state/north/provider-usage-observations.json");

function fail(path: string, message: string): never {
  throw new Error(`invalid North routing policy at ${path}: ${message}`);
}

function object(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function keysOnly(path: string, label: string, value: Record<string, unknown>, allowed: string[]): void {
  const unknown = Object.keys(value).filter((key) => !allowed.includes(key));
  if (unknown.length) fail(path, `${label} has unknown field(s): ${unknown.join(", ")}`);
}

function provider(path: string, value: unknown, label: string): ProviderId {
  if (!PROVIDERS.includes(value as ProviderId)) fail(path, `${label} must be anthropic or openai`);
  return value as ProviderId;
}

function finitePositive(path: string, value: unknown, label: string, integer = false, allowZero = false): number {
  if (typeof value !== "number" || !Number.isFinite(value) || (allowZero ? value < 0 : value <= 0) || (integer && !Number.isInteger(value)))
    fail(path, `${label} must be a ${allowZero ? "non-negative" : "positive"}${integer ? " integer" : " number"}`);
  return value;
}

function timestamp(path: string, value: unknown, label: string): string {
  if (typeof value !== "string" || !Number.isFinite(Date.parse(value)))
    fail(path, `${label} must be an ISO-8601 timestamp`);
  return value;
}

function parseLimits(path: string, value: unknown, label: string): EnvelopeLimits {
  if (!object(value)) fail(path, `${label} must be an object`);
  keysOnly(path, label, value, ["runs", "frontierRuns", "retries", "parallelism"]);
  const result: EnvelopeLimits = {};
  for (const key of ["runs", "frontierRuns", "retries", "parallelism"] as const) {
    if (value[key] !== undefined)
      result[key] = finitePositive(path, value[key], `${label}.${key}`, true, key !== "parallelism");
  }
  return result;
}

function parseNamedLimits(path: string, value: unknown, label: string): Record<string, EnvelopeLimits> {
  if (!object(value)) fail(path, `${label} must be an object keyed by name`);
  return Object.fromEntries(Object.entries(value).map(([name, limits]) => {
    if (!name.trim()) fail(path, `${label} contains an empty name`);
    return [name, parseLimits(path, limits, `${label}.${name}`)];
  }));
}

function parseEnvelopes(path: string, value: unknown): ResourceEnvelopes {
  if (!object(value)) fail(path, "envelopes must be an object");
  keysOnly(path, "envelopes", value, ["default", "month", "week", "projects", "sessions"]);
  return {
    ...(value.default === undefined ? {} : { default: parseLimits(path, value.default, "envelopes.default") }),
    ...(value.month === undefined ? {} : { month: parseLimits(path, value.month, "envelopes.month") }),
    ...(value.week === undefined ? {} : { week: parseLimits(path, value.week, "envelopes.week") }),
    ...(value.projects === undefined ? {} : { projects: parseNamedLimits(path, value.projects, "envelopes.projects") }),
    ...(value.sessions === undefined ? {} : { sessions: parseNamedLimits(path, value.sessions, "envelopes.sessions") }),
  };
}

function parseTargets(path: string, value: unknown): RoutingTarget[] {
  if (!Array.isArray(value) || value.length === 0) fail(path, "targets must be a non-empty array");
  const targets = value.map((entry, index) => {
    const label = `targets[${index}]`;
    if (!object(entry)) fail(path, `${label} must be an object`);
    keysOnly(path, label, entry, ["id", "provider", "authMode", "profile"]);
    if (typeof entry.id !== "string" || !entry.id.trim()) fail(path, `${label}.id must be a non-empty string`);
    if (entry.authMode !== undefined && !TARGET_AUTH_MODES.includes(entry.authMode as TargetAuthMode))
      fail(path, `${label}.authMode must be ambient or isolated`);
    if (entry.profile !== undefined && (typeof entry.profile !== "string" || !entry.profile.trim()))
      fail(path, `${label}.profile must be a non-empty string`);
    const authMode = (entry.authMode ?? "ambient") as TargetAuthMode;
    if (authMode === "isolated" && typeof entry.profile !== "string")
      fail(path, `${label}.profile is required when authMode is isolated`);
    if (authMode === "isolated" && !PORTABLE_PROFILE_SLUG.test(entry.profile as string))
      fail(path, `${label}.profile must be a portable slug (lowercase letters, digits, _ or -; max 64 characters)`);
    return { id: entry.id, provider: provider(path, entry.provider, `${label}.provider`), authMode,
      ...(entry.profile === undefined ? {} : { profile: entry.profile }) } as RoutingTarget;
  });
  if (new Set(targets.map(({ id }) => id)).size !== targets.length) fail(path, "target ids must be unique");
  const isolatedRoots = targets
    .filter(({ authMode }) => authMode === "isolated")
    .map(({ provider, profile }) => `${provider}\u0000${profile}`);
  if (new Set(isolatedRoots).size !== isolatedRoots.length)
    fail(path, "isolated targets must not reuse the same provider/profile root");
  const ambientProviders = targets
    .filter(({ authMode }) => authMode === "ambient")
    .map(({ provider }) => provider);
  if (new Set(ambientProviders).size !== ambientProviders.length)
    fail(path, "ambient targets must not reuse the same provider account");
  return targets;
}

function stringList(path: string, value: unknown, label: string): string[] {
  if (!Array.isArray(value) || value.some((item) => typeof item !== "string" || !item.trim()))
    fail(path, `${label} must be an array of non-empty strings`);
  if (new Set(value).size !== value.length) fail(path, `${label} must not contain duplicates`);
  return value as string[];
}

function parsePressure(path: string, value: unknown, label: string): PressureObservation {
  if (!object(value)) fail(path, `${label} must be an object`);
  keysOnly(path, label, value, ["state", "observedAt", "until"]);
  if (!PRESSURES.includes(value.state as EntitlementPressure))
    fail(path, `${label}.state must be plenty, normal, low, exhausted, or unknown`);
  return {
    state: value.state as EntitlementPressure,
    observedAt: timestamp(path, value.observedAt, `${label}.observedAt`),
    ...(value.until === undefined ? {} : { until: timestamp(path, value.until, `${label}.until`) }),
  };
}

export function pressureObservationIsFresh(
  observation: Pick<PressureObservation, "observedAt" | "until"> | undefined,
  now = new Date(),
): boolean {
  if (!observation) return false;
  const observedAt = Date.parse(observation.observedAt);
  if (observedAt > now.getTime() + OBSERVATION_CLOCK_SKEW_MS) return false;
  const expiry = observation.until ? Date.parse(observation.until) : observedAt + PRESSURE_TTL_MS;
  return now.getTime() <= expiry;
}

export function effectivePressure(observation: PressureObservation | undefined, now = new Date()): EntitlementPressure {
  return pressureObservationIsFresh(observation, now) ? observation!.state : "unknown";
}

/** Provider-neutral utilization thresholds; the most constrained live window wins. */
export function pressureFromUsageWindows(windows: ProviderUsageWindow[], now = new Date()): EntitlementPressure | undefined {
  const relevant = windows.filter(({ resetsAt }) => Date.parse(resetsAt) > now.getTime());
  if (!relevant.length) return undefined;
  const worst = Math.max(...relevant.map(({ usedPercent }) => usedPercent));
  if (worst >= 100) return "exhausted";
  if (worst >= 80) return "low";
  if (worst >= 50) return "normal";
  return "plenty";
}

export function automatedPressure(
  observation: ProviderUsageObservation | undefined,
  now = new Date(),
): EntitlementPressure | undefined {
  if (collectionFailureIsFresh(observation, now)) {
    const windowPressure = observation?.windows?.length
      ? pressureFromUsageWindows(observation.windows, now)
      : undefined;
    const priorPressure = [windowPressure, observation?.state]
      .filter((value): value is EntitlementPressure => value !== undefined)
      .sort((left, right) => PRESSURE_RANK[right] - PRESSURE_RANK[left])[0];
    return priorPressure === "exhausted" ? "exhausted" : "unknown";
  }
  if (!pressureObservationIsFresh(observation, now)) {
    const liveWindowPressure = observation?.windows?.length
      ? pressureFromUsageWindows(observation.windows, now)
      : undefined;
    // A proven rejection/exhaustion is monotonic through its provider reset.
    // Lesser stale percentages are not precise enough to steer new work.
    return liveWindowPressure === "exhausted" ? "exhausted" : undefined;
  }
  if (observation?.windows?.length) {
    const windowPressure = pressureFromUsageWindows(observation.windows, now);
    return [windowPressure, observation.state]
      .filter((value): value is EntitlementPressure => value !== undefined)
      .sort((left, right) => PRESSURE_RANK[right] - PRESSURE_RANK[left])[0];
  }
  return observation?.state;
}

export function collectionFailureIsFresh(
  observation: ProviderUsageObservation | undefined,
  now = new Date(),
): boolean {
  const failureAt = observation?.collectionFailure?.observedAt;
  return failureAt !== undefined
    && Number.isFinite(Date.parse(failureAt))
    && Date.parse(failureAt) <= now.getTime() + OBSERVATION_CLOCK_SKEW_MS
    && now.getTime() - Date.parse(failureAt) <= COLLECTION_FAILURE_TTL_MS;
}

export function parseProviderUsageObservations(input: unknown, path = "<memory>"): ProviderUsageObservationStore {
  if (!object(input)) failObservations(path, "top level must be an object");
  const unknown = Object.keys(input).filter((key) => !["version", "observations"].includes(key));
  if (unknown.length) failObservations(path, `top level has unknown field(s): ${unknown.join(", ")}`);
  if (input.version !== 1) failObservations(path, "version must be 1");
  if (!Array.isArray(input.observations)) failObservations(path, "observations must be an array");
  const observations = input.observations.map((entry, index) => {
    const label = `observations[${index}]`;
    if (!object(entry)) failObservations(path, `${label} must be an object`);
    const unknownFields = Object.keys(entry).filter((key) => !["targetId", "provider", "source", "state", "windows", "unavailableComponents", "collectionFailure", "observedAt", "until"].includes(key));
    if (unknownFields.length) failObservations(path, `${label} has unknown field(s): ${unknownFields.join(", ")}`);
    if (typeof entry.targetId !== "string" || !entry.targetId.trim())
      failObservations(path, `${label}.targetId must be a non-empty string`);
    if (!PROVIDERS.includes(entry.provider as ProviderId))
      failObservations(path, `${label}.provider must be anthropic or openai`);
    if (entry.source !== undefined && !USAGE_SOURCES.includes(entry.source as ProviderUsageSource))
      failObservations(path, `${label}.source is not a recognized provider usage source`);
    if (entry.source !== undefined) {
      const sourceProvider: ProviderId = String(entry.source).startsWith("codex-") ? "openai" : "anthropic";
      if (entry.provider !== sourceProvider)
        failObservations(path, `${label}.source does not belong to provider ${entry.provider}`);
    }
    if (entry.state !== undefined && !PRESSURES.includes(entry.state as EntitlementPressure))
      failObservations(path, `${label}.state must be plenty, normal, low, exhausted, or unknown`);
    let windows: ProviderUsageWindow[] | undefined;
    if (entry.windows !== undefined) {
      if (!Array.isArray(entry.windows) || entry.windows.length === 0)
        failObservations(path, `${label}.windows must be a non-empty array`);
      windows = entry.windows.map((window, windowIndex) => {
        const windowLabel = `${label}.windows[${windowIndex}]`;
        if (!object(window)) failObservations(path, `${windowLabel} must be an object`);
        const unknownWindowFields = Object.keys(window).filter((key) => !["limitId", "usedPercent", "resetsAt"].includes(key));
        if (unknownWindowFields.length)
          failObservations(path, `${windowLabel} has unknown field(s): ${unknownWindowFields.join(", ")}`);
        if (window.limitId !== undefined && (typeof window.limitId !== "string" || !window.limitId.trim()))
          failObservations(path, `${windowLabel}.limitId must be a non-empty string`);
        if (typeof window.usedPercent !== "number" || !Number.isFinite(window.usedPercent) || window.usedPercent < 0)
          failObservations(path, `${windowLabel}.usedPercent must be a non-negative number`);
        return {
          ...(window.limitId === undefined ? {} : { limitId: window.limitId }),
          usedPercent: window.usedPercent,
          resetsAt: observationTimestamp(path, window.resetsAt, `${windowLabel}.resetsAt`),
        } as ProviderUsageWindow;
      });
    }
    let unavailableComponents: ProviderUsageUnavailableComponent[] | undefined;
    if (entry.unavailableComponents !== undefined) {
      if (!Array.isArray(entry.unavailableComponents) || entry.unavailableComponents.length === 0)
        failObservations(path, `${label}.unavailableComponents must be a non-empty array`);
      unavailableComponents = entry.unavailableComponents.map((component, componentIndex) => {
        const componentLabel = `${label}.unavailableComponents[${componentIndex}]`;
        if (!object(component)) failObservations(path, `${componentLabel} must be an object`);
        const unknownComponentFields = Object.keys(component).filter((key) => !["limitId", "reason"].includes(key));
        if (unknownComponentFields.length)
          failObservations(path, `${componentLabel} has unknown field(s): ${unknownComponentFields.join(", ")}`);
        if (typeof component.limitId !== "string" || !component.limitId.trim())
          failObservations(path, `${componentLabel}.limitId must be a non-empty string`);
        if (!["reset_unavailable", "utilization_unavailable", "component_schema_changed"].includes(component.reason as string))
          failObservations(path, `${componentLabel}.reason is not recognized`);
        return { limitId: component.limitId, reason: component.reason } as ProviderUsageUnavailableComponent;
      });
    }
    let collectionFailure: ProviderUsageObservation["collectionFailure"];
    if (entry.collectionFailure !== undefined) {
      const failureLabel = `${label}.collectionFailure`;
      if (!object(entry.collectionFailure)) failObservations(path, `${failureLabel} must be an object`);
      const unknownFailureFields = Object.keys(entry.collectionFailure)
        .filter((key) => !["observedAt", "reason"].includes(key));
      if (unknownFailureFields.length)
        failObservations(path, `${failureLabel} has unknown field(s): ${unknownFailureFields.join(", ")}`);
      if (!COLLECTION_FAILURE_REASONS.includes(entry.collectionFailure.reason as ProviderUsageCollectionFailureReason))
        failObservations(path, `${failureLabel}.reason is not recognized`);
      const reasonProvider: ProviderId = String(entry.collectionFailure.reason).startsWith("codex_")
        ? "openai" : "anthropic";
      if (entry.provider !== reasonProvider)
        failObservations(path, `${failureLabel}.reason does not belong to provider ${entry.provider}`);
      collectionFailure = {
        observedAt: observationTimestamp(path, entry.collectionFailure.observedAt, `${failureLabel}.observedAt`),
        reason: entry.collectionFailure.reason as ProviderUsageCollectionFailureReason,
      };
    }
    if (entry.state === undefined && windows === undefined)
      failObservations(path, `${label} must contain state or windows`);
    const observedAt = observationTimestamp(path, entry.observedAt, `${label}.observedAt`);
    return {
      targetId: entry.targetId,
      provider: entry.provider as ProviderId,
      ...(entry.source === undefined ? {} : { source: entry.source as ProviderUsageSource }),
      observedAt,
      ...(entry.state === undefined ? {} : { state: entry.state as EntitlementPressure }),
      ...(windows === undefined ? {} : { windows }),
      ...(unavailableComponents === undefined ? {} : { unavailableComponents }),
      ...(collectionFailure === undefined ? {} : { collectionFailure }),
      ...(entry.until === undefined ? {} : { until: observationTimestamp(path, entry.until, `${label}.until`) }),
    } as ProviderUsageObservation;
  });
  return { version: 1, observations };
}

function failObservations(path: string, message: string): never {
  throw new Error(`invalid North provider usage observations at ${path}: ${message}`);
}

function observationTimestamp(path: string, value: unknown, label: string): string {
  if (typeof value !== "string" || !Number.isFinite(Date.parse(value)))
    failObservations(path, `${label} must be an ISO-8601 timestamp`);
  return value;
}

export function loadProviderUsageObservations(
  path = process.env.NORTH_PROVIDER_OBSERVATIONS ?? DEFAULT_PROVIDER_OBSERVATIONS_PATH,
): ProviderUsageObservationStore | undefined {
  if (!existsSync(path)) return undefined;
  let input: unknown;
  try { input = JSON.parse(readFileSync(path, "utf8")); }
  catch (error) {
    failObservations(path, `could not parse JSON: ${error instanceof Error ? error.message : String(error)}`);
  }
  return parseProviderUsageObservations(input, path);
}

/**
 * Overlay fresh automated observations onto fresh manual policy observations.
 * Every target remains executable data; provider pressures are the compatibility
 * projection of the first ordered target for each provider.
 */
export function applyProviderUsageObservations(
  policy: ResourcePolicy,
  store: ProviderUsageObservationStore | undefined,
  now = new Date(),
): ResourcePolicy {
  const targets = policy.targets ?? [];
  const configuredOrder = policy.targetOrder ?? targets.map(({ id }) => id);
  const order = [...configuredOrder, ...targets.map(({ id }) => id).filter((id) => !configuredOrder.includes(id))];
  const latestByTargetSource = new Map<string, ProviderUsageObservation>();
  for (const observation of store?.observations ?? []) {
    if (automatedPressure(observation, now) === undefined) continue;
    const target = targets.find(({ id }) => id === observation.targetId);
    if (!target || target.provider !== observation.provider) continue;
    const key = `${observation.targetId}\u0000${observation.source ?? "legacy"}`;
    const previous = latestByTargetSource.get(key);
    if (!previous || Date.parse(observation.observedAt) > Date.parse(previous.observedAt))
      latestByTargetSource.set(key, observation);
  }
  const candidates = new Map<string, ProviderUsageObservation[]>();
  for (const observation of latestByTargetSource.values())
    candidates.set(observation.targetId, [...(candidates.get(observation.targetId) ?? []), observation]);
  // Source-less observations are a migration format. Once a target has any
  // explicit-source sample, keeping the legacy lane as an independent sensor
  // can let duplicate stale data conservatively override newer telemetry.
  for (const [targetId, observations] of candidates) {
    if (observations.some((observation) =>
      observation.source !== undefined && automatedPressure(observation, now) !== "unknown"))
      candidates.set(targetId, observations.filter(({ source }) => source !== undefined));
  }
  const pressures: Partial<Record<ProviderId, EntitlementPressure>> = {};
  const targetPressures: Record<string, EntitlementPressure> = {};
  const automatedPressureObservations: Record<string, ProviderUsageObservation> = {};
  const automatedPressureObservationSets: Record<string, ProviderUsageObservation[]> = {};
  for (const id of order) {
    const target = targets.find((candidate) => candidate.id === id);
    if (!target) continue;
    const targetCandidates = candidates.get(id) ?? [];
    if (targetCandidates.length)
      automatedPressureObservationSets[id] = [...targetCandidates]
        .sort((left, right) => Date.parse(right.observedAt) - Date.parse(left.observedAt));
    const knownCandidates = targetCandidates.filter((candidate) => automatedPressure(candidate, now) !== "unknown");
    const usableCandidates = knownCandidates.length ? knownCandidates : targetCandidates;
    const pressureRank: Record<EntitlementPressure, number> = {
      exhausted: 4, low: 3, normal: 2, plenty: 1, unknown: 0,
    };
    const automated = usableCandidates.sort((left, right) => {
      const pressureDifference = pressureRank[automatedPressure(right, now)!] - pressureRank[automatedPressure(left, now)!];
      return pressureDifference || Date.parse(right.observedAt) - Date.parse(left.observedAt);
    })[0];
    const manual = policy.pressureObservations?.[id];
    if (automated) {
      const observedPressure = automatedPressure(automated, now)!;
      const manualPressure = effectivePressure(manual, now);
      // A failed/partial usage probe is absence of knowledge. It must never
      // turn a known manual exhaustion into an eligible account.
      targetPressures[id] = observedPressure === "unknown" ? manualPressure : observedPressure;
      automatedPressureObservations[id] = automated;
    } else {
      targetPressures[id] = effectivePressure(manual, now);
    }
    if (pressures[target.provider] === undefined) pressures[target.provider] = targetPressures[id];
  }
  return {
    ...policy,
    pressures,
    targetPressures,
    automatedPressureObservations,
    automatedPressureObservationSets,
  };
}

export function parseResourcePolicy(input: unknown, path = "<memory>", now = new Date()): ResourcePolicy {
  if (!object(input)) fail(path, "top level must be an object");
  keysOnly(path, "top level", input, ["version", "mode", "targets", "targetOrder", "providerOrder", "weights",
    "reservedFrontierTarget", "pressures", "envelopes"]);
  if (input.version !== 1) fail(path, "version must be 1");
  if (!MODES.includes(input.mode as AllocationMode)) fail(path, "mode must be preferential, balanced, or reserved");
  const targets = parseTargets(path, input.targets);
  const ids = new Set(targets.map(({ id }) => id));
  const targetOrder = input.targetOrder === undefined ? targets.map(({ id }) => id) : stringList(path, input.targetOrder, "targetOrder");
  for (const id of targetOrder) if (!ids.has(id)) fail(path, `targetOrder references unknown target ${id}`);
  const providerOrder = input.providerOrder === undefined
    ? [...new Set(targetOrder.map((id) => targets.find((target) => target.id === id)!.provider))]
    : stringList(path, input.providerOrder, "providerOrder").map((id) => provider(path, id, "providerOrder entry"));

  const pressureObservations: Record<string, PressureObservation> = {};
  if (input.pressures !== undefined) {
    if (!object(input.pressures)) fail(path, "pressures must be an object keyed by target id");
    for (const [id, observation] of Object.entries(input.pressures)) {
      if (!ids.has(id)) fail(path, `pressures references unknown target ${id}`);
      pressureObservations[id] = parsePressure(path, observation, `pressures.${id}`);
    }
  }
  const targetWeights: Record<string, number> = {};
  if (input.weights !== undefined) {
    if (!object(input.weights)) fail(path, "weights must be an object keyed by target id");
    for (const [id, value] of Object.entries(input.weights)) {
      if (!ids.has(id)) fail(path, `weights references unknown target ${id}`);
      targetWeights[id] = finitePositive(path, value, `weights.${id}`);
    }
  }
  const reserved = input.reservedFrontierTarget;
  if (reserved !== undefined && (typeof reserved !== "string" || !ids.has(reserved)))
    fail(path, "reservedFrontierTarget must reference a declared target");

  // Targets are the executable account boundary. Provider-level pressure and
  // weight remain only compatibility projections for older callers; selection,
  // authentication, usage evidence, and fallback all operate per target.
  const pressureByProvider: Partial<Record<ProviderId, EntitlementPressure>> = {};
  const targetPressures: Record<string, EntitlementPressure> = {};
  const weightByProvider: Partial<Record<ProviderId, number>> = {};
  const projectionOrder = [...targetOrder, ...targets.map(({ id }) => id).filter((id) => !targetOrder.includes(id))];
  for (const id of projectionOrder) {
    const target = targets.find((candidate) => candidate.id === id)!;
    targetPressures[id] = effectivePressure(pressureObservations[id], now);
    if (pressureByProvider[target.provider] === undefined)
      pressureByProvider[target.provider] = targetPressures[id];
    if (weightByProvider[target.provider] === undefined)
      weightByProvider[target.provider] = targetWeights[id] ?? 1;
  }
  return {
    version: 1,
    mode: input.mode as AllocationMode,
    targets,
    targetOrder,
    providerOrder,
    pressures: pressureByProvider,
    weights: weightByProvider,
    targetPressures,
    pressureObservations,
    targetWeights,
    ...(reserved === undefined ? {} : {
      reservedFrontierTarget: reserved,
      reservedFrontierProvider: targets.find(({ id }) => id === reserved)!.provider,
    }),
    ...(input.envelopes === undefined ? {} : { envelopes: parseEnvelopes(path, input.envelopes) }),
  };
}

export function loadResourcePolicy(path = process.env.NORTH_ROUTING_POLICY ?? DEFAULT_ROUTING_POLICY_PATH, now = new Date()): ResourcePolicy | undefined {
  if (!existsSync(path)) return undefined;
  let input: unknown;
  try { input = JSON.parse(readFileSync(path, "utf8")); }
  catch (error) { fail(path, `could not parse JSON: ${error instanceof Error ? error.message : String(error)}`); }
  return parseResourcePolicy(input, path, now);
}
