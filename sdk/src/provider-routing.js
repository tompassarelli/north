import { aset as $$bh$aset, js_obj as $$bh$js_obj } from 'beagle/host.js';

const catalog = require("./providers/catalog");

const resource_policy = require("./resource-policy");

const accounts = require("./accounts");

const auth_cache = require("./provider-auth-cache");

const capabilities_module = require("./orchestration-capabilities");

const spend_guard = require("./spend-guard");



































function RouteAxes(capabilityFloor, serviceClass, reasoning) {
  return Object.freeze({_tag: "RouteAxes", capabilityFloor, serviceClass, reasoning});
}

function routeaxes_capabilityFloor(r) { return r.capabilityFloor; }

function routeaxes_serviceClass(r) { return r.serviceClass; }

function routeaxes_reasoning(r) { return r.reasoning; }

const PROVIDERS = ["anthropic", "openai"];

const CACHED_AUTH_TTL_MS = 86400000;

const BOOT__ROUTING__TIMEOUT__MS = 2000;
export { BOOT__ROUTING__TIMEOUT__MS as "BOOT_ROUTING_TIMEOUT_MS" };

function route_axes(context) {
  const env = process.env;
  return RouteAxes(((context ? (() => { return context.capabilityFloor; })() : null) || env.AGENT_CAPABILITY_FLOOR || "standard"), ((context ? (() => { return context.serviceClass; })() : null) || env.AGENT_SERVICE_CLASS || "balanced"), ((context ? (() => { return context.reasoning; })() : null) || env.AGENT_REASONING || "medium"));
}

function provider_selection_error_new_bang(kind, message) {
  const error = new Error(message);
  Object.setPrototypeOf(error, provider_selection_error_new_bang.prototype);
  (error.name = "ProviderSelectionError");
  (error.kind = kind);
  (error.preSideEffect = true);
  if ((kind === "blocked_preflight")) {
    (error.processOutcome = "blocked_preflight");
  }
  return error;
}

Object.setPrototypeOf(provider_selection_error_new_bang.prototype, Error.prototype);

const ProviderSelectionError = provider_selection_error_new_bang;
export { ProviderSelectionError as "ProviderSelectionError" };



function pressure(value) {
  return (((value === "plenty") || (value === "normal") || (value === "low") || (value === "exhausted")) ? value : "unknown");
}

function foreign_field(container, key) {
  return Reflect.get(container, key);
}

function default_policy() {
  return $$bh$js_obj("version", 1, "mode", "balanced", "targets", [$$bh$js_obj("id", "anthropic", "provider", "anthropic", "authMode", "ambient"), $$bh$js_obj("id", "openai", "provider", "openai", "authMode", "ambient")], "targetOrder", ["anthropic", "openai"], "providerOrder", ["anthropic", "openai"]);
}

function targets(policy) {
  return ((policy.targets && (policy.targets.length > 0)) ? policy.targets : default_policy().targets);
}

