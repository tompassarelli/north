import { record_value as $$bc$record_value, str as $$bc$str } from './bridge/generated/beagle/core.js';
import { admit_host_array as $$bh$admit_host_array, admit_host_object as $$bh$admit_host_object, aget as $$bh$aget, array as $$bh$array, aset as $$bh$aset, js_obj as $$bh$js_obj } from './bridge/generated/beagle/host.js';
import { catch_dispatch as $$bd$catch_dispatch } from './bridge/generated/beagle/exception-dispatch.js';

const catalog = require("./providers/catalog");

const resource_policy = require("./resource-policy");

const accounts = require("./accounts");

const auth_cache = require("./provider-auth-cache");

const capabilities_module = require("./orchestration-capabilities");

const spend_guard = require("./spend-guard");

function RouteAxes(capabilityFloor, serviceClass, reasoning) {
  return $$bc$record_value("north.provider-routing/RouteAxes", {_tag: "RouteAxes", capabilityFloor, serviceClass, reasoning});
}

function routeaxes_capabilityFloor(r) { return r.capabilityFloor; }

function routeaxes_serviceClass(r) { return r.serviceClass; }

function routeaxes_reasoning(r) { return r.reasoning; }

const PROVIDERS = ["anthropic", "openai"];

const CACHED_AUTH_TTL_MS = 86400000;

const BOOT__ROUTING__TIMEOUT__MS = 2000;

function route_axes(context) {
  const env = process.env;
  return RouteAxes(((_logical) => (_logical !== false && _logical != null ? _logical : ((_logical) => (_logical !== false && _logical != null ? _logical : "standard"))(env.AGENT_CAPABILITY_FLOOR)))((((_truthy) => _truthy !== false && _truthy != null)(context) ? (() => { return context.capabilityFloor; })() : null)), ((_logical) => (_logical !== false && _logical != null ? _logical : ((_logical) => (_logical !== false && _logical != null ? _logical : "balanced"))(env.AGENT_SERVICE_CLASS)))((((_truthy) => _truthy !== false && _truthy != null)(context) ? (() => { return context.serviceClass; })() : null)), ((_logical) => (_logical !== false && _logical != null ? _logical : ((_logical) => (_logical !== false && _logical != null ? _logical : "medium"))(env.AGENT_REASONING)))((((_truthy) => _truthy !== false && _truthy != null)(context) ? (() => { return context.reasoning; })() : null)));
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

function pressure(value) {
  return (((value === "plenty") || ((value === "normal") || ((value === "low") || (value === "exhausted")))) ? value : "unknown");
}

function foreign_field(container, key) {
  return Reflect.get(container, key);
}

function default_policy() {
  return $$bh$js_obj("version", 1, "mode", "balanced", "targets", [$$bh$js_obj("id", "anthropic", "provider", "anthropic", "authMode", "ambient"), $$bh$js_obj("id", "openai", "provider", "openai", "authMode", "ambient")], "targetOrder", ["anthropic", "openai"], "providerOrder", ["anthropic", "openai"]);
}

function targets(policy) {
  return (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (policy.targets.length > 0) : _logical))(policy.targets)) ? policy.targets : default_policy().targets);
}

