import { spawnSync } from "node:child_process";
import type { SemanticTier } from "./providers/catalog";
import type {
  EntitlementPressure,
  ProviderAvailability,
  ProviderId,
  ProviderPreference,
  RoutingPreference,
  ResourcePolicy,
  RoutingDecision,
  RoutingTarget,
} from "./providers/types";
import { applyProviderUsageObservations, loadProviderUsageObservations, loadResourcePolicy } from "./resource-policy";
import { codexConfigArguments, isClaudeSubscriptionStatus, providerEnvironmentForTarget } from "./accounts";

const PROVIDERS: ProviderId[] = ["anthropic", "openai"];

export type ProviderSelectionFailure =
  | "provider_unavailable"
  | "entitlement_exhausted"
  | "no_provider_available";

// Provider selection happens before a provider query is constructed, so this
// error is an explicit no-side-effect signal to callers such as discovery.
export class ProviderSelectionError extends Error {
  readonly preSideEffect = true;

  constructor(readonly kind: ProviderSelectionFailure, message: string) {
    super(message);
    this.name = "ProviderSelectionError";
  }
}

function providerList(value: string | undefined): ProviderId[] {
  const parsed = (value ?? "anthropic,openai")
    .split(",")
    .map((entry) => entry.trim())
    .filter((entry): entry is ProviderId => PROVIDERS.includes(entry as ProviderId));
  return [...new Set(parsed.length ? parsed : PROVIDERS)];
}

function pressure(value: string | undefined): EntitlementPressure {
  return value === "plenty" || value === "normal" || value === "low" || value === "exhausted"
    ? value
    : "unknown";
}

function weights(value: string | undefined): Partial<Record<ProviderId, number>> {
  const result: Partial<Record<ProviderId, number>> = {};
  for (const item of (value ?? "").split(",")) {
    const [id, raw] = item.split("=").map((part) => part.trim());
    const parsed = Number(raw);
    if (PROVIDERS.includes(id as ProviderId) && Number.isFinite(parsed) && parsed > 0)
      result[id as ProviderId] = parsed;
  }
  return result;
}

function declaredTargets(policy: ResourcePolicy): RoutingTarget[] {
  if (policy.targets?.length) return policy.targets;
  const providers = [...new Set([...policy.providerOrder, ...PROVIDERS])];
  return providers.map((id) => ({ id, provider: id, authMode: "ambient" }));
}

function orderedTargets(policy: ResourcePolicy): RoutingTarget[] {
  const targets = declaredTargets(policy);
  const byId = new Map(targets.map((target) => [target.id, target]));
  const configured = policy.targetOrder?.filter((id) => byId.has(id)) ?? [];
  const derived = policy.providerOrder.flatMap((provider) => targets.filter((target) => target.provider === provider).map(({ id }) => id));
  const order = [...new Set([...configured, ...derived, ...targets.map(({ id }) => id)])];
  return order.map((id) => byId.get(id)!);
}

function targetOrderForProviders(policy: ResourcePolicy, providers: ProviderId[]): string[] {
  const targets = orderedTargets(policy);
  return [...providers, ...PROVIDERS.filter((provider) => !providers.includes(provider))]
    .flatMap((provider) => targets.filter((target) => target.provider === provider).map(({ id }) => id));
}

