import { keyword as $$bc$keyword, record_instance_p as $$bc$record_instance_p, str as $$bc$str } from '../bridge/generated/beagle/core.js';
import { admit_host_object as $$bh$admit_host_object, aget as $$bh$aget, array as $$bh$array, aset as $$bh$aset, host_object as $$bh$host_object, js_obj as $$bh$js_obj } from '../bridge/generated/beagle/host.js';
import { catch_dispatch as $$bd$catch_dispatch } from '../bridge/generated/beagle/exception-dispatch.js';

const sdk_module = require("@anthropic-ai/claude-agent-sdk");

const sdk_query = sdk_module.query;

const types_module = require("./types");

const providerPreacceptError = types_module.providerPreacceptError;

const provider_routing_module = require("../provider-routing");

const probeAnthropic = provider_routing_module.probeAnthropic;

const observations_module = require("./anthropic-observations");

const observeAnthropicQuery = observations_module.observeAnthropicQuery;

const accounts_module = require("../accounts");

const providerEnvironmentForTarget = accounts_module.providerEnvironmentForTarget;

const capabilities_module = require("../orchestration-capabilities");

const requireOrchestrationCapabilities = capabilities_module.requireOrchestrationCapabilities;

const admission_module = require("../execution-admission");

const admitExecution = admission_module.admitExecution;

const admitPinnedProvider = admission_module.admitPinnedProvider;

const consumeExecutionAdmission = admission_module.consumeExecutionAdmission;

const validateManagedExecutionEnvelope = admission_module.validateManagedExecutionEnvelope;

const readonly_shell_module = require("../readonly-shell");

const READONLY_SHELL_SERVER = readonly_shell_module.READONLY_SHELL_SERVER;

const READONLY_SHELL_TOOL = readonly_shell_module.READONLY_SHELL_TOOL;

const harness_module = require("../harness");

const canonicalHarnessModelAvailability = harness_module.canonicalHarnessModelAvailability;

const COORDINATION_TOOLS = harness_module.COORDINATION_TOOLS;

const hasCanonicalAuthoringHooks = harness_module.hasCanonicalAuthoringHooks;

const hasCanonicalHarnessAuthority = harness_module.hasCanonicalHarnessAuthority;

const managedToolPolicy = harness_module.managedToolPolicy;

const NATIVE_AGENT_TOOLS = harness_module.NATIVE_AGENT_TOOLS;

const ORCHESTRATION_TOOLS = harness_module.ORCHESTRATION_TOOLS;

const observation_store_module = require("../provider-model-observation-store");

const validateModelAdmissionReceipt = observation_store_module.validateModelAdmissionReceipt;

const process_module = require("./anthropic-process");

const createAnthropicProcessLifecycle = process_module.createAnthropicProcessLifecycle;

const settleAnthropicProcessOwner = process_module.settleAnthropicProcessOwner;

const anthropic_wire_module = require("./anthropic-wire");

const AnthropicWireNormalizer = anthropic_wire_module.AnthropicWireNormalizer;

const activity_module = require("../execution-activity");

const createExecutionActivityEmitter = activity_module.createExecutionActivityEmitter;

const tool_activity_module = require("../tool-activity");

const McpActivityAccumulator = tool_activity_module.McpActivityAccumulator;

const wire_module = require("../wire");

const WireContractError = wire_module.WireContractError;

const path_module = require("node:path");

const resolve_path = path_module.resolve;

const SUBSCRIPTION_SAFE_API_KEY_SOURCES = new Set(["oauth", "none"]);

async function dispose_anthropic_sdk_query_bang(raw_query, lifecycle, abort, grace_ms) {
  if (((!((_truthy) => _truthy !== false && _truthy != null)(lifecycle)) || (!((_truthy) => _truthy !== false && _truthy != null)(abort)))) {
    if (((_truthy) => _truthy !== false && _truthy != null)(raw_query)) {
      await raw_query.return(undefined);
    }
    return null;
  } else {
    await settleAnthropicProcessOwner((() => { const options = $$bh$host_object($$bc$keyword("lifecycle"), lifecycle, $$bc$keyword("abortController"), abort); if (((_truthy) => _truthy !== false && _truthy != null)(raw_query)) {
  (options.dispose = () => raw_query.return(undefined));
}
if (((_truthy) => _truthy !== false && _truthy != null)(grace_ms)) {
  (options.disposalGraceMs = grace_ms);
}
return options; })());
    return null;
  }
}

const disposeAnthropicSdkQuery = dispose_anthropic_sdk_query_bang;

function exact_strings_p(actual, expected) {
  return (Array.isArray(actual) && ((actual.length === expected.length) && actual.every((value, index) => (value === expected[index]))));
}

function record_value(value) {
  return (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? ((typeof value === "object") && (!Array.isArray(value))) : _logical))(value)) ? value : null);
}

function object_copy(source, updates) {
  return Object.assign($$bh$js_obj(), source, updates);
}

