import { str as $$bc$str } from '../bridge/generated/beagle/core.js';
import { admit_host_object as $$bh$admit_host_object, aget as $$bh$aget, array as $$bh$array, aset as $$bh$aset, js_obj as $$bh$js_obj } from '../bridge/generated/beagle/host.js';
import { catch_dispatch as $$bd$catch_dispatch } from '../bridge/generated/beagle/exception-dispatch.js';

const execution_admission_module = require("../execution-admission");

const markExecutionAdmission = execution_admission_module.markExecutionAdmission;

const execution_activity_module = require("../execution-activity");

const createExecutionActivityEmitter = execution_activity_module.createExecutionActivityEmitter;

const forwardExecutionActivity = execution_activity_module.forwardExecutionActivity;

const harness_module = require("../harness");

const applyHarnessRoute = harness_module.applyHarnessRoute;

const harnessRouteSeed = harness_module.harnessRouteSeed;

const wire_query_module = require("../wire/query");

const wireQueryRoute = wire_query_module.wireQueryRoute;

const authority_module = require("./authority");

const compileProviderAuthoritySurface = authority_module.compileProviderAuthoritySurface;

const catalog_module = require("./catalog");

const observeProviderContextWindow = catalog_module.observeProviderContextWindow;

const resolveRoute = catalog_module.resolveRoute;

const types_module = require("./types");

const isProvedUnsentPreacceptFailure = types_module.isProvedUnsentPreacceptFailure;

const ProviderEscalationUnsupportedError = types_module.ProviderEscalationUnsupportedError;

const ProviderRetrySafeError = types_module.ProviderRetrySafeError;

