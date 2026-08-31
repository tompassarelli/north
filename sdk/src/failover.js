import { get as $$bc$get, into_value as $$bc$into_value, str as $$bc$str } from './bridge/generated/beagle/core.js';
import { admit_host_object as $$bh$admit_host_object, aget as $$bh$aget, aset as $$bh$aset, js_obj as $$bh$js_obj } from './bridge/generated/beagle/host.js';
import { catch_dispatch as $$bd$catch_dispatch } from './bridge/generated/beagle/exception-dispatch.js';

const node_module = process.getBuiltinModule("node:module");

const create_require = node_module.createRequire;

const require_module = create_require(import.meta.url);

const crypto_module = process.getBuiltinModule("node:crypto");

const child_process_module = process.getBuiltinModule("node:child_process");

const fs_module = process.getBuiltinModule("node:fs");

const path_module = process.getBuiltinModule("node:path");

const north_client = require_module("./north-client");

const provider_catalog = require_module("./providers/catalog");

const routing_economics = require_module("./routing-economics");

const beagle_store = require_module("./beagle-store");

const create_hash = crypto_module.createHash;

const exec_file_sync = child_process_module.execFileSync;

const spawn_sync = child_process_module.spawnSync;

const read_file_sync = fs_module.readFileSync;

const resolve_path = path_module.resolve;

const get_thread_facts = north_client.getThreadFacts;

const get_children = north_client.getChildren;

const model_family = provider_catalog.modelFamily;

const provider_supports_route = provider_catalog.providerSupportsRoute;

const resolve_model_alias = provider_catalog.resolveModelAlias;

const resolve_route = provider_catalog.resolveRoute;

const babashka_arguments = beagle_store.beagleStoreBabashkaArguments;

const child_timeout = beagle_store.beagleStoreCoordinatorChildTimeout;

const store_environment = beagle_store.beagleStoreEnvironment;

const REPO = resolve_path(import.meta.dir, "..", "..");

const DEFAULT_THRESHOLD = 80;

const PIN_LIFETIME_MS = (60 * 60 * 1000);

const ROUTING_PIN_POLICY_VERSION = routing_economics.ROUTING_PIN_POLICY_VERSION;

function error_bang(message) {
  return (() => { throw new Error(message); })();
}

function as_object(value) {
  return (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))($$bh$js_obj("value", value), "value");
}

function as_array(value) {
  return (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))($$bh$js_obj("value", value), "value");
}

function as_string(value) {
  return (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))($$bh$js_obj("value", value), "value");
}

function as_number(value) {
  return (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))($$bh$js_obj("value", value), "value");
}

function foreign_object_bang(value, label) {
  if (((value == null) || ((!(typeof value === "object")) || Array.isArray(value)))) {
    error_bang($$bc$str(label, " must be an object"));
  }
  return as_object(value);
}

function exact_fields_bang(value, expected, label) {
  const fields = Object.keys(value);
  const unknown = fields.filter((field) => (!((_truthy) => _truthy !== false && _truthy != null)(expected.includes(field))));
  const missing = expected.filter((field) => (!((_truthy) => _truthy !== false && _truthy != null)(fields.includes(field))));
  if (((unknown.length > 0) || (missing.length > 0))) {
    error_bang($$bc$str(label, " fields mismatch (missing=", ((_logical) => (_logical !== false && _logical != null ? _logical : "none"))(missing.join(",")), "; unknown=", ((_logical) => (_logical !== false && _logical != null ? _logical : "none"))(unknown.join(",")), ")"));
  }
  return null;
}

function text_bang(value, label) {
  if (((!(typeof value === "string")) || (as_string(value).trim() === ""))) {
    error_bang($$bc$str(label, " must be a non-empty string"));
  }
  return as_string(value);
}

function percent_bang(value, label) {
  if (((!(typeof value === "number")) || ((!Number.isFinite(value)) || ((as_number(value) < 0) || (as_number(value) > 100))))) {
    error_bang($$bc$str(label, " must be a number from 0 through 100"));
  }
  return as_number(value);
}

function timestamp_bang(value, label) {
  const rendered = text_bang(value, label);
  if ((!Number.isFinite(Date.parse(rendered)))) {
    error_bang($$bc$str(label, " must be an ISO timestamp"));
  }
  return rendered;
}

function availability_verdict_bang(value, label) {
  const verdict = text_bang(value, label);
  if ((!((_truthy) => _truthy !== false && _truthy != null)(/^(available|unknown|cooked-week|cooked-window|model-cooked\\[[^\\]]+\\])$/.test(verdict)))) {
    error_bang($$bc$str(label, " is outside the pinned contract"));
  }
  return verdict;
}

function parse_rung_bang(...$beagle$args) {
  if (arguments.length === 2) {
    const value = $beagle$args[0];
    const label = $beagle$args[1];
    return parse_rung_bang(value, label, false);
  }
  if (arguments.length === 3) {
    const value = $beagle$args[0];
    const label = $beagle$args[1];
    const named = $beagle$args[2];
    const raw = foreign_object_bang(value, label);
    const untouched = ((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "resetState") === "untouched");
    const expected = (named ? ["name", "pct", (untouched ? "resetState" : "resetsAt")] : ["pct", (untouched ? "resetState" : "resetsAt")]);
    const pct = (() => { exact_fields_bang(raw, expected, label);
return percent_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "pct"), $$bc$str(label, ".pct")); })();
    const result = $$bh$js_obj("pct", pct);
    if ((untouched && (!(pct === 0)))) {
      error_bang($$bc$str(label, ".untouched must have zero usage"));
    }
    if (named) {
      (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(result, "name", text_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "name"), $$bc$str(label, ".name")));
    }
    if (untouched) {
      (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(result, "resetState", "untouched");
    } else {
      (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(result, "resetsAt", timestamp_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "resetsAt"), $$bc$str(label, ".resetsAt")));
    }
    return result;
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