function normalized_anthropic_message_bang(message) {
  const source = record_value(message);
  return ((!((_truthy) => _truthy !== false && _truthy != null)(source)) ? message : ((((source.type === "system") && ((source.subtype === "init") && (!((_truthy) => _truthy !== false && _truthy != null)(SUBSCRIPTION_SAFE_API_KEY_SOURCES.has(String(source.apiKeySource))))))) ? (() => { throw new Error("anthropic_subscription_authentication_required"); })() : (((source.type === "result") && (!(source.subtype === "success")))) ? object_copy(source, $$bh$host_object($$bc$keyword("errors"), ["anthropic_provider_execution_failed"])) : (((_truthy) => _truthy !== false && _truthy != null)(((source.type === "assistant") && ((_logical) => (_logical !== false && _logical != null ? record_value(source.message) : _logical))(source.error)))) ? object_copy(source, $$bh$host_object($$bc$keyword("message"), object_copy(source.message, $$bh$host_object($$bc$keyword("content"), [])))) : ((source.type === "auth_status")) ? (() => { const normalized = object_copy(source, $$bh$host_object($$bc$keyword("output"), [])); if ((!(source.error === undefined))) {
  (normalized.error = "anthropic_provider_authentication_failed");
}
return normalized; })() : (((source.type === "system") && (source.subtype === "mirror_error"))) ? object_copy(source, $$bh$host_object($$bc$keyword("error"), "anthropic_provider_execution_failed")) : (((source.type === "system") && ((source.subtype === "status") && (!(source.compact_error === undefined))))) ? object_copy(source, $$bh$host_object($$bc$keyword("compact_error"), "anthropic_provider_execution_failed")) : source));
}

function async_iterator(source) {
  return Reflect.apply((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(source, Symbol.asyncIterator), source, $$bh$array());
}

async function normalized_next_bang(iterator) {
  const step = await iterator.next();
  return (((_truthy) => _truthy !== false && _truthy != null)(step.done) ? step : $$bh$host_object($$bc$keyword("done"), false, $$bc$keyword("value"), normalized_anthropic_message_bang(step.value)));
}

function normalized_anthropic_events_bang(source) {
  const source_iterator = async_iterator(source);
  const iterator = $$bh$js_obj();
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(iterator, "next", () => normalized_next_bang(source_iterator));
  if (((_truthy) => _truthy !== false && _truthy != null)(source_iterator.return)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(iterator, "return", () => source_iterator.return());
  }
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(iterator, Symbol.asyncIterator, () => iterator);
  return iterator;
}

function require_denied_bang(denied, tools, capability) {
  if (((_truthy) => _truthy !== false && _truthy != null)(tools.some((tool_name) => (!((_truthy) => _truthy !== false && _truthy != null)(denied.has(tool_name)))))) {
    (() => { throw providerPreacceptError($$bc$str("anthropic_adapter_did_not_enforce_absent_", capability, "_capability")); })();
  }
  return null;
}

function require_allowed_bang(allowed, tools, capability) {
  if (((_truthy) => _truthy !== false && _truthy != null)(tools.some((tool_name) => (!((_truthy) => _truthy !== false && _truthy != null)(allowed.has(tool_name)))))) {
    (() => { throw providerPreacceptError($$bc$str("anthropic_adapter_did_not_apply_", capability, "_capability")); })();
  }
  return null;
}

function exact_capability_bang(capabilities, allowed, denied, data_only, capability, tools) {
  if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (!data_only) : _logical))(capabilities.includes(capability)))) {
    require_allowed_bang(allowed, tools, capability.replace(".", "_"));
  } else {
    require_denied_bang(denied, tools, capability.replace(".", "_"));
  }
  return null;
}

