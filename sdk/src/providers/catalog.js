import { keyword as $$bc$keyword, str as $$bc$str } from '../bridge/generated/beagle/core.js';
import { admit_host_object as $$bh$admit_host_object, aget as $$bh$aget, host_object as $$bh$host_object, js_obj as $$bh$js_obj } from '../bridge/generated/beagle/host.js';
import { catch_dispatch as $$bd$catch_dispatch } from '../bridge/generated/beagle/exception-dispatch.js';

const fs_module = require("node:fs");

const readFileSync = fs_module.readFileSync;

const statSync = fs_module.statSync;

const path_module = require("node:path");

const resolve = path_module.resolve;

const sep = path_module.sep;

const graph_module = require("../orchestration-graph-source");

const projectProviderCatalog = graph_module.projectProviderCatalog;

const staffingSource = graph_module.staffingSource;

const warnGraphCatalogFallback = graph_module.warnGraphCatalogFallback;

const machinery_module = require("../../../agent-machinery/index.mjs");

const loadModelSelectionCatalog = machinery_module.loadModelSelectionCatalog;

const resolveExecutionPlan = machinery_module.resolveExecutionPlan;

function catalog_file_identity(path) {
  const stats = statSync(path, $$bh$host_object($$bc$keyword("bigint"), true));
  return $$bh$host_object($$bc$keyword("dev"), stats.dev, $$bc$keyword("ino"), stats.ino, $$bc$keyword("size"), stats.size, $$bc$keyword("mtimeNs"), stats.mtimeNs, $$bc$keyword("ctimeNs"), stats.ctimeNs);
}

function same_catalog_file_p(left, right) {
  return ((left.dev === right.dev) && ((left.ino === right.ino) && ((left.size === right.size) && ((left.mtimeNs === right.mtimeNs) && (left.ctimeNs === right.ctimeNs)))));
}

const node_catalog_file_reader = $$bh$host_object($$bc$keyword("identity"), catalog_file_identity, $$bc$keyword("read"), (path) => readFileSync(path, "utf8"));

function provider_catalog_file_cache_impl_bang(reader, attempts) {
  const entries = new Map();
  const selected_reader = ((_logical) => (_logical !== false && _logical != null ? _logical : node_catalog_file_reader))(reader);
  const selected_attempts = ((_logical) => (_logical !== false && _logical != null ? _logical : 2))(attempts);
  const cache = $$bh$js_obj();
  (cache.load = (path, parse) => (() => { let attempt = 0; while (true) {
    ((attempt >= selected_attempts) ? (() => { return (() => { throw new Error($$bc$str("Orchestration provider catalog changed while reading ", path)); })(); })() : null); const before = (selected_reader.identity)(path); const cached = entries.get(path); if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? same_catalog_file_p(cached.identity, before) : _logical))(cached))) { return cached.value; } else { const source = (selected_reader.read)(path); const after = (selected_reader.identity)(path); if ((!same_catalog_file_p(before, after))) { const _recur_0 = (attempt + 1); attempt = _recur_0; continue; } else { return (() => { const value = parse(source); entries.set(path, $$bh$host_object($$bc$keyword("identity"), after, $$bc$keyword("value"), value));
return value; })(); } }
  } })());
  return cache;
}

const ProviderCatalogFileCache = provider_catalog_file_cache_impl_bang;

const provider_catalog_cache = provider_catalog_file_cache_impl_bang(null, null);

function orchestration_home() {
  return resolve(((_logical) => (_logical !== false && _logical != null ? _logical : resolve(import.meta.dir, "..", "..", "..", "agent-runtime/orchestration")))(process.env.NORTH_AGENT_RUNTIME_HOME), "");
}

function validate_provider_catalog(catalog, provider, where) {
  if (((!(catalog.provider === provider)) || ((!((_truthy) => _truthy !== false && _truthy != null)(catalog.modelAliases)) || ((!((_truthy) => _truthy !== false && _truthy != null)(catalog.models)) || (!((_truthy) => _truthy !== false && _truthy != null)(catalog.modelDeltas)))))) {
    (() => { throw new Error($$bc$str("invalid Orchestration provider catalog for ", provider, " at ", where)); })();
  }
  return catalog;
}