function parse_nullable_rung_bang(value, label, named) {
  return ((value == null) ? null : parse_rung_bang(value, label, named));
}

function parse_availability_rows_bang(value) {
  if ((!Array.isArray(value))) {
    error_bang("account availability JSON must be an array");
  }
  const rows = as_array(value).map((entry, index) => { const label = $$bc$str("account availability row[", index, "]");
const raw = foreign_object_bang(entry, label);
exact_fields_bang(raw, ["account", "provider", "observedAt", "stale", "rungs", "verdict", "usableModels"], label);
const provider = text_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "provider"), $$bc$str(label, ".provider"));
const stale = (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "stale");
const rungs = foreign_object_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "rungs"), $$bc$str(label, ".rungs"));
if ((!((provider === "anthropic") || (provider === "openai")))) {
  error_bang($$bc$str(label, ".provider must be anthropic or openai"));
}
if ((!(typeof stale === "boolean"))) {
  error_bang($$bc$str(label, ".stale must be boolean"));
}
exact_fields_bang(rungs, ["window", "week", "models"], $$bc$str(label, ".rungs"));
const models = foreign_object_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(rungs, "models"), $$bc$str(label, ".rungs.models"));
const usable = (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "usableModels");
if (((!Array.isArray(usable)) || (!((_truthy) => _truthy !== false && _truthy != null)(as_array(usable).every((model) => (typeof model === "string")))))) {
  error_bang($$bc$str(label, ".usableModels must be an array of strings"));
}
return $$bh$js_obj("account", text_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "account"), $$bc$str(label, ".account")), "provider", provider, "observedAt", timestamp_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "observedAt"), $$bc$str(label, ".observedAt")), "stale", stale, "rungs", $$bh$js_obj("window", parse_nullable_rung_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(rungs, "window"), $$bc$str(label, ".rungs.window"), true), "week", parse_nullable_rung_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(rungs, "week"), $$bc$str(label, ".rungs.week"), false), "models", Object.fromEntries(Object.entries(models).map((entry) => { const model = text_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(entry, 0), $$bc$str(label, ".rungs.models key"));
const pair = [model, parse_rung_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(entry, 1), $$bc$str(label, ".rungs.models.", model))];
return pair; })), "verdict", availability_verdict_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "verdict"), $$bc$str(label, ".verdict")), "usableModels", as_array(usable).slice())); });
  const identities = rows.map((row) => $$bc$str(row.provider, "\x00", row.account));
  if ((!(new Set(identities).size === identities.length))) {
    error_bang("account availability JSON contains duplicate provider/account rows");
  }
  return rows;
}

const parseAvailabilityRows = parse_availability_rows_bang;