function resource_policy_from_env(...$beagle$args) {
  if (arguments.length === 2) {
    const base = $beagle$args[0];
    const observations = $beagle$args[1];
    const foundation = ((_logical) => (_logical !== false && _logical != null ? _logical : default_policy()))(base);
    const observed = (((_truthy) => _truthy !== false && _truthy != null)(observations) ? (resource_policy.applyProviderUsageObservations)(foundation, observations) : foundation);
    const result = Object.assign($$bh$js_obj(), observed);
    const env = process.env;
    const raw_mode = env.NORTH_ALLOCATION_MODE;
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(result, "mode", (((raw_mode === "balanced") || ((raw_mode === "reserved") || (raw_mode === "preferential"))) ? raw_mode : ((_logical) => (_logical !== false && _logical != null ? _logical : "balanced"))(observed.mode)));
    return result;
  }
  if (arguments.length === 1) {
    const base = $beagle$args[0];
    return resource_policy_from_env(base, (() => { try {
    return (resource_policy.loadProviderUsageObservations)();
  } catch (_catch_0) {
    switch ($$bd$catch_dispatch(_catch_0, [Error])) {
      case 0: {
        const __error = _catch_0;
        return null;
        break;
      }
    }
  } })());
  }
  if (arguments.length === 0) {
    return resource_policy_from_env((() => { try {
    return (resource_policy.loadResourcePolicy)();
  } catch (_catch_1) {
    switch ($$bd$catch_dispatch(_catch_1, [Error])) {
      case 0: {
        const __error = _catch_1;
        return null;
        break;
      }
    }
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

function availability(provider, target) {
  const env = (accounts.observeEnvironmentForTarget)(provider, target);
  const disabled = (((provider === "anthropic") ? env.NORTH_DISABLE_ANTHROPIC : env.NORTH_DISABLE_OPENAI) === "1");
  const target_id = (((_truthy) => _truthy !== false && _truthy != null)(target) ? (() => { return target.id; })() : null);
  const cache_path = (auth_cache.authStateCachePath)();
  const cached = (auth_cache.readAuthState)(cache_path, (auth_cache.authCacheKey)(provider, target_id));
  const result = $$bh$js_obj("provider", provider, "installed", (((_truthy) => _truthy !== false && _truthy != null)(cached) ? cached.installed : false), "authenticated", (((_truthy) => _truthy !== false && _truthy != null)(cached) ? cached.authenticated : false), "available", (disabled ? false : (((_truthy) => _truthy !== false && _truthy != null)(cached) ? cached.available : false)), "reason", (disabled ? "disabled" : (((_truthy) => _truthy !== false && _truthy != null)(cached) ? cached.reason : "unknown")));
  if (((_truthy) => _truthy !== false && _truthy != null)(target_id)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(result, "targetId", target_id);
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

function cached_availability(target, now) {
  const cache_path = (auth_cache.authStateCachePath)();
  const cached = (auth_cache.readAuthState)(cache_path, (auth_cache.authCacheKey)(target.provider, target.id));
  if (((!((_truthy) => _truthy !== false && _truthy != null)(cached)) || ((now - cached.at) > CACHED_AUTH_TTL_MS))) {
    return null;
  } else {
    const result = availability(target.provider, target);
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(result, "targetId", target.id);
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

function cached_routing(policy, now) {
  return targets(policy).map((target) => { const state = cached_availability(target, now);
const headroom = pressure((((_truthy) => _truthy !== false && _truthy != null)(policy.targetPressures) ? (() => { return foreign_field(policy.targetPressures, target.id); })() : null));
return $$bh$js_obj("target", target, "headroom", headroom, "eligible", ((_logical) => (_logical !== false && _logical != null ? ((_logical) => (_logical !== false && _logical != null ? (!(headroom === "exhausted")) : _logical))(state.available) : _logical))(state)); });
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

function state_for(states, target) {
  return ((_logical) => (_logical !== false && _logical != null ? _logical : $$bh$js_obj("targetId", target.id, "provider", target.provider, "installed", false, "authenticated", false, "available", false, "reason", "unknown")))(states.find((state) => (((_truthy) => _truthy !== false && _truthy != null)(state.targetId) ? (state.targetId === target.id) : (state.provider === target.provider))));
}

function supports_p(target, axes, model, capabilities) {
  return ((_logical) => (_logical !== false && _logical != null ? ((_logical) => (_logical !== false && _logical != null ? (capabilities_module.providerSupportsCapabilities)(target.provider, capabilities) : _logical))((catalog.providerSupportsModel)(target.provider, model)) : _logical))((catalog.providerSupportsRoute)(target.provider, routeaxes_capabilityFloor(axes), routeaxes_serviceClass(axes), routeaxes_reasoning(axes), model));
}

function select_route(requested, states, policy, axes, stable_key, model, capabilities) {
  const request = ((typeof requested === "string") ? $$bh$js_obj("provider", requested) : Object.assign($$bh$js_obj(), requested));
  const requested_provider = ((_logical) => (_logical !== false && _logical != null ? _logical : "auto"))(request.provider);
  const candidates = $$bh$array();
  targets(policy).forEach((target) => {
  const state = state_for(states, target);
  const target_pressure = pressure((((_truthy) => _truthy !== false && _truthy != null)(policy.targetPressures) ? (() => { return foreign_field(policy.targetPressures, target.id); })() : null));
  if (((_truthy) => _truthy !== false && _truthy != null)((((!((_truthy) => _truthy !== false && _truthy != null)(request.target)) || (request.target === target.id)) && (((requested_provider === "auto") || (requested_provider === target.provider)) && (supports_p(target, axes, model, capabilities) && ((_logical) => (_logical !== false && _logical != null ? ((!(target_pressure === "exhausted")) && (spend_guard.spendGuardEligible)(target.provider, target.id)) : _logical))(state.available)))))) {
    candidates.push(target);
  }
});
  if ((candidates.length === 0)) {
    (() => { throw new ProviderSelectionError((((_truthy) => _truthy !== false && _truthy != null)(model) ? "blocked_preflight" : "no_provider_available"), $$bc$str("no eligible provider resolves capabilityFloor=", routeaxes_capabilityFloor(axes), " serviceClass=", routeaxes_serviceClass(axes), " reasoning=", routeaxes_reasoning(axes))); })();
  }
  const chosen = (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_array($beagle$host$arg$0), $beagle$host$arg$1))(candidates, 0);
  const routing_targets = $$bh$js_obj();
  const target_pressures = $$bh$js_obj();
  targets(policy).forEach((target) => {
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(routing_targets, target.id, Object.freeze(target));
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(target_pressures, target.id, pressure((((_truthy) => _truthy !== false && _truthy != null)(policy.targetPressures) ? (() => { return foreign_field(policy.targetPressures, target.id); })() : null)));
});
  return $$bh$js_obj("requestedProvider", requested_provider, "target", chosen.id, "provider", chosen.provider, "routingTargets", Object.freeze(routing_targets), "selectionReason", $$bc$str("route=", routeaxes_capabilityFloor(axes), "/", routeaxes_serviceClass(axes), "/", routeaxes_reasoning(axes), "; target=", chosen.id, "; stable-key=", stable_key), "availability", states, "fallbackTargets", [], "fallbackTargetPath", [chosen.id], "fallbackProviders", [], "fallbackCount", 0, "fallbackPath", [chosen.provider], "fallbackReasons", [], "allocationMode", ((_logical) => (_logical !== false && _logical != null ? _logical : "balanced"))(policy.mode), "entitlementPressure", (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(target_pressures, chosen.id), "targetEntitlementPressures", target_pressures);
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
    return select_route(requested, states, policy, RouteAxes(capability_floor, service_class, reasoning), ((_logical) => (_logical !== false && _logical != null ? _logical : "default"))(stable_key), model, capabilities);
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
    return select_route(requested, states, policy, RouteAxes(capability_floor, service_class, reasoning), ((_logical) => (_logical !== false && _logical != null ? _logical : "default"))(stable_key), model, capabilities);
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

function requested_value(requested) {
  return ((_logical) => (_logical !== false && _logical != null ? _logical : ((_logical) => (_logical !== false && _logical != null ? _logical : "auto"))(process.env.AGENT_PROVIDER)))(requested);
}

function request_object(requested) {
  const value = requested_value(requested);
  return ((typeof value === "string") ? $$bh$js_obj("provider", value) : Object.assign($$bh$js_obj(), value));
}

function selected_states(request, policy) {
  const provider = ((_logical) => (_logical !== false && _logical != null ? _logical : "auto"))(request.provider);
  return targets(policy).filter((target) => (((_truthy) => _truthy !== false && _truthy != null)(request.target) ? (request.target === target.id) : ((!(provider === "auto")) ? (provider === target.provider) : true))).map((target) => availability(target.provider, target));
}

function select_provider_route(requested, policy, context) {
  const value = requested_value(requested);
  const request = request_object(requested);
  const axes = route_axes(context);
  return select_route(value, selected_states(request, policy), policy, axes, ((_logical) => (_logical !== false && _logical != null ? _logical : "default"))(context.stableKey), context.model, context.capabilities);
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
    const states = targets(policy).filter((target) => (((_truthy) => _truthy !== false && _truthy != null)(request.target) ? (request.target === target.id) : ((((_logical) => (_logical !== false && _logical != null ? _logical : "auto"))(request.provider) === "auto") || (request.provider === target.provider)))).map((target) => ((_logical) => (_logical !== false && _logical != null ? _logical : $$bh$js_obj("targetId", target.id, "provider", target.provider, "installed", false, "authenticated", false, "available", false, "reason", "unknown")))(cached_availability(target, now)));
    const axes = route_axes(context);
    return Promise.resolve((() => { try {
    return select_route(value, states, policy, axes, ((_logical) => (_logical !== false && _logical != null ? _logical : "default"))(context.stableKey), context.model, context.capabilities);
  } catch (_catch_2) {
    switch ($$bd$catch_dispatch(_catch_2, [Error])) {
      case 0: {
        const __error = _catch_2;
        return null;
        break;
      }
    }
  } })());
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

const refresh_promise = ({value: null, watches: {}});

function refresh_provider_routing_bang(requested, refresh) {
  const current = refresh_promise.value;
  if (((_truthy) => _truthy !== false && _truthy != null)(current)) {
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
const target_pressure = pressure((((_truthy) => _truthy !== false && _truthy != null)(policy.targetPressures) ? (() => { return foreign_field(policy.targetPressures, target.id); })() : null));
const eligible = (supports_p(target, axes, model, capabilities) && ((_logical) => (_logical !== false && _logical != null ? (!(target_pressure === "exhausted")) : _logical))(state.available));
return $$bh$js_obj("target", target.id, "provider", target.provider, "eligible", eligible, "pressure", target_pressure, "effectiveWeight", (eligible ? 1 : 0), "approximateShare", 0, "allocationEvidence", $$bh$js_obj("kind", "policy-pressure", "pressure", target_pressure)); });
    const count = estimates.filter((item) => item.eligible).length;
    return estimates.map((item) => { if (((_truthy) => _truthy !== false && _truthy != null)(item.eligible)) {
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(item, "approximateShare", (1 / count));
}
return item; });
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

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

export { BOOT__ROUTING__TIMEOUT__MS as "BOOT_ROUTING_TIMEOUT_MS" };
export { ProviderSelectionError as "ProviderSelectionError" };
export { balancedAllocationEstimates as "balancedAllocationEstimates" };
export { cachedAvailability as "cachedAvailability" };
export { cachedTargetRouting as "cachedTargetRouting" };
export { collectExecutionModelRefreshAttempts as "collectExecutionModelRefreshAttempts" };
export { configuredDefaultTarget as "configuredDefaultTarget" };
export { probeAnthropic as "probeAnthropic" };
export { probeOpenAI as "probeOpenAI" };
export { refreshProviderRoutingInBackground as "refreshProviderRoutingInBackground" };
export { resourcePolicyFromEnv as "resourcePolicyFromEnv" };
export { selectProvider as "selectProvider" };
export { selectProviderForExecution as "selectProviderForExecution" };
export { selectProviderFromAvailability as "selectProviderFromAvailability" };
export { selectProviderFromCachedState as "selectProviderFromCachedState" };