function async_iterator(source) {
  return Reflect.apply((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(source, Symbol.asyncIterator), source, $$bh$array());
}

async function read_replay_source_bang(state) {
  if (((_truthy) => _truthy !== false && _truthy != null)(state.done)) {
    return $$bh$js_obj("done", true, "value", undefined);
  } else {
    if ((state.pending === undefined)) {
      (state.pending = state.source.next());
    }
    const item = await state.pending;
    (state.pending = undefined);
    if (((_truthy) => _truthy !== false && _truthy != null)(item.done)) {
      (state.done = true);
      return item;
    } else {
      if (((!(item.value.kind === "user.input")) || (!(typeof item.value.text === "string")))) {
        (() => { throw new TypeError("wire query input message must contain kind=user.input and text"); })();
      }
      const message = Object.freeze($$bh$js_obj("kind", "user.input", "text", item.value.text));
      state.cache.push(message);
      return $$bh$js_obj("done", false, "value", message);
    }
  }
}

async function replay_next_bang(state, cursor) {
  if ((cursor.index < state.cache.length)) {
    const value = $$bh$aget(state.cache, cursor.index);
    (cursor.index = (cursor.index + 1));
    return $$bh$js_obj("done", false, "value", value);
  } else {
    const item = await read_replay_source_bang(state);
    if ((!((_truthy) => _truthy !== false && _truthy != null)(item.done))) {
      (cursor.index = (cursor.index + 1));
    }
    return item;
  }
}

function replayable_input_bang(input) {
  if ((typeof input === 'string')) {
    return input;
  } else {
    const state = $$bh$js_obj("source", async_iterator(input), "cache", [], "done", false, "pending", undefined);
    const replayable = $$bh$js_obj();
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(replayable, Symbol.asyncIterator, () => { const cursor = $$bh$js_obj("index", 0);
const iterator = $$bh$js_obj();
(($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(iterator, "next", () => replay_next_bang(state, cursor));
(($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(iterator, Symbol.asyncIterator, () => iterator);
return iterator; });
    return replayable;
  }
}

function capability_class(authority) {
  const capabilities = (((_truthy) => _truthy !== false && _truthy != null)(authority) ? ((_logical) => (_logical !== false && _logical != null ? _logical : []))(authority.capabilities) : []);
  return ((((_truthy) => _truthy !== false && _truthy != null)(capabilities.includes("coordination"))) ? "orchestrator" : (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? _logical : capabilities.includes("shell")))(capabilities.includes("filesystem.write")))) ? "authoring" : (((_truthy) => _truthy !== false && _truthy != null)(capabilities.includes("web"))) ? "readonly-web" : (((_truthy) => _truthy !== false && _truthy != null)(capabilities.some((capability) => ((capability === "filesystem.read") || ((capability === "filesystem.search") || (capability === "shell.readonly")))))) ? "readonly" : "unknown");
}

function checkpoint_unchanged_p(writer, checkpoint) {
  const current = writer.snapshot();
  return Boolean(((current === checkpoint) || ((_logical) => (_logical !== false && _logical != null ? ((current.lastSequence === checkpoint.lastSequence) && (current.lastEventId === checkpoint.lastEventId)) : _logical))(current)));
}

function publish_bang(state, event) {
  state.providerEventListeners.forEach((listener) => { (() => { try {
    return listener(event);
  } catch (_catch_0) {
    switch ($$bd$catch_dispatch(_catch_0, [Error])) {
      case 0: {
        const __ = _catch_0;
        return null;
        break;
      }
    }
  } })();
return null; });
  return null;
}

function detach_active_observers_bang(state) {
  (state.stopActivity)();
  (state.stopActivity = () => null);
  (state.stopProviderEvents)();
  (state.stopProviderEvents = () => null);
  (state.activePublishesEvents = false);
  return null;
}

function attach_active_observers_bang(state, query, iteration) {
  detach_active_observers_bang(state);
  (state.stopActivity = forwardExecutionActivity(query.executionActivity, state.activity));
  (state.activePublishesEvents = (!(query.subscribeProviderEvents === undefined)));
  if (((_truthy) => _truthy !== false && _truthy != null)(query.subscribeProviderEvents)) {
    (state.stopProviderEvents = query.subscribeProviderEvents((event) => { (iteration.adapterEvents = (iteration.adapterEvents + 1));
return publish_bang(state, event); }));
  }
  return null;
}

function routing_request(options) {
  return ((_logical) => (_logical !== false && _logical != null ? _logical : null))(options.northRoutingRequest);
}

function resolved_route(state, provider, model) {
  const request = routing_request(state.args.options);
  const capability_floor = (((_truthy) => _truthy !== false && _truthy != null)(request) ? ((_logical) => (_logical !== false && _logical != null ? _logical : "standard"))(request.capabilityFloor) : "standard");
  const service_class = (((_truthy) => _truthy !== false && _truthy != null)(request) ? ((_logical) => (_logical !== false && _logical != null ? _logical : "balanced"))(request.serviceClass) : "balanced");
  const reasoning = (((_truthy) => _truthy !== false && _truthy != null)(request) ? ((_logical) => (_logical !== false && _logical != null ? _logical : ((_logical) => (_logical !== false && _logical != null ? _logical : "medium"))(state.requestedReasoning)))(request.reasoning) : ((_logical) => (_logical !== false && _logical != null ? _logical : "medium"))(state.requestedReasoning));
  return resolveRoute(provider, capability_floor, service_class, model, reasoning);
}

function options_for_bang(state, provider) {
  const seed = state.seed;
  const preserve_seed = Boolean(((state.decision.fallbackCount === 0) || ((_logical) => (_logical !== false && _logical != null ? (seed.provider === provider) : _logical))(seed)));
  const resolved = (preserve_seed ? $$bh$js_obj("model", (((_truthy) => _truthy !== false && _truthy != null)(seed) ? seed.model : undefined), "effort", state.requestedReasoning) : resolved_route(state, provider, null));
  const receipts = ((_logical) => (_logical !== false && _logical != null ? _logical : null))(state.decision.modelAvailabilityReceipts);
  const rebuilt = applyHarnessRoute(state.args.options, provider, resolved.model, resolved.effort, $$bh$js_obj("targetId", state.decision.target, "receipt", (((_truthy) => _truthy !== false && _truthy != null)(receipts) ? (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(receipts, state.decision.target) : undefined)));
  const options = ((rebuilt.options === state.args.options) ? Object.assign($$bh$js_obj(), rebuilt.options) : rebuilt.options);
  if (((_truthy) => _truthy !== false && _truthy != null)(((rebuilt.options === state.args.options) && resolved.model))) {
    (options.model = resolved.model);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(((rebuilt.options === state.args.options) && resolved.effort))) {
    (options.effort = resolved.effort);
  }
  (state.decision.resolvedModel = ((_logical) => (_logical !== false && _logical != null ? _logical : ((_logical) => (_logical !== false && _logical != null ? _logical : undefined))(resolved.model)))(options.model));
  (state.decision.resolvedEffort = options.effort);
  return $$bh$js_obj("options", options, "evidence", rebuilt.evidence);
}

function managed_options_p(options) {
  return ((_logical) => (_logical !== false && _logical != null ? (!(options.northCapabilities === undefined)) : _logical))(Object.hasOwn(options, "northCapabilities"));
}

async function close_active_bang(state) {
  if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? state.active.close : _logical))(state.active))) {
    await state.active.close();
  }
  return null;
}

