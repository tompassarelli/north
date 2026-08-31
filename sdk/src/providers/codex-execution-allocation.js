import { conj_value as $$bc$conj_value, empty_p as $$bc$empty_p, first as $$bc$first, rest as $$bc$rest } from '../bridge/generated/beagle/core.js';
import { js_obj as $$bh$js_obj } from '../bridge/generated/beagle/host.js';

const accounts = require("../accounts");

const observations = require("../provider-observation-store");

const catalog = require("./catalog");

const read_authority = accounts.readCodexAccountAuthority;

const load_observation = observations.loadStoreProviderUsageObservation;

const supports_route_p = catalog.providerSupportsRoute;

const resolve_route = catalog.resolveRoute;

function credential_locator_p(target) {
  return ((target.provider === "openai") && ((target.authMode === "isolated") && (!(target.profile == null))));
}

function runtime_available_p(availability, target) {
  return (availability.find((state) => ((state.targetId === target.id) && ((state.provider === "openai") && state.available))) != null);
}

function default_usage(target) {
  return load_observation($$bh$js_obj("targetId", target.id, "provider", "openai", "source", "codex-app-server:account-rate-limits"));
}

function quota_usage_percent(...$beagle$args) {
  if (arguments.length === 1) {
    const observation = $beagle$args[0];
    return quota_usage_percent(observation, Date.now());
  }
  if (arguments.length === 2) {
    const observation = $beagle$args[0];
    const now = $beagle$args[1];
    if (((!(observation.provider === "openai")) || (!(observation.source === "codex-app-server:account-rate-limits")))) {
      return null;
    } else {
      const windows = ((_logical) => (_logical !== false && _logical != null ? _logical : []))(observation.windows);
      const primary = windows.find((window) => (window.limitId === "codex:primary"));
      return (((primary == null) || ((!(typeof primary.resetsAt === "string")) || ((!Number.isFinite(primary.usedPercent)) || ((primary.usedPercent < 0) || ((primary.usedPercent >= 100) || (Date.parse(primary.resetsAt) <= now)))))) ? null : primary.usedPercent);
    }
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

async function admitted_targets(targets, read_fn) {
  return (async () => { let remaining = targets; let admitted = []; while (true) {
    if ($$bc$empty_p(remaining)) { return admitted; } else { const target = $$bc$first(remaining); const authority = (credential_locator_p(target) ? await read_fn(target) : null); const _recur_0 = Array.from($$bc$rest(remaining)); const _recur_1 = $$bc$conj_value(admitted, $$bh$js_obj("target", target, "authority", authority)); remaining = _recur_0; admitted = _recur_1; continue; }
  } })();
}

async function ranked_candidates(candidates, load_fn) {
  return (async () => { let index = 0; let ranked = []; while (true) {
    if ((index >= candidates.length)) { return ranked; } else { const candidate = candidates[index]; const target = candidate.target; const authority = candidate.authority; const usage = await load_fn(target); const used = (((_truthy) => _truthy !== false && _truthy != null)(usage) ? quota_usage_percent(usage.observation) : null); const _recur_0 = (index + 1); const _recur_1 = (((usage == null) || (used == null)) ? ranked : $$bc$conj_value(ranked, $$bh$js_obj("target", target, "authority", authority, "usage", usage, "order", index, "pressure", used))); index = _recur_0; ranked = _recur_1; continue; }
  } })();
}

async function allocate_codex_execution_account_bang(targets, availability, capability_floor, service_class, reasoning, dependencies) {
  const selected = ((_logical) => (_logical !== false && _logical != null ? _logical : "medium"))(reasoning);
  if ((!supports_route_p("openai", capability_floor, service_class, selected))) {
    return null;
  } else {
    const route = resolve_route("openai", capability_floor, service_class, null, selected);
    if ((route.model == null)) {
      return null;
    } else {
      const read_fn = ((_logical) => (_logical !== false && _logical != null ? _logical : read_authority))((((_truthy) => _truthy !== false && _truthy != null)(dependencies) ? dependencies.readAuthority : null));
      const load_fn = ((_logical) => (_logical !== false && _logical != null ? _logical : default_usage))((((_truthy) => _truthy !== false && _truthy != null)(dependencies) ? dependencies.loadUsage : null));
      const available_targets = targets.filter((target) => runtime_available_p(availability, target));
      const admitted = await admitted_targets(available_targets, read_fn);
      const candidates = admitted.filter((entry) => { const authority = entry.authority;
return ((_logical) => (_logical !== false && _logical != null ? ((authority.role === "execution") && authority.executionEligible) : _logical))(authority); });
      const ranked = await ranked_candidates(candidates, load_fn);
      ranked.sort((left, right) => { const pressure = (left.pressure - right.pressure);
return ((pressure === 0) ? (left.order - right.order) : pressure); });
      const chosen = (() => { const _x = ranked, _i = 0; return _x[_i] != null ? _x[_i] : null; })();
      return ((chosen == null) ? null : Object.freeze($$bh$js_obj("target", chosen.target, "model", route.model, "effort", route.effort, "receipt", Object.freeze($$bh$js_obj("accountAuthority", chosen.authority.receipt, "usage", chosen.usage)))));
    }
  }
}

const allocateCodexExecutionAccount = allocate_codex_execution_account_bang;

export { allocateCodexExecutionAccount as "allocateCodexExecutionAccount" };
