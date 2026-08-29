import { createHash, randomUUID } from 'crypto';
import { acquireDeliveryAttemptLeases, commitDeliveryAttemptProviderStart, commitDeliveryAttemptProvedUnsent, commitDeliveryAttemptTerminal, DELIVERY_ATTEMPT_LEASE_TTL_MS, newDeliveryRunContext, reserveDeliveryRun, reserveDeliveryRunWithRecovery, writeDeliveryAttemptLaunchIntent } from 'north-sdk/internal/delivery-evidence';
import { getThreadFacts, normalizeNorthEntityId } from 'north-sdk/internal/north-client';
import { selectProviderForExecution } from 'north-sdk/internal/provider-routing';
import { resolveTier } from 'north-sdk/internal/providers-catalog';
import { newRunId } from 'north-sdk/internal/telemetry';
import { encodeWireJsonlLine } from 'north-sdk/internal/wire';
import { StoreBridgeCommandReceipts } from 'north-sdk/internal/bridge-command-receipts';
import { resolveBridgeLaunchSelection } from 'north-sdk/internal/bridge-provider';
import { keyword as $$bc$keyword, property_key as $$bc$property_key, str as $$bc$str } from '../../beagle/core.js';
import { catch_dispatch as $$bd$catch_dispatch } from '../../beagle/exception-dispatch.js';

function sha256(value) {
  return createHash("sha256").update(value, "utf8").digest("hex");
}

function exact_registered_thread_bang(selected_thread_id, env, load_thread_facts) {
  const selected = ((typeof selected_thread_id === "string") ? selected_thread_id.trim() : "");
  const bridge_thread = env.NORTH_BRIDGE_CONTROL_THREAD;
  const north_thread = env.NORTH_THREAD_ID;
  const agent_thread = env.AGENT_THREAD;
  const control = ((((_truthy) => _truthy !== false && _truthy != null)(((typeof bridge_thread === "string") && (!(bridge_thread.trim() === ""))))) ? bridge_thread.trim() : (((_truthy) => _truthy !== false && _truthy != null)(((typeof north_thread === "string") && (!(north_thread.trim() === ""))))) ? north_thread.trim() : ((typeof agent_thread === "string")) ? agent_thread.trim() : "");
  const candidate = ((selected === "") ? control : selected);
  if ((candidate === "")) {
    (() => { throw new Error("Bridge app launch requires an exact selected or managed control thread"); })();
  }
  const thread_id = normalizeNorthEntityId(candidate);
  const titles = load_thread_facts(thread_id).filter((fact) => (fact.predicate === "title")).map((fact) => fact.value);
  if (((_truthy) => _truthy !== false && _truthy != null)(((!(titles.length === 1)) || ((!(typeof (() => { const _x = titles, _i = 0; return _x[_i] != null ? _x[_i] : null; })() === "string")) || (titles[0].trim() === ""))))) {
    (() => { throw new Error($$bc$str("Bridge app launch thread @", thread_id, " is not registered in Store")); })();
  }
  return thread_id;
}

function reporter_agent_id(env, execution_id) {
  const agent = env.AGENT_ID;
  const managed = ((typeof agent === "string") ? agent.trim().replace(new RegExp("^@?agent:", "u"), "") : "");
  return ((managed === "") ? $$bc$str("bridge-app-", execution_id) : managed);
}

function unsent_receipt(execution_id, attempt_id, reason) {
  return sha256($$bc$str("north:bridge-app-launch-unsent:v1\x00", execution_id, "\x00", attempt_id, "\x00", reason));
}

function optional_close_bang(receipts) {
  if (((_truthy) => _truthy !== false && _truthy != null)(receipts.close)) {
    return receipts.close();
  }
}