async function prepare_route_bang(state, iteration) {
  const decision = state.decision;
  const args = state.args;
  const provider_id = decision.provider;
  const route = options_for_bang(state, provider_id);
  const options = route.options;
  const provider = $$bh$aget(state.providerRegistry, provider_id);
  const managed = managed_options_p(options);
  if (((_truthy) => _truthy !== false && _truthy != null)(state.onRouteAttempt)) {
    (state.onRouteAttempt)(decision);
  }
  if ((managed && (!((_truthy) => _truthy !== false && _truthy != null)(provider.admit)))) {
    (() => { throw ProviderRetrySafeError.provedUnsent("managed_provider_admission_unavailable", $$bh$js_obj("mode", "managed", "source", "adapter_preflight", "requestBytesPrepared", 0)); })();
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(provider.admit)) {
    await Promise.resolve(provider.admit($$bh$js_obj("options", options, "target", $$bh$aget(decision.routingTargets, decision.target))));
    markExecutionAdmission(provider_id, options);
  }
  const authority = (managed ? compileProviderAuthoritySurface(provider_id, options) : null);
  if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (!(authority.provider === provider_id)) : _logical))(authority))) {
    (() => { throw ProviderRetrySafeError.provedUnsent("provider_authority_route_mismatch", $$bh$js_obj("mode", "managed", "source", "adapter_preflight", "requestBytesPrepared", 0)); })();
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(state.onRoute)) {
    await Promise.resolve((state.onRoute)(decision, route.evidence, authority));
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(state.closed)) {
    (() => { throw new Error("routed_query_closed"); })();
  }
  const semantic_effort = ((_logical) => (_logical !== false && _logical != null ? _logical : state.requestedReasoning))(options.effort);
  if ((!((_truthy) => _truthy !== false && _truthy != null)(semantic_effort))) {
    (() => { throw ProviderRetrySafeError.provedUnsent("provider_semantic_effort_unresolved", $$bh$js_obj("mode", "managed", "source", "adapter_preflight", "requestBytesPrepared", 0)); })();
  }
  const model_selection = $$bh$js_obj("provider", provider_id, "capabilityClass", capability_class(authority));
  if (((_truthy) => _truthy !== false && _truthy != null)(state.routeClass)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(model_selection, "tier", state.routeClass);
  }
  const observation = observeProviderContextWindow(provider_id, options.model);
  const route_input = $$bh$js_obj("model", Object.freeze(model_selection), "effort", semantic_effort, "attempt", (decision.fallbackCount + 1));
  if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? observation.tokens : _logical))(observation))) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(route_input, "contextWindow", observation.tokens);
  }
  const context = $$bh$js_obj("writer", args.writer, "route", wireQueryRoute(route_input));
  if (((_truthy) => _truthy !== false && _truthy != null)(args.artifacts)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(context, "artifacts", args.artifacts);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(args.eventCommitter)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(context, "eventCommitter", args.eventCommitter);
  }
  (state.active = provider.query($$bh$js_obj("input", state.input, "options", options, "target", $$bh$aget(decision.routingTargets, decision.target), "context", context)));
  attach_active_observers_bang(state, state.active, iteration);
  if (((_truthy) => _truthy !== false && _truthy != null)(state.closed)) {
    detach_active_observers_bang(state);
    await close_active_bang(state);
  }
  (iteration.source = async_iterator(state.active));
  return null;
}

function fallback_allowed_p(state, iteration, error) {
  const decision = state.decision;
  return ((decision.requestedTarget === undefined) && ((iteration.adapterEvents === 0) && ((!($$bh$aget(decision.fallbackTargets, 0) === undefined)) && ((!($$bh$aget(decision.fallbackProviders, 0) === undefined)) && isProvedUnsentPreacceptFailure(error)))));
}