function provider_catalog(provider) {
  const graph_catalog = ((staffingSource() === "graph") ? (() => { try {
    return validate_provider_catalog(projectProviderCatalog(provider), provider, $$bc$str("graph @catalog:current provider ", provider));
  } catch (_catch_0) {
    switch ($$bd$catch_dispatch(_catch_0, [Error])) {
      case 0: {
        const error = _catch_0;
        warnGraphCatalogFallback($$bc$str("provider catalog ", provider), error);
        return null;
        break;
      }
    }
  } })() : null);
  if (((_truthy) => _truthy !== false && _truthy != null)(graph_catalog)) {
    return graph_catalog;
  } else {
    const path = resolve(orchestration_home(), "providers", $$bc$str(provider, ".json"));
    return (provider_catalog_cache.load)(path, (source) => validate_provider_catalog(JSON.parse(source), provider, path));
  }
}

function selection_catalog() {
  return loadModelSelectionCatalog();
}

function provider_selection_row(provider) {
  return selection_catalog().providers.find((row) => (row.id === provider));
}

function inventory() {
  return selection_catalog().providers.flatMap((provider) => provider.models.map((model) => $$bh$host_object($$bc$keyword("provider"), provider.id, $$bc$keyword("model"), model.id, $$bc$keyword("available"), true, $$bc$keyword("efforts"), model.efforts)));
}

function routing_request(capability_floor, service_class, reasoning) {
  const overrides = [];
  if ((!(capability_floor === "standard"))) {
    overrides.push("capabilityFloor");
  }
  if ((!(service_class === "balanced"))) {
    overrides.push("serviceClass");
  }
  if ((!(reasoning === "medium"))) {
    overrides.push("reasoning");
  }
  return $$bh$host_object($$bc$keyword("role"), "implementer", $$bc$keyword("taskGrade"), "mid", $$bc$keyword("domainRequirements"), [], $$bc$keyword("topology"), "worker", $$bc$keyword("capabilityFloor"), capability_floor, $$bc$keyword("serviceClass"), service_class, $$bc$keyword("reasoning"), reasoning, $$bc$keyword("posture"), "deliver", $$bc$keyword("composition"), (((_truthy) => _truthy !== false && _truthy != null)((overrides.length === 0)) ? $$bh$host_object($$bc$keyword("kind"), "template", $$bc$keyword("id"), "implementer", $$bc$keyword("overrides"), overrides) : $$bh$host_object($$bc$keyword("kind"), "template", $$bc$keyword("id"), "implementer", $$bc$keyword("overrides"), overrides, $$bc$keyword("overrideReason"), "Provider route resolution")));
}

function resolve_route_internal(provider, capability_floor, service_class, model, reasoning) {
  const selected_reasoning = ((_logical) => (_logical !== false && _logical != null ? _logical : "medium"))(reasoning);
  const constraints = (((_truthy) => _truthy !== false && _truthy != null)(model) ? $$bh$host_object($$bc$keyword("provider"), provider, $$bc$keyword("model"), model) : $$bh$host_object($$bc$keyword("provider"), provider));
  const plan = resolveExecutionPlan($$bh$host_object($$bc$keyword("request"), routing_request(capability_floor, service_class, selected_reasoning), $$bc$keyword("inventory"), inventory(), $$bc$keyword("constraints"), constraints));
  const selected = plan.selected;
  return $$bh$host_object($$bc$keyword("capabilityFloor"), capability_floor, $$bc$keyword("serviceClass"), service_class, $$bc$keyword("model"), selected.model, $$bc$keyword("effort"), selected.effort);
}