function validate_anthropic_harness_bang(options) {
  if ((!((_truthy) => _truthy !== false && _truthy != null)(Object.hasOwn(options, "northCapabilities")))) {
    return null;
  } else {
    const capabilities = requireOrchestrationCapabilities(options.northCapabilities, "northCapabilities");
    const data_only = (options.northDataOnly === true);
    if ((!hasCanonicalHarnessAuthority(options, "anthropic"))) {
      (() => { throw providerPreacceptError("anthropic_harness_authority_seal_missing"); })();
    }
    validateManagedExecutionEnvelope("anthropic", capabilities, options);
    admitPinnedProvider("anthropic", capabilities);
    const policy = managedToolPolicy(capabilities);
    const denied = new Set(((_logical) => (_logical !== false && _logical != null ? _logical : []))(options.disallowedTools));
    const allowed = new Set(((_logical) => (_logical !== false && _logical != null ? _logical : []))(options.allowedTools));
    if (((!Array.isArray(options.settingSources)) || (!(options.settingSources.length === 0)))) {
      (() => { throw providerPreacceptError("anthropic_setting_sources_must_be_isolated"); })();
    }
    if ((!(options.strictMcpConfig === true))) {
      (() => { throw providerPreacceptError("anthropic_strict_mcp_config_required"); })();
    }
    require_denied_bang(denied, NATIVE_AGENT_TOOLS, "native_agent");
    if (data_only) {
      require_denied_bang(denied, COORDINATION_TOOLS, "north");
    } else {
      require_allowed_bang(allowed, COORDINATION_TOOLS, "north");
    }
    exact_capability_bang(capabilities, allowed, denied, data_only, "filesystem.read", ["Read"]);
    exact_capability_bang(capabilities, allowed, denied, data_only, "filesystem.search", ["Grep", "Glob"]);
    exact_capability_bang(capabilities, allowed, denied, data_only, "filesystem.write", ["Edit", "Write", "NotebookEdit"]);
    exact_capability_bang(capabilities, allowed, denied, data_only, "web", ["WebSearch", "WebFetch"]);
    if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (!data_only) : _logical))(capabilities.includes("shell")))) {
      require_allowed_bang(allowed, ["Bash"], "shell");
      require_denied_bang(denied, [READONLY_SHELL_TOOL], "readonly_shell");
    } else if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (!data_only) : _logical))(capabilities.includes("shell.readonly")))) {
      require_denied_bang(denied, ["Bash"], "shell");
      require_allowed_bang(allowed, [READONLY_SHELL_TOOL], "readonly_shell");
    } else {
      require_denied_bang(denied, ["Bash", READONLY_SHELL_TOOL], "shell");
    }
    const expected_mcp_servers = [];
    if ((!data_only)) {
      expected_mcp_servers.push("north");
      if (((_truthy) => _truthy !== false && _truthy != null)(capabilities.includes("coordination"))) {
        expected_mcp_servers.push("north-peer");
      }
      if (((_truthy) => _truthy !== false && _truthy != null)(capabilities.includes("shell.readonly"))) {
        expected_mcp_servers.push(READONLY_SHELL_SERVER);
      }
    }
    if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (!data_only) : _logical))(capabilities.includes("coordination")))) {
      require_allowed_bang(allowed, ORCHESTRATION_TOOLS, "coordination");
      const peer = $$bh$aget(((_logical) => (_logical !== false && _logical != null ? _logical : $$bh$host_object()))(options.mcpServers), "north-peer");
      if (((!((_truthy) => _truthy !== false && _truthy != null)(peer)) || ((!(peer.type === "sdk")) || (!(peer.name === "north-peer"))))) {
        (() => { throw providerPreacceptError("anthropic_coordination_server_contract_missing"); })();
      }
    } else {
      require_denied_bang(denied, ORCHESTRATION_TOOLS, "coordination");
    }
    const permission_mode = (((_truthy) => _truthy !== false && _truthy != null)(capabilities.includes("filesystem.write")) ? "acceptEdits" : "default");
    if ((!(options.permissionMode === permission_mode))) {
      (() => { throw providerPreacceptError("anthropic_permission_mode_contract_missing"); })();
    }
    if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (!data_only) : _logical))(capabilities.includes("shell.readonly")))) {
      const readonly = $$bh$aget(((_logical) => (_logical !== false && _logical != null ? _logical : $$bh$host_object()))(options.mcpServers), READONLY_SHELL_SERVER);
      if (((!((_truthy) => _truthy !== false && _truthy != null)(readonly)) || ((!(readonly.type === "sdk")) || (!(readonly.name === READONLY_SHELL_SERVER))))) {
        (() => { throw providerPreacceptError("anthropic_readonly_shell_contract_missing"); })();
      }
    }
    if ((!exact_strings_p(Object.keys(((_logical) => (_logical !== false && _logical != null ? _logical : $$bh$host_object()))(options.mcpServers)), expected_mcp_servers))) {
      (() => { throw providerPreacceptError("anthropic_mcp_server_surface_contract_missing"); })();
    }
    if ((!exact_strings_p(options.tools, (data_only ? [] : policy.tools)))) {
      (() => { throw providerPreacceptError("anthropic_builtin_tool_surface_contract_missing"); })();
    }
    if ((!exact_strings_p(options.allowedTools, (data_only ? [] : policy.allowedTools)))) {
      (() => { throw providerPreacceptError("anthropic_auto_approval_contract_missing"); })();
    }
    const expected_denied = (data_only ? Array.from(new Set(policy.allowedTools.concat(policy.disallowedTools))) : policy.disallowedTools);
    if ((!exact_strings_p(options.disallowedTools, expected_denied))) {
      (() => { throw providerPreacceptError("anthropic_denied_tool_contract_missing"); })();
    }
    if ((!hasCanonicalAuthoringHooks(options))) {
      (() => { throw providerPreacceptError("anthropic_authoring_guard_contract_missing"); })();
    }
    return capabilities;
  }
}

