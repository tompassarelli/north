import { keyword as $$bc$keyword, str as $$bc$str } from './generated/beagle/core.js';
import { admit_host_object as $$bh$admit_host_object, aget as $$bh$aget, array as $$bh$array, aset as $$bh$aset, host_object as $$bh$host_object, js_obj as $$bh$js_obj } from './generated/beagle/host.js';
import { catch_dispatch as $$bd$catch_dispatch } from './generated/beagle/exception-dispatch.js';

const fs_module = require("node:fs");

const readFileSync = fs_module.readFileSync;

const path_module = require("node:path");

const resolve = path_module.resolve;

const execution_admission_module = require("../execution-admission");

const markCoordinationOptional = execution_admission_module.markCoordinationOptional;

const harness_module = require("../harness");

const harnessOptions = harness_module.harnessOptions;

const staffing_module = require("../orchestration-staffing");

const applyOrchestrationStaffing = staffing_module.applyOrchestrationStaffing;

const provider_routing_module = require("../provider-routing");

const anthropic_module = require("../providers/anthropic");

const anthropicProvider = anthropic_module.anthropicProvider;

const openai_module = require("../providers/openai");

const openaiProvider = openai_module.openaiProvider;

const catalog_module = require("../providers/catalog");

const resolveRoute = catalog_module.resolveRoute;

const run_artifacts_module = require("../run-artifacts");

const RunArtifactStore = run_artifacts_module.RunArtifactStore;

const wire_module = require("../wire");

const wireQueryRoute = wire_module.wireQueryRoute;

const DIRECTOR_PROMPT = readFileSync(resolve(import.meta.dir, "director-prompt.md"), "utf8");

const IMPLEMENTER_PROMPT = readFileSync(resolve(import.meta.dir, "implementer-prompt.md"), "utf8");

function selection_override_fields(selection, base) {
  const overrides = [];
  if ((!(selection.capabilityFloor === base.capabilityFloor))) {
    overrides.push("capabilityFloor");
  }
  if ((!(selection.serviceClass === base.serviceClass))) {
    overrides.push("serviceClass");
  }
  if ((!(selection.reasoning === base.reasoning))) {
    overrides.push("reasoning");
  }
  return overrides;
}

function resolveBridgeLaunchSelection(provider, role, selection) {
  const base = applyOrchestrationStaffing($$bh$host_object($$bc$keyword("role"), role));
  const capability_floor = ((_logical) => (_logical !== false && _logical != null ? _logical : base.capabilityFloor))(selection.capabilityFloor);
  const service_class = ((_logical) => (_logical !== false && _logical != null ? _logical : base.serviceClass))(selection.serviceClass);
  const reasoning = ((_logical) => (_logical !== false && _logical != null ? _logical : base.reasoning))(selection.reasoning);
  const resolved = resolveRoute(provider, capability_floor, service_class, selection.model, reasoning);
  const selected = $$bh$host_object($$bc$keyword("capabilityFloor"), capability_floor, $$bc$keyword("serviceClass"), service_class, $$bc$keyword("reasoning"), reasoning);
  const overrides = selection_override_fields(selected, base);
  const routing_metadata = ((overrides.length === 0) ? base : applyOrchestrationStaffing($$bh$host_object($$bc$keyword("role"), role, $$bc$keyword("capabilityFloor"), capability_floor, $$bc$keyword("serviceClass"), service_class, $$bc$keyword("reasoning"), reasoning, $$bc$keyword("composition"), $$bh$host_object($$bc$keyword("kind"), "template", $$bc$keyword("id"), base.role, $$bc$keyword("overrides"), overrides, $$bc$keyword("overrideReason"), "Bridge launch selection"))));
  return $$bh$host_object($$bc$keyword("routingMetadata"), routing_metadata, $$bc$keyword("resolved"), resolved);
}

function bridgeSystemPrompt(role) {
  return ((role === "director") ? DIRECTOR_PROMPT.trim() : IMPLEMENTER_PROMPT.trim());
}

const BRIDGE_QUERY_TEARDOWN_TIMEOUT_MS = 1000;

function bridge_provider_teardown_timeout_error_impl_bang(timeout_ms) {
  const error = new Error($$bc$str("provider session teardown timed out after ", timeout_ms, "ms"));
  Object.setPrototypeOf(error, bridge_provider_teardown_timeout_error_impl_bang.prototype);
  (error.name = "BridgeProviderTeardownTimeoutError");
  return error;
}