function resolveRoute(...$beagle$args) {
  if (arguments.length === 3) {
    const provider = $beagle$args[0];
    const capability_floor = $beagle$args[1];
    const service_class = $beagle$args[2];
    return resolve_route_internal(provider, capability_floor, service_class, null, null);
  }
  if (arguments.length === 4) {
    const provider = $beagle$args[0];
    const capability_floor = $beagle$args[1];
    const service_class = $beagle$args[2];
    const model = $beagle$args[3];
    return resolve_route_internal(provider, capability_floor, service_class, model, null);
  }
  if (arguments.length === 5) {
    const provider = $beagle$args[0];
    const capability_floor = $beagle$args[1];
    const service_class = $beagle$args[2];
    const model = $beagle$args[3];
    const reasoning = $beagle$args[4];
    return resolve_route_internal(provider, capability_floor, service_class, model, reasoning);
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

function providerSupportsRoute(...$beagle$args) {
  if (arguments.length === 4) {
    const provider = $beagle$args[0];
    const capability_floor = $beagle$args[1];
    const service_class = $beagle$args[2];
    const reasoning = $beagle$args[3];
    return (() => { try {
    resolve_route_internal(provider, capability_floor, service_class, null, reasoning);
  return true;
  } catch (_catch_1) {
    switch ($$bd$catch_dispatch(_catch_1, [Error])) {
      case 0: {
        const __ = _catch_1;
        return false;
        break;
      }
    }
  } })();
  }
  if (arguments.length === 5) {
    const provider = $beagle$args[0];
    const capability_floor = $beagle$args[1];
    const service_class = $beagle$args[2];
    const reasoning = $beagle$args[3];
    const model = $beagle$args[4];
    return (() => { try {
    resolve_route_internal(provider, capability_floor, service_class, model, reasoning);
  return true;
  } catch (_catch_2) {
    switch ($$bd$catch_dispatch(_catch_2, [Error])) {
      case 0: {
        const __ = _catch_2;
        return false;
        break;
      }
    }
  } })();
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

function supportedReasoning(provider, capability_floor) {
  const row = provider_selection_row(provider);
  if (((_truthy) => _truthy !== false && _truthy != null)(row)) {
    const levels = new Map();
    row.models.forEach((model) => {
  if (((_truthy) => _truthy !== false && _truthy != null)(model.capabilityFloors.includes(capability_floor))) {
    model.efforts.forEach((effort) => {
  levels.set(effort, true);
});
  }
});
    return ["low", "medium", "high", "xhigh", "max"].filter((effort) => levels.has(effort));
  } else {
    return [];
  }
}

function resolveModelAlias(provider, model) {
  if ((!((_truthy) => _truthy !== false && _truthy != null)(model))) {
    return null;
  } else {
    const aliases = provider_catalog(provider).modelAliases;
    return (((_truthy) => _truthy !== false && _truthy != null)(Object.hasOwn(aliases, model)) ? (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(aliases, model) : model);
  }
}

function modelFamily(provider, model) {
  if ((!((_truthy) => _truthy !== false && _truthy != null)(model))) {
    return null;
  } else {
    const catalog = provider_catalog(provider);
    const concrete = ((_logical) => (_logical !== false && _logical != null ? _logical : model))(resolveModelAlias(provider, model));
    const entry = Object.entries(catalog.modelAliases).find((candidate) => ((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(candidate, 1) === concrete));
    return (((_truthy) => _truthy !== false && _truthy != null)(entry) ? (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(entry, 0) : null);
  }
}

function modelFamilies(provider) {
  return Object.keys(provider_catalog(provider).modelAliases).sort();
}

function providerSupportsModel(provider, model) {
  if ((!((_truthy) => _truthy !== false && _truthy != null)(model))) {
    return true;
  } else {
    const concrete = ((_logical) => (_logical !== false && _logical != null ? _logical : model))(resolveModelAlias(provider, model));
    return Object.hasOwn(provider_catalog(provider).models, concrete);
  }
}

function observeProviderContextWindow(provider, model) {
  if ((!((_truthy) => _truthy !== false && _truthy != null)(model))) {
    return null;
  } else {
    const catalog = provider_catalog(provider);
    const concrete = ((_logical) => (_logical !== false && _logical != null ? _logical : model))(resolveModelAlias(provider, model));
    const declaration = $$bh$aget(catalog.models, concrete);
    const value = ((_logical) => (_logical !== false && _logical != null ? declaration.contextWindow : _logical))(declaration);
    return (((!((_truthy) => _truthy !== false && _truthy != null)(value)) || ((!Number.isSafeInteger(value.tokens)) || ((value.tokens < 1) || (!(typeof value.effectiveFrom === "string"))))) ? null : $$bh$host_object($$bc$keyword("provider"), provider, $$bc$keyword("model"), concrete, $$bc$keyword("tokens"), value.tokens, $$bc$keyword("effectiveFrom"), value.effectiveFrom, $$bc$keyword("source"), "orchestration-provider-catalog"));
  }
}

function canonicalWriteModel(provider, model) {
  return (((!((_truthy) => _truthy !== false && _truthy != null)(provider)) || (!((_truthy) => _truthy !== false && _truthy != null)(model))) ? null : (() => { try {
    const concrete = ((_logical) => (_logical !== false && _logical != null ? _logical : model))(resolveModelAlias(provider, model));
  return (providerSupportsModel(provider, concrete) ? concrete : null);
  } catch (_catch_3) {
    switch ($$bd$catch_dispatch(_catch_3, [Error])) {
      case 0: {
        const __ = _catch_3;
        return model;
        break;
      }
    }
  } })());
}

function resolveModelDelta(provider, model) {
  const descriptor = $$bh$aget(provider_catalog(provider).modelDeltas, model);
  if ((!((_truthy) => _truthy !== false && _truthy != null)(descriptor))) {
    (() => { throw new Error($$bc$str("provider ", provider, " model ", model, " has no exact modelDeltas entry; declare a calibrated path ", "or explicit none in Orchestration's provider catalog")); })();
  }
  if ((descriptor.kind === "none")) {
    if (((!(typeof descriptor.reason === "string")) || (descriptor.reason.trim() === ""))) {
      (() => { throw new Error($$bc$str("provider ", provider, " model ", model, " has malformed none model delta")); })();
    }
    return $$bh$host_object($$bc$keyword("provider"), provider, $$bc$keyword("model"), model, $$bc$keyword("kind"), "none", $$bc$keyword("reason"), descriptor.reason.trim());
  } else {
    if (((!(descriptor.kind === "calibrated")) || ((!(typeof descriptor.path === "string")) || (descriptor.path.trim() === "")))) {
      (() => { throw new Error($$bc$str("provider ", provider, " model ", model, " has malformed calibrated model delta")); })();
    }
    const root = orchestration_home();
    const absolute_path = resolve(root, descriptor.path);
    if ((!((_truthy) => _truthy !== false && _truthy != null)(absolute_path.startsWith($$bc$str(root, sep))))) {
      (() => { throw new Error($$bc$str("provider ", provider, " model ", model, " delta path escapes Orchestration contract root")); })();
    }
    return $$bh$host_object($$bc$keyword("provider"), provider, $$bc$keyword("model"), model, $$bc$keyword("kind"), "calibrated", $$bc$keyword("path"), descriptor.path, $$bc$keyword("absolutePath"), absolute_path);
  }
}

export { ProviderCatalogFileCache as "ProviderCatalogFileCache" };
export { canonicalWriteModel as "canonicalWriteModel" };
export { modelFamilies as "modelFamilies" };
export { modelFamily as "modelFamily" };
export { observeProviderContextWindow as "observeProviderContextWindow" };
export { providerSupportsModel as "providerSupportsModel" };
export { providerSupportsRoute as "providerSupportsRoute" };
export { resolveModelAlias as "resolveModelAlias" };
export { resolveModelDelta as "resolveModelDelta" };
export { resolveRoute as "resolveRoute" };
export { supportedReasoning as "supportedReasoning" };