async function apply_fallback_bang(state, iteration, error) {
  const decision = state.decision;
  const fallback_target = $$bh$aget(decision.fallbackTargets, 0);
  const fallback_provider = $$bh$aget(decision.fallbackProviders, 0);
  const checkpoint = iteration.checkpoint;
  const previous_target = decision.target;
  const previous_provider = decision.provider;
  detach_active_observers_bang(state);
  await close_active_bang(state);
  if ((!checkpoint_unchanged_p(state.args.writer, checkpoint))) {
    (() => { throw error; })();
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(state.beforeFallback)) {
    await Promise.resolve((state.beforeFallback)($$bh$js_obj("fromTarget", previous_target, "fromProvider", previous_provider, "fromLiveInput", $$bh$aget(state.providerRegistry, previous_provider).liveInput, "toTarget", fallback_target, "toProvider", fallback_provider, "toLiveInput", $$bh$aget(state.providerRegistry, fallback_provider).liveInput)));
  }
  if ((!checkpoint_unchanged_p(state.args.writer, checkpoint))) {
    (() => { throw error; })();
  }
  decision.fallbackTargets.shift();
  decision.fallbackProviders.shift();
  (decision.target = fallback_target);
  (decision.provider = fallback_provider);
  (decision.entitlementPressure = ((_logical) => (_logical !== false && _logical != null ? _logical : "unknown"))($$bh$aget(decision.targetEntitlementPressures, fallback_target)));
  (decision.fallbackCount = (decision.fallbackCount + 1));
  decision.fallbackTargetPath.push(fallback_target);
  decision.fallbackPath.push(fallback_provider);
  decision.fallbackReasons.push(Object.freeze($$bh$js_obj("sequence", decision.fallbackCount, "reason", "provider_retry_safe_before_acceptance", "fromTarget", previous_target, "fromProvider", previous_provider, "toTarget", fallback_target, "toProvider", fallback_provider, "phase", "preaccept", "replay", "proved_unsent", "proof", error.unsentProof)));
  const event = state.args.writer.append($$bh$js_obj("kind", "run.progress", "lifecycle", "running", "progress", $$bh$js_obj("fallback", $$bh$js_obj("fromProvider", previous_provider, "toProvider", fallback_provider, "reason", "provider_retry_safe_before_acceptance", "phase", "preaccept"))));
  (state.active = undefined);
  (iteration.source = undefined);
  (iteration.adapterEvents = 0);
  publish_bang(state, event);
  return event;
}

async function routed_next_bang(state, iteration) {
  return (((_truthy) => _truthy !== false && _truthy != null)(iteration.done) ? $$bh$js_obj("done", true, "value", undefined) : (async () => { try {
    return (async () => {  while (true) {
    if (((_truthy) => _truthy !== false && _truthy != null)(state.closed)) { return (() => { (iteration.done = true);
return $$bh$js_obj("done", true, "value", undefined); })(); } else if (((_truthy) => _truthy !== false && _truthy != null)(iteration.source)) { return await (async () => { const step = await iteration.source.next(); if (((_truthy) => _truthy !== false && _truthy != null)(step.done)) {
  (state.turnStreaming = false);
  (iteration.done = true);
  return step;
} else {
  (iteration.adapterEvents = (iteration.adapterEvents + 1));
  if ((!((_truthy) => _truthy !== false && _truthy != null)(state.activePublishesEvents))) {
    publish_bang(state, step.value);
  }
  return step;
} })(); } else if (((_truthy) => _truthy !== false && _truthy != null)(state.continuationReady)) { (state.continuationReady = false); (state.turnStreaming = true); (iteration.source = async_iterator(state.active));  continue; } else if (((_truthy) => _truthy !== false && _truthy != null)(state.active)) { return (() => { throw new Error("routed wire query requires continueTurn before another turn"); })(); } else { await (async () => { const checkpoint = state.args.writer.snapshot(); if (((!((_truthy) => _truthy !== false && _truthy != null)(checkpoint)) || (!(checkpoint.lifecycle === "running")))) {
  (() => { throw new Error("routed wire query writer is no longer running"); })();
}
(iteration.checkpoint = checkpoint);
(iteration.adapterEvents = 0);
await prepare_route_bang(state, iteration);
if ((!((_truthy) => _truthy !== false && _truthy != null)(state.closed))) {
  return (state.turnStreaming = true);
} })();  continue; }
  } })();
  } catch (_catch_1) {
    switch ($$bd$catch_dispatch(_catch_1, [Error])) {
      case 0: {
        const error = _catch_1;
        if (fallback_allowed_p(state, iteration, error)) {
          const event = await apply_fallback_bang(state, iteration, error);
          return $$bh$js_obj("done", false, "value", event);
        } else {
          (state.turnStreaming = false);
          (iteration.done = true);
          return (() => { throw error; })();
        }
        break;
      }
    }
  } })());
}