function resource_policy_from_env(...$beagle$args) {
  if (arguments.length === 2) {
    const base = $beagle$args[0];
    const observations = $beagle$args[1];
    const foundation = (base || default_policy());
    const observed = (observations ? (resource_policy.applyProviderUsageObservations)(foundation, observations) : foundation);
    const result = Object.assign($$bh$js_obj(), observed);
    const env = process.env;
    const raw_mode = env.NORTH_ALLOCATION_MODE;
    $$bh$aset(result, "mode", (((raw_mode === "balanced") || (raw_mode === "reserved") || (raw_mode === "preferential")) ? raw_mode : (observed.mode || "balanced")));
    return result;
  }
  if (arguments.length === 1) {
    const base = $beagle$args[0];
    return resource_policy_from_env(base, (() => { try {
    return (resource_policy.loadProviderUsageObservations)();
  } catch (__error) {
    return null;
  } })());
  }
  if (arguments.length === 0) {
    return resource_policy_from_env((() => { try {
    return (resource_policy.loadResourcePolicy)();
  } catch (__error) {
    return null;
  } })());
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

function resourcePolicyFromEnv(...$beagle$args) {
  if (arguments.length === 0) {
    return resource_policy_from_env();
  }
  if (arguments.length === 1) {
    const base = $beagle$args[0];
    return resource_policy_from_env(base);
  }
  if (arguments.length === 2) {
    const base = $beagle$args[0];
    const observations = $beagle$args[1];
    return resource_policy_from_env(base, observations);
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}
export { resourcePolicyFromEnv as "resourcePolicyFromEnv" };

function availability(provider, target) {
  const env = (accounts.observeEnvironmentForTarget)(provider, target);
  const disabled = (((provider === "anthropic") ? env.NORTH_DISABLE_ANTHROPIC : env.NORTH_DISABLE_OPENAI) === "1");
  const target_id = (target ? (() => { return target.id; })() : null);
  const cache_path = (auth_cache.authStateCachePath)();
  const cached = (auth_cache.readAuthState)(cache_path, (auth_cache.authCacheKey)(provider, target_id));
  const result = $$bh$js_obj("provider", provider, "installed", (cached ? cached.installed : false), "authenticated", (cached ? cached.authenticated : false), "available", (disabled ? false : (cached ? cached.available : false)), "reason", (disabled ? "disabled" : (cached ? cached.reason : "unknown")));
  if (target_id) {
    $$bh$aset(result, "targetId", target_id);
  }
  return result;
}

function probeAnthropic(...$beagle$args) {
  if (arguments.length === 0) {
    return availability("anthropic", null);
  }
  if (arguments.length === 1) {
    const target = $beagle$args[0];
    return availability("anthropic", target);
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}
export { probeAnthropic as "probeAnthropic" };

function probeOpenAI(...$beagle$args) {
  if (arguments.length === 0) {
    return availability("openai", null);
  }
  if (arguments.length === 1) {
    const target = $beagle$args[0];
    return availability("openai", target);
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}
export { probeOpenAI as "probeOpenAI" };

function cached_availability(target, now) {
  const cache_path = (auth_cache.authStateCachePath)();
  const cached = (auth_cache.readAuthState)(cache_path, (auth_cache.authCacheKey)(target.provider, target.id));
  if (((!cached) || ((now - cached.at) > CACHED_AUTH_TTL_MS))) {
    return null;
  } else {
    const result = availability(target.provider, target);
    $$bh$aset(result, "targetId", target.id);
    return result;
  }
}

function cachedAvailability(...$beagle$args) {
  if (arguments.length === 1) {
    const target = $beagle$args[0];
    return cached_availability(target, Date.now());
  }
  if (arguments.length === 2) {
    const target = $beagle$args[0];
    const now = $beagle$args[1];
    return cached_availability(target, now);
  }
  if (arguments.length === 3) {
    const target = $beagle$args[0];
    const now = $beagle$args[1];
    const __cache_path = $beagle$args[2];
    return cached_availability(target, now);
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}
export { cachedAvailability as "cachedAvailability" };

function cached_routing(policy, now) {
  return targets(policy).map((target) => { const state = cached_availability(target, now);
const headroom = pressure((policy.targetPressures ? (() => { return foreign_field(policy.targetPressures, target.id); })() : null));
return $$bh$js_obj("target", target, "headroom", headroom, "eligible", (state && state.available && (!(headroom === "exhausted")))); });
}

function cachedTargetRouting(...$beagle$args) {
  if (arguments.length === 0) {
    return cached_routing(resource_policy_from_env(), Date.now());
  }
  if (arguments.length === 1) {
    const policy = $beagle$args[0];
    return cached_routing(policy, Date.now());
  }
  if (arguments.length === 2) {
    const policy = $beagle$args[0];
    const now = $beagle$args[1];
    return cached_routing(policy, now);
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}
export { cachedTargetRouting as "cachedTargetRouting" };

function configuredDefaultTarget(...$beagle$args) {
  if (arguments.length === 1) {
    const provider = $beagle$args[0];
    return configuredDefaultTarget(provider, resource_policy_from_env());
  }
  if (arguments.length === 2) {
    const provider = $beagle$args[0];
    const policy = $beagle$args[1];
    return targets(policy).find((target) => (target.provider === provider));
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}
export { configuredDefaultTarget as "configuredDefaultTarget" };

function state_for(states, target) {
  return (states.find((state) => (state.targetId ? (state.targetId === target.id) : (state.provider === target.provider))) || $$bh$js_obj("targetId", target.id, "provider", target.provider, "installed", false, "authenticated", false, "available", false, "reason", "unknown"));
}

function supports_p(target, axes, model, capabilities) {
  return ((catalog.providerSupportsRoute)(target.provider, routeaxes_capabilityFloor(axes), routeaxes_serviceClass(axes), routeaxes_reasoning(axes), model) && (catalog.providerSupportsModel)(target.provider, model) && (capabilities_module.providerSupportsCapabilities)(target.provider, capabilities));
}

function select_route(requested, states, policy, axes, stable_key, model, capabilities) {
  const request = ((typeof requested === "string") ? $$bh$js_obj("provider", requested) : Object.assign($$bh$js_obj(), requested));
  const requested_provider = (request.provider || "auto");
  const candidates = array();
  (() => { targets(policy).forEach((target) => {
  const state = state_for(states, target);
  const target_pressure = pressure((policy.targetPressures ? (() => { return foreign_field(policy.targetPressures, target.id); })() : null));
  if ((((!request.target) || (request.target === target.id)) && ((requested_provider === "auto") || (requested_provider === target.provider)) && supports_p(target, axes, model, capabilities) && state.available && (!(target_pressure === "exhausted")) && (spend_guard.spendGuardEligible)(target.provider, target.id))) {
    candidates.push(target);
  }
}); })();
  if ((candidates.length === 0)) {
    (() => { throw new ProviderSelectionError((model ? "blocked_preflight" : "no_provider_available"), ("".concat("no eligible provider resolves capabilityFloor=", routeaxes_capabilityFloor(axes), " serviceClass=", routeaxes_serviceClass(axes), " reasoning=", routeaxes_reasoning(axes)))); })();
  }
  const chosen = aget(candidates, 0);
  const routing_targets = $$bh$js_obj();
  const target_pressures = $$bh$js_obj();
  (() => { targets(policy).forEach((target) => {
  $$bh$aset(routing_targets, target.id, Object.freeze(target));
  $$bh$aset(target_pressures, target.id, pressure((policy.targetPressures ? (() => { return foreign_field(policy.targetPressures, target.id); })() : null)));
}); })();
  return $$bh$js_obj("requestedProvider", requested_provider, "target", chosen.id, "provider", chosen.provider, "routingTargets", Object.freeze(routing_targets), "selectionReason", ("".concat("route=", routeaxes_capabilityFloor(axes), "/", routeaxes_serviceClass(axes), "/", routeaxes_reasoning(axes), "; target=", chosen.id, "; stable-key=", stable_key)), "availability", states, "fallbackTargets", [], "fallbackTargetPath", [chosen.id], "fallbackProviders", [], "fallbackCount", 0, "fallbackPath", [chosen.provider], "fallbackReasons", [], "allocationMode", (policy.mode || "balanced"), "entitlementPressure", aget(target_pressures, chosen.id), "targetEntitlementPressures", target_pressures);
}

function selectProviderFromAvailability(...$beagle$args) {
  if (arguments.length === 3) {
    const requested = $beagle$args[0];
    const states = $beagle$args[1];
    const policy = $beagle$args[2];
    return select_route(requested, states, policy, RouteAxes("standard", "balanced", "medium"), "default", null, null);
  }
  if (arguments.length === 9) {
    const requested = $beagle$args[0];
    const states = $beagle$args[1];
    const policy = $beagle$args[2];
    const capability_floor = $beagle$args[3];
    const service_class = $beagle$args[4];
    const stable_key = $beagle$args[5];
    const reasoning = $beagle$args[6];
    const model = $beagle$args[7];
    const capabilities = $beagle$args[8];
    return select_route(requested, states, policy, RouteAxes(capability_floor, service_class, reasoning), (stable_key || "default"), model, capabilities);
  }
  if (arguments.length === 10) {
    const requested = $beagle$args[0];
    const states = $beagle$args[1];
    const policy = $beagle$args[2];
    const capability_floor = $beagle$args[3];
    const service_class = $beagle$args[4];
    const stable_key = $beagle$args[5];
    const reasoning = $beagle$args[6];
    const model = $beagle$args[7];
    const capabilities = $beagle$args[8];
    const __evidence = $beagle$args[9];
    return select_route(requested, states, policy, RouteAxes(capability_floor, service_class, reasoning), (stable_key || "default"), model, capabilities);
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}
export { selectProviderFromAvailability as "selectProviderFromAvailability" };

function requested_value(requested) {
  return (requested || process.env.AGENT_PROVIDER || "auto");
}

function request_object(requested) {
  const value = requested_value(requested);
  return ((typeof value === "string") ? $$bh$js_obj("provider", value) : Object.assign($$bh$js_obj(), value));
}

function selected_states(request, policy) {
  const provider = (request.provider || "auto");
  return targets(policy).filter((target) => (request.target ? (request.target === target.id) : ((!(provider === "auto")) ? (provider === target.provider) : true))).map((target) => availability(target.provider, target));
}

function select_provider_route(requested, policy, context) {
  const value = requested_value(requested);
  const request = request_object(requested);
  const axes = route_axes(context);
  return select_route(value, selected_states(request, policy), policy, axes, (context.stableKey || "default"), context.model, context.capabilities);
}

function selectProvider(...$beagle$args) {
  if (arguments.length === 0) {
    return select_provider_route(null, resource_policy_from_env(), $$bh$js_obj());
  }
  if (arguments.length === 1) {
    const requested = $beagle$args[0];
    return select_provider_route(requested, resource_policy_from_env(), $$bh$js_obj());
  }
  if (arguments.length === 2) {
    const requested = $beagle$args[0];
    const policy = $beagle$args[1];
    return select_provider_route(requested, policy, $$bh$js_obj());
  }
  if (arguments.length === 3) {
    const requested = $beagle$args[0];
    const policy = $beagle$args[1];
    const context = $beagle$args[2];
    return select_provider_route(requested, policy, context);
  }
  if (arguments.length === 4) {
    const requested = $beagle$args[0];
    const policy = $beagle$args[1];
    const context = $beagle$args[2];
    const __dependencies = $beagle$args[3];
    return select_provider_route(requested, policy, context);
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}
export { selectProvider as "selectProvider" };

function selectProviderFromCachedState(...$beagle$args) {
  if (arguments.length === 1) {
    const requested = $beagle$args[0];
    return selectProviderFromCachedState(requested, resource_policy_from_env(), $$bh$js_obj(), Date.now());
  }
  if (arguments.length === 2) {
    const requested = $beagle$args[0];
    const policy = $beagle$args[1];
    return selectProviderFromCachedState(requested, policy, $$bh$js_obj(), Date.now());
  }
  if (arguments.length === 3) {
    const requested = $beagle$args[0];
    const policy = $beagle$args[1];
    const context = $beagle$args[2];
    return selectProviderFromCachedState(requested, policy, context, Date.now());
  }
  if (arguments.length === 4) {
    const requested = $beagle$args[0];
    const policy = $beagle$args[1];
    const context = $beagle$args[2];
    const now = $beagle$args[3];
    const value = requested_value(requested);
    const request = request_object(requested);
    const states = targets(policy).filter((target) => (request.target ? (request.target === target.id) : (((request.provider || "auto") === "auto") || (request.provider === target.provider)))).map((target) => (cached_availability(target, now) || $$bh$js_obj("targetId", target.id, "provider", target.provider, "installed", false, "authenticated", false, "available", false, "reason", "unknown")));
    const axes = route_axes(context);
    return Promise.resolve((() => { try {
    return select_route(value, states, policy, axes, (context.stableKey || "default"), context.model, context.capabilities);
  } catch (__error) {
    return null;
  } })());
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}
export { selectProviderFromCachedState as "selectProviderFromCachedState" };

const refresh_promise = ({value: null, watches: {}});

function refresh_provider_routing_bang(requested, refresh) {
  const current = refresh_promise.value;
  if (current) {
    return current;
  } else {
    const pending = refresh(requested).then((__value) => null).catch((__error) => null).finally(() => { (() => { const _a = refresh_promise, _v = null; const _old = _a.value; _a.value = _v; for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _v); return _v; })();
return null; });
    (() => { const _a = refresh_promise, _v = pending; const _old = _a.value; _a.value = _v; for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _v); return _v; })();
    return pending;
  }
}

function refresh_provider_routing_dispatch_bang(...$beagle$args) {
  if (arguments.length === 1) {
    const requested = $beagle$args[0];
    return refresh_provider_routing_bang(requested, (value) => selectProviderForExecution(value));
  }
  if (arguments.length === 2) {
    const requested = $beagle$args[0];
    const refresh = $beagle$args[1];
    return refresh_provider_routing_bang(requested, refresh);
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

const refreshProviderRoutingInBackground = refresh_provider_routing_dispatch_bang;
export { refreshProviderRoutingInBackground as "refreshProviderRoutingInBackground" };

function balancedAllocationEstimates(...$beagle$args) {
  if (arguments.length === 2) {
    const states = $beagle$args[0];
    const policy = $beagle$args[1];
    return balancedAllocationEstimates(states, policy, "standard", "balanced", "medium", null, null);
  }
  if (arguments.length === 7) {
    const states = $beagle$args[0];
    const policy = $beagle$args[1];
    const capability_floor = $beagle$args[2];
    const service_class = $beagle$args[3];
    const reasoning = $beagle$args[4];
    const model = $beagle$args[5];
    const capabilities = $beagle$args[6];
    const axes = RouteAxes(capability_floor, service_class, reasoning);
    const estimates = targets(policy).map((target) => { const state = state_for(states, target);
const target_pressure = pressure((policy.targetPressures ? (() => { return foreign_field(policy.targetPressures, target.id); })() : null));
const eligible = (supports_p(target, axes, model, capabilities) && state.available && (!(target_pressure === "exhausted")));
return $$bh$js_obj("target", target.id, "provider", target.provider, "eligible", eligible, "pressure", target_pressure, "effectiveWeight", (eligible ? 1 : 0), "approximateShare", 0, "allocationEvidence", $$bh$js_obj("kind", "policy-pressure", "pressure", target_pressure)); });
    const count = estimates.filter((item) => item.eligible).length;
    return estimates.map((item) => { if (item.eligible) {
  $$bh$aset(item, "approximateShare", (1 / count));
}
return item; });
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}
export { balancedAllocationEstimates as "balancedAllocationEstimates" };

function collectExecutionModelRefreshAttempts(...$beagle$args) {
  if (arguments.length === 3) {
    const probe_targets = $beagle$args[0];
    const required_targets = $beagle$args[1];
    const __preference = $beagle$args[2];
    return Promise.resolve([]);
  }
  if (arguments.length === 4) {
    const probe_targets = $beagle$args[0];
    const required_targets = $beagle$args[1];
    const __preference = $beagle$args[2];
    const __refresh = $beagle$args[3];
    return Promise.resolve([]);
  }
  if (arguments.length === 5) {
    const probe_targets = $beagle$args[0];
    const required_targets = $beagle$args[1];
    const __preference = $beagle$args[2];
    const __refresh = $beagle$args[3];
    const __signal = $beagle$args[4];
    return Promise.resolve([]);
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}
export { collectExecutionModelRefreshAttempts as "collectExecutionModelRefreshAttempts" };

function selectProviderForExecution(...$beagle$args) {
  if (arguments.length === 0) {
    return Promise.resolve(select_provider_route(null, resource_policy_from_env(), $$bh$js_obj()));
  }
  if (arguments.length === 1) {
    const requested = $beagle$args[0];
    return Promise.resolve(select_provider_route(requested, resource_policy_from_env(), $$bh$js_obj()));
  }
  if (arguments.length === 2) {
    const requested = $beagle$args[0];
    const policy = $beagle$args[1];
    return Promise.resolve(select_provider_route(requested, policy, $$bh$js_obj()));
  }
  if (arguments.length === 3) {
    const requested = $beagle$args[0];
    const policy = $beagle$args[1];
    const context = $beagle$args[2];
    return Promise.resolve(select_provider_route(requested, policy, context));
  }
  if (arguments.length === 4) {
    const requested = $beagle$args[0];
    const policy = $beagle$args[1];
    const context = $beagle$args[2];
    const __dependencies = $beagle$args[3];
    return Promise.resolve(select_provider_route(requested, policy, context));
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}
export { selectProviderForExecution as "selectProviderForExecution" };