async function admit_anthropic_bang(...$beagle$args) {
  if (arguments.length === 1) {
    const options = $beagle$args[0];
    return await admit_anthropic_bang(options, null);
  }
  if (arguments.length === 2) {
    const options = $beagle$args[0];
    const target = $beagle$args[1];
    const capabilities = validate_anthropic_harness_bang(options);
    if (((_truthy) => _truthy !== false && _truthy != null)(capabilities)) {
      const model_availability = canonicalHarnessModelAvailability(options, "anthropic");
      if ((!((_truthy) => _truthy !== false && _truthy != null)(model_availability))) {
        (() => { throw providerPreacceptError("anthropic_model_availability_authority_missing"); })();
      }
      if (((_truthy) => _truthy !== false && _truthy != null)(model_availability.required)) {
        if (((!((_truthy) => _truthy !== false && _truthy != null)(target)) || ((!(model_availability.targetId === target.id)) || ((!(model_availability.model === options.model)) || ((!(typeof options.model === "string")) || (!await validateModelAdmissionReceipt(model_availability.receipt, target, options.model, model_availability.observationPath))))))) {
          (() => { throw providerPreacceptError("anthropic_model_availability_unproven"); })();
        }
      }
      await admitExecution("anthropic", capabilities, resolve_path(((_logical) => (_logical !== false && _logical != null ? _logical : process.cwd()))(options.cwd)), options, target);
    }
    return null;
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

const admitAnthropic = admit_anthropic_bang;

async function sdk_input_next_bang(source_iterator) {
  const step = await source_iterator.next();
  if (((_truthy) => _truthy !== false && _truthy != null)(step.done)) {
    return step;
  } else {
    const message = step.value;
    if (((!(message.kind === "user.input")) || (!(typeof message.text === "string")))) {
      (() => { throw new TypeError("anthropic wire input message is malformed"); })();
    }
    return $$bh$host_object($$bc$keyword("done"), false, $$bc$keyword("value"), $$bh$host_object($$bc$keyword("type"), "user", $$bc$keyword("message"), $$bh$host_object($$bc$keyword("role"), "user", $$bc$keyword("content"), message.text), $$bc$keyword("parent_tool_use_id"), null));
  }
}

function anthropic_input_bang(input) {
  if ((typeof input === "string")) {
    return input;
  } else {
    const source_iterator = async_iterator(input);
    const iterator = $$bh$js_obj();
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(iterator, "next", () => sdk_input_next_bang(source_iterator));
    if (((_truthy) => _truthy !== false && _truthy != null)(source_iterator.return)) {
      (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(iterator, "return", () => source_iterator.return());
    }
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(iterator, Symbol.asyncIterator, () => iterator);
    return iterator;
  }
}

function provider_failure() {
  return new Error("anthropic_provider_execution_failed");
}

function anthropic_continuation_extra_arg_p(key) {
  const flag = ((_logical) => (_logical !== false && _logical != null ? _logical : key))((() => { const _x = key.split("="), _i = 0; return _x[_i] != null ? _x[_i] : null; })());
  const stripped = flag.replace(/^--?/, "");
  const normalized = stripped.replaceAll("-", "").replaceAll("_", "").toLowerCase();
  return ["continue", "continueconversation", "resume", "forksession", "resumesessionat", "sessionid"].includes(normalized);
}

function without_caller_session_continuation_bang(options) {
  const neutral = object_copy(options, $$bh$host_object());
  const extra_args = options.extraArgs;
  ["resume", "continue", "continueConversation", "forkSession", "resumeSessionAt", "sessionId", "extraArgs"].forEach((field) => {
  Reflect.deleteProperty(neutral, field);
});
  if ((!(extra_args === undefined))) {
    const neutral_extra = $$bh$js_obj();
    Object.keys(extra_args).forEach((key) => {
  if ((!anthropic_continuation_extra_arg_p(key))) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(neutral_extra, key, (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(extra_args, key));
  }
});
    if ((Object.keys(neutral_extra).length > 0)) {
      (neutral.extraArgs = neutral_extra);
    }
  }
  return neutral;
}

function ensure_open_bang(state) {
  if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? _logical : ((_logical) => (_logical !== false && _logical != null ? state.callerSignal.aborted : _logical))(state.callerSignal)))(state.closed))) {
    (() => { throw new Error("anthropic_query_closed"); })();
  }
  return null;
}