function begin_iteration_bang(state) {
  const iteration = $$bh$js_obj("source", undefined, "checkpoint", undefined, "adapterEvents", 0, "done", false);
  const iterator = $$bh$js_obj();
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(iterator, "next", () => routed_next_bang(state, iteration));
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(iterator, "return", () => { (iteration.done = true);
(state.turnStreaming = false);
return Promise.resolve($$bh$js_obj("done", true, "value", undefined)); });
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(iterator, Symbol.asyncIterator, () => iterator);
  return iterator;
}

async function continue_turn_bang(state, continued_input) {
  if (((_truthy) => _truthy !== false && _truthy != null)(state.closed)) {
    (() => { throw new Error("routed wire query is closed"); })();
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(state.turnStreaming)) {
    (() => { throw new Error("routed wire query cannot continue while a turn is streaming"); })();
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(state.continuationReady)) {
    (() => { throw new Error("routed wire query already has a continued turn ready"); })();
  }
  if (((!((_truthy) => _truthy !== false && _truthy != null)(state.active)) || (!((_truthy) => _truthy !== false && _truthy != null)(state.active.continueTurn)))) {
    (() => { throw new ProviderEscalationUnsupportedError($$bc$str("provider ", state.decision.provider, " does not support provider-neutral continuation")); })();
  }
  await state.active.continueTurn(continued_input);
  (state.continuationReady = true);
  return null;
}

async function close_query_bang(state) {
  (state.closed = true);
  detach_active_observers_bang(state);
  await close_active_bang(state);
  return null;
}

function close_query_once_bang(state) {
  if ((state.closePromise === undefined)) {
    (state.closePromise = close_query_bang(state));
  }
  return state.closePromise;
}

async function set_model_bang(state, selection) {
  const provider = state.decision.provider;
  if ((!(selection.provider === provider))) {
    (() => { throw new ProviderEscalationUnsupportedError($$bc$str("active provider ", provider, " cannot apply a ", selection.provider, " model selection")); })();
  }
  if (((!((_truthy) => _truthy !== false && _truthy != null)(state.active)) || (!((_truthy) => _truthy !== false && _truthy != null)(state.active.setModel)))) {
    (() => { throw new ProviderEscalationUnsupportedError($$bc$str("provider ", provider, " does not support in-flight model escalation")); })();
  }
  await state.active.setModel(selection);
  const resolved = resolved_route(state, provider, null);
  if (((_truthy) => _truthy !== false && _truthy != null)(resolved.model)) {
    (state.decision.resolvedModel = resolved.model);
  }
  return null;
}

async function apply_flag_settings_bang(state, settings) {
  if (((!((_truthy) => _truthy !== false && _truthy != null)(state.active)) || (!((_truthy) => _truthy !== false && _truthy != null)(state.active.applyFlagSettings)))) {
    (() => { throw new ProviderEscalationUnsupportedError($$bc$str("provider ", state.decision.provider, " does not support in-flight effort escalation")); })();
  }
  await state.active.applyFlagSettings(settings);
  if (((!(settings.effortLevel === undefined)) && (!(settings.effortLevel === null)))) {
    (state.decision.resolvedEffort = settings.effortLevel);
  }
  return null;
}