function load_availability_rows_bang(...$beagle$args) {
  if (arguments.length === 1) {
    const north_bin = $beagle$args[0];
    return load_availability_rows_bang(north_bin, exec_file_sync);
  }
  if (arguments.length === 2) {
    const north_bin = $beagle$args[0];
    const invoke = $beagle$args[1];
    const output = invoke(north_bin, ["account", "availability", "--json"], $$bh$js_obj("encoding", "utf8", "timeout", 10000, "stdio", ["ignore", "pipe", "pipe"]));
    return (() => { try {
    return parse_availability_rows_bang(JSON.parse(String(output)));
  } catch (_catch_0) {
    switch ($$bd$catch_dispatch(_catch_0, [Error])) {
      case 0: {
        const failure = _catch_0;
        error_bang($$bc$str("north account availability --json returned invalid data: ", failure.message));
        return [];
        break;
      }
    }
  } })();
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

function load_availability_rows_export_bang(...$beagle$args) {
  if (arguments.length === 0) {
    return load_availability_rows_bang(((_logical) => (_logical !== false && _logical != null ? _logical : $$bc$str(REPO, "/bin/north")))(process.env.NORTH_BIN));
  }
  if (arguments.length === 1) {
    const north_bin = $beagle$args[0];
    return load_availability_rows_bang(north_bin);
  }
  if (arguments.length === 2) {
    const north_bin = $beagle$args[0];
    const invoke = $beagle$args[1];
    return load_availability_rows_bang(north_bin, invoke);
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

const loadAvailabilityRows = load_availability_rows_export_bang;

function failover_threshold_bang(value) {
  const parsed = ((typeof value === "number") ? as_number(value) : Number(value));
  if (((!Number.isFinite(parsed)) || ((parsed <= 0) || (parsed > 100)))) {
    error_bang("failover threshold must be greater than 0 and at most 100");
  }
  return parsed;
}

function failover_threshold_export_bang(...$beagle$args) {
  if (arguments.length === 0) {
    return failover_threshold_bang(DEFAULT_THRESHOLD);
  }
  if (arguments.length === 1) {
    const value = $beagle$args[0];
    return failover_threshold_bang(value);
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

const failoverThreshold = failover_threshold_export_bang;

function normalized_model(provider, model) {
  return ((_logical) => (_logical !== false && _logical != null ? _logical : model))(resolve_model_alias(provider, model));
}

function model_matches_p(provider, candidate, expected) {
  const exact_candidate = normalized_model(provider, candidate);
  const exact_expected = normalized_model(provider, expected);
  return ((exact_candidate === exact_expected) || ((model_family(provider, exact_candidate) === expected) || (candidate === model_family(provider, exact_expected))));
}

function active_row_bang(rows, route) {
  const provider_rows = rows.filter((row) => (row.provider === route.provider));
  const matches = provider_rows.filter((row) => (row.account === route.account));
  return (((matches.length === 1)) ? matches[0] : (((!((_truthy) => _truthy !== false && _truthy != null)(route.account)) && (provider_rows.length === 1))) ? provider_rows[0] : (() => { error_bang($$bc$str("active account ", ((_logical) => (_logical !== false && _logical != null ? _logical : "(unspecified)"))(route.account), " is not a unique ", route.provider, " availability row"));
return $$bh$js_obj(); })());
}

const availabilityForRoute = active_row_bang;

function trigger_for(row, model, threshold) {
  const week = row.rungs.week;
  const window = row.rungs.window;
  return ((((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? ((_logical) => (_logical !== false && _logical != null ? (week.pct >= threshold) : _logical))(week.resetsAt) : _logical))(week))) ? $$bh$js_obj("rung", "week", "name", "week", "pct", week.pct, "resetsAt", week.resetsAt) : (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? ((_logical) => (_logical !== false && _logical != null ? (window.pct >= threshold) : _logical))(window.resetsAt) : _logical))(window))) ? $$bh$js_obj("rung", "window", "name", window.name, "pct", window.pct, "resetsAt", window.resetsAt) : ((!((_truthy) => _truthy !== false && _truthy != null)(model))) ? null : (() => { const entry = Object.entries(row.rungs.models).find((candidate) => model_matches_p(row.provider, (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(candidate, 0), model)); return (((!((_truthy) => _truthy !== false && _truthy != null)(entry)) || ((!((_truthy) => _truthy !== false && _truthy != null)((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(entry, 1).resetsAt)) || ((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(entry, 1).pct < threshold))) ? null : $$bh$js_obj("rung", "model", "name", (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(entry, 0), "model", (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(entry, 0), "pct", (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(entry, 1).pct, "resetsAt", (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(entry, 1).resetsAt)); })());
}

function unknown_reason(row) {
  return (((!((_truthy) => _truthy !== false && _truthy != null)(row.rungs.window))) ? $$bc$str(row.provider, "/", row.account, " window rung is unavailable") : (((row.provider === "anthropic") && (!((_truthy) => _truthy !== false && _truthy != null)(row.rungs.week)))) ? $$bc$str(row.provider, "/", row.account, " week rung is unavailable") : ((row.verdict === "unknown")) ? $$bc$str(row.provider, "/", row.account, " availability verdict is unknown") : null);
}

function classification_for(trigger) {
  return (((!((_truthy) => _truthy !== false && _truthy != null)(trigger))) ? "available" : ((trigger.rung === "week")) ? "account-dead" : ((trigger.rung === "window")) ? "window-dead" : "model-dead");
}

function candidate_usable_p(row, model, threshold) {
  const week = row.rungs.week;
  const window = row.rungs.window;
  if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? _logical : ((_logical) => (_logical !== false && _logical != null ? _logical : ((_logical) => (_logical !== false && _logical != null ? _logical : ((_logical) => (_logical !== false && _logical != null ? (window.pct >= threshold) : _logical))(window)))(((_logical) => (_logical !== false && _logical != null ? (week.pct >= threshold) : _logical))(week))))(unknown_reason(row))))(row.stale))) {
    return false;
  } else {
    const scoped = Object.entries(row.rungs.models).find((entry) => model_matches_p(row.provider, (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(entry, 0), model));
    return ((!((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? ((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(scoped, 1).pct >= threshold) : _logical))(scoped))) && row.usableModels.some((candidate) => model_matches_p(row.provider, candidate, model)));
  }
}

function heir_for(rows, active, threshold) {
  const candidates = rows.flatMap((row) => { if ((((row.provider === active.provider) && (row.account === active.account)) || (!provider_supports_route(row.provider, active.capabilityFloor, active.serviceClass, active.reasoning)))) {
  return [];
} else {
  const resolved = resolve_route(row.provider, active.capabilityFloor, active.serviceClass, null, active.reasoning);
  const model = resolved.model;
  const reasoning = resolved.effort;
  return (((!((_truthy) => _truthy !== false && _truthy != null)(model)) || ((!((_truthy) => _truthy !== false && _truthy != null)(reasoning)) || (!candidate_usable_p(row, model, threshold)))) ? [] : [$$bh$js_obj("route", $$bh$js_obj("provider", row.provider, "account", row.account, "model", model, "capabilityFloor", resolved.capabilityFloor, "serviceClass", resolved.serviceClass, "reasoning", reasoning, "observedAt", row.observedAt), "receipt", row)]);
} });
  candidates.sort((left, right) => ((_logical) => (_logical !== false && _logical != null ? _logical : left.route.account.localeCompare(right.route.account)))((((left.route.provider === active.provider) ? 1 : 0) - ((right.route.provider === active.provider) ? 1 : 0))));
  return (() => { const _x = candidates, _i = 0; return _x[_i] != null ? _x[_i] : null; })();
}

function check_failover_bang(rows, route, threshold_value) {
  const threshold = failover_threshold_bang(threshold_value);
  const receipt = active_row_bang(rows, route);
  if (((_truthy) => _truthy !== false && _truthy != null)(receipt.stale)) {
    error_bang($$bc$str("active availability evidence for ", receipt.provider, "/", receipt.account, " is stale"));
  }
  const active = $$bh$js_obj("provider", route.provider, "account", receipt.account, "capabilityFloor", route.capabilityFloor, "serviceClass", route.serviceClass, "reasoning", route.reasoning);
  if (((_truthy) => _truthy !== false && _truthy != null)(route.model)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(active, "model", normalized_model(route.provider, route.model));
  }
  const reason = unknown_reason(receipt);
  if (((_truthy) => _truthy !== false && _truthy != null)(reason)) {
    return $$bh$js_obj("threshold", threshold, "classification", "unknown", "active", active, "unknownReason", reason, "receipts", $$bh$js_obj("active", receipt));
  } else {
    const trigger = trigger_for(receipt, active.model, threshold);
    const heir = (((_truthy) => _truthy !== false && _truthy != null)(trigger) ? heir_for(rows, active, threshold) : null);
    const result = $$bh$js_obj("threshold", threshold, "classification", classification_for(trigger), "active", active, "receipts", $$bh$js_obj("active", receipt));
    if (((_truthy) => _truthy !== false && _truthy != null)(trigger)) {
      (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(result, "trigger", trigger);
    }
    if (((_truthy) => _truthy !== false && _truthy != null)(heir)) {
      (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(result, "heir", heir.route);
      $$bh$aset(result.receipts, "heir", heir.receipt);
    }
    return result;
  }
}

function check_failover_export_bang(...$beagle$args) {
  if (arguments.length === 2) {
    const rows = $beagle$args[0];
    const route = $beagle$args[1];
    return check_failover_bang(rows, route, DEFAULT_THRESHOLD);
  }
  if (arguments.length === 3) {
    const rows = $beagle$args[0];
    const route = $beagle$args[1];
    const threshold = $beagle$args[2];
    return check_failover_bang(rows, route, threshold);
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

const checkFailover = check_failover_export_bang;

function threshold_crossings_bang(row, threshold_value) {
  const threshold = failover_threshold_bang(threshold_value);
  const result = [];
  const week = row.rungs.week;
  const window = row.rungs.window;
  if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? ((_logical) => (_logical !== false && _logical != null ? (week.pct >= threshold) : _logical))(week.resetsAt) : _logical))(week))) {
    result.push($$bh$js_obj("rung", "week", "name", "week", "pct", week.pct, "resetsAt", week.resetsAt));
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? ((_logical) => (_logical !== false && _logical != null ? (window.pct >= threshold) : _logical))(window.resetsAt) : _logical))(window))) {
    result.push($$bh$js_obj("rung", "window", "name", window.name, "pct", window.pct, "resetsAt", window.resetsAt));
  }
  Object.entries(row.rungs.models).forEach((entry) => { const rung = (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(entry, 1);
if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (rung.pct >= threshold) : _logical))(rung.resetsAt))) {
  result.push($$bh$js_obj("rung", "model", "name", (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(entry, 0), "model", (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(entry, 0), "pct", rung.pct, "resetsAt", rung.resetsAt));
}
return null; });
  return result;
}

function threshold_crossings_export_bang(...$beagle$args) {
  if (arguments.length === 1) {
    const row = $beagle$args[0];
    return threshold_crossings_bang(row, DEFAULT_THRESHOLD);
  }
  if (arguments.length === 2) {
    const row = $beagle$args[0];
    const threshold = $beagle$args[1];
    return threshold_crossings_bang(row, threshold);
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

const thresholdCrossings = threshold_crossings_export_bang;

function runtime_env(runtime) {
  return ((_logical) => (_logical !== false && _logical != null ? _logical : process.env))(runtime.env);
}

function context_package_bang(root_thread, brief_path, runtime) {
  const read_brief = (((_truthy) => _truthy !== false && _truthy != null)(runtime.readBrief) ? (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))($$bh$js_obj("value", runtime.readBrief), "value") : (path) => read_file_sync(path, "utf8"));
  const facts_for = (((_truthy) => _truthy !== false && _truthy != null)(runtime.getFacts) ? (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))($$bh$js_obj("value", runtime.getFacts), "value") : get_thread_facts);
  const children_for = (((_truthy) => _truthy !== false && _truthy != null)(runtime.getChildren) ? (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))($$bh$js_obj("value", runtime.getChildren), "value") : get_children);
  const ids = $$bc$into_value([root_thread], children_for(root_thread));
  const thread_map = ids.map((id) => { const facts = facts_for(id);
const title_fact = facts.find((fact) => (fact.predicate === "title"));
const entry = $$bh$js_obj("id", id, "facts", facts);
if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? title_fact.value : _logical))(title_fact))) {
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(entry, "title", title_fact.value);
}
return entry; });
  const content = read_brief(brief_path);
  return $$bh$js_obj("brief", $$bh$js_obj("path", brief_path, "sha256", create_hash("sha256").update(content).digest("hex"), "content", content), "threadMap", thread_map);
}