function publish_bang(state, event) {
  state.listeners.forEach((listener) => { (() => { try {
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

async function cleanup_turn_impl_bang(turn) {
  await (async () => { try {
    return await dispose_anthropic_sdk_query_bang(turn.rawQuery, turn.lifecycle, turn.abort, null);
  } finally {
    if (((_truthy) => _truthy !== false && _truthy != null)(turn.detachCallerAbort)) {
      (turn.detachCallerAbort)();
    }
  } })();
  return null;
}

function cleanup_turn_bang(state, turn) {
  if ((!((_truthy) => _truthy !== false && _truthy != null)(turn.cleanup))) {
    (turn.cleanup = cleanup_turn_impl_bang(turn));
  }
  (state.latestCleanup = turn.cleanup);
  return turn.cleanup;
}

function observe_session_id_bang(session_state, session_id) {
  if ((!((_truthy) => _truthy !== false && _truthy != null)(session_state.ids.has(session_id)))) {
    if ((session_state.ids.size >= 2)) {
      (session_state.lost = true);
    } else {
      session_state.ids.add(session_id);
    }
  }
  return null;
}

async function build_turn_bang(state) {
  ensure_open_bang(state);
  const args = state.args;
  const runtime = state.runtime;
  const options = args.options;
  if (((_truthy) => _truthy !== false && _truthy != null)(state.admitted)) {
    validate_anthropic_harness_bang(options);
  } else {
    await (((_logical) => (_logical !== false && _logical != null ? _logical : admit_anthropic_bang))(runtime.admit))(options, args.target);
    ensure_open_bang(state);
    (state.admitted = true);
  }
  const input = state.pendingInput;
  const resume = state.pendingResume;
  if ((input === undefined)) {
    (() => { throw new Error("anthropic_turn_input_unavailable"); })();
  }
  const abort = new AbortController();
  const caller_signal = state.callerSignal;
  const detach_state = $$bh$host_object($$bc$keyword("detach"), undefined);
  if (((_truthy) => _truthy !== false && _truthy != null)(caller_signal)) {
    const forward = () => { abort.abort(caller_signal.reason);
return null; };
    if (((_truthy) => _truthy !== false && _truthy != null)(caller_signal.aborted)) {
      forward();
    } else {
      caller_signal.addEventListener("abort", forward, $$bh$host_object($$bc$keyword("once"), true));
      (detach_state.detach = () => { caller_signal.removeEventListener("abort", forward);
return null; });
    }
  }
  const lifecycle = (() => { try {
    return (runtime.createLifecycle)();
  } catch (_catch_1) {
    switch ($$bd$catch_dispatch(_catch_1, [Error])) {
      case 0: {
        const __ = _catch_1;
        if (((_truthy) => _truthy !== false && _truthy != null)(detach_state.detach)) {
          (detach_state.detach)();
        }
        return (() => { throw provider_failure(); })();
        break;
      }
    }
  } })();
  const holder = $$bh$host_object($$bc$keyword("rawQuery"), undefined);
  return (async () => { try {
    const query_options = object_copy(without_caller_session_continuation_bang(options), $$bh$host_object($$bc$keyword("abortController"), abort, $$bc$keyword("spawnClaudeCodeProcess"), lifecycle.spawnClaudeCodeProcess, $$bc$keyword("env"), providerEnvironmentForTarget("anthropic", args.target, $$bh$host_object($$bc$keyword("env"), options.env))));
  if (((_truthy) => _truthy !== false && _truthy != null)(resume)) {
    (query_options.resume = resume);
  }
  (holder.rawQuery = (runtime.query)($$bh$host_object($$bc$keyword("prompt"), anthropic_input_bang(input), $$bc$keyword("options"), query_options)));
  const session_state = $$bh$host_object($$bc$keyword("ids"), new Set(), $$bc$keyword("lost"), false);
  const observed = (runtime.observe)(normalized_anthropic_events_bang(holder.rawQuery), $$bh$host_object($$bc$keyword("targetId"), () => (((_truthy) => _truthy !== false && _truthy != null)(args.target) ? args.target.id : "anthropic"), $$bc$keyword("mcpAccumulator"), state.mcp, $$bc$keyword("onSessionId"), (session_id) => observe_session_id_bang(session_state, session_id)));
  const turn = $$bh$host_object($$bc$keyword("rawQuery"), holder.rawQuery, $$bc$keyword("observed"), observed, $$bc$keyword("sourceIterator"), async_iterator(observed), $$bc$keyword("lifecycle"), lifecycle, $$bc$keyword("abort"), abort, $$bc$keyword("detachCallerAbort"), detach_state.detach, $$bc$keyword("sessionState"), session_state, $$bc$keyword("consumed"), false, $$bc$keyword("cleanup"), undefined);
  (state.pendingInput = undefined);
  (state.pendingResume = undefined);
  (state.activeTurn = turn);
  return turn;
  } catch (_catch_2) {
    switch ($$bd$catch_dispatch(_catch_2, [Error])) {
      case 0: {
        const __ = _catch_2;
        await (async () => { try {
    return await dispose_anthropic_sdk_query_bang(holder.rawQuery, lifecycle, abort, null);
  } catch (_catch_3) {
    switch ($$bd$catch_dispatch(_catch_3, [Error])) {
      case 0: {
        const __dispose = _catch_3;
        return null;
        break;
      }
    }
  } })();
        if (((_truthy) => _truthy !== false && _truthy != null)(detach_state.detach)) {
          (detach_state.detach)();
        }
        return (() => { throw provider_failure(); })();
        break;
      }
    }
  } })();
}

async function initialize_turn_bang(state) {
  ensure_open_bang(state);
  if (((_truthy) => _truthy !== false && _truthy != null)(state.activeTurn)) {
    return state.activeTurn;
  } else {
    if ((state.pendingInput === undefined)) {
      (() => { throw new Error("anthropic_turn_input_unavailable"); })();
    }
    if ((!((_truthy) => _truthy !== false && _truthy != null)(state.initialization))) {
      (state.initialization = build_turn_bang(state));
    }
    return await state.initialization;
  }
}

function continue_turn_bang(state, input) {
  ensure_open_bang(state);
  if (((!((_truthy) => _truthy !== false && _truthy != null)(state.turnCompleted)) || ((!((_truthy) => _truthy !== false && _truthy != null)(state.continuationSessionId)) || (!(state.pendingInput === undefined))))) {
    (() => { throw new Error("anthropic_continuation_unavailable"); })();
  }
  state.normalizer.beginNextTurn();
  (state.pendingInput = input);
  (state.pendingResume = state.continuationSessionId);
  (state.continuationSessionId = undefined);
  (state.turnCompleted = false);
  return Promise.resolve(null);
}

async function interrupt_turn_bang(state) {
  await (async () => { try {
    return await (await initialize_turn_bang(state)).rawQuery.interrupt();
  } catch (_catch_4) {
    switch ($$bd$catch_dispatch(_catch_4, [Error])) {
      case 0: {
        const __ = _catch_4;
        return (() => { throw provider_failure(); })();
        break;
      }
    }
  } })();
  return null;
}

async function close_query_impl_bang(state) {
  (state.closed = true);
  if (((!((_truthy) => _truthy !== false && _truthy != null)(state.initialization)) && ((!((_truthy) => _truthy !== false && _truthy != null)(state.activeTurn)) && (!((_truthy) => _truthy !== false && _truthy != null)(state.latestCleanup))))) {
    null;
  } else {
    await (async () => { try {
    return ((((_truthy) => _truthy !== false && _truthy != null)(state.activeTurn)) ? await cleanup_turn_bang(state, state.activeTurn) : (((_truthy) => _truthy !== false && _truthy != null)(state.initialization)) ? await cleanup_turn_bang(state, await state.initialization) : (((_truthy) => _truthy !== false && _truthy != null)(state.latestCleanup)) ? await state.latestCleanup : null);
  } catch (_catch_5) {
    switch ($$bd$catch_dispatch(_catch_5, [Error])) {
      case 0: {
        const error = _catch_5;
        if ((!(error.message === "anthropic_query_closed"))) {
          return (() => { throw provider_failure(); })();
        }
        break;
      }
    }
  } })();
  }
  return null;
}

function close_query_bang(state) {
  if ((!((_truthy) => _truthy !== false && _truthy != null)(state.closePromise))) {
    (state.closePromise = close_query_impl_bang(state));
  }
  return state.closePromise;
}

function force_close_query_bang(state) {
  (state.closed = true);
  if (((_truthy) => _truthy !== false && _truthy != null)(state.activeTurn)) {
    state.activeTurn.lifecycle.forceKill();
  }
  return null;
}

async function set_model_bang(state, selection) {
  if ((!(selection.provider === "anthropic"))) {
    (() => { throw provider_failure(); })();
  }
  const model = selection.model;
  await (async () => { try {
    if (((!(typeof model === "string")) || (model === ""))) {
    (() => { throw provider_failure(); })();
  }
  await (await initialize_turn_bang(state)).rawQuery.setModel(model);
  return state.normalizer.setModel(selection);
  } catch (_catch_6) {
    switch ($$bd$catch_dispatch(_catch_6, [Error])) {
      case 0: {
        const __ = _catch_6;
        return (() => { throw provider_failure(); })();
        break;
      }
    }
  } })();
  return null;
}

async function apply_flag_settings_bang(state, settings) {
  await (async () => { try {
    const wire_settings = $$bh$js_obj();
  if ((!(settings.effortLevel === undefined))) {
    (wire_settings.effortLevel = settings.effortLevel);
  }
  await (await initialize_turn_bang(state)).rawQuery.applyFlagSettings(wire_settings);
  return state.normalizer.setEffort(((_logical) => (_logical !== false && _logical != null ? _logical : undefined))(settings.effortLevel));
  } catch (_catch_7) {
    switch ($$bd$catch_dispatch(_catch_7, [Error])) {
      case 0: {
        const __ = _catch_7;
        return (() => { throw provider_failure(); })();
        break;
      }
    }
  } })();
  return null;
}

function finish_iteration_bang(state, iteration) {
  (iteration.done = true);
  (state.iterating = false);
  return $$bh$host_object($$bc$keyword("done"), true, $$bc$keyword("value"), undefined);
}

function record_accepted_events_bang(state, iteration, accepted, turn) {
  state.activity.record("provider", "provider.anthropic.event.accepted");
  accepted.events.forEach((event) => { if (((_truthy) => _truthy !== false && _truthy != null)(((event.kind === "model-call.completed") && accepted.turnOutcome))) {
  (iteration.terminalSeen = true);
  (state.turnCompleted = true);
  const session_state = turn.sessionState;
  (state.continuationSessionId = (((!((_truthy) => _truthy !== false && _truthy != null)(session_state.lost)) && (session_state.ids.size === 1)) ? session_state.ids.values().next().value : undefined));
}
publish_bang(state, event);
iteration.queue.push(event);
return null; });
  return null;
}

function abrupt_settlement_bang(state, iteration, failure) {
  (state.pendingInput = undefined);
  (state.pendingResume = undefined);
  (state.continuationSessionId = undefined);
  const cancelled = ((_logical) => (_logical !== false && _logical != null ? _logical : ((_logical) => (_logical !== false && _logical != null ? state.callerSignal.aborted : _logical))(state.callerSignal)))(state.closed);
  const settled = state.normalizer.settleAbrupt((cancelled ? "cancelled" : "failed"));
  settled.events.forEach((event) => { publish_bang(state, event);
iteration.queue.push(event);
return null; });
  if (cancelled) {
    (iteration.finishAfterQueue = true);
  } else {
    (iteration.errorAfterQueue = ($$bc$record_instance_p("north.providers.anthropic/WireContractError", failure) ? failure : provider_failure()));
  }
  return null;
}

async function settle_finished_turn_bang(state, iteration, turn, failure) {
  const settled_failure = $$bh$host_object($$bc$keyword("value"), failure);
  await (async () => { try {
    return await cleanup_turn_bang(state, turn);
  } catch (_catch_8) {
    switch ($$bd$catch_dispatch(_catch_8, [Error])) {
      case 0: {
        const error = _catch_8;
        if ((!((_truthy) => _truthy !== false && _truthy != null)(settled_failure.value))) {
          return (settled_failure.value = error);
        }
        break;
      }
    }
  } })();
  (turn.consumed = true);
  (state.activeTurn = undefined);
  (state.initialization = undefined);
  if (((_truthy) => _truthy !== false && _truthy != null)(settled_failure.value)) {
    abrupt_settlement_bang(state, iteration, settled_failure.value);
  } else {
    if ((state.pendingInput === undefined)) {
      (iteration.finishAfterQueue = true);
    } else {
      (state.turnCompleted = false);
      (iteration.terminalSeen = false);
    }
  }
  return null;
}

async function wire_next_bang(state, iteration) {
  return (((_truthy) => _truthy !== false && _truthy != null)(iteration.done) ? $$bh$host_object($$bc$keyword("done"), true, $$bc$keyword("value"), undefined) : (async () => { try {
    return (async () => {  while (true) {
    if ((iteration.queue.length > 0)) { return $$bh$host_object($$bc$keyword("done"), false, $$bc$keyword("value"), iteration.queue.shift()); } else if (((_truthy) => _truthy !== false && _truthy != null)(iteration.errorAfterQueue)) { return (() => { (state.iterating = false);
(iteration.done = true);
return (() => { throw iteration.errorAfterQueue; })(); })(); } else if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? _logical : state.closed))(iteration.finishAfterQueue))) { return finish_iteration_bang(state, iteration); } else { const turn = await initialize_turn_bang(state); (((_truthy) => _truthy !== false && _truthy != null)(turn.consumed) ? (() => { return (() => { throw new Error("anthropic_turn_already_consumed"); })(); })() : null); const step_state = $$bh$host_object($$bc$keyword("step"), undefined, $$bc$keyword("failure"), undefined); await (async () => { try {
    return (step_state.step = await turn.sourceIterator.next());
  } catch (_catch_9) {
    switch ($$bd$catch_dispatch(_catch_9, [Error])) {
      case 0: {
        const error = _catch_9;
        return (step_state.failure = error);
        break;
      }
    }
  } })(); if (((_truthy) => _truthy !== false && _truthy != null)(step_state.failure)) { await settle_finished_turn_bang(state, iteration, turn, step_state.failure);  continue; } else { if (((_truthy) => _truthy !== false && _truthy != null)(step_state.step.done)) { await settle_finished_turn_bang(state, iteration, turn, (((_truthy) => _truthy !== false && _truthy != null)(iteration.terminalSeen) ? null : provider_failure()));  continue; } else { (() => { const observed = step_state.step.value; const accept_options = $$bh$js_obj(); if ((!(observed.providerJoin === undefined))) {
  (accept_options.providerJoin = observed.providerJoin);
}
return record_accepted_events_bang(state, iteration, state.normalizer.accept(observed.event, accept_options), turn); })();  continue; } } }
  } })();
  } catch (_catch_10) {
    switch ($$bd$catch_dispatch(_catch_10, [Error])) {
      case 0: {
        const error = _catch_10;
        if ((!((_truthy) => _truthy !== false && _truthy != null)(iteration.done))) {
          (iteration.done = true);
          (state.iterating = false);
        }
        return (() => { throw error; })();
        break;
      }
    }
  } })());
}

function return_iteration_bang(state, iteration) {
  (iteration.done = true);
  (state.iterating = false);
  return Promise.resolve($$bh$host_object($$bc$keyword("done"), true, $$bc$keyword("value"), undefined));
}

function begin_iteration_bang(state) {
  if (((_truthy) => _truthy !== false && _truthy != null)(state.closed)) {
    const closed_iterator = $$bh$js_obj();
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(closed_iterator, "next", () => Promise.resolve($$bh$host_object($$bc$keyword("done"), true, $$bc$keyword("value"), undefined)));
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(closed_iterator, Symbol.asyncIterator, () => closed_iterator);
    return closed_iterator;
  } else {
    if (((_truthy) => _truthy !== false && _truthy != null)(state.iterating)) {
      (() => { throw new Error("anthropic_turn_already_consumed"); })();
    }
    (state.iterating = true);
    const iteration = $$bh$host_object($$bc$keyword("queue"), [], $$bc$keyword("terminalSeen"), false, $$bc$keyword("finishAfterQueue"), false, $$bc$keyword("errorAfterQueue"), undefined, $$bc$keyword("done"), false);
    const iterator = $$bh$js_obj();
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(iterator, "next", () => wire_next_bang(state, iteration));
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(iterator, "return", () => return_iteration_bang(state, iteration));
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(iterator, Symbol.asyncIterator, () => iterator);
    return iterator;
  }
}

function default_anthropic_runtime() {
  return $$bh$host_object($$bc$keyword("query"), sdk_query, $$bc$keyword("observe"), observeAnthropicQuery, $$bc$keyword("createLifecycle"), createAnthropicProcessLifecycle, $$bc$keyword("admit"), admit_anthropic_bang);
}

function create_anthropic_query_bang(args, admitted, runtime) {
  const normalizer = new AnthropicWireNormalizer(args.context.writer, args.context.route, args.context.artifacts);
  const activity = createExecutionActivityEmitter();
  const mcp = new McpActivityAccumulator("anthropic-agent-sdk:assistant-tool-use");
  const caller_signal = (((_truthy) => _truthy !== false && _truthy != null)(args.options.abortController) ? args.options.abortController.signal : undefined);
  const state = $$bh$host_object($$bc$keyword("args"), args, $$bc$keyword("runtime"), runtime, $$bc$keyword("normalizer"), normalizer, $$bc$keyword("activity"), activity, $$bc$keyword("mcp"), mcp, $$bc$keyword("listeners"), new Set(), $$bc$keyword("callerSignal"), caller_signal, $$bc$keyword("pendingInput"), args.input, $$bc$keyword("pendingResume"), undefined, $$bc$keyword("continuationSessionId"), undefined, $$bc$keyword("activeTurn"), undefined, $$bc$keyword("initialization"), undefined, $$bc$keyword("latestCleanup"), undefined, $$bc$keyword("closePromise"), undefined, $$bc$keyword("closed"), false, $$bc$keyword("iterating"), false, $$bc$keyword("turnCompleted"), false, $$bc$keyword("admitted"), admitted);
  const wire_query = $$bh$js_obj();
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(wire_query, "executionTransport", "sdk-stream");
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(wire_query, "executionActivity", activity.source);
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(wire_query, "subscribeProviderEvents", (listener) => { state.listeners.add(listener);
return () => { state.listeners.delete(listener);
return null; }; });
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(wire_query, "mcpActivity", () => state.mcp.snapshot());
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(wire_query, "continueTurn", (input) => continue_turn_bang(state, input));
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(wire_query, "interruptTurn", () => interrupt_turn_bang(state));
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(wire_query, "interrupt", () => interrupt_turn_bang(state));
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(wire_query, "close", () => close_query_bang(state));
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(wire_query, "forceClose", () => force_close_query_bang(state));
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(wire_query, "setModel", (selection) => set_model_bang(state, selection));
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(wire_query, "applyFlagSettings", (settings) => apply_flag_settings_bang(state, settings));
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(wire_query, "supportsInFlightEscalation", () => true);
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(wire_query, Symbol.asyncIterator, () => begin_iteration_bang(state));
  return wire_query;
}

function create_anthropic_query_default_bang(args, admitted) {
  return create_anthropic_query_bang(args, admitted, default_anthropic_runtime());
}

const createAnthropicQuery = function(...$beagle$args) {
  if (arguments.length === 2) {
    const args = $beagle$args[0];
    const admitted = $beagle$args[1];
    return create_anthropic_query_default_bang(args, admitted);
  }
  if (arguments.length === 3) {
    const args = $beagle$args[0];
    const admitted = $beagle$args[1];
    const runtime = $beagle$args[2];
    return create_anthropic_query_bang(args, admitted, runtime);
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
};

function provider_probe(target) {
  return probeAnthropic(target);
}

function provider_admit_bang(admission) {
  return admit_anthropic_bang(admission.options, admission.target);
}

function provider_query_bang(args) {
  return create_anthropic_query_default_bang(args, consumeExecutionAdmission("anthropic", args.options));
}

const canonical_anthropic_provider = $$bh$host_object($$bc$keyword("id"), "anthropic", $$bc$keyword("liveInput"), "streaming", $$bc$keyword("probe"), provider_probe, $$bc$keyword("admit"), provider_admit_bang, $$bc$keyword("query"), provider_query_bang);

const anthropicProvider = Object.freeze(canonical_anthropic_provider);

export { admitAnthropic as "admitAnthropic" };
export { anthropicProvider as "anthropicProvider" };
export { createAnthropicQuery as "createAnthropicQuery" };
export { disposeAnthropicSdkQuery as "disposeAnthropicSdkQuery" };