export function resourcePolicyFromEnv(
  base: ResourcePolicy | undefined = loadResourcePolicy(),
  observations = loadProviderUsageObservations(),
): ResourcePolicy {
  const foundation: ResourcePolicy = base ?? {
    version: 1,
    mode: "preferential",
    targets: PROVIDERS.map((id) => ({ id, provider: id, authMode: "ambient" })),
    targetOrder: PROVIDERS,
    providerOrder: PROVIDERS,
    pressures: {},
    weights: {},
  };
  const observed = observations ? applyProviderUsageObservations(foundation, observations) : foundation;
  const rawMode = process.env.NORTH_ALLOCATION_MODE;
  const mode = rawMode === "balanced" || rawMode === "reserved" || rawMode === "preferential" ? rawMode : observed?.mode ?? "preferential";
  const reserved = process.env.NORTH_RESERVED_FRONTIER_PROVIDER;
  const envOrder = process.env.NORTH_PROVIDER_ORDER;
  const envWeights = process.env.NORTH_PROVIDER_WEIGHTS;
  const anthropicPressure = process.env.NORTH_ANTHROPIC_ENTITLEMENT_PRESSURE;
  const openaiPressure = process.env.NORTH_OPENAI_ENTITLEMENT_PRESSURE;
  const providerPressureOverrides: Partial<Record<ProviderId, EntitlementPressure>> = {
    ...(anthropicPressure === undefined ? {} : { anthropic: pressure(anthropicPressure) }),
    ...(openaiPressure === undefined ? {} : { openai: pressure(openaiPressure) }),
  };
  const targets = declaredTargets(observed);
  const targetOrder = envOrder === undefined ? observed.targetOrder : targetOrderForProviders(observed, providerList(envOrder));
  const targetPressures = Object.fromEntries(targets.map((target) => [
    target.id,
    providerPressureOverrides[target.provider] ?? observed.targetPressures?.[target.id]
      ?? observed.pressures[target.provider] ?? "unknown",
  ])) as Record<string, EntitlementPressure>;
  const projectedPressures: Partial<Record<ProviderId, EntitlementPressure>> = {};
  const ordered = [...(targetOrder ?? []), ...targets.map(({ id }) => id).filter((id) => !(targetOrder ?? []).includes(id))];
  for (const id of ordered) {
    const target = targets.find((candidate) => candidate.id === id);
    if (target && projectedPressures[target.provider] === undefined)
      projectedPressures[target.provider] = targetPressures[id];
  }
  const reservedProvider = PROVIDERS.includes(reserved as ProviderId)
    ? reserved as ProviderId : observed.reservedFrontierProvider;
  const reservedTarget = PROVIDERS.includes(reserved as ProviderId)
    ? targets.find((target) => target.provider === reserved)?.id
    : observed.reservedFrontierTarget;
  return {
    ...observed,
    targets,
    targetOrder,
    targetPressures,
    mode,
    providerOrder: envOrder === undefined ? observed?.providerOrder ?? PROVIDERS : providerList(envOrder),
    pressures: { ...observed.pressures, ...projectedPressures, ...providerPressureOverrides },
    weights: envWeights === undefined ? observed?.weights ?? {} : weights(envWeights),
    reservedFrontierProvider: reservedProvider,
    reservedFrontierTarget: reservedTarget,
  };
}

export function probeAnthropic(target?: RoutingTarget): ProviderAvailability {
  const env = providerEnvironmentForTarget("anthropic", target);
  const disabled = env.NORTH_DISABLE_ANTHROPIC === "1";
  const command = env.NORTH_CLAUDE_BIN ?? "claude";
  const version = spawnSync(command, ["--version"], { env, encoding: "utf8", timeout: 3000 });
  if (version.error || version.status !== 0) return {
    ...(target ? { targetId: target.id } : {}),
    provider: "anthropic", installed: false, authenticated: false, available: false,
    reason: disabled ? "disabled" : "command_missing",
  };
  const auth = spawnSync(command, ["auth", "status", "--json"], { env, encoding: "utf8", timeout: 3000 });
  let loggedIn = false;
  try {
    const status = JSON.parse(auth.stdout || "{}");
    loggedIn = isClaudeSubscriptionStatus(status);
  } catch { /* malformed output is not authenticated */ }
  if (auth.error || auth.status !== 0 || !loggedIn) return {
    ...(target ? { targetId: target.id } : {}),
    provider: "anthropic", installed: true, authenticated: false, available: false,
    reason: disabled ? "disabled" : "authentication_missing",
  };
  return { ...(target ? { targetId: target.id } : {}), provider: "anthropic", installed: true, authenticated: true, available: !disabled,
    reason: disabled ? "disabled" : "ready" };
}

export function probeOpenAI(target?: RoutingTarget): ProviderAvailability {
  const env = providerEnvironmentForTarget("openai", target);
  const disabled = env.NORTH_DISABLE_OPENAI === "1";
  const command = env.NORTH_CODEX_BIN ?? "codex";
  const result = spawnSync(command, ["--version"], { env, encoding: "utf8", timeout: 3000 });
  if (result.error || result.status !== 0)
    return {
      ...(target ? { targetId: target.id } : {}),
      provider: "openai",
      installed: false,
      authenticated: false,
      available: false,
      reason: disabled ? "disabled" : "command_missing",
  };
  const auth = spawnSync(command, ["login", "status", ...codexConfigArguments(env)], { env, encoding: "utf8", timeout: 3000 });
  const authLines = `${auth.stdout}\n${auth.stderr}`.split(/\r?\n/).map((line) => line.trim()).filter(Boolean);
  const loggedIn = auth.status === 0 && authLines.includes("Logged in using ChatGPT");
  if (auth.error || !loggedIn) return {
    ...(target ? { targetId: target.id } : {}),
    provider: "openai", installed: true, authenticated: false, available: false,
    reason: disabled ? "disabled" : "authentication_missing",
  };
  return { ...(target ? { targetId: target.id } : {}), provider: "openai", installed: true, authenticated: true, available: !disabled,
    reason: disabled ? "disabled" : "ready" };
}