function routed_query_with_registry_bang(decision, args, route_class, provider_registry, before_fallback, on_route, on_route_attempt) {
  const initial_snapshot = args.writer.snapshot();
  if (((!((_truthy) => _truthy !== false && _truthy != null)(initial_snapshot)) || (!(initial_snapshot.lifecycle === "running")))) {
    (() => { throw new Error("routed wire query requires an already-started running writer"); })();
  }
  const activity = createExecutionActivityEmitter();
  const state = $$bh$js_obj("decision", decision, "args", args, "routeClass", route_class, "providerRegistry", provider_registry, "beforeFallback", before_fallback, "onRoute", on_route, "onRouteAttempt", on_route_attempt, "activity", activity, "providerEventListeners", new Set(), "stopActivity", () => null, "stopProviderEvents", () => null, "activePublishesEvents", false, "continuationReady", false, "turnStreaming", false, "closed", false, "closePromise", undefined, "active", undefined, "input", replayable_input_bang(args.input), "requestedReasoning", args.options.effort, "seed", harnessRouteSeed(args.options));
  const query = $$bh$js_obj();
  Object.defineProperty(query, "executionTransport", $$bh$js_obj("get", () => (((_truthy) => _truthy !== false && _truthy != null)(state.active) ? state.active.executionTransport : undefined)));
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(query, "executionActivity", activity.source);
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(query, "subscribeProviderEvents", (listener) => { state.providerEventListeners.add(listener);
return () => { state.providerEventListeners.delete(listener);
return null; }; });
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(query, "mcpActivity", () => (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? state.active.mcpActivity : _logical))(state.active)) ? state.active.mcpActivity() : $$bh$js_obj("source", "provider-route-unavailable", "coverage", "unknown", "tools", [], "operationReceipts", [], "operationAggregates", [])));
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(query, "nativeCommandActivity", () => (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? state.active.nativeCommandActivity : _logical))(state.active)) ? state.active.nativeCommandActivity() : $$bh$js_obj("source", "provider-route-unavailable", "coverage", "unknown", "northBinaryProbe", "not_observed", "completions", [])));
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(query, "continueTurn", (input) => continue_turn_bang(state, input));
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(query, "interruptTurn", () => (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? state.active.interruptTurn : _logical))(state.active)) ? state.active.interruptTurn() : (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? state.active.interrupt : _logical))(state.active)) ? state.active.interrupt() : Promise.resolve(null))));
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(query, "interrupt", () => (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? state.active.interrupt : _logical))(state.active)) ? state.active.interrupt() : Promise.resolve(null)));
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(query, "close", () => close_query_once_bang(state));
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(query, "forceClose", () => { (state.closed = true);
detach_active_observers_bang(state);
if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? state.active.forceClose : _logical))(state.active))) {
  state.active.forceClose();
}
return null; });
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(query, "supportsInFlightEscalation", () => Boolean(((_logical) => (_logical !== false && _logical != null ? ((_logical) => (_logical !== false && _logical != null ? ((_logical) => (_logical !== false && _logical != null ? (((_truthy) => _truthy !== false && _truthy != null)(state.active.supportsInFlightEscalation) ? state.active.supportsInFlightEscalation() : true) : _logical))(state.active.applyFlagSettings) : _logical))(state.active.setModel) : _logical))(state.active)));
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(query, "setModel", (selection) => set_model_bang(state, selection));
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(query, "applyFlagSettings", (settings) => apply_flag_settings_bang(state, settings));
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(query, Symbol.asyncIterator, () => begin_iteration_bang(state));
  return query;
}

function routed_query_with_registry_overloads_bang(...$beagle$args) {
  if (arguments.length === 4) {
    const decision = $beagle$args[0];
    const args = $beagle$args[1];
    const route_class = $beagle$args[2];
    const provider_registry = $beagle$args[3];
    return routed_query_with_registry_bang(decision, args, route_class, provider_registry, null, null, null);
  }
  if (arguments.length === 5) {
    const decision = $beagle$args[0];
    const args = $beagle$args[1];
    const route_class = $beagle$args[2];
    const provider_registry = $beagle$args[3];
    const before_fallback = $beagle$args[4];
    return routed_query_with_registry_bang(decision, args, route_class, provider_registry, before_fallback, null, null);
  }
  if (arguments.length === 6) {
    const decision = $beagle$args[0];
    const args = $beagle$args[1];
    const route_class = $beagle$args[2];
    const provider_registry = $beagle$args[3];
    const before_fallback = $beagle$args[4];
    const on_route = $beagle$args[5];
    return routed_query_with_registry_bang(decision, args, route_class, provider_registry, before_fallback, on_route, null);
  }
  if (arguments.length === 7) {
    const decision = $beagle$args[0];
    const args = $beagle$args[1];
    const route_class = $beagle$args[2];
    const provider_registry = $beagle$args[3];
    const before_fallback = $beagle$args[4];
    const on_route = $beagle$args[5];
    const on_route_attempt = $beagle$args[6];
    return routed_query_with_registry_bang(decision, args, route_class, provider_registry, before_fallback, on_route, on_route_attempt);
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

const routedQueryWithRegistry = routed_query_with_registry_overloads_bang;

export { routedQueryWithRegistry as "routedQueryWithRegistry" };