function context_package_export_bang(...$beagle$args) {
  if (arguments.length === 2) {
    const root_thread = $beagle$args[0];
    const brief_path = $beagle$args[1];
    return context_package_bang(root_thread, brief_path, $$bh$js_obj());
  }
  if (arguments.length === 3) {
    const root_thread = $beagle$args[0];
    const brief_path = $beagle$args[1];
    const runtime = $beagle$args[2];
    return context_package_bang(root_thread, brief_path, runtime);
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

const contextPackage = context_package_export_bang;

function recovery_detail(check) {
  return $$bc$str("provider-recovery ", JSON.stringify($$bh$js_obj("threshold", check.threshold, "trigger", check.trigger, "activeReceipt", check.receipts.active, "heirReceipt", check.receipts.heir)));
}

function recovery_pin_evidence_bang(check, now) {
  if ((!((_truthy) => _truthy !== false && _truthy != null)(check.heir))) {
    error_bang("cannot compose provider-recovery pin evidence without an heir");
  }
  return $$bh$js_obj("policyVersion", ROUTING_PIN_POLICY_VERSION, "issuedAt", now.toISOString(), "expiresAt", new Date(((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))($$bh$js_obj("value", now.getTime()), "value") + PIN_LIFETIME_MS)).toISOString(), "reasonCode", "provider-recovery", "detail", recovery_detail(check), "pins", [$$bh$js_obj("kind", "provider", "value", check.heir.provider), $$bh$js_obj("kind", "account", "value", check.heir.account), $$bh$js_obj("kind", "model", "value", check.heir.model)]);
}

function recovery_pin_evidence_export_bang(...$beagle$args) {
  if (arguments.length === 1) {
    const check = $beagle$args[0];
    return recovery_pin_evidence_bang(check, new Date());
  }
  if (arguments.length === 2) {
    const check = $beagle$args[0];
    const now = $beagle$args[1];
    return recovery_pin_evidence_bang(check, now);
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

const recoveryPinEvidence = recovery_pin_evidence_export_bang;

function context_prompt(root_thread, context) {
  return [$$bc$str("You are the heir team-lead orchestrator for root thread @", root_thread, "."), "Continue the workstream from this sealed succession context package.", "Treat the embedded North thread facts as asserted coordination state; reconcile fresh evidence before acting.", "", $$bc$str("BRIEF ", context.brief.path, " sha256=", context.brief.sha256), context.brief.content, "", "THREAD MAP", JSON.stringify(context.threadMap)].join("\n");
}

function compose_failover_spawn_bang(check, root_thread, brief_path, notify_target, runtime) {
  if ((check.classification === "unknown")) {
    error_bang($$bc$str("failover fire refused: active availability is unknown (", ((_logical) => (_logical !== false && _logical != null ? _logical : "required rung unavailable"))(check.unknownReason), ")"));
  }
  if ((check.classification === "available")) {
    error_bang("failover fire refused: active route has not crossed the threshold");
  }
  if ((!((_truthy) => _truthy !== false && _truthy != null)(check.heir))) {
    error_bang("failover fire refused: no compatible provider/account/model heir has fresh capacity");
  }
  if ((notify_target.trim() === "")) {
    error_bang("failover fire requires NORTH_FAILOVER_NOTIFY or AGENT_COORDINATOR");
  }
  const env = runtime_env(runtime);
  const now = (((_truthy) => _truthy !== false && _truthy != null)(runtime.now) ? (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))($$bh$js_obj("value", runtime.now), "value") : new Date());
  const context = context_package_bang(root_thread, brief_path, runtime);
  const pin_evidence = recovery_pin_evidence_bang(check, now);
  const prompt = context_prompt(root_thread, context);
  const north_bin = ((_logical) => (_logical !== false && _logical != null ? _logical : ((_logical) => (_logical !== false && _logical != null ? _logical : $$bc$str(REPO, "/bin/north")))($$bc$get(env, "NORTH_BIN"))))(runtime.northBin);
  const subject = "PROVIDER FAILOVER FIRED";
  const body = $$bc$str("@", root_thread, " -> ", check.heir.provider, "/", check.heir.account, "/", check.heir.model, " (", check.heir.capabilityFloor, "/", check.heir.serviceClass, "/", check.heir.reasoning, "); reason=provider-recovery");
  const sender = ((_logical) => (_logical !== false && _logical != null ? _logical : "north-failover"))($$bc$get(env, "AGENT_ID"));
  const port = ((_logical) => (_logical !== false && _logical != null ? _logical : "7977"))($$bc$get(env, "NORTH_PORT"));
  return $$bh$js_obj("version", 1, "check", check, "context", context, "pinEvidence", pin_evidence, "prompt", prompt, "command", $$bh$js_obj("executable", north_bin, "args", ["spawn", "team-lead", prompt, "--thread", root_thread, "--provider", check.heir.provider, "--target", check.heir.account, "--model", check.heir.model, "--pin-evidence", JSON.stringify(pin_evidence), "--notify", notify_target]), "notification", $$bh$js_obj("executable", ((_logical) => (_logical !== false && _logical != null ? _logical : ((_logical) => (_logical !== false && _logical != null ? _logical : "bb"))($$bc$get(env, "NORTH_PEER_BB"))))(runtime.peerBb), "args", babashka_arguments([((_logical) => (_logical !== false && _logical != null ? _logical : $$bc$str(REPO, "/cli/msg-cli.clj")))(runtime.msgCli), port, "send", sender, notify_target, subject, body], env), "target", notify_target, "subject", subject, "body", body));
}

function compose_failover_spawn_export_bang(...$beagle$args) {
  if (arguments.length === 4) {
    const check = $beagle$args[0];
    const root_thread = $beagle$args[1];
    const brief_path = $beagle$args[2];
    const notify_target = $beagle$args[3];
    return compose_failover_spawn_bang(check, root_thread, brief_path, notify_target, $$bh$js_obj());
  }
  if (arguments.length === 5) {
    const check = $beagle$args[0];
    const root_thread = $beagle$args[1];
    const brief_path = $beagle$args[2];
    const notify_target = $beagle$args[3];
    const runtime = $beagle$args[4];
    return compose_failover_spawn_bang(check, root_thread, brief_path, notify_target, runtime);
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

const composeFailoverSpawn = compose_failover_spawn_export_bang;

function run_checked_bang(run, executable, args, label) {
  const result = run(executable, args);
  if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? _logical : (!(result.status === 0))))(result.error))) {
    const detail = String(((_logical) => (_logical !== false && _logical != null ? _logical : ((_logical) => (_logical !== false && _logical != null ? _logical : ""))(((_logical) => (_logical !== false && _logical != null ? result.error.message : _logical))(result.error))))(result.stderr)).trim();
    error_bang($$bc$str(label, " failed", (((_truthy) => _truthy !== false && _truthy != null)(detail) ? $$bc$str(": ", detail) : "")));
  }
  return null;
}