function stableHash(value: string): number {
  let hash = 2166136261;
  for (let i = 0; i < value.length; i++) {
    hash ^= value.charCodeAt(i);
    hash = Math.imul(hash, 16777619);
  }
  return hash >>> 0;
}

const pressureWeight: Record<EntitlementPressure, number> = {
  plenty: 4,
  normal: 2,
  unknown: 1,
  low: 0.25,
  exhausted: 0,
};

function stateOf(availability: ProviderAvailability[], id: ProviderId): ProviderAvailability {
  return availability.find((entry) => entry.provider === id && entry.targetId === undefined) ?? {
    provider: id, installed: false, authenticated: false, available: false, reason: "unknown",
  };
}

function stateOfTarget(availability: ProviderAvailability[], target: RoutingTarget): ProviderAvailability {
  const exact = availability.find((entry) => entry.targetId === target.id && entry.provider === target.provider);
  if (exact) return exact;
  if ((target.authMode ?? "ambient") === "ambient") return stateOf(availability, target.provider);
  return {
    targetId: target.id, provider: target.provider, installed: false, authenticated: false,
    available: false, reason: "unknown", detail: "isolated target has not been probed",
  };
}

export function selectProviderFromAvailability(
  requested: RoutingPreference,
  availability: ProviderAvailability[],
  policy: ResourcePolicy,
  tier?: SemanticTier,
  stableKey = "default",
): RoutingDecision {
  const request = typeof requested === "string" ? { provider: requested } : requested;
  const requestedProvider = request.provider ?? "auto";
  const requestedTarget = request.target;
  const targets = orderedTargets(policy);
  const targetPressures = Object.fromEntries(targets.map((target) => [
    target.id, policy.targetPressures?.[target.id] ?? policy.pressures[target.provider] ?? "unknown",
  ])) as Record<string, EntitlementPressure>;
  const targetAvailable = (target: RoutingTarget) => stateOfTarget(availability, target).available;
  const eligible = (target: RoutingTarget) => targetAvailable(target) && targetPressures[target.id] !== "exhausted";

  let candidates: RoutingTarget[];
  if (requestedTarget !== undefined) {
    const target = targets.find(({ id }) => id === requestedTarget);
    if (!target)
      throw new ProviderSelectionError("provider_unavailable", `routing target ${requestedTarget} is not configured`);
    if (requestedProvider !== "auto" && target.provider !== requestedProvider)
      throw new ProviderSelectionError("provider_unavailable",
        `routing target ${requestedTarget} belongs to ${target.provider}, not requested provider ${requestedProvider}`);
    const state = stateOfTarget(availability, target);
    if (!state.available)
      throw new ProviderSelectionError("provider_unavailable",
        `routing target ${target.id} unavailable through ${target.provider}: ${state.reason}`);
    if (targetPressures[target.id] === "exhausted")
      throw new ProviderSelectionError("entitlement_exhausted", `routing target ${target.id} entitlement exhausted`);
    candidates = [target];
  } else if (requestedProvider !== "auto") {
    const providerTargets = targets.filter((target) => target.provider === requestedProvider);
    if (!providerTargets.length)
      throw new ProviderSelectionError("provider_unavailable", `provider ${requestedProvider} has no configured routing target`);
    candidates = providerTargets.filter(eligible);
    if (!candidates.length && providerTargets.every((target) => !targetAvailable(target))) {
      if (providerTargets.length === 1) {
        const state = stateOfTarget(availability, providerTargets[0]);
        throw new ProviderSelectionError("provider_unavailable",
          `provider ${requestedProvider} unavailable: ${state.reason}`);
      }
      const states = providerTargets.map((target) => `${target.id}=${stateOfTarget(availability, target).reason}`).join(", ");
      throw new ProviderSelectionError("provider_unavailable",
        `provider ${requestedProvider} unavailable across routing targets: ${states}`);
    }
    if (!candidates.length)
      throw new ProviderSelectionError("entitlement_exhausted",
        `provider ${requestedProvider} entitlement exhausted (all routing targets)`);
  } else {
    candidates = targets.filter(eligible);
  }

  if (!candidates.length)
    throw new ProviderSelectionError("no_provider_available",
      `no agent target available: ${targets.map((target) => `${target.id}=${stateOfTarget(availability, target).reason}/${targetPressures[target.id]}`).join(", ")}`);

  let chosen: RoutingTarget;
  let detail: string;
  const reserve = policy.reservedFrontierTarget
    ?? targets.find((target) => target.provider === policy.reservedFrontierProvider)?.id;
  if (requestedTarget !== undefined) {
    chosen = candidates[0];
    detail = `exact target=${chosen.id}`;
  } else if (policy.mode === "reserved" && reserve) {
    const reservedTarget = candidates.find(({ id }) => id === reserve);
    if (tier === "frontier" && reservedTarget) {
      chosen = reservedTarget;
      detail = `frontier reserve=${reserve}`;
    } else {
      const alternatives = candidates.filter(({ id }) => id !== reserve);
      chosen = alternatives[0] ?? candidates[0];
      detail = tier === "frontier" ? `reserve=${reserve} unavailable` : `preserving frontier reserve=${reserve}`;
    }
  } else if (policy.mode === "balanced") {
    const weighted = candidates.map((target) => ({
      target,
      weight: Math.max(0.001, (policy.targetWeights?.[target.id] ?? policy.weights?.[target.provider] ?? 1)
        * pressureWeight[targetPressures[target.id]]),
    }));
    const total = weighted.reduce((sum, item) => sum + item.weight, 0);
    let slot = (stableHash(stableKey) / 0x1_0000_0000) * total;
    chosen = weighted[weighted.length - 1].target;
    for (const item of weighted) {
      slot -= item.weight;
      if (slot < 0) { chosen = item.target; break; }
    }
    detail = `stable-key=${stableKey}; effective-weights=${weighted.map(({ target, weight }) => `${target.id}:${weight}`).join(",")}`;
  } else {
    chosen = candidates[0];
    detail = `order=${targets.map(({ id }) => id).join(" -> ")}`;
  }

  const fallbacks = requestedTarget === undefined ? candidates.filter(({ id }) => id !== chosen.id) : [];
  const selectionReason = `${requestedProvider === "auto" ? "" : `explicit provider=${requestedProvider}; `}mode=${policy.mode}; target=${chosen.id}; pressure=${targetPressures[chosen.id]}; ${detail}`;
  const decision: RoutingDecision = {
    requested: requestedProvider,
    requestedProvider,
    ...(requestedTarget === undefined ? {} : { requestedTarget }),
    target: chosen.id,
    provider: chosen.provider,
    routingTargets: Object.fromEntries(targets.map((target) => [target.id, target])),
    selectionReason,
    reason: selectionReason,
    availability,
    fallbackTargets: fallbacks.map(({ id }) => id),
    fallbackTargetPath: [chosen.id],
    fallbackProviders: fallbacks.map(({ provider }) => provider),
    fallbackCount: 0,
    fallbackPath: [chosen.provider],
    fallbackReasons: [],
    allocationMode: policy.mode,
    entitlementPressure: targetPressures[chosen.id],
    targetEntitlementPressures: targetPressures,
    entitlementPressures: policy.pressures,
  };
  // RoutingDecision remains live for final provider/model/pressure attribution,
  // but the explanation of the original allocator choice is provenance. Make
  // both the canonical field and compatibility alias immutable at runtime.
  Object.defineProperties(decision, {
    selectionReason: { value: selectionReason, enumerable: true, writable: false, configurable: false },
    reason: { value: selectionReason, enumerable: true, writable: false, configurable: false },
  });
  return decision;
}

export function selectProvider(
  requested?: RoutingPreference,
  policy: ResourcePolicy = resourcePolicyFromEnv(),
  context: { tier?: SemanticTier; stableKey?: string } = {},
): RoutingDecision {
  const preference = requested ?? (process.env.AGENT_PROVIDER as ProviderPreference | undefined) ?? "auto";
  const availability = orderedTargets(policy).map((target) => ({
    ...(target.provider === "anthropic" ? probeAnthropic(target) : probeOpenAI(target)),
    targetId: target.id,
  }));
  return selectProviderFromAvailability(preference, availability, policy, context.tier, context.stableKey);
}