Object.setPrototypeOf(bridge_provider_teardown_timeout_error_impl_bang.prototype, Error.prototype);

const BridgeProviderTeardownTimeoutError = bridge_provider_teardown_timeout_error_impl_bang;

async function bounded_query_teardown_bang(query, timeout_ms) {
  const tasks = [];
  (() => { try {
    if (((_truthy) => _truthy !== false && _truthy != null)(query.interrupt)) {
    return tasks.push(Promise.resolve(query.interrupt()));
  }
  } catch (_catch_0) {
    switch ($$bd$catch_dispatch(_catch_0, [Error])) {
      case 0: {
        const error = _catch_0;
        return tasks.push(Promise.reject(error));
        break;
      }
    }
  } })();
  (() => { try {
    if (((_truthy) => _truthy !== false && _truthy != null)(query.close)) {
    return tasks.push(Promise.resolve(query.close()));
  }
  } catch (_catch_1) {
    switch ($$bd$catch_dispatch(_catch_1, [Error])) {
      case 0: {
        const error = _catch_1;
        return tasks.push(Promise.reject(error));
        break;
      }
    }
  } })();
  if ((tasks.length === 0)) {
    return null;
  } else {
    const timeout = Promise.withResolvers();
    const timer = setTimeout(() => { timeout.reject(bridge_provider_teardown_timeout_error_impl_bang(timeout_ms));
return null; }, timeout_ms);
    return (async () => { try {
    const results = await Promise.race([Promise.allSettled(tasks), timeout.promise]);
  const failures = results.filter((result) => (result.status === "rejected")).map((result) => result.reason);
  return (((failures.length === 1)) ? (() => { throw (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(failures, 0); })() : ((failures.length > 1)) ? (() => { throw new AggregateError(failures); })() : null);
  } catch (_catch_2) {
    switch ($$bd$catch_dispatch(_catch_2, [Error])) {
      case 0: {
        const error = _catch_2;
        if ((error.name === "BridgeProviderTeardownTimeoutError")) {
          if (((_truthy) => _truthy !== false && _truthy != null)(query.forceClose)) {
            query.forceClose();
          }
        }
        return (() => { throw error; })();
        break;
      }
    }
  } finally {
    clearTimeout(timer);
  } })();
  }
}

function wake_continuation_waiter_bang(state) {
  const waiting = state.continuationWaiting;
  (state.continuationWaiting = undefined);
  if (((_truthy) => _truthy !== false && _truthy != null)(waiting)) {
    waiting.resolve();
  }
  return null;
}

function observe_continuation_bang(state) {
  const continuation = state.continuation;
  if (((_truthy) => _truthy !== false && _truthy != null)(continuation)) {
    (continuation.observed = true);
    (state.continuation = undefined);
  }
  return null;
}

async function await_continuation_iterator_bang(state) {
  return (async () => {  while (true) {
    if (((_truthy) => _truthy !== false && _truthy != null)(state.closed)) { return false; } else { const continuation = state.continuation; if ((!((_truthy) => _truthy !== false && _truthy != null)(continuation))) { (state.continuationWaiting = Promise.withResolvers()); await state.continuationWaiting.promise; (state.continuationWaiting = undefined);  continue; } else { await continuation.settled.promise; if (((_truthy) => _truthy !== false && _truthy != null)(state.closed)) { return false; } else if (((_truthy) => _truthy !== false && _truthy != null)(((!((_truthy) => _truthy !== false && _truthy != null)(continuation.accepted)) || continuation.observed))) { ((state.continuation === continuation) ? (() => { return (state.continuation = undefined); })() : null);  continue; } else if (((_truthy) => _truthy !== false && _truthy != null)(continuation.iterationStarted)) { ((state.continuation === continuation) ? (() => { return (state.continuation = undefined); })() : null);  continue; } else { return (() => { (continuation.iterationStarted = true);
return true; })(); } } }
  } })();
}

async function submit_input_bang(state, input) {
  if (((_truthy) => _truthy !== false && _truthy != null)(state.closed)) {
    (() => { throw new Error("provider session is closed"); })();
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? _logical : state.continuation))(state.submitting))) {
    (() => { throw new Error("provider session already has a continuation in flight"); })();
  }
  if ((!((_truthy) => _truthy !== false && _truthy != null)(state.query.continueTurn))) {
    (() => { throw new Error("provider does not support provider-neutral continuation"); })();
  }
  const continuation = $$bh$host_object($$bc$keyword("settled"), Promise.withResolvers(), $$bc$keyword("accepted"), false, $$bc$keyword("observed"), false, $$bc$keyword("iterationStarted"), false);
  (state.submitting = true);
  (state.continuation = continuation);
  wake_continuation_waiter_bang(state);
  return (async () => { try {
    await state.query.continueTurn(input);
  (continuation.accepted = true);
  return null;
  } catch (_catch_3) {
    switch ($$bd$catch_dispatch(_catch_3, [Error])) {
      case 0: {
        const error = _catch_3;
        if ((state.continuation === continuation)) {
          (state.continuation = undefined);
        }
        return (() => { throw error; })();
        break;
      }
    }
  } finally {
    (state.submitting = false);
    continuation.settled.resolve();
    wake_continuation_waiter_bang(state);
  } })();
}