function fire_failover_bang(spawn, runtime) {
  const run_spawn = (((_truthy) => _truthy !== false && _truthy != null)(runtime.run) ? (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))($$bh$js_obj("value", runtime.run), "value") : (executable, args) => spawn_sync(executable, args, $$bh$js_obj("encoding", "utf8", "stdio", ["ignore", "pipe", "pipe"])));
  run_checked_bang(run_spawn, spawn.command.executable, spawn.command.args, "heir spawn");
  (() => { try {
    const run_notification = (((_truthy) => _truthy !== false && _truthy != null)(runtime.run) ? (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))($$bh$js_obj("value", runtime.run), "value") : (executable, args) => spawn_sync(executable, args, $$bh$js_obj("encoding", "utf8", "env", store_environment(runtime_env(runtime)), "timeout", child_timeout(), "stdio", ["ignore", "pipe", "pipe"])));
  return run_notification(spawn.notification.executable, spawn.notification.args);
  } catch (_catch_1) {
    switch ($$bd$catch_dispatch(_catch_1, [Error])) {
      case 0: {
        const __ = _catch_1;
        return null;
        break;
      }
    }
  } })();
  return null;
}

function fire_failover_export_bang(...$beagle$args) {
  if (arguments.length === 1) {
    const spawn = $beagle$args[0];
    return fire_failover_bang(spawn, $$bh$js_obj());
  }
  if (arguments.length === 2) {
    const spawn = $beagle$args[0];
    const runtime = $beagle$args[1];
    return fire_failover_bang(spawn, runtime);
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

const fireFailover = fire_failover_export_bang;

function identity_value(facts, predicate) {
  const fact = facts.find((candidate) => (candidate.predicate === predicate));
  return (((_truthy) => _truthy !== false && _truthy != null)(fact) ? fact.value : null);
}

function active_session_route_bang(rows, provider_override, env, identity_facts) {
  const raw_provider = ((_logical) => (_logical !== false && _logical != null ? _logical : (() => { const provider = $$bc$get(env, "AGENT_PROVIDER"); return (((provider === "anthropic") || (provider === "openai")) ? provider : identity_value(identity_facts, "provider")); })()))(provider_override);
  if ((!((raw_provider === "anthropic") || (raw_provider === "openai")))) {
    error_bang("active provider is unavailable; pass --provider anthropic|openai");
  }
  const provider = as_string(raw_provider);
  const provider_rows = rows.filter((row) => (row.provider === provider));
  const account = ((_logical) => (_logical !== false && _logical != null ? _logical : ((_logical) => (_logical !== false && _logical != null ? _logical : ((provider_rows.length === 1) ? provider_rows[0].account : "")))(identity_value(identity_facts, "provider_target"))))($$bc$get(env, "AGENT_TARGET"));
  const capability_floor = text_bang(((_logical) => (_logical !== false && _logical != null ? _logical : identity_value(identity_facts, "capability_floor")))($$bc$get(env, "AGENT_CAPABILITY_FLOOR")), "active capabilityFloor");
  const service_class = text_bang(((_logical) => (_logical !== false && _logical != null ? _logical : identity_value(identity_facts, "service_class")))($$bc$get(env, "AGENT_SERVICE_CLASS")), "active serviceClass");
  const reasoning = text_bang(((_logical) => (_logical !== false && _logical != null ? _logical : identity_value(identity_facts, "reasoning")))($$bc$get(env, "AGENT_REASONING")), "active reasoning");
  const raw_model = ((_logical) => (_logical !== false && _logical != null ? _logical : identity_value(identity_facts, "model")))($$bc$get(env, "AGENT_MODEL"));
  const model = (((_truthy) => _truthy !== false && _truthy != null)(raw_model) ? normalized_model(provider, raw_model) : null);
  if ((!provider_supports_route(provider, capability_floor, service_class, reasoning, model))) {
    error_bang("active provider route is outside the current model-selection catalog");
  }
  const route = $$bh$js_obj("provider", provider, "account", account, "capabilityFloor", capability_floor, "serviceClass", service_class, "reasoning", reasoning);
  if (((_truthy) => _truthy !== false && _truthy != null)(model)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(route, "model", model);
  }
  return route;
}

function active_session_route_export_bang(...$beagle$args) {
  if (arguments.length === 2) {
    const rows = $beagle$args[0];
    const provider_override = $beagle$args[1];
    return active_session_route_bang(rows, provider_override, process.env, []);
  }
  if (arguments.length === 3) {
    const rows = $beagle$args[0];
    const provider_override = $beagle$args[1];
    const env = $beagle$args[2];
    return active_session_route_bang(rows, provider_override, (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))($$bh$js_obj("value", env), "value"), []);
  }
  if (arguments.length === 4) {
    const rows = $beagle$args[0];
    const provider_override = $beagle$args[1];
    const env = $beagle$args[2];
    const identity_facts = $beagle$args[3];
    return active_session_route_bang(rows, provider_override, (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))($$bh$js_obj("value", env), "value"), identity_facts);
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

const activeSessionRoute = active_session_route_export_bang;

function active_session_identity_facts_bang(provider_override, runtime) {
  const env = runtime_env(runtime);
  if ((!((_truthy) => _truthy !== false && _truthy != null)($$bc$get(env, "AGENT_ID")))) {
    return [];
  } else {
    const provider_known = ((provider_override === "anthropic") || ((provider_override === "openai") || (($$bc$get(env, "AGENT_PROVIDER") === "anthropic") || ($$bc$get(env, "AGENT_PROVIDER") === "openai"))));
    return (((_truthy) => _truthy !== false && _truthy != null)((provider_known && ((_logical) => (_logical !== false && _logical != null ? ((_logical) => (_logical !== false && _logical != null ? ((_logical) => (_logical !== false && _logical != null ? ((_logical) => (_logical !== false && _logical != null ? $$bc$get(env, "AGENT_REASONING") : _logical))($$bc$get(env, "AGENT_SERVICE_CLASS")) : _logical))($$bc$get(env, "AGENT_CAPABILITY_FLOOR")) : _logical))($$bc$get(env, "AGENT_MODEL")) : _logical))($$bc$get(env, "AGENT_TARGET")))) ? [] : (((_logical) => (_logical !== false && _logical != null ? _logical : get_thread_facts))(runtime.getFacts))($$bc$str("agent:", $$bc$get(env, "AGENT_ID"))));
  }
}

function active_session_identity_facts_export_bang(...$beagle$args) {
  if (arguments.length === 1) {
    const provider_override = $beagle$args[0];
    return active_session_identity_facts_bang(provider_override, $$bh$js_obj());
  }
  if (arguments.length === 2) {
    const provider_override = $beagle$args[0];
    const runtime = $beagle$args[1];
    return active_session_identity_facts_bang(provider_override, runtime);
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

const activeSessionIdentityFacts = active_session_identity_facts_export_bang;

function automatic_fire_enabled_p(env) {
  return ["1", "true", "on"].includes(((_logical) => (_logical !== false && _logical != null ? _logical : ""))($$bc$get(env, "NORTH_FAILOVER_AUTO_FIRE")).toLowerCase());
}

function automatic_fire_enabled_export_p(...$beagle$args) {
  if (arguments.length === 0) {
    return automatic_fire_enabled_p(process.env);
  }
  if (arguments.length === 1) {
    const env = $beagle$args[0];
    return automatic_fire_enabled_p((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))($$bh$js_obj("value", env), "value"));
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

const automaticFailoverFireEnabled = automatic_fire_enabled_export_p;

function failover_warning_commands_bang(warning, runtime) {
  const env = runtime_env(runtime);
  const north_bin = ((_logical) => (_logical !== false && _logical != null ? _logical : ((_logical) => (_logical !== false && _logical != null ? _logical : $$bc$str(REPO, "/bin/north")))($$bc$get(env, "NORTH_BIN"))))(runtime.northBin);
  const commands = [];
  if (((_truthy) => _truthy !== false && _truthy != null)(warning.thread)) {
    commands.push($$bh$js_obj("executable", north_bin, "args", ["tell", warning.thread, "failover_warning", JSON.stringify(warning)]));
  }
  const notify = ((_logical) => (_logical !== false && _logical != null ? _logical : $$bc$get(env, "AGENT_COORDINATOR")))($$bc$get(env, "NORTH_FAILOVER_NOTIFY"));
  if (((_truthy) => _truthy !== false && _truthy != null)(notify)) {
    const route = [warning.active.provider, warning.active.account, warning.active.model].filter((value) => Boolean(value)).join("/");
    commands.push($$bh$js_obj("executable", ((_logical) => (_logical !== false && _logical != null ? _logical : ((_logical) => (_logical !== false && _logical != null ? _logical : "bb"))($$bc$get(env, "NORTH_PEER_BB"))))(runtime.peerBb), "args", babashka_arguments([((_logical) => (_logical !== false && _logical != null ? _logical : $$bc$str(REPO, "/cli/msg-cli.clj")))(runtime.msgCli), ((_logical) => (_logical !== false && _logical != null ? _logical : "7977"))($$bc$get(env, "NORTH_PORT")), "send", ((_logical) => (_logical !== false && _logical != null ? _logical : "north-failover"))($$bc$get(env, "AGENT_ID")), notify, "PROVIDER CAPACITY WARNING", $$bc$str(route, " ", warning.crossing.rung, ":", warning.crossing.name, "=", warning.crossing.pct, "% threshold=", warning.threshold, " resets=", warning.crossing.resetsAt, "; automatic-fire=", (((_truthy) => _truthy !== false && _truthy != null)(warning.automaticFire) ? "enabled" : "off"))], env)));
  }
  return commands;
}

function failover_warning_commands_export_bang(...$beagle$args) {
  if (arguments.length === 1) {
    const warning = $beagle$args[0];
    return failover_warning_commands_bang(warning, $$bh$js_obj());
  }
  if (arguments.length === 2) {
    const warning = $beagle$args[0];
    const runtime = $beagle$args[1];
    return failover_warning_commands_bang(warning, runtime);
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

const failoverWarningCommands = failover_warning_commands_export_bang;

function run_best_effort_bang(executable, args, runtime) {
  (() => { try {
    const run = (((_truthy) => _truthy !== false && _truthy != null)(runtime.run) ? (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))($$bh$js_obj("value", runtime.run), "value") : (command, command_args) => spawn_sync(command, command_args, $$bh$js_obj("encoding", "utf8", "env", store_environment(runtime_env(runtime)), "timeout", child_timeout(10000), "stdio", ["ignore", "ignore", "ignore"])));
  return run(executable, args);
  } catch (_catch_2) {
    switch ($$bd$catch_dispatch(_catch_2, [Error])) {
      case 0: {
        const __ = _catch_2;
        return null;
        break;
      }
    }
  } })();
  return null;
}

function observe_failover_usage_sample_bang(runtime) {
  const env = runtime_env(runtime);
  const provider = $$bc$get(env, "AGENT_PROVIDER");
  return (((!(provider === "anthropic")) && ((!(provider === "openai")) && (!((_truthy) => _truthy !== false && _truthy != null)($$bc$get(env, "AGENT_ID"))))) ? [] : (() => { try {
    const load_rows = (((_truthy) => _truthy !== false && _truthy != null)(runtime.loadRows) ? (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))($$bh$js_obj("value", runtime.loadRows), "value") : () => load_availability_rows_bang(((_logical) => (_logical !== false && _logical != null ? _logical : ((_logical) => (_logical !== false && _logical != null ? _logical : $$bc$str(REPO, "/bin/north")))($$bc$get(env, "NORTH_BIN"))))(runtime.northBin)));
  const rows = load_rows();
  const active = active_session_route_bang(rows, null, env, active_session_identity_facts_bang(null, runtime));
  const receipt = active_row_bang(rows, active);
  if (((_truthy) => _truthy !== false && _truthy != null)(receipt.stale)) {
    return [];
  } else {
    const threshold = failover_threshold_bang(((_logical) => (_logical !== false && _logical != null ? _logical : DEFAULT_THRESHOLD))($$bc$get(env, "NORTH_FAILOVER_WARN_THRESHOLD")));
    const automatic_fire = automatic_fire_enabled_p(env);
    const warnings = threshold_crossings_bang(receipt, threshold).map((crossing) => { const warning = $$bh$js_obj("version", 1, "threshold", threshold, "active", active, "observedAt", receipt.observedAt, "crossing", crossing, "automaticFire", automatic_fire);
if (((_truthy) => _truthy !== false && _truthy != null)($$bc$get(env, "AGENT_THREAD"))) {
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(warning, "thread", $$bc$get(env, "AGENT_THREAD"));
}
return warning; });
    warnings.forEach((warning) => { failover_warning_commands_bang(warning, runtime).forEach((command) => run_best_effort_bang(command.executable, command.args, runtime));
return null; });
    if ((automatic_fire && (warnings.length > 0))) {
      const root = ((_logical) => (_logical !== false && _logical != null ? _logical : $$bc$get(env, "AGENT_THREAD")))($$bc$get(env, "NORTH_FAILOVER_ROOT_THREAD"));
      const brief = $$bc$get(env, "NORTH_FAILOVER_BRIEF");
      const check = check_failover_bang(rows, active, threshold);
      if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? ((_logical) => (_logical !== false && _logical != null ? ((!(check.classification === "available")) && check.heir) : _logical))(brief) : _logical))(root))) {
        run_best_effort_bang(((_logical) => (_logical !== false && _logical != null ? _logical : ((_logical) => (_logical !== false && _logical != null ? _logical : $$bc$str(REPO, "/bin/north")))($$bc$get(env, "NORTH_BIN"))))(runtime.northBin), ["failover", "fire", "--thread", root, "--brief", brief], runtime);
      }
    }
    return warnings;
  }
  } catch (_catch_3) {
    switch ($$bd$catch_dispatch(_catch_3, [Error])) {
      case 0: {
        const __ = _catch_3;
        return [];
        break;
      }
    }
  } })());
}

function observe_failover_usage_sample_export_bang(...$beagle$args) {
  if (arguments.length === 0) {
    return observe_failover_usage_sample_bang($$bh$js_obj());
  }
  if (arguments.length === 1) {
    const runtime = $beagle$args[0];
    return observe_failover_usage_sample_bang(runtime);
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

const observeFailoverUsageSample = observe_failover_usage_sample_export_bang;

export { activeSessionIdentityFacts as "activeSessionIdentityFacts" };
export { activeSessionRoute as "activeSessionRoute" };
export { automaticFailoverFireEnabled as "automaticFailoverFireEnabled" };
export { availabilityForRoute as "availabilityForRoute" };
export { checkFailover as "checkFailover" };
export { composeFailoverSpawn as "composeFailoverSpawn" };
export { contextPackage as "contextPackage" };
export { failoverThreshold as "failoverThreshold" };
export { failoverWarningCommands as "failoverWarningCommands" };
export { fireFailover as "fireFailover" };
export { loadAvailabilityRows as "loadAvailabilityRows" };
export { observeFailoverUsageSample as "observeFailoverUsageSample" };
export { parseAvailabilityRows as "parseAvailabilityRows" };
export { recoveryPinEvidence as "recoveryPinEvidence" };
export { thresholdCrossings as "thresholdCrossings" };