async function prepare_managed_bridge_app_launch_bang(...$beagle$args) {
  if (arguments.length === 1) {
    const request = $beagle$args[0];
    return await prepare_managed_bridge_app_launch_bang(request, {});
  }
  if (arguments.length === 2) {
    const request = $beagle$args[0];
    const dependencies = $beagle$args[1];
    if ((((_logical) => (_logical !== false && _logical != null ? _logical : ""))(request.prompt).trim() === "")) {
      (() => { throw new Error("Bridge app launch requires a prompt"); })();
    }
    if ((((_logical) => (_logical !== false && _logical != null ? _logical : ""))(request.cwd).trim() === "")) {
      (() => { throw new Error("Bridge app launch requires a working directory"); })();
    }
    if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (!(request.provider === "openai")) : _logical))(request.provider))) {
      (() => { throw new Error("Bridge app launch requires a Store-authorized OpenAI execution route"); })();
    }
    const env = ((_logical) => (_logical !== false && _logical != null ? _logical : process.env))(dependencies.env);
    const load_thread_facts = ((_logical) => (_logical !== false && _logical != null ? _logical : getThreadFacts))(dependencies.loadThreadFacts);
    const thread_id = exact_registered_thread_bang(request.selectedThreadId, env, load_thread_facts);
    const execution_id = ((_logical) => (_logical !== false && _logical != null ? _logical : randomUUID()))(dependencies.executionId);
    const reporter = reporter_agent_id(env, execution_id);
    const selection = resolveBridgeLaunchSelection("openai", request.role, request);
    const routing = await (((_logical) => (_logical !== false && _logical != null ? _logical : selectProviderForExecution))(dependencies.selectProvider))({[$$bc$property_key($$bc$keyword("provider"))]: "openai"}, null, {[$$bc$property_key($$bc$keyword("tier"))]: selection.resolved.tier, [$$bc$property_key($$bc$keyword("reasoning"))]: selection.resolved.effort, [$$bc$property_key($$bc$keyword("model"))]: request.model, [$$bc$property_key($$bc$keyword("stableKey"))]: reporter});
    const account_receipt = routing.executionAccountReceipt;
    if (((_truthy) => _truthy !== false && _truthy != null)(((!(routing.provider === "openai")) || (!((_truthy) => _truthy !== false && _truthy != null)(account_receipt))))) {
      (() => { throw new Error("Bridge app launch has no Store-authorized OpenAI execution route"); })();
    }
    const resolved = resolveTier(routing.provider, selection.resolved.tier, request.model, selection.resolved.effort);
    if ((!((_truthy) => _truthy !== false && _truthy != null)(resolved.model))) {
      (() => { throw new Error("Bridge app launch could not resolve an execution model"); })();
    }
    const context = newDeliveryRunContext(newRunId(reporter), thread_id, reporter);
    const leases = await (((_logical) => (_logical !== false && _logical != null ? _logical : acquireDeliveryAttemptLeases))(dependencies.acquireLeases))(context, routing.target);
    const reserve = ((_logical) => (_logical !== false && _logical != null ? _logical : reserveDeliveryRun))(dependencies.reserve);
    const launch_intent = ((_logical) => (_logical !== false && _logical != null ? _logical : writeDeliveryAttemptLaunchIntent))(dependencies.launchIntent);
    const proved_unsent = ((_logical) => (_logical !== false && _logical != null ? _logical : commitDeliveryAttemptProvedUnsent))(dependencies.provedUnsent);
    const command_receipts = ((_logical) => (_logical !== false && _logical != null ? _logical : new StoreBridgeCommandReceipts()))(dependencies.commandReceipts);
    const setup = {[$$bc$property_key($$bc$keyword("reservation"))]: null, [$$bc$property_key($$bc$keyword("intent"))]: null};
    await (async () => { try {
    (setup.reservation = reserveDeliveryRunWithRecovery(context, {[$$bc$property_key($$bc$keyword("provider"))]: routing.provider, [$$bc$property_key($$bc$keyword("accountId"))]: routing.target, [$$bc$property_key($$bc$keyword("model"))]: resolved.model, [$$bc$property_key($$bc$keyword("accountAuthorityReceiptSha256"))]: account_receipt.accountAuthority.digest, [$$bc$property_key($$bc$keyword("routeObservationReceiptSha256"))]: account_receipt.usage.receipt.digest, [$$bc$property_key($$bc$keyword("threadLease"))]: leases.threadLease, [$$bc$property_key($$bc$keyword("accountLease"))]: leases.accountLease}, reserve));
  (setup.intent = launch_intent(context, setup.reservation));
  return await command_receipts.bindExecution(execution_id, setup.reservation.attemptId, {[$$bc$property_key($$bc$keyword("provider"))]: routing.provider, [$$bc$property_key($$bc$keyword("model"))]: resolved.model});
  } catch (_catch_0) {
    switch ($$bd$catch_dispatch(_catch_0, [Error])) {
      case 0: {
        const error = _catch_0;
        if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? setup.intent : _logical))(setup.reservation))) {
          await (async () => { try {
    return proved_unsent(context, setup.reservation, setup.intent, unsent_receipt(execution_id, setup.reservation.attemptId, "attempt-binding-refused"));
  } catch (_catch_1) {
    switch ($$bd$catch_dispatch(_catch_1, [Error])) {
      case 0: {
        const settlement_error = _catch_1;
        await leases.release();
        optional_close_bang(command_receipts);
        return (() => { throw new AggregateError([error, settlement_error], $$bc$str("Bridge app attempt binding and proved-unsent ", "settlement both failed")); })();
        break;
      }
    }
  } })();
        }
        await leases.release();
        optional_close_bang(command_receipts);
        return (() => { throw error; })();
        break;
      }
    }
  } })();
    optional_close_bang(command_receipts);
    const state = {[$$bc$property_key($$bc$keyword("effectObserved"))]: false, [$$bc$property_key($$bc$keyword("providerStart"))]: null, [$$bc$property_key($$bc$keyword("settled"))]: false, [$$bc$property_key($$bc$keyword("released"))]: false, [$$bc$property_key($$bc$keyword("renewalTimer"))]: null, [$$bc$property_key($$bc$keyword("renewing"))]: null, [$$bc$property_key($$bc$keyword("managed"))]: null};
    const lease_failure = Promise.withResolvers();
    const renewal_interval = Math.max(1, ((_logical) => (_logical !== false && _logical != null ? _logical : Math.floor((DELIVERY_ATTEMPT_LEASE_TTL_MS / 3))))(dependencies.leaseRenewIntervalMs));
    const release_bang = async () => { if ((!((_truthy) => _truthy !== false && _truthy != null)(state.released))) {
  (state.released = true);
  if (((_truthy) => _truthy !== false && _truthy != null)(state.renewalTimer)) {
    clearTimeout(state.renewalTimer);
  }
  (state.renewalTimer = null);
  if (((_truthy) => _truthy !== false && _truthy != null)(state.renewing)) {
    await state.renewing.catch((__) => null);
  }
  return await leases.release();
} };
    const commit_unsent_bang = async (receipt) => { if ((!((_truthy) => _truthy !== false && _truthy != null)(state.settled))) {
  if (((_truthy) => _truthy !== false && _truthy != null)(state.effectObserved)) {
    (() => { throw new Error($$bc$str("Bridge app launch observed a provider effect ", "and cannot be proved unsent")); })();
  }
  proved_unsent(context, setup.reservation, setup.intent, receipt);
  (state.settled = true);
  (state.managed.settled = true);
  return await release_bang();
} };
    return (() => { function schedule_bang() { if (((_truthy) => _truthy !== false && _truthy != null)(((!((_truthy) => _truthy !== false && _truthy != null)(state.released)) && (!((_truthy) => _truthy !== false && _truthy != null)(state.settled))))) {
  return (state.renewalTimer = setTimeout(() => { (state.renewalTimer = null);
if (((_truthy) => _truthy !== false && _truthy != null)(((!((_truthy) => _truthy !== false && _truthy != null)(state.released)) && (!((_truthy) => _truthy !== false && _truthy != null)(state.settled))))) {
  (state.renewing = leases.renew());
  state.renewing.then(() => { (state.renewing = null);
return schedule_bang(); }, (error) => { (state.renewing = null);
return (lease_failure.resolve)(((error instanceof Error) ? error : new Error($$bc$str("Bridge app launch lease ", "renewal failed")))); });
}
return renewal_interval; }));
} } const managed = {[$$bc$property_key($$bc$keyword("attemptId"))]: setup.reservation.attemptId, [$$bc$property_key($$bc$keyword("executionId"))]: execution_id, [$$bc$property_key($$bc$keyword("threadId"))]: thread_id, [$$bc$property_key($$bc$keyword("provider"))]: "openai", [$$bc$property_key($$bc$keyword("model"))]: resolved.model, [$$bc$property_key($$bc$keyword("providerEffectObserved"))]: false, [$$bc$property_key($$bc$keyword("settled"))]: false, [$$bc$property_key($$bc$keyword("leaseFailure"))]: lease_failure.promise, [$$bc$property_key($$bc$keyword("observeDurableWireEvent"))]: async (event) => { if (((_truthy) => _truthy !== false && _truthy != null)(((event.kind === "model-call.started") && (!((_truthy) => _truthy !== false && _truthy != null)(state.providerStart))))) {
  (state.effectObserved = true);
  (state.managed.providerEffectObserved = true);
  (state.providerStart = (((_logical) => (_logical !== false && _logical != null ? _logical : commitDeliveryAttemptProviderStart))(dependencies.providerStart))(context, setup.reservation, setup.intent, sha256(encodeWireJsonlLine(event))));
}
if (((_truthy) => _truthy !== false && _truthy != null)(((event.kind === "run.terminated") && (!((_truthy) => _truthy !== false && _truthy != null)(state.settled))))) {
  if ((!((_truthy) => _truthy !== false && _truthy != null)(state.effectObserved))) {
    await commit_unsent_bang(sha256(encodeWireJsonlLine(event)));
  } else {
    if ((!((_truthy) => _truthy !== false && _truthy != null)(state.providerStart))) {
      (() => { throw new Error($$bc$str("Bridge app provider-start settlement ", "is unavailable at terminal")); })();
    }
    (((_logical) => (_logical !== false && _logical != null ? _logical : commitDeliveryAttemptTerminal))(dependencies.terminal))(context, setup.reservation, setup.intent, state.providerStart, sha256(encodeWireJsonlLine(event)));
    (state.settled = true);
    (state.managed.settled = true);
    await release_bang();
  }
}
return null; }, [$$bc$property_key($$bc$keyword("proveUnsent"))]: async (reason) => await commit_unsent_bang(unsent_receipt(execution_id, setup.reservation.attemptId, reason))};
(state.managed = managed);
schedule_bang();
return managed; })();
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

export { prepare_managed_bridge_app_launch_bang as "prepare-managed-bridge-app-launch!" };