async function interrupt_turn_bang(state) {
  if ((!((_truthy) => _truthy !== false && _truthy != null)(state.query.interruptTurn))) {
    (() => { throw new Error("provider does not support turn interruption"); })();
  }
  return await state.query.interruptTurn();
}

function terminate_session_bang(state) {
  if (((_truthy) => _truthy !== false && _truthy != null)(state.termination)) {
    return state.termination;
  } else {
    (state.closed = true);
    state.signal.removeEventListener("abort", state.signalAbort);
    state.abort.abort();
    wake_continuation_waiter_bang(state);
    (state.termination = bounded_query_teardown_bang(state.query, BRIDGE_QUERY_TEARDOWN_TIMEOUT_MS));
    return state.termination;
  }
}

function force_terminate_session_bang(state) {
  (state.closed = true);
  state.signal.removeEventListener("abort", state.signalAbort);
  state.abort.abort();
  wake_continuation_waiter_bang(state);
  if (((_truthy) => _truthy !== false && _truthy != null)(state.query.forceClose)) {
    state.query.forceClose();
  }
  return null;
}

function query_iterator(query) {
  return Reflect.apply((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(query, Symbol.asyncIterator), query, $$bh$array());
}

async function finalize_event_iterator_bang(state, iterator_state) {
  if ((!((_truthy) => _truthy !== false && _truthy != null)(iterator_state.done))) {
    (iterator_state.done = true);
    state.signal.removeEventListener("abort", state.signalAbort);
    if ((!((_truthy) => _truthy !== false && _truthy != null)(state.closed))) {
      await terminate_session_bang(state);
    }
  }
  return null;
}

async function prepare_source_iterator_bang(state, iterator_state) {
  if (((_truthy) => _truthy !== false && _truthy != null)(iterator_state.sourceIterator)) {
    return true;
  } else {
    if (((_truthy) => _truthy !== false && _truthy != null)(iterator_state.first)) {
      (iterator_state.first = false);
      (iterator_state.sourceIterator = query_iterator(state.query));
      return true;
    } else {
      if (await await_continuation_iterator_bang(state)) {
        (iterator_state.sourceIterator = query_iterator(state.query));
        return true;
      } else {
        await finalize_event_iterator_bang(state, iterator_state);
        return false;
      }
    }
  }
}

async function next_event_bang(state, iterator_state) {
  return (((_truthy) => _truthy !== false && _truthy != null)(iterator_state.done) ? $$bh$host_object($$bc$keyword("done"), true, $$bc$keyword("value"), undefined) : (async () => { try {
    return (async () => {  while (true) {
    if (((_truthy) => _truthy !== false && _truthy != null)(state.closed)) { return await (async () => { await finalize_event_iterator_bang(state, iterator_state);
return $$bh$host_object($$bc$keyword("done"), true, $$bc$keyword("value"), undefined); })(); } else { const ready = await prepare_source_iterator_bang(state, iterator_state); if ((!ready)) { return $$bh$host_object($$bc$keyword("done"), true, $$bc$keyword("value"), undefined); } else { const step = await iterator_state.sourceIterator.next(); if (((_truthy) => _truthy !== false && _truthy != null)(step.done)) { (iterator_state.sourceIterator = undefined);  continue; } else { return (() => { observe_continuation_bang(state);
return $$bh$host_object($$bc$keyword("done"), false, $$bc$keyword("value"), step.value); })(); } } }
  } })();
  } catch (_catch_4) {
    switch ($$bd$catch_dispatch(_catch_4, [Error])) {
      case 0: {
        const error = _catch_4;
        await finalize_event_iterator_bang(state, iterator_state);
        return (() => { throw error; })();
        break;
      }
    }
  } })());
}

async function return_event_iterator_bang(state, iterator_state) {
  const source = iterator_state.sourceIterator;
  if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? source.return : _logical))(source))) {
    await source.return();
  }
  await finalize_event_iterator_bang(state, iterator_state);
  return $$bh$host_object($$bc$keyword("done"), true, $$bc$keyword("value"), undefined);
}

function events_bang(state) {
  if (((_truthy) => _truthy !== false && _truthy != null)(state.eventsConsumed)) {
    (() => { throw new Error("provider event stream is single-consumer"); })();
  }
  (state.eventsConsumed = true);
  const iterator_state = $$bh$host_object($$bc$keyword("first"), true, $$bc$keyword("sourceIterator"), undefined, $$bc$keyword("done"), false);
  const iterator = $$bh$js_obj();
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(iterator, "next", () => next_event_bang(state, iterator_state));
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(iterator, "return", () => return_event_iterator_bang(state, iterator_state));
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(iterator, Symbol.asyncIterator, () => iterator);
  return iterator;
}

function bridge_wire_session_impl_bang(query, abort, signal, presentation) {
  const state = $$bh$host_object($$bc$keyword("query"), query, $$bc$keyword("abort"), abort, $$bc$keyword("signal"), signal, $$bc$keyword("signalAbort"), undefined, $$bc$keyword("continuation"), undefined, $$bc$keyword("continuationWaiting"), undefined, $$bc$keyword("submitting"), false, $$bc$keyword("eventsConsumed"), false, $$bc$keyword("closed"), false, $$bc$keyword("termination"), undefined);
  const session = $$bh$js_obj();
  const signal_abort = () => { terminate_session_bang(state).catch((__error) => null);
return null; };
  (state.signalAbort = signal_abort);
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(session, "presentation", presentation);
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(session, "submitInput", (input) => submit_input_bang(state, input));
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(session, "interruptTurn", () => interrupt_turn_bang(state));
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(session, "terminateSession", () => terminate_session_bang(state));
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(session, "forceTerminateSession", () => force_terminate_session_bang(state));
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(session, "events", () => events_bang(state));
  signal.addEventListener("abort", signal_abort, $$bh$host_object($$bc$keyword("once"), true));
  if (((_truthy) => _truthy !== false && _truthy != null)(signal.aborted)) {
    signal_abort();
  }
  return session;
}

const BridgeWireSession = bridge_wire_session_impl_bang;

function attempt_credential_target(routing, authority) {
  const policy_loader = ((_logical) => (_logical !== false && _logical != null ? _logical : provider_routing_module.resourcePolicyFromEnv))(routing.resourcePolicyFromEnv);
  const policy = policy_loader();
  const matches = ((_logical) => (_logical !== false && _logical != null ? _logical : []))(policy.targets).filter((target) => ((target.id === authority.accountId) && (target.provider === authority.provider)));
  if ((!(matches.length === 1))) {
    (() => { throw new Error("Bridge Store attempt account has no unique configured credential locator"); })();
  }
  const target = (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(matches, 0);
  if (((authority.provider === "openai") && ((!(target.authMode === "isolated")) || ((!((_truthy) => _truthy !== false && _truthy != null)(target.profile)) || (!(target.profile === authority.credentialProfile)))))) {
    (() => { throw new Error("Bridge Store-authorized OpenAI account lacks an isolated credential locator"); })();
  }
  return target;
}

function route_result(decision) {
  const result = $$bh$js_obj();
  const target = $$bh$aget(decision.routingTargets, decision.target);
  const receipt = (((_truthy) => _truthy !== false && _truthy != null)(decision.modelAvailabilityReceipts) ? (() => { return $$bh$aget(decision.modelAvailabilityReceipts, decision.target); })() : null);
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(result, "target", target);
  if (((_truthy) => _truthy !== false && _truthy != null)(receipt)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(result, "receipt", receipt);
  }
  return result;
}

async function bridge_route_bang(routing, provider, context) {
  const selected_context = ((_logical) => (_logical !== false && _logical != null ? _logical : $$bh$js_obj()))(context);
  const cached = await (routing.selectProviderFromCachedState)($$bh$host_object($$bc$keyword("provider"), provider), undefined, selected_context);
  if (((_truthy) => _truthy !== false && _truthy != null)(cached)) {
    (routing.refreshProviderRoutingInBackground)($$bh$host_object($$bc$keyword("provider"), provider));
    return route_result(cached);
  } else {
    return (async () => { try {
    const route_context = Object.assign({}, selected_context, $$bh$host_object($$bc$keyword("signal"), AbortSignal.timeout(routing.BOOT_ROUTING_TIMEOUT_MS)));
  const decision = await (routing.selectProviderForExecution)($$bh$host_object($$bc$keyword("provider"), provider), undefined, route_context);
  return route_result(decision);
  } catch (_catch_5) {
    switch ($$bd$catch_dispatch(_catch_5, [Error])) {
      case 0: {
        const __error = _catch_5;
        if (((_truthy) => _truthy !== false && _truthy != null)(selected_context.model)) {
          return $$bh$js_obj();
        } else {
          const fallback = (routing.configuredDefaultTarget)(provider);
          return (((_truthy) => _truthy !== false && _truthy != null)(fallback) ? $$bh$host_object($$bc$keyword("target"), fallback) : $$bh$js_obj());
        }
        break;
      }
    }
  } })();
  }
}

const bridgeRoute = bridge_route_bang;

function bridge_selection_input(context, model) {
  const selection = $$bh$js_obj();
  if (((_truthy) => _truthy !== false && _truthy != null)(context.capabilityFloor)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(selection, "capabilityFloor", context.capabilityFloor);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(context.serviceClass)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(selection, "serviceClass", context.serviceClass);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(context.reasoning)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(selection, "reasoning", context.reasoning);
  }
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(selection, "model", model);
  return selection;
}

async function provider_open_bang(providers, routing, context) {
  const authority = context.attemptRoute;
  if (((!(context.provider === authority.provider)) || (!(context.model === authority.model)))) {
    (() => { throw new Error("Bridge provider context conflicts with its Store attempt authority"); })();
  }
  const agent_provider = (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(providers, authority.provider);
  const model = authority.model;
  const target = attempt_credential_target(routing, authority);
  const selection = resolveBridgeLaunchSelection(authority.provider, context.role, bridge_selection_input(context, model));
  if ((!(selection.resolved.model === model))) {
    (() => { throw new Error("Bridge Store attempt model is not an exact canonical route"); })();
  }
  const routing_metadata = selection.routingMetadata;
  const resolved = selection.resolved;
  const abort_controller = new AbortController();
  const artifacts = new RunArtifactStore(context.writer.runId);
  const options = harnessOptions($$bh$host_object($$bc$keyword("self"), $$bc$str("bridge-", context.executionId), $$bc$keyword("provider"), authority.provider, $$bc$keyword("routingMetadata"), routing_metadata, $$bc$keyword("cwd"), context.cwd, $$bc$keyword("model"), resolved.model, $$bc$keyword("effort"), resolved.effort, $$bc$keyword("modelAvailability"), $$bh$host_object($$bc$keyword("exactModelPinned"), false, $$bc$keyword("targetId"), target.id), $$bc$keyword("presenceRegistrar"), false, $$bc$keyword("presenceRenewer"), false, $$bc$keyword("systemPrompt"), bridgeSystemPrompt(context.role), $$bc$keyword("abortController"), abort_controller, $$bc$keyword("artifactDirectory"), artifacts.directory));
  markCoordinationOptional(options);
  if (((_truthy) => _truthy !== false && _truthy != null)(agent_provider.admit)) {
    await agent_provider.admit($$bh$host_object($$bc$keyword("options"), options, $$bc$keyword("target"), target));
  }
  const effort = resolved.effort;
  const query = agent_provider.query($$bh$host_object($$bc$keyword("input"), context.prompt, $$bc$keyword("options"), options, $$bc$keyword("target"), target, $$bc$keyword("context"), $$bh$host_object($$bc$keyword("writer"), context.writer, $$bc$keyword("artifacts"), artifacts, $$bc$keyword("route"), wireQueryRoute($$bh$host_object($$bc$keyword("model"), $$bh$host_object($$bc$keyword("provider"), authority.provider, $$bc$keyword("capabilityClass"), (((_truthy) => _truthy !== false && _truthy != null)((context.role === "director")) ? "orchestrator" : "authoring")), $$bc$keyword("effort"), effort, $$bc$keyword("attempt"), 1)))));
  const presentation = Object.freeze($$bh$host_object($$bc$keyword("model"), resolved.model, $$bc$keyword("effort"), effort, $$bc$keyword("cwd"), ((_logical) => (_logical !== false && _logical != null ? _logical : context.cwd))(options.cwd), $$bc$keyword("permissionMode"), (((_truthy) => _truthy !== false && _truthy != null)((authority.provider === "openai")) ? "bypassPermissions" : options.permissionMode)));
  return bridge_wire_session_impl_bang(query, abort_controller, context.signal, presentation);
}

function bridge_provider_with_dependencies_for_test_bang(providers, routing) {
  return Object.freeze($$bh$host_object($$bc$keyword("open"), (context) => provider_open_bang(providers, routing, context)));
}

const bridgeProviderWithDependenciesForTest = bridge_provider_with_dependencies_for_test_bang;

const bridgeProvider = bridge_provider_with_dependencies_for_test_bang($$bh$host_object($$bc$keyword("anthropic"), anthropicProvider, $$bc$keyword("openai"), openaiProvider), provider_routing_module);

const BRIDGE_PRESSURE_RANK = $$bh$host_object($$bc$keyword("plenty"), 4, $$bc$keyword("normal"), 3, $$bc$keyword("low"), 2, $$bc$keyword("unknown"), 1, $$bc$keyword("exhausted"), 0);

const BRIDGE_PROVIDER_ORDER = ["anthropic", "openai"];

function route_requested_p(selection) {
  return ((!(selection.capabilityFloor === undefined)) || ((!(selection.serviceClass === undefined)) || ((!(selection.model === undefined)) || (!(selection.reasoning === undefined)))));
}

function provider_compatible_p(provider, selection) {
  return (() => { try {
    resolveBridgeLaunchSelection(provider, selection.role, selection);
  return true;
  } catch (_catch_6) {
    switch ($$bd$catch_dispatch(_catch_6, [Error])) {
      case 0: {
        const __error = _catch_6;
        return false;
        break;
      }
    }
  } })();
}

async function select_bridge_provider_bang(selection) {
  const selected = ((_logical) => (_logical !== false && _logical != null ? _logical : $$bh$host_object($$bc$keyword("role"), "implementer")))(selection);
  const compatible = (route_requested_p(selected) ? BRIDGE_PROVIDER_ORDER.filter((provider) => provider_compatible_p(provider, selected)) : BRIDGE_PROVIDER_ORDER);
  if ((compatible.length === 0)) {
    (() => { throw new Error("no Bridge provider supports the requested launch route"); })();
  }
  const fallback = (((_truthy) => _truthy !== false && _truthy != null)(compatible.includes("openai")) ? "openai" : compatible[0]);
  return (() => { try {
    const routing = (provider_routing_module.cachedTargetRouting)();
  const state = $$bh$host_object($$bc$keyword("best"), undefined);
  (() => { compatible.forEach((provider) => {
  (() => { routing.forEach((entry) => {
  if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (entry.target.provider === provider) : _logical))(entry.eligible))) {
    const rank = ((_logical) => (_logical !== false && _logical != null ? _logical : 0))((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(BRIDGE_PRESSURE_RANK, entry.headroom));
    const best = state.best;
    if (((!((_truthy) => _truthy !== false && _truthy != null)(best)) || (rank > best.rank))) {
      (state.best = $$bh$host_object($$bc$keyword("provider"), provider, $$bc$keyword("rank"), rank));
    }
  }
}); })();
}); })();
  return (((_truthy) => _truthy !== false && _truthy != null)(state.best) ? state.best.provider : fallback);
  } catch (_catch_7) {
    switch ($$bd$catch_dispatch(_catch_7, [Error])) {
      case 0: {
        const __error = _catch_7;
        return fallback;
        break;
      }
    }
  } })();
}

const selectBridgeProvider = select_bridge_provider_bang;

export { BridgeProviderTeardownTimeoutError as "BridgeProviderTeardownTimeoutError" };
export { BridgeWireSession as "BridgeWireSession" };
export { bridgeProvider as "bridgeProvider" };
export { bridgeProviderWithDependenciesForTest as "bridgeProviderWithDependenciesForTest" };
export { bridgeRoute as "bridgeRoute" };
export { bridgeSystemPrompt as "bridgeSystemPrompt" };
export { resolveBridgeLaunchSelection as "resolveBridgeLaunchSelection" };
export { selectBridgeProvider as "selectBridgeProvider" };
