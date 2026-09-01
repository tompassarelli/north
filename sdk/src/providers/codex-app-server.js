import { assoc_value as $$bc$assoc_value, conj_value as $$bc$conj_value, containsV as $$bc$contains, count as $$bc$count, eager_seq as $$bc$eager_seq, first as $$bc$first, get as $$bc$get, interpose as $$bc$interpose, into_value as $$bc$into_value, property_key as $$bc$property_key, record_value as $$bc$record_value, rest as $$bc$rest, str as $$bc$str } from '../bridge/generated/beagle/core.js';
import { clj_to_js as $$bh$clj_to_js, js_obj as $$bh$js_obj, js_to_clj as $$bh$js_to_clj } from '../bridge/generated/beagle/host.js';
import { catch_dispatch as $$bd$catch_dispatch, default_catch as $$bd$default_catch } from '../bridge/generated/beagle/exception-dispatch.js';
import { ExceptionInfo as $$be$ExceptionInfo } from '../bridge/generated/beagle/exception-info.js';

function jsonStringify(value) {
  return JSON.stringify($$bh$clj_to_js(value));
}

function externalized_spawn(spawn_process, command, arguments$, options) {
  return spawn_process(command, arguments$, $$bh$clj_to_js(options));
}

function hostObjectKeys(value) {
  return Object.keys($$bh$clj_to_js(value));
}

function hostObjectEntries(value) {
  return Object.entries($$bh$clj_to_js(value));
}

function decodeJsonValue(value) {
  return $$bh$js_to_clj(value);
}

function providerGet(collection, key) {
  return (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? ((typeof collection === "object") && Object.hasOwn(collection, key)) : _logical))(collection)) ? Reflect.get(collection, key) : $$bc$get(collection, key));
}

function providerContains_p(collection, key) {
  return (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? ((typeof collection === "object") && Object.hasOwn(collection, key)) : _logical))(collection)) ? true : $$bc$contains(collection, key));
}

function foreign_string_property(object, key) {
  const value = Reflect.get(object, key);
  return ((typeof value === "string") ? value : null);
}

function TypeScriptStdoutV1(isTTY) {
  return $$bc$record_value("north.providers.codex-app-server/TypeScriptStdoutV1", {_tag: "TypeScriptStdoutV1", isTTY});
}

function typescriptstdoutv1_isTTY(r) { return r.isTTY; }

function TypeScriptProcessV1(stdout, env, pid, platform, execPath, getBuiltinModule) {
  return $$bc$record_value("north.providers.codex-app-server/TypeScriptProcessV1", {_tag: "TypeScriptProcessV1", stdout, env, pid, platform, execPath, getBuiltinModule});
}

function typescriptprocessv1_stdout(r) { return r.stdout; }

function typescriptprocessv1_env(r) { return r.env; }

function typescriptprocessv1_pid(r) { return r.pid; }

function typescriptprocessv1_platform(r) { return r.platform; }

function typescriptprocessv1_execPath(r) { return r.execPath; }

function typescriptprocessv1_getBuiltinModule(r) { return r.getBuiltinModule; }

const child_process_module = process.getBuiltinModule("node:child_process");

const spawn = child_process_module.spawn;

const crypto_module = process.getBuiltinModule("node:crypto");

const createHash = crypto_module.createHash;

const fs_module = process.getBuiltinModule("node:fs");

const accessSync = fs_module.accessSync;

const closeSync = fs_module.closeSync;

const constants = fs_module.constants;

const FS__X__OK = constants.X_OK;

const FS__O__CREAT = constants.O_CREAT;

const FS__O__EXCL = constants.O_EXCL;

const FS__O__WRONLY = constants.O_WRONLY;

const FS__O__NOFOLLOW = constants.O_NOFOLLOW;

const fsyncSync = fs_module.fsyncSync;

const lstatSync = fs_module.lstatSync;

const mkdtempSync = fs_module.mkdtempSync;

const openSync = fs_module.openSync;

const realpathSync = fs_module.realpathSync;

const renameSync = fs_module.renameSync;

const rmSync = fs_module.rmSync;

const statSync = fs_module.statSync;

const unlinkSync = fs_module.unlinkSync;

const writeFileSync = fs_module.writeFileSync;

const os_module = process.getBuiltinModule("node:os");

const homedir = os_module.homedir;

const tmpdir = os_module.tmpdir;

const path_module = process.getBuiltinModule("node:path");

const delimiter = path_module.delimiter;

const dirname = path_module.dirname;

const join = path_module.join;

const resolve = path_module.resolve;

const writeStringToFdSync = writeFileSync;

const syncFdSync = fsyncSync;

const closeFdSync = closeSync;

function SupervisorRemovalOptions(recursive, force) {
  return $$bc$record_value("north.providers.codex-app-server/SupervisorRemovalOptions", {_tag: "SupervisorRemovalOptions", recursive, force});
}

function supervisorremovaloptions_recursive(r) { return r.recursive; }

function supervisorremovaloptions_force(r) { return r.force; }

const removeTreeSync = rmSync;

const accounts_module = require("../accounts");

const codexConfigArguments = accounts_module.codexConfigArguments;

const admission_module = require("../execution-admission");

const managedNorthMcpEnvironment = admission_module.managedNorthMcpEnvironment;

const invocation_module = require("../invocation-observation");

const invocationObservationKey = invocation_module.invocationObservationKey;

const parseInvocationObservationReceipt = invocation_module.parseInvocationObservationReceipt;

const native_command_module = require("../native-command-activity");

const NORTH__BINARY__PROBE__SCRIPT = native_command_module.NORTH_BINARY_PROBE_SCRIPT;

const NativeCommandActivityAccumulator = native_command_module.NativeCommandActivityAccumulator;

const unknownNativeCommandActivity = native_command_module.unknownNativeCommandActivity;

const strict_json_module = require("../strict-json");

const StrictJsonlMessages = strict_json_module.StrictJsonlMessages;

const parseStrictJson = strict_json_module.parseStrictJson;

function strictJsonLimits(maxBytes, maxDepth, maxNodes) {
  return $$bh$clj_to_js(Object.assign({}, {}, {[$$bc$property_key("maxBytes")]: maxBytes}, ((maxDepth == null) ? {} : {[$$bc$property_key("maxDepth")]: maxDepth}), ((maxNodes == null) ? {} : {[$$bc$property_key("maxNodes")]: maxNodes})));
}

function strictJsonlLimits(label, maxLineBytes, maxTotalBytes, maxMessages) {
  return $$bh$clj_to_js({[$$bc$property_key("label")]: label, [$$bc$property_key("maxLineBytes")]: maxLineBytes, [$$bc$property_key("maxTotalBytes")]: maxTotalBytes, [$$bc$property_key("maxMessages")]: maxMessages});
}

const trusted_runtime_module = require("../trusted-runtime");

const trustedGitMetadataRoots = trusted_runtime_module.trustedGitMetadataRoots;

const trustedGitProjectRoot = trusted_runtime_module.trustedGitProjectRoot;

const trustedManagedCodexExecutable = trusted_runtime_module.trustedManagedCodexExecutable;

const tool_activity_module = require("../tool-activity");

const McpActivityAccumulator = tool_activity_module.McpActivityAccumulator;

const mcpReceiptMetadata = tool_activity_module.mcpReceiptMetadata;

const normalizeCodexMcpIdentity = tool_activity_module.normalizeCodexMcpIdentity;

const hooks_module = require("./codex-managed-hooks");

const assertInstalledManagedCodexHooks = hooks_module.assertInstalledManagedCodexHooks;

const expectedManagedCodexHooks = hooks_module.expectedManagedCodexHooks;

const network_module = require("./codex-network-policy");

const managedCodexNetworkArguments = network_module.managedCodexNetworkArguments;

const managedCodexNetworkPolicy = network_module.managedCodexNetworkPolicy;

const supervisor_protocol_module = require("./codex-supervisor-protocol");

const CODEX__SUPERVISOR__STATUS__PREFIX = supervisor_protocol_module.CODEX_SUPERVISOR_STATUS_PREFIX;

const CODEX__SUPERVISOR__STDERR__FLAG = supervisor_protocol_module.CODEX_SUPERVISOR_STDERR_FLAG;

const codexSupervisorStderrLine = supervisor_protocol_module.codexSupervisorStderrLine;

const stderr_tail_module = require("./codex-stderr-tail");

const ProviderStderrRing = stderr_tail_module.ProviderStderrRing;

const STDERR__TAIL__LINES = stderr_tail_module.STDERR_TAIL_LINES;

const formatProviderStderrTail = stderr_tail_module.formatProviderStderrTail;

const openai_wire_module = require("./openai-wire");

const openAIWireCountsAsToolItem = openai_wire_module.openAIWireCountsAsToolItem;

const openAIWireItemIsPassive = openai_wire_module.openAIWireItemIsPassive;

const openAIWireToolIdentity = openai_wire_module.openAIWireToolIdentity;

const provider_join_module = require("./provider-join");

const providerJoinEvidence = provider_join_module.providerJoinEvidence;

const defineAnyProperty = Object.defineProperty;

// TypeScriptStringLiteralV1 : scalar

// TypeScriptStringLiteralV2 : scalar

// TypeScriptStringLiteralV3 : scalar

function TypeScriptAnonymousObjectV1(cached__input__tokens, input__tokens, output__tokens, reasoning__output__tokens) {
  return $$bc$record_value("north.providers.codex-app-server/TypeScriptAnonymousObjectV1", {_tag: "TypeScriptAnonymousObjectV1", cached_input_tokens: cached__input__tokens, input_tokens: input__tokens, output_tokens: output__tokens, reasoning_output_tokens: reasoning__output__tokens});
}

function typescriptanonymousobjectv1_cached__input__tokens(r) { return r.cached_input_tokens; }

function typescriptanonymousobjectv1_input__tokens(r) { return r.input_tokens; }

function typescriptanonymousobjectv1_output__tokens(r) { return r.output_tokens; }

function typescriptanonymousobjectv1_reasoning__output__tokens(r) { return r.reasoning_output_tokens; }

function TypeScriptAnonymousObjectV2(ageMs, id, kind) {
  return $$bc$record_value("north.providers.codex-app-server/TypeScriptAnonymousObjectV2", {_tag: "TypeScriptAnonymousObjectV2", ageMs, id, kind});
}

function typescriptanonymousobjectv2_ageMs(r) { return r.ageMs; }

function typescriptanonymousobjectv2_id(r) { return r.id; }

function typescriptanonymousobjectv2_kind(r) { return r.kind; }

function TypeScriptAnonymousObjectV3(cause) {
  return $$bc$record_value("north.providers.codex-app-server/TypeScriptAnonymousObjectV3", {_tag: "TypeScriptAnonymousObjectV3", cause});
}

function typescriptanonymousobjectv3_cause(r) { return r.cause; }

function TypeScriptStructuralObjectV1(configurable, enumerable, get, set, value, writable) {
  return $$bc$record_value("north.providers.codex-app-server/TypeScriptStructuralObjectV1", {_tag: "TypeScriptStructuralObjectV1", configurable, enumerable, get, set, value, writable});
}

function typescriptstructuralobjectv1_configurable(r) { return r.configurable; }

function typescriptstructuralobjectv1_enumerable(r) { return r.enumerable; }

function typescriptstructuralobjectv1_get(r) { return r.get; }

function typescriptstructuralobjectv1_set(r) { return r.set; }

function typescriptstructuralobjectv1_value(r) { return r.value; }

function typescriptstructuralobjectv1_writable(r) { return r.writable; }

function TypeScriptStructuralObjectV2(cause) {
  return $$bc$record_value("north.providers.codex-app-server/TypeScriptStructuralObjectV2", {_tag: "TypeScriptStructuralObjectV2", cause});
}

function typescriptstructuralobjectv2_cause(r) { return r.cause; }

// TypeScriptStringLiteralV5 : scalar

function TypeScriptAnonymousObjectV4(domains, networkAccess, networkProxyEnabled) {
  return $$bc$record_value("north.providers.codex-app-server/TypeScriptAnonymousObjectV4", {_tag: "TypeScriptAnonymousObjectV4", domains, networkAccess, networkProxyEnabled});
}

function typescriptanonymousobjectv4_domains(r) { return r.domains; }

function typescriptanonymousobjectv4_networkAccess(r) { return r.networkAccess; }

function typescriptanonymousobjectv4_networkProxyEnabled(r) { return r.networkProxyEnabled; }

function TypeScriptAnonymousObjectV5(error, processDeath) {
  return $$bc$record_value("north.providers.codex-app-server/TypeScriptAnonymousObjectV5", {_tag: "TypeScriptAnonymousObjectV5", error, processDeath});
}

function typescriptanonymousobjectv5_error(r) { return r.error; }

function typescriptanonymousobjectv5_processDeath(r) { return r.processDeath; }

function TypeScriptAnonymousObjectV6(promise, reject, resolve) {
  return $$bc$record_value("north.providers.codex-app-server/TypeScriptAnonymousObjectV6", {_tag: "TypeScriptAnonymousObjectV6", promise, reject, resolve});
}

function typescriptanonymousobjectv6_promise(r) { return r.promise; }

function typescriptanonymousobjectv6_reject(r) { return r.reject; }

function typescriptanonymousobjectv6_resolve(r) { return r.resolve; }

function TypeScriptStructuralObjectV3(disabledReason, name, version, config) {
  return $$bc$record_value("north.providers.codex-app-server/TypeScriptStructuralObjectV3", {_tag: "TypeScriptStructuralObjectV3", disabledReason, name, version, config});
}

function typescriptstructuralobjectv3_disabledReason(r) { return r.disabledReason; }

function typescriptstructuralobjectv3_name(r) { return r.name; }

function typescriptstructuralobjectv3_version(r) { return r.version; }

function typescriptstructuralobjectv3_config(r) { return r.config; }

function EffectiveMcpConfig(environment__id, tool__timeout__sec) {
  return $$bc$record_value("north.providers.codex-app-server/EffectiveMcpConfig", {_tag: "EffectiveMcpConfig", environment_id: environment__id, tool_timeout_sec: tool__timeout__sec});
}

function effectivemcpconfig_environment__id(r) { return r.environment_id; }

function effectivemcpconfig_tool__timeout__sec(r) { return r.tool_timeout_sec; }

function ManagedHookRow(eventName, handlerType, matcher, command, timeoutSec, sourcePath, source, enabled, isManaged, trustStatus) {
  return $$bc$record_value("north.providers.codex-app-server/ManagedHookRow", {_tag: "ManagedHookRow", eventName, handlerType, matcher, command, timeoutSec, sourcePath, source, enabled, isManaged, trustStatus});
}

function managedhookrow_eventName(r) { return r.eventName; }

function managedhookrow_handlerType(r) { return r.handlerType; }

function managedhookrow_matcher(r) { return r.matcher; }

function managedhookrow_command(r) { return r.command; }

function managedhookrow_timeoutSec(r) { return r.timeoutSec; }

function managedhookrow_sourcePath(r) { return r.sourcePath; }

function managedhookrow_source(r) { return r.source; }

function managedhookrow_enabled(r) { return r.enabled; }

function managedhookrow_isManaged(r) { return r.isManaged; }

function managedhookrow_trustStatus(r) { return r.trustStatus; }

function TypeScriptAnonymousObjectV9(kind, name) {
  return $$bc$record_value("north.providers.codex-app-server/TypeScriptAnonymousObjectV9", {_tag: "TypeScriptAnonymousObjectV9", kind, name});
}

function typescriptanonymousobjectv9_kind(r) { return r.kind; }

function typescriptanonymousobjectv9_name(r) { return r.name; }

function TypeScriptAnonymousObjectV8(kind, observedAtMs, pending, startedAtMs) {
  return $$bc$record_value("north.providers.codex-app-server/TypeScriptAnonymousObjectV8", {_tag: "TypeScriptAnonymousObjectV8", kind, observedAtMs, pending, startedAtMs});
}

function typescriptanonymousobjectv8_kind(r) { return r.kind; }

function typescriptanonymousobjectv8_observedAtMs(r) { return r.observedAtMs; }

function typescriptanonymousobjectv8_pending(r) { return r.pending; }

function typescriptanonymousobjectv8_startedAtMs(r) { return r.startedAtMs; }

function TypeScriptAnonymousObjectV10(pendingItemCount, pendingItems) {
  return $$bc$record_value("north.providers.codex-app-server/TypeScriptAnonymousObjectV10", {_tag: "TypeScriptAnonymousObjectV10", pendingItemCount, pendingItems});
}

function typescriptanonymousobjectv10_pendingItemCount(r) { return r.pendingItemCount; }

function typescriptanonymousobjectv10_pendingItems(r) { return r.pendingItems; }

// TypeScriptStringLiteralV6 : scalar

// TypeScriptStringLiteralV7 : scalar

// TypeScriptStringLiteralV8 : scalar

// TypeScriptStringLiteralV9 : scalar

// TypeScriptStringLiteralV11 : scalar

function TypeScriptAnonymousObjectV11(promise, reject, resolve) {
  return $$bc$record_value("north.providers.codex-app-server/TypeScriptAnonymousObjectV11", {_tag: "TypeScriptAnonymousObjectV11", promise, reject, resolve});
}

function typescriptanonymousobjectv11_promise(r) { return r.promise; }

function typescriptanonymousobjectv11_reject(r) { return r.reject; }

function typescriptanonymousobjectv11_resolve(r) { return r.resolve; }

function TypeScriptAnonymousObjectV12(promise, reject, resolve) {
  return $$bc$record_value("north.providers.codex-app-server/TypeScriptAnonymousObjectV12", {_tag: "TypeScriptAnonymousObjectV12", promise, reject, resolve});
}

function typescriptanonymousobjectv12_promise(r) { return r.promise; }

function typescriptanonymousobjectv12_reject(r) { return r.reject; }

function typescriptanonymousobjectv12_resolve(r) { return r.resolve; }

function TypeScriptAnonymousObjectV13(promise, reject, resolve) {
  return $$bc$record_value("north.providers.codex-app-server/TypeScriptAnonymousObjectV13", {_tag: "TypeScriptAnonymousObjectV13", promise, reject, resolve});
}

function typescriptanonymousobjectv13_promise(r) { return r.promise; }

function typescriptanonymousobjectv13_reject(r) { return r.reject; }

function typescriptanonymousobjectv13_resolve(r) { return r.resolve; }

function TypeScriptAnonymousObjectV14(alive, exitCode, exitSignal) {
  return $$bc$record_value("north.providers.codex-app-server/TypeScriptAnonymousObjectV14", {_tag: "TypeScriptAnonymousObjectV14", alive, exitCode, exitSignal});
}

function typescriptanonymousobjectv14_alive(r) { return r.alive; }

function typescriptanonymousobjectv14_exitCode(r) { return r.exitCode; }

function typescriptanonymousobjectv14_exitSignal(r) { return r.exitSignal; }

function TypeScriptAnonymousObjectV15(diagnostics, reason) {
  return $$bc$record_value("north.providers.codex-app-server/TypeScriptAnonymousObjectV15", {_tag: "TypeScriptAnonymousObjectV15", diagnostics, reason});
}

function typescriptanonymousobjectv15_diagnostics(r) { return r.diagnostics; }

function typescriptanonymousobjectv15_reason(r) { return r.reason; }

function TypeScriptAnonymousObjectV18(pendingItemCount, pendingItems) {
  return $$bc$record_value("north.providers.codex-app-server/TypeScriptAnonymousObjectV18", {_tag: "TypeScriptAnonymousObjectV18", pendingItemCount, pendingItems});
}

function typescriptanonymousobjectv18_pendingItemCount(r) { return r.pendingItemCount; }

function typescriptanonymousobjectv18_pendingItems(r) { return r.pendingItems; }

function TypeScriptAnonymousObjectV22(promise, reject, resolve) {
  return $$bc$record_value("north.providers.codex-app-server/TypeScriptAnonymousObjectV22", {_tag: "TypeScriptAnonymousObjectV22", promise, reject, resolve});
}

function typescriptanonymousobjectv22_promise(r) { return r.promise; }

function typescriptanonymousobjectv22_reject(r) { return r.reject; }

function typescriptanonymousobjectv22_resolve(r) { return r.resolve; }

function TypeScriptAnonymousObjectV24(exitCode) {
  return $$bc$record_value("north.providers.codex-app-server/TypeScriptAnonymousObjectV24", {_tag: "TypeScriptAnonymousObjectV24", exitCode});
}

function typescriptanonymousobjectv24_exitCode(r) { return r.exitCode; }

function TypeScriptAnonymousObjectV25(exitSignal) {
  return $$bc$record_value("north.providers.codex-app-server/TypeScriptAnonymousObjectV25", {_tag: "TypeScriptAnonymousObjectV25", exitSignal});
}

function typescriptanonymousobjectv25_exitSignal(r) { return r.exitSignal; }

function TypeScriptAnonymousObjectV28(threadId) {
  return $$bc$record_value("north.providers.codex-app-server/TypeScriptAnonymousObjectV28", {_tag: "TypeScriptAnonymousObjectV28", threadId});
}

function typescriptanonymousobjectv28_threadId(r) { return r.threadId; }

function TypeScriptAnonymousObjectV29(stderrTail) {
  return $$bc$record_value("north.providers.codex-app-server/TypeScriptAnonymousObjectV29", {_tag: "TypeScriptAnonymousObjectV29", stderrTail});
}

function typescriptanonymousobjectv29_stderrTail(r) { return r.stderrTail; }

function TypeScriptAnonymousObjectV30(method, value) {
  return $$bc$record_value("north.providers.codex-app-server/TypeScriptAnonymousObjectV30", {_tag: "TypeScriptAnonymousObjectV30", method, value});
}

function typescriptanonymousobjectv30_method(r) { return r.method; }

function typescriptanonymousobjectv30_value(r) { return r.value; }

function TypeScriptAnonymousObjectV31(id, kind, observedAtMs) {
  return $$bc$record_value("north.providers.codex-app-server/TypeScriptAnonymousObjectV31", {_tag: "TypeScriptAnonymousObjectV31", id, kind, observedAtMs});
}

function typescriptanonymousobjectv31_id(r) { return r.id; }

function typescriptanonymousobjectv31_kind(r) { return r.kind; }

function typescriptanonymousobjectv31_observedAtMs(r) { return r.observedAtMs; }

function TypeScriptAnonymousObjectV33(answers) {
  return $$bc$record_value("north.providers.codex-app-server/TypeScriptAnonymousObjectV33", {_tag: "TypeScriptAnonymousObjectV33", answers});
}

function typescriptanonymousobjectv33_answers(r) { return r.answers; }

function TypeScriptAnonymousObjectV32(answers) {
  return $$bc$record_value("north.providers.codex-app-server/TypeScriptAnonymousObjectV32", {_tag: "TypeScriptAnonymousObjectV32", answers});
}

function typescriptanonymousobjectv32_answers(r) { return r.answers; }

function TypeScriptAnonymousObjectV34(toolItems) {
  return $$bc$record_value("north.providers.codex-app-server/TypeScriptAnonymousObjectV34", {_tag: "TypeScriptAnonymousObjectV34", toolItems});
}

function typescriptanonymousobjectv34_toolItems(r) { return r.toolItems; }

function TypeScriptAnonymousObjectV35(invocationObservations) {
  return $$bc$record_value("north.providers.codex-app-server/TypeScriptAnonymousObjectV35", {_tag: "TypeScriptAnonymousObjectV35", invocationObservations});
}

function typescriptanonymousobjectv35_invocationObservations(r) { return r.invocationObservations; }

function TypeScriptAnonymousObjectV36(interrupt) {
  return $$bc$record_value("north.providers.codex-app-server/TypeScriptAnonymousObjectV36", {_tag: "TypeScriptAnonymousObjectV36", interrupt});
}

function typescriptanonymousobjectv36_interrupt(r) { return r.interrupt; }

function TypeScriptAnonymousObjectV37(providerAlive) {
  return $$bc$record_value("north.providers.codex-app-server/TypeScriptAnonymousObjectV37", {_tag: "TypeScriptAnonymousObjectV37", providerAlive});
}

function typescriptanonymousobjectv37_providerAlive(r) { return r.providerAlive; }

function TypeScriptStructuralObjectV11(completedTurns, exitCode, exitSignal, interrupt, invocationObservations, pendingItemCount, pendingItems, respawnCount, respawns, stderrTail, text, threadId, toolItems, turnIds, unsupportedNotifications, usage) {
  return $$bc$record_value("north.providers.codex-app-server/TypeScriptStructuralObjectV11", {_tag: "TypeScriptStructuralObjectV11", completedTurns, exitCode, exitSignal, interrupt, invocationObservations, pendingItemCount, pendingItems, respawnCount, respawns, stderrTail, text, threadId, toolItems, turnIds, unsupportedNotifications, usage});
}

function typescriptstructuralobjectv11_completedTurns(r) { return r.completedTurns; }

function typescriptstructuralobjectv11_exitCode(r) { return r.exitCode; }

function typescriptstructuralobjectv11_exitSignal(r) { return r.exitSignal; }

function typescriptstructuralobjectv11_interrupt(r) { return r.interrupt; }

function typescriptstructuralobjectv11_invocationObservations(r) { return r.invocationObservations; }

function typescriptstructuralobjectv11_pendingItemCount(r) { return r.pendingItemCount; }

function typescriptstructuralobjectv11_pendingItems(r) { return r.pendingItems; }

function typescriptstructuralobjectv11_respawnCount(r) { return r.respawnCount; }

function typescriptstructuralobjectv11_respawns(r) { return r.respawns; }

function typescriptstructuralobjectv11_stderrTail(r) { return r.stderrTail; }

function typescriptstructuralobjectv11_text(r) { return r.text; }

function typescriptstructuralobjectv11_threadId(r) { return r.threadId; }

function typescriptstructuralobjectv11_toolItems(r) { return r.toolItems; }

function typescriptstructuralobjectv11_turnIds(r) { return r.turnIds; }

function typescriptstructuralobjectv11_unsupportedNotifications(r) { return r.unsupportedNotifications; }

function typescriptstructuralobjectv11_usage(r) { return r.usage; }

const SUPERVISOR = resolve(import.meta.dir, "codex-supervisor.ts");

const ENGINE = resolve(import.meta.dir, "../../../bin/north");

const RPC__TIMEOUT__MS = 20000;

const MAX__LINE__BYTES = ((8 * 1024) * 1024);

const MAX__TOTAL__BYTES = ((128 * 1024) * 1024);

const MAX__MESSAGES = 20000;

const MAX__PENDING__RPC__MESSAGES = 256;

const MAX__INVENTORY__PAGES = 32;

const MAX__MCP__SERVERS = 64;

const MAX__ID__BYTES = 512;

const MAX__CWD__BYTES = 4096;

const MAX__QUEUED__NOTIFICATIONS = 256;

const MAX__UNSUPPORTED__NOTIFICATION__METHODS = 16;

const MAX__UNSUPPORTED__NOTIFICATIONS__PER__METHOD = 512;

const MAX__DISABLED__PROJECT__CONFIG__BYTES = (64 * 1024);

const MAX__DISABLED__PROJECT__CONFIG__DEPTH = 16;

const MAX__DISABLED__PROJECT__CONFIG__NODES = 2048;

const MAX__SAFETY__BUFFERING__VALUES = 64;

const MAX__SAFETY__BUFFERING__VALUE__BYTES = 4096;

const MANAGED__CODEX__VERSION = "0.146.0";

const SUPERVISOR__STATUS__MAX__LINE__BYTES = 2048;

const SUPERVISOR__STATUS__MAX__MESSAGES = 4096;

const SUPERVISOR__STATUS__MAX__TOTAL__BYTES = ((4 * 1024) * 1024);

const TURN__DEADLINE__MS = 600000;

const TURN__DEADLINE__INACTIVITY__MS = (5 * 60000);

const IN__FLIGHT__ITEM__CEILING__MS = (45 * 60000);

const POST__TOOL__QUIET__MS = (5 * 60000);

const TURN__INTERRUPT__MS = 5000;

const REPLACEMENT__TURN__INTERRUPT__UNAVAILABLE = "managed_provider_replacement_turn_unavailable";

const PROVIDER__HAS__NO__ACTIVE__TURN = "provider_has_no_active_turn";

const PROVIDER__TURN__INTERRUPT__FAILED = "provider_turn_interrupt_failed";

const MAX__RESPAWNS = 2;

const MAX__RECOVERED__TEXT__BYTES = (8 * 1024);

const MAX__RECOVERED__CONTEXT__BYTES = (96 * 1024);

const MAX__PENDING__ITEM__SUMMARIES = 16;

const SUPERVISOR__MESSAGE__PREFIX = "NORTH_CODEX_RPC 1 ";

const CODEX__SHELL__PREFLIGHT__TIMEOUT__MS = 5000;

const CODEX__SHELL__PREFLIGHT__OUTPUT__BYTES = 4096;

const CODEX__SHELL__PREFLIGHT__COMMAND = Object.freeze(["bash", "--noprofile", "--norc", "-c", NORTH__BINARY__PROBE__SCRIPT]);

const MANAGED__CODEX__ENABLED__FEATURES = (() => { const tuple_value_1 = ["hooks", "shell_tool", "unified_exec"]; return tuple_value_1; })();

const MANAGED__CODEX__DISABLED__FEATURES = (() => { const tuple_value_2 = ["apply_patch_freeform", "apps", "apply_patch_streaming_events", "artifact", "auth_elicitation", "background_paginated_rollout_migration", "browser_use", "browser_use_external", "browser_use_full_cdp_access", "chronicle", "code_mode", "code_mode_buffered_exec", "code_mode_host", "code_mode_only", "computer_use", "concurrent_reasoning_summaries", "current_time_reminder", "default_mode_request_user_input", "deferred_executor", "deferred_tool_world_state", "enable_request_compression", "enable_fanout", "enable_mcp_apps", "exec_permission_approvals", "executor_capability_discovery", "external_agent_memory_import", "fast_mode", "goals", "guardian_approval", "guardianv2", "image_generation", "in_app_browser", "in_app_updates", "item_ids", "local_thread_store_compression", "mcp_2026_07_28", "memories", "mentions_v2", "multi_agent", "multi_agent_v2", "non_prefixed_mcp_tool_names", "personality", "plugin_sharing", "plugins", "prevent_idle_sleep", "realtime_conversation", "remote_compaction_v2", "remote_plugin", "request_permissions_tool", "respect_system_proxy", "rollout_budget", "runtime_metrics", "secret_auth_storage", "shell_snapshot", "shell_zsh_fork", "skill_mcp_dependency_install", "skill_search", "standalone_web_search", "terminal_visualization_instructions", "token_budget", "tool_call_mcp_elicitation", "tool_suggest", "unified_exec_zsh_fork", "use_agent_identity", "use_legacy_landlock", "web_search_cached", "web_search_request", "workspace_dependencies"]; return tuple_value_2; })();

const REVIEWED__DISABLED__PROJECT__CONFIG__KEYS = (() => { const tuple_value_3 = ["agents", "approval_policy", "approvals_reviewer", "default_permissions", "exec_policy", "features", "hooks", "mcp_servers", "model", "model_reasoning_effort", "notice", "project_doc_fallback_filenames", "projects", "sandbox_mode", "tui"]; return tuple_value_3; })();

function PendingReplacementTurnInterrupt(settlement, dispatched) {
  return $$bc$record_value("north.providers.codex-app-server/PendingReplacementTurnInterrupt", {_tag: "PendingReplacementTurnInterrupt", settlement, dispatched});
}

function pendingreplacementturninterrupt_settlement(r) { return r.settlement; }

function pendingreplacementturninterrupt_dispatched(r) { return r.dispatched; }

function ManagedCodexNorthServer(command, args, env) {
  return $$bc$record_value("north.providers.codex-app-server/ManagedCodexNorthServer", {_tag: "ManagedCodexNorthServer", command, args, env});
}

function managedcodexnorthserver_command(r) { return r.command; }

function managedcodexnorthserver_args(r) { return r.args; }

function managedcodexnorthserver_env(r) { return r.env; }

function ManagedCodexAppServerOptions(command, commandPrefix, useSupervisor, spawnProcess, testExpectedExecutable, env, cwd, prompt, model, effort, developerInstructions, surface, north, timeoutMs, turnDeadlineMs, turnDeadlineInactivityMs, inFlightItemCeilingMs, postToolQuietMs, maxRespawns, onActivity, onEvent, onRespawn, beforeLaunch) {
  return $$bc$record_value("north.providers.codex-app-server/ManagedCodexAppServerOptions", {_tag: "ManagedCodexAppServerOptions", command, commandPrefix, useSupervisor, spawnProcess, testExpectedExecutable, env, cwd, prompt, model, effort, developerInstructions, surface, north, timeoutMs, turnDeadlineMs, turnDeadlineInactivityMs, inFlightItemCeilingMs, postToolQuietMs, maxRespawns, onActivity, onEvent, onRespawn, beforeLaunch});
}

function managedcodexappserveroptions_command(r) { return r.command; }

function managedcodexappserveroptions_commandPrefix(r) { return r.commandPrefix; }

function managedcodexappserveroptions_useSupervisor(r) { return r.useSupervisor; }

function managedcodexappserveroptions_spawnProcess(r) { return r.spawnProcess; }

function managedcodexappserveroptions_testExpectedExecutable(r) { return r.testExpectedExecutable; }

function managedcodexappserveroptions_env(r) { return r.env; }

function managedcodexappserveroptions_cwd(r) { return r.cwd; }

function managedcodexappserveroptions_prompt(r) { return r.prompt; }

function managedcodexappserveroptions_model(r) { return r.model; }

function managedcodexappserveroptions_effort(r) { return r.effort; }

function managedcodexappserveroptions_developerInstructions(r) { return r.developerInstructions; }

function managedcodexappserveroptions_surface(r) { return r.surface; }

function managedcodexappserveroptions_north(r) { return r.north; }

function managedcodexappserveroptions_timeoutMs(r) { return r.timeoutMs; }

function managedcodexappserveroptions_turnDeadlineMs(r) { return r.turnDeadlineMs; }

function managedcodexappserveroptions_turnDeadlineInactivityMs(r) { return r.turnDeadlineInactivityMs; }

function managedcodexappserveroptions_inFlightItemCeilingMs(r) { return r.inFlightItemCeilingMs; }

function managedcodexappserveroptions_postToolQuietMs(r) { return r.postToolQuietMs; }

function managedcodexappserveroptions_maxRespawns(r) { return r.maxRespawns; }

function managedcodexappserveroptions_onActivity(r) { return r.onActivity; }

function managedcodexappserveroptions_onEvent(r) { return r.onEvent; }

function managedcodexappserveroptions_onRespawn(r) { return r.onRespawn; }

function managedcodexappserveroptions_beforeLaunch(r) { return r.beforeLaunch; }

function reportManagedActivity_bang(options, activity) {
  const onActivity = managedcodexappserveroptions_onActivity(options);
  if (((_truthy) => _truthy !== false && _truthy != null)(onActivity)) {
    return onActivity(activity);
  }
}

function ManagedCodexResult(text, usage, providerDurationMs, providerJoin, toolItems, invocationObservations) {
  return $$bc$record_value("north.providers.codex-app-server/ManagedCodexResult", {_tag: "ManagedCodexResult", text, usage, providerDurationMs, providerJoin, toolItems, invocationObservations});
}

function managedcodexresult_text(r) { return r.text; }

function managedcodexresult_usage(r) { return r.usage; }

function managedcodexresult_providerDurationMs(r) { return r.providerDurationMs; }

function managedcodexresult_providerJoin(r) { return r.providerJoin; }

function managedcodexresult_toolItems(r) { return r.toolItems; }

function managedcodexresult_invocationObservations(r) { return r.invocationObservations; }

function ManagedCodexIteratorStepV1(done, value) {
  return $$bc$record_value("north.providers.codex-app-server/ManagedCodexIteratorStepV1", {_tag: "ManagedCodexIteratorStepV1", done, value});
}

function managedcodexiteratorstepv1_done(r) { return r.done; }

function managedcodexiteratorstepv1_value(r) { return r.value; }

function ManagedCodexInvocationObservation(count, schema, hook, operation, classification, decision) {
  return $$bc$record_value("north.providers.codex-app-server/ManagedCodexInvocationObservation", {_tag: "ManagedCodexInvocationObservation", count, schema, hook, operation, classification, decision});
}

function managedcodexinvocationobservation_count(r) { return r.count; }

function managedcodexinvocationobservation_schema(r) { return r.schema; }

function managedcodexinvocationobservation_hook(r) { return r.hook; }

function managedcodexinvocationobservation_operation(r) { return r.operation; }

function managedcodexinvocationobservation_classification(r) { return r.classification; }

function managedcodexinvocationobservation_decision(r) { return r.decision; }

function ManagedCodexDiagnostics(stderrTail, exitCode, exitSignal, providerAlive) {
  return $$bc$record_value("north.providers.codex-app-server/ManagedCodexDiagnostics", {_tag: "ManagedCodexDiagnostics", stderrTail, exitCode, exitSignal, providerAlive});
}

function managedcodexdiagnostics_stderrTail(r) { return r.stderrTail; }

function managedcodexdiagnostics_exitCode(r) { return r.exitCode; }

function managedcodexdiagnostics_exitSignal(r) { return r.exitSignal; }

function managedcodexdiagnostics_providerAlive(r) { return r.providerAlive; }

function ManagedCodexRespawnAttempt(attempt, reason, threadId, completedTurns, stderrTail, exitCode, exitSignal) {
  return $$bc$record_value("north.providers.codex-app-server/ManagedCodexRespawnAttempt", {_tag: "ManagedCodexRespawnAttempt", attempt, reason, threadId, completedTurns, stderrTail, exitCode, exitSignal});
}

function managedcodexrespawnattempt_attempt(r) { return r.attempt; }

function managedcodexrespawnattempt_reason(r) { return r.reason; }

function managedcodexrespawnattempt_threadId(r) { return r.threadId; }

function managedcodexrespawnattempt_completedTurns(r) { return r.completedTurns; }

function managedcodexrespawnattempt_stderrTail(r) { return r.stderrTail; }

function managedcodexrespawnattempt_exitCode(r) { return r.exitCode; }

function managedcodexrespawnattempt_exitSignal(r) { return r.exitSignal; }

function ManagedCodexRespawnRecord(respawnCount, completedTurns, respawns) {
  return $$bc$record_value("north.providers.codex-app-server/ManagedCodexRespawnRecord", {_tag: "ManagedCodexRespawnRecord", respawnCount, completedTurns, respawns});
}

function managedcodexrespawnrecord_respawnCount(r) { return r.respawnCount; }

function managedcodexrespawnrecord_completedTurns(r) { return r.completedTurns; }

function managedcodexrespawnrecord_respawns(r) { return r.respawns; }

function managed_codex_pre_thread_error_new(...$beagle$args) {
  if (arguments.length === 1) {
    const message = $beagle$args[0];
    return managed_codex_pre_thread_error_new(message, null);
  }
  if (arguments.length === 2) {
    const message = $beagle$args[0];
    const options = $beagle$args[1];
    const error = (((_truthy) => _truthy !== false && _truthy != null)(options) ? new Error(message, options) : new Error(message));
    Object.setPrototypeOf(error, managed_codex_pre_thread_error_new.prototype);
    Object.assign(error, $$bh$js_obj("name", "ManagedCodexPreThreadError"));
    return error;
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

Object.setPrototypeOf(managed_codex_pre_thread_error_new.prototype, Error.prototype);

function managed_codex_pre_thread_error_public_new(message, options) {
  return managed_codex_pre_thread_error_new(message, options);
}

(managed_codex_pre_thread_error_public_new.prototype = managed_codex_pre_thread_error_new.prototype);

const ManagedCodexPreThreadError = managed_codex_pre_thread_error_public_new;

function new_ManagedCodexPreThreadError(...$beagle$args) {
  if (arguments.length === 1) {
    const message = $beagle$args[0];
    return managed_codex_pre_thread_error_new(message);
  }
  if (arguments.length === 2) {
    const message = $beagle$args[0];
    const options = $beagle$args[1];
    return managed_codex_pre_thread_error_new(message, options);
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

function managedCodexPreThreadError_p(error) {
  return ((_logical) => (_logical !== false && _logical != null ? ((typeof error === "object") && (Object.getPrototypeOf(error) === managed_codex_pre_thread_error_new.prototype)) : _logical))(error);
}

function ManagedCodexHarvest(threadId, turnIds, completedTurns, text, toolItems, pendingItemCount, pendingItems, usage, invocationObservations, mcp, nativeCommands, unsupportedNotifications, landedWork, stderrTail, exitCode, exitSignal, respawnCount, respawns, interrupt) {
  return $$bc$record_value("north.providers.codex-app-server/ManagedCodexHarvest", {_tag: "ManagedCodexHarvest", threadId, turnIds, completedTurns, text, toolItems, pendingItemCount, pendingItems, usage, invocationObservations, mcp, nativeCommands, unsupportedNotifications, landedWork, stderrTail, exitCode, exitSignal, respawnCount, respawns, interrupt});
}

function managedcodexharvest_threadId(r) { return r.threadId; }

function managedcodexharvest_turnIds(r) { return r.turnIds; }

function managedcodexharvest_completedTurns(r) { return r.completedTurns; }

function managedcodexharvest_text(r) { return r.text; }

function managedcodexharvest_toolItems(r) { return r.toolItems; }

function managedcodexharvest_pendingItemCount(r) { return r.pendingItemCount; }

function managedcodexharvest_pendingItems(r) { return r.pendingItems; }

function managedcodexharvest_usage(r) { return r.usage; }

function managedcodexharvest_invocationObservations(r) { return r.invocationObservations; }

function managedcodexharvest_mcp(r) { return r.mcp; }

function managedcodexharvest_nativeCommands(r) { return r.nativeCommands; }

function managedcodexharvest_unsupportedNotifications(r) { return r.unsupportedNotifications; }

function managedcodexharvest_landedWork(r) { return r.landedWork; }

function managedcodexharvest_stderrTail(r) { return r.stderrTail; }

function managedcodexharvest_exitCode(r) { return r.exitCode; }

function managedcodexharvest_exitSignal(r) { return r.exitSignal; }

function managedcodexharvest_respawnCount(r) { return r.respawnCount; }

function managedcodexharvest_respawns(r) { return r.respawns; }

function managedcodexharvest_interrupt(r) { return r.interrupt; }

function ManagedCodexPendingItemSummary(kind, name, count) {
  return $$bc$record_value("north.providers.codex-app-server/ManagedCodexPendingItemSummary", {_tag: "ManagedCodexPendingItemSummary", kind, name, count});
}

function managedcodexpendingitemsummary_kind(r) { return r.kind; }

function managedcodexpendingitemsummary_name(r) { return r.name; }

function managedcodexpendingitemsummary_count(r) { return r.count; }

function ManagedCodexInterruptEvidence(reason, deadlineMs, inactivityThresholdMs, lastActivityAgeMs, openItemCount, openItem, eventCount, eventCounts) {
  return $$bc$record_value("north.providers.codex-app-server/ManagedCodexInterruptEvidence", {_tag: "ManagedCodexInterruptEvidence", reason, deadlineMs, inactivityThresholdMs, lastActivityAgeMs, openItemCount, openItem, eventCount, eventCounts});
}

function managedcodexinterruptevidence_reason(r) { return r.reason; }

function managedcodexinterruptevidence_deadlineMs(r) { return r.deadlineMs; }

function managedcodexinterruptevidence_inactivityThresholdMs(r) { return r.inactivityThresholdMs; }

function managedcodexinterruptevidence_lastActivityAgeMs(r) { return r.lastActivityAgeMs; }

function managedcodexinterruptevidence_openItemCount(r) { return r.openItemCount; }

function managedcodexinterruptevidence_openItem(r) { return r.openItem; }

function managedcodexinterruptevidence_eventCount(r) { return r.eventCount; }

function managedcodexinterruptevidence_eventCounts(r) { return r.eventCounts; }

function publicJsValue(value) {
  if (Array.isArray(value)) {
    return (value).map((entry) => publicJsValue(entry));
  } else {
    if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (typeof value === "object") : _logical))(value))) {
      const result = $$bh$js_obj();
      $$bc$eager_seq(Object.keys(value)).forEach((key) => {
  if ((!(key === "_tag"))) {
    Reflect.set(result, key, publicJsValue(providerGet(value, key)));
  }
});
      return result;
    } else {
      return value;
    }
  }
}

function externalManagedCodexHarvest(harvest) {
  const result = publicJsValue($$bh$clj_to_js(harvest));
  $$bc$eager_seq(["threadId", "toolItems", "pendingItemCount", "pendingItems", "usage", "invocationObservations", "stderrTail", "exitCode", "exitSignal", "respawnCount", "respawns", "interrupt"]).forEach((key) => {
  if ((providerGet(result, key) === null)) {
    Reflect.deleteProperty(result, key);
  }
});
  return result;
}

function managed_codex_harvest_error_new(...$beagle$args) {
  if (arguments.length === 1) {
    const harvest = $beagle$args[0];
    return managed_codex_harvest_error_new(harvest, null);
  }
  if (arguments.length === 2) {
    const harvest = $beagle$args[0];
    const options = $beagle$args[1];
    const error = (((_truthy) => _truthy !== false && _truthy != null)(options) ? new Error("openai_provider_execution_failed", options) : new Error("openai_provider_execution_failed"));
    Object.setPrototypeOf(error, managed_codex_harvest_error_new.prototype);
    Object.assign(error, $$bh$js_obj("name", "ManagedCodexHarvestError", "harvest", externalManagedCodexHarvest(harvest)));
    return error;
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

Object.setPrototypeOf(managed_codex_harvest_error_new.prototype, Error.prototype);

function managed_codex_harvest_error_public_new(harvest, options) {
  return managed_codex_harvest_error_new(harvest, options);
}

(managed_codex_harvest_error_public_new.prototype = managed_codex_harvest_error_new.prototype);

const ManagedCodexHarvestError = managed_codex_harvest_error_public_new;

function new_ManagedCodexHarvestError(...$beagle$args) {
  if (arguments.length === 1) {
    const harvest = $beagle$args[0];
    return managed_codex_harvest_error_new(harvest);
  }
  if (arguments.length === 2) {
    const harvest = $beagle$args[0];
    const options = $beagle$args[1];
    return managed_codex_harvest_error_new(harvest, options);
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

function managedCodexHarvestError_p(error) {
  return ((_logical) => (_logical !== false && _logical != null ? ((typeof error === "object") && (Object.getPrototypeOf(error) === managed_codex_harvest_error_new.prototype)) : _logical))(error);
}

const DIAGNOSTIC__CAUSE = Symbol.for("north.codex.diagnostics");

function attachDiagnostics_bang(error, diagnostics) {
  (error.diagnostics = diagnostics);
  if (managedCodexHarvestError_p(error)) {
    if ((!(managedcodexdiagnostics_stderrTail(diagnostics).length === 0))) {
      ((error).harvest.stderrTail = $$bc$into_value([], managedcodexdiagnostics_stderrTail(diagnostics)));
    }
    if ((!(managedcodexdiagnostics_exitCode(diagnostics) === null))) {
      ((error).harvest.exitCode = managedcodexdiagnostics_exitCode(diagnostics));
    }
    if ((!(managedcodexdiagnostics_exitSignal(diagnostics) === null))) {
      ((error).harvest.exitSignal = managedcodexdiagnostics_exitSignal(diagnostics));
    }
    null;
  }
  const exit = ((!(managedcodexdiagnostics_exitCode(diagnostics) === null)) ? $$bc$str("provider exit code ", managedcodexdiagnostics_exitCode(diagnostics), "") : ((!(managedcodexdiagnostics_exitSignal(diagnostics) === null)) ? $$bc$str("provider died on ", managedcodexdiagnostics_exitSignal(diagnostics), "") : ((managedcodexdiagnostics_providerAlive(diagnostics) === true) ? "provider still running" : null)));
  const tail = formatProviderStderrTail(managedcodexdiagnostics_stderrTail(diagnostics));
  const rendered = (($beagle$apply$fn, $beagle$apply$tail) => $beagle$apply$fn.call($beagle$apply$fn, ...($beagle$apply$tail ?? [])))(((..._xs) => "".concat(..._xs)), $$bc$interpose("\n", [exit, tail].filter((part) => (!(part === null)))));
  if ((rendered === "")) {
    return null;
  } else {
    let current = error;
    let depth = 0;
    const for_break_4 = ({value: false, watches: {}});
    (() => {  while (true) {
    if ((depth < 8)) { (() => { const for_continue_5 = ({value: false, watches: {}}); const cause = current.cause;
return ((!(cause instanceof Error)) ? (() => { const _a = for_break_4, _v = true; const _old = _a.value; _a.value = _v; for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _v); return _v; })() : (current = cause)); })(); if ((!for_break_4.value)) { (depth = (depth + 1));  continue; } else { return null; } } else { return null; }
  } })();
    if ((($beagle$jst$receiver, $beagle$jst$key) => ($beagle$jst$key in $beagle$jst$receiver))(current, DIAGNOSTIC__CAUSE)) {
      (current.message = rendered);
      return null;
    } else {
      if ((current.cause != null)) {
        return null;
      } else {
        const link = new Error(rendered);
        defineAnyProperty(link, DIAGNOSTIC__CAUSE, $$bh$clj_to_js({[$$bc$property_key("value")]: true}));
        return (current.cause = link);
      }
    }
  }
}

function record(value, label) {
  if ((((!((_truthy) => _truthy !== false && _truthy != null)(value)) || (!(typeof value === "object"))) || Array.isArray(value))) {
    (() => { throw new $$be$ExceptionInfo($$bc$str("", label, " must be an object"), {}); })();
  }
  return value;
}

function foreignRecord(value, label) {
  if ((((!((_truthy) => _truthy !== false && _truthy != null)(value)) || (!(typeof value === "object"))) || Array.isArray(value))) {
    (() => { throw new $$be$ExceptionInfo($$bc$str("", label, " must be an object"), {}); })();
  }
  return value;
}

function boundedString(...$beagle$args) {
  if (arguments.length === 2) {
    const value = $beagle$args[0];
    const label = $beagle$args[1];
    return boundedString(value, label, MAX__ID__BYTES);
  }
  if (arguments.length === 3) {
    const value = $beagle$args[0];
    const label = $beagle$args[1];
    const maxBytes = $beagle$args[2];
    if (((_truthy) => _truthy !== false && _truthy != null)((((((!(typeof value === "string")) || (value === "")) || (!(value === (value).trim()))) || (Buffer.byteLength(value, "utf8") > maxBytes)) || /[\u0000-\u001f\u007f]/.test(value)))) {
      (() => { throw new $$be$ExceptionInfo($$bc$str("", label, " must be a bounded canonical string"), {}); })();
    }
    return value;
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

function boundedProviderProse(value, label, maxBytes) {
  if (((_truthy) => _truthy !== false && _truthy != null)(((((!(typeof value === "string")) || (value === "")) || (Buffer.byteLength(value, "utf8") > maxBytes)) || /[\u0000\u000b\u000c\u000e-\u001f\u007f]/.test(value)))) {
    (() => { throw new $$be$ExceptionInfo($$bc$str("", label, " must be bounded provider prose"), {}); })();
  }
  return value;
}

function protocolId(value, label) {
  const id = boundedString(value, label);
  if ((!((_truthy) => _truthy !== false && _truthy != null)(/^[A-Za-z0-9._:-]+$/.test(id)))) {
    (() => { throw new $$be$ExceptionInfo($$bc$str("", label, " is invalid"), {}); })();
  }
  return id;
}

function providerThreadId_bang(envelope, thread, label) {
  let selected = null;
  const for_break_6 = ({value: false, watches: {}});
  $$bc$eager_seq((() => { const tuple_value_12 = [(() => { const tuple_value_8 = [providerGet(thread, "id"), "thread id"]; return tuple_value_8; })(), (() => { const tuple_value_9 = [providerGet(thread, "sessionId"), "thread session id"]; return tuple_value_9; })(), (() => { const tuple_value_10 = [providerGet(envelope, "sessionId"), "session id"]; return tuple_value_10; })(), (() => { const tuple_value_11 = [providerGet(envelope, "threadId"), "thread id"]; return tuple_value_11; })()]; return tuple_value_12; })()).forEach(($beagle$item) => {
  let value = $beagle$item[0];
  let source = $beagle$item[1];
  if ((!for_break_6.value)) {
    const for_continue_7 = ({value: false, watches: {}});
    if (((value === null) || (value === null))) {
      (() => { const _a = for_continue_7, _v = true; const _old = _a.value; _a.value = _v; for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _v); return _v; })();
    } else {
      const id = protocolId(value, $$bc$str("", label, " ", source, ""));
      if ((selected == null)) {
        (selected = id);
      } else {
        selected;
      }
    }
  }
});
  if ((!((_truthy) => _truthy !== false && _truthy != null)(selected))) {
    (() => { throw new $$be$ExceptionInfo($$bc$str("", label, " omitted its protocol id"), {}); })();
  }
  return selected;
}

function canonical(value) {
  const host = $$bh$clj_to_js(value);
  return (Array.isArray(host) ? (host).map((entry) => canonical(entry)) : (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (typeof host === "object") : _logical))(host)) ? Object.fromEntries(hostObjectKeys(host).sort().map((key) => { const tuple_value_13 = [key, canonical(Reflect.get(host, key))];
return tuple_value_13; })) : host));
}

function exact(value, expected, label) {
  if ((!(jsonStringify(canonical(value)) === jsonStringify(canonical(expected))))) {
    (() => { throw new $$be$ExceptionInfo($$bc$str("", label, " does not match North's exact managed Codex contract"), {}); })();
  }
  return null;
}

function exactDiagnosable(value, expected, label) {
  const observed = jsonStringify(canonical(value));
  const wanted = jsonStringify(canonical(expected));
  return ((observed === wanted) ? null : (() => { throw new Error($$bc$str("", label, " does not match North's exact managed Codex contract"), TypeScriptStructuralObjectV2(new Error($$bc$str("observed=", String(observed).slice(0, 600), " expected=", String(wanted).slice(0, 600), "")))); })());
}

function exactNamingKeys(value, expected, label) {
  const observed = canonical(value);
  const wanted = canonical(expected);
  if ((jsonStringify(observed) === jsonStringify(wanted))) {
    return null;
  } else {
    const asRecord = (input) => (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (!Array.isArray(input)) : _logical))(((_logical) => (_logical !== false && _logical != null ? (typeof input === "object") : _logical))(input))) ? input : null);
    const left = asRecord(observed);
    const right = asRecord(wanted);
    const drift = (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? right : _logical))(left)) ? $$bc$into_value([], new Set($$bc$into_value($$bc$into_value([], hostObjectKeys(left)), hostObjectKeys(right)))).sort().flatMap((key) => ((!(($beagle$jst$receiver, $beagle$jst$key) => ($beagle$jst$key in $beagle$jst$receiver))(right, key)) ? [$$bc$str("unexpected ", key, "")] : ((!(($beagle$jst$receiver, $beagle$jst$key) => ($beagle$jst$key in $beagle$jst$receiver))(left, key)) ? [$$bc$str("missing ", key, "")] : ((jsonStringify(Reflect.get(left, key)) === jsonStringify(Reflect.get(right, key))) ? [] : [$$bc$str("changed ", key, "")])))) : ["not the expected shape"]);
    return (() => { throw new Error($$bc$str("", label, " does not match North's exact managed Codex contract"), TypeScriptStructuralObjectV2(new Error($$bc$str("drifted: ", ((_logical) => (_logical !== false && _logical != null ? _logical : "ordering"))((($beagle$apply$fn, $beagle$apply$tail) => $beagle$apply$fn.call($beagle$apply$fn, ...($beagle$apply$tail ?? [])))(((..._xs) => "".concat(..._xs)), $$bc$interpose(", ", drift))), "")))); })();
  }
}

function mustBeEmptyLayer(value, label) {
  const present = hostObjectKeys(canonical(value)).sort();
  if ((!($$bc$count(present) === 0))) {
    (() => { throw new $$be$ExceptionInfo($$bc$str("", label, " must be empty but carries: ", (($beagle$apply$fn, $beagle$apply$tail) => $beagle$apply$fn.call($beagle$apply$fn, ...($beagle$apply$tail ?? [])))(((..._xs) => "".concat(..._xs)), $$bc$interpose(", ", present)), ""), {}); })();
  }
  return null;
}

function validateShellPreflight(response) {
  const result = record(response, "Codex command/exec response");
  onlyKeys(result, ["exitCode", "stdout", "stderr"], "Codex command/exec response");
  const expectedOutput = $$bc$str("", ENGINE, "\n", ENGINE, "\n");
  if (((((!Number.isSafeInteger(providerGet(result, "exitCode"))) || (!(providerGet(result, "exitCode") === 0))) || (!(providerGet(result, "stdout") === expectedOutput))) || (!(providerGet(result, "stderr") === "")))) {
    const seen = (value) => jsonStringify(String((() => { const coalesce_value_14 = value; return ((coalesce_value_14 == null) ? "" : coalesce_value_14); })()).slice(0, 512));
    (() => { throw new Error("Codex command/exec did not preserve North's managed shell identity", TypeScriptStructuralObjectV2(new Error($$bc$str($$bc$str($$bc$str("exitCode ", jsonStringify(providerGet(result, "exitCode")), ""), $$bc$str("; stdout ", seen(providerGet(result, "stdout")), " wanted ", jsonStringify(expectedOutput), "")), $$bc$str("; stderr ", seen(providerGet(result, "stderr")), ""))))); })();
  }
  return null;
}

function onlyKeys(value, expected, label) {
  const present = new Set(hostObjectKeys(value));
  const wanted = new Set(expected);
  const drift = $$bc$into_value($$bc$into_value([], $$bc$into_value([], present).filter((key) => (!((_truthy) => _truthy !== false && _truthy != null)(wanted.has(key)))).sort().map((key) => $$bc$str("unexpected ", key, ""))), $$bc$into_value([], wanted).filter((key) => (!((_truthy) => _truthy !== false && _truthy != null)(present.has(key)))).sort().map((key) => $$bc$str("missing ", key, "")));
  if ((!(drift.length === 0))) {
    (() => { throw new Error($$bc$str("", label, " fields do not match North's exact managed Codex contract"), TypeScriptStructuralObjectV2(new Error($$bc$str("drifted: ", (($beagle$apply$fn, $beagle$apply$tail) => $beagle$apply$fn.call($beagle$apply$fn, ...($beagle$apply$tail ?? [])))(((..._xs) => "".concat(..._xs)), $$bc$interpose(", ", drift)), "")))); })();
  }
  return null;
}

function optionalBoundedString(...$beagle$args) {
  if (arguments.length === 2) {
    const value = $beagle$args[0];
    const label = $beagle$args[1];
    return optionalBoundedString(value, label, MAX__ID__BYTES);
  }
  if (arguments.length === 3) {
    const value = $beagle$args[0];
    const label = $beagle$args[1];
    const maxBytes = $beagle$args[2];
    return ((value == null) ? null : boundedString(value, label, maxBytes));
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

function tomlStringMap(values) {
  return $$bc$str("{", (($beagle$apply$fn, $beagle$apply$tail) => $beagle$apply$fn.call($beagle$apply$fn, ...($beagle$apply$tail ?? [])))(((..._xs) => "".concat(..._xs)), $$bc$interpose(",", (hostObjectEntries($$bh$clj_to_js(values))).sort(($beagle$param$0, $beagle$param$1) => { let left = $beagle$param$0[0]; let right = $beagle$param$1[0]; return left.localeCompare(right); }).map(($beagle$param$0) => { let key = $beagle$param$0[0]; let value = $beagle$param$0[1]; return $$bc$str("", jsonStringify(key), "=", jsonStringify(value), ""); }))), "}");
}

function tomlProjectMap(root) {
  return $$bc$str("{", jsonStringify(root), "={trust_level=\"untrusted\"}}");
}

function assertNoFilesystemAuthority_bang(codexHome) {
  const for_break_15 = ({value: false, watches: {}});
  $$bc$eager_seq(["config.toml", "hooks.json", "rules"]).forEach((name) => {
  if ((!for_break_15.value)) {
    const for_continue_16 = ({value: false, watches: {}});
    (() => { try {
    lstatSync(resolve(codexHome, name));
  return (() => { throw new $$be$ExceptionInfo($$bc$str("managed Codex account contains authority-bearing ", name, ""), {}); })();
  } catch (_catch_0) {
    switch ($$bd$catch_dispatch(_catch_0, [$$bd$default_catch])) {
      case 0: {
        const error = _catch_0;
        return ((((error instanceof Error) && (($beagle$jst$receiver, $beagle$jst$key) => ($beagle$jst$key in $beagle$jst$receiver))(error, "code")) && (error.code === "ENOENT")) ? (() => { const _a = for_continue_16, _v = true; const _old = _a.value; _a.value = _v; for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _v); return _v; })() : (() => { throw error; })());
        break;
      }
    }
  } })();
  }
});
  return null;
}

function LaunchContract(args, expectedSessionConfig, executable, codexHome, sqliteHome, cwd, projectRoot, writableRoots, network, installedManagedHookFailureMode) {
  return $$bc$record_value("north.providers.codex-app-server/LaunchContract", {_tag: "LaunchContract", args, expectedSessionConfig, executable, codexHome, sqliteHome, cwd, projectRoot, writableRoots, network, installedManagedHookFailureMode});
}

function launchcontract_args(r) { return r.args; }

function launchcontract_expectedSessionConfig(r) { return r.expectedSessionConfig; }

function launchcontract_executable(r) { return r.executable; }

function launchcontract_codexHome(r) { return r.codexHome; }

function launchcontract_sqliteHome(r) { return r.sqliteHome; }

function launchcontract_cwd(r) { return r.cwd; }

function launchcontract_projectRoot(r) { return r.projectRoot; }

function launchcontract_writableRoots(r) { return r.writableRoots; }

function launchcontract_network(r) { return r.network; }

function launchcontract_installedManagedHookFailureMode(r) { return r.installedManagedHookFailureMode; }

function ManagedCodexNetworkPolicy(networkAccess, networkProxyEnabled, domains) {
  return $$bc$record_value("north.providers.codex-app-server/ManagedCodexNetworkPolicy", {_tag: "ManagedCodexNetworkPolicy", networkAccess, networkProxyEnabled, domains});
}

function managedcodexnetworkpolicy_networkAccess(r) { return r.networkAccess; }

function managedcodexnetworkpolicy_networkProxyEnabled(r) { return r.networkProxyEnabled; }

function managedcodexnetworkpolicy_domains(r) { return r.domains; }

function managedCodexWritableRoots(cwd) {
  const northStateRoot = resolve(homedir(), ".local/state/north");
  return $$bc$into_value([], new Set($$bc$conj_value($$bc$into_value([], trustedGitMetadataRoots(cwd)), northStateRoot))).sort();
}

function sandboxWritableRoots(surface, cwd) {
  return ((!(surface.sandbox === "workspace-write")) ? [] : managedCodexWritableRoots(cwd));
}

function executableFile(candidate) {
  return (() => { try {
    if ((!((_truthy) => _truthy !== false && _truthy != null)(statSync(candidate).isFile()))) {
    return null;
  } else {
    accessSync(candidate, FS__X__OK);
    return candidate;
  }
  } catch (_catch_1) {
    switch ($$bd$catch_dispatch(_catch_1, [$$bd$default_catch])) {
      case 0: {
        const typescript_error = _catch_1;
        return null;
        break;
      }
    }
  } })();
}

function sandboxCapablePath(env) {
  const path = foreign_string_property(env, "PATH");
  if (((!(typeof path === "string")) || (path === ""))) {
    return path;
  } else {
    const entries = path.split(delimiter).filter(Boolean);
    if ((!(((_pred, _coll) => { if (_coll == null) return null; for (const _item of _coll) { const _value = _pred(_item); if (_value !== false && _value != null) return _value; } return null; })((directory) => executableFile(join(directory, "bwrap")), entries) == null))) {
      return path;
    } else {
      const override = (() => { const optional_call_18 = foreign_string_property(env, "NORTH_BWRAP_BIN"); return ((optional_call_18 == null) ? null : optional_call_18.trim()); })();
      const resolved = (((_truthy) => _truthy !== false && _truthy != null)(override) ? executableFile(resolve(override)) : null);
      if ((!((_truthy) => _truthy !== false && _truthy != null)(resolved))) {
        (() => { throw new_ManagedCodexPreThreadError("openai_codex_sandbox_binary_unavailable"); })();
      }
      const northBin = $$bc$first(entries);
      return (($beagle$apply$fn, $beagle$apply$tail) => $beagle$apply$fn.call($beagle$apply$fn, ...($beagle$apply$tail ?? [])))(((..._xs) => "".concat(..._xs)), $$bc$interpose(delimiter, $$bc$into_value($$bc$conj_value($$bc$conj_value([], northBin), dirname(resolved)), $$bc$rest(entries))));
    }
  }
}

function launchStageString(code, run) {
  return (() => { try {
    return run();
  } catch (_catch_2) {
    switch ($$bd$catch_dispatch(_catch_2, [$$bd$default_catch])) {
      case 0: {
        const cause = _catch_2;
        return (() => { throw new_ManagedCodexPreThreadError(code, TypeScriptStructuralObjectV2(cause)); })();
        break;
      }
    }
  } })();
}

function launchStageStrings(code, run) {
  return (() => { try {
    return run();
  } catch (_catch_3) {
    switch ($$bd$catch_dispatch(_catch_3, [$$bd$default_catch])) {
      case 0: {
        const cause = _catch_3;
        return (() => { throw new_ManagedCodexPreThreadError(code, TypeScriptStructuralObjectV2(cause)); })();
        break;
      }
    }
  } })();
}

function launchStageNil(code, run) {
  return (() => { try {
    return run();
  } catch (_catch_4) {
    switch ($$bd$catch_dispatch(_catch_4, [$$bd$default_catch])) {
      case 0: {
        const cause = _catch_4;
        return (() => { throw new_ManagedCodexPreThreadError(code, TypeScriptStructuralObjectV2(cause)); })();
        break;
      }
    }
  } })();
}

function managedCodexAppServerLaunch_bang(options) {
  const codexHomeValue = (() => { const optional_call_19 = foreign_string_property(managedcodexappserveroptions_env(options), "CODEX_HOME"); return ((optional_call_19 == null) ? null : optional_call_19.trim()); })();
  const sqliteHomeValue = (() => { const optional_call_20 = foreign_string_property(managedcodexappserveroptions_env(options), "CODEX_SQLITE_HOME"); return ((optional_call_20 == null) ? null : optional_call_20.trim()); })();
  if (((!((_truthy) => _truthy !== false && _truthy != null)(codexHomeValue)) || (!((_truthy) => _truthy !== false && _truthy != null)(sqliteHomeValue)))) {
    (() => { throw new_ManagedCodexPreThreadError("openai_target_state_roots_missing"); })();
  }
  const stage = (code, run) => (() => { try {
    return run();
  } catch (_catch_5) {
    switch ($$bd$catch_dispatch(_catch_5, [$$bd$default_catch])) {
      case 0: {
        const cause = _catch_5;
        return (() => { throw new_ManagedCodexPreThreadError(code, TypeScriptStructuralObjectV2(cause)); })();
        break;
      }
    }
  } })();
  const codexHome = launchStageString("openai_codex_state_root_unresolvable", () => realpathSync(codexHomeValue));
  const sqliteHome = launchStageString("openai_codex_state_root_unresolvable", () => realpathSync(sqliteHomeValue));
  const cwd = launchStageString("openai_codex_cwd_unresolvable", () => realpathSync(managedcodexappserveroptions_cwd(options)));
  const projectRoot = launchStageString("openai_codex_project_root_untrusted", () => trustedGitProjectRoot(cwd));
  const executable = launchStageString("openai_codex_executable_pin_mismatch", () => { const resolved = realpathSync(managedcodexappserveroptions_command(options));
const expectedExecutable = realpathSync((((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? managedcodexappserveroptions_testExpectedExecutable(options) : _logical))(managedcodexappserveroptions_spawnProcess(options))) ? managedcodexappserveroptions_testExpectedExecutable(options) : trustedManagedCodexExecutable()));
if ((!(resolved === expectedExecutable))) {
  (() => { throw new $$be$ExceptionInfo($$bc$str("managed Codex executable ", resolved, " is not the pinned provider binary ", expectedExecutable, ""), {}); })();
}
return resolved; });
  const installedManagedHookFailureMode = (((_truthy) => _truthy !== false && _truthy != null)(managedcodexappserveroptions_spawnProcess(options)) ? null : launchStageString("openai_managed_hooks_contract_unavailable", () => { assertInstalledManagedCodexHooks();
return "block"; }));
  launchStageNil("openai_codex_authority_filesystem_invalid", () => assertNoFilesystemAuthority_bang(codexHome));
  (managedcodexappserveroptions_env(options).CODEX_HOME = codexHome);
  (managedcodexappserveroptions_env(options).CODEX_SQLITE_HOME = sqliteHome);
  (managedcodexappserveroptions_env(options).CODEX_INTERNAL_APP_SERVER_REMOTE_CONTROL_DISABLED = "1");
  const managedPath = sandboxCapablePath(managedcodexappserveroptions_env(options));
  if ((((((!(typeof managedPath === "string")) || (managedPath === "")) || (!(managedPath === (managedPath).trim()))) || (!(providerGet((managedPath).split(delimiter), 0) === dirname(ENGINE)))) || (!(foreign_string_property(managedcodexappserveroptions_env(options), "NORTH_BIN") === ENGINE)))) {
    (() => { throw new_ManagedCodexPreThreadError("openai_managed_shell_environment_invalid"); })();
  }
  (managedcodexappserveroptions_env(options).PATH = managedPath);
  const shellEnvironmentPolicy = Object.assign({}, {}, {[$$bc$property_key("inherit")]: "core"}, {[$$bc$property_key("set")]: Object.assign({}, {}, {[$$bc$property_key("PATH")]: managedPath}, {[$$bc$property_key("NORTH_BIN")]: ENGINE})});
  const northEnv = managedNorthMcpEnvironment(managedcodexnorthserver_env(managedcodexappserveroptions_north(options)));
  const network = managedCodexNetworkPolicy(managedcodexappserveroptions_surface(options));
  const features = $$bc$into_value({}, $$bc$into_value($$bc$into_value([], MANAGED__CODEX__ENABLED__FEATURES.map((name) => { const tuple_value_21 = [name, true];
return tuple_value_21; })), MANAGED__CODEX__DISABLED__FEATURES.map((name) => { const tuple_value_22 = [name, false];
return tuple_value_22; })));
  const sessionFeatures = Object.assign({}, {}, features, {[$$bc$property_key("network_proxy")]: (network.networkProxyEnabled ? Object.assign({}, {}, {[$$bc$property_key("enabled")]: true}, {[$$bc$property_key("domains")]: network.domains}) : false)});
  const writableRoots = launchStageStrings("openai_codex_git_metadata_unresolvable", () => sandboxWritableRoots(managedcodexappserveroptions_surface(options), cwd));
  const expectedSessionConfig = Object.assign({}, {}, {[$$bc$property_key("cli_auth_credentials_store")]: "file"}, {[$$bc$property_key("forced_login_method")]: "chatgpt"}, {[$$bc$property_key("model_provider")]: "openai"}, {[$$bc$property_key("sqlite_home")]: sqliteHome}, ((!(writableRoots.length === 0)) ? {[$$bc$property_key("sandbox_workspace_write")]: Object.assign({}, {}, {[$$bc$property_key("writable_roots")]: writableRoots}, {[$$bc$property_key("network_access")]: network.networkAccess})} : {}), {[$$bc$property_key("project_root_markers")]: [".git"]}, {[$$bc$property_key("projects")]: Object.fromEntries([[projectRoot, {[$$bc$property_key("trust_level")]: "untrusted"}]])}, {[$$bc$property_key("project_doc_max_bytes")]: 0}, {[$$bc$property_key("allow_login_shell")]: false}, {[$$bc$property_key("shell_environment_policy")]: shellEnvironmentPolicy}, {[$$bc$property_key("mcp_servers")]: {[$$bc$property_key("north")]: Object.assign({}, {}, {[$$bc$property_key("command")]: managedcodexnorthserver_command(managedcodexappserveroptions_north(options))}, {[$$bc$property_key("args")]: managedcodexnorthserver_args(managedcodexappserveroptions_north(options))}, {[$$bc$property_key("env")]: northEnv}, {[$$bc$property_key("enabled")]: true}, {[$$bc$property_key("required")]: true}, {[$$bc$property_key("enabled_tools")]: managedcodexappserveroptions_surface(options).northEnabledTools})}}, {[$$bc$property_key("web_search")]: managedcodexappserveroptions_surface(options).web}, {[$$bc$property_key("features")]: sessionFeatures});
  const args = $$bc$conj_value($$bc$conj_value($$bc$conj_value($$bc$into_value($$bc$into_value($$bc$into_value($$bc$conj_value($$bc$conj_value($$bc$conj_value($$bc$conj_value($$bc$conj_value($$bc$conj_value($$bc$conj_value($$bc$conj_value($$bc$conj_value($$bc$conj_value($$bc$conj_value($$bc$conj_value($$bc$conj_value($$bc$conj_value($$bc$conj_value($$bc$conj_value($$bc$conj_value($$bc$conj_value($$bc$conj_value($$bc$conj_value($$bc$conj_value($$bc$conj_value($$bc$conj_value($$bc$conj_value($$bc$into_value($$bc$conj_value($$bc$conj_value($$bc$into_value([], codexConfigArguments(managedcodexappserveroptions_env(options))), "-c"), "project_root_markers=[\".git\"]"), ((!(writableRoots.length === 0)) ? ["-c", $$bc$str("sandbox_workspace_write.writable_roots=", jsonStringify(writableRoots), ""), "-c", $$bc$str("sandbox_workspace_write.network_access=", network.networkAccess, "")] : [])), "-c"), $$bc$str("projects=", tomlProjectMap(projectRoot), "")), "-c"), "project_doc_max_bytes=0"), "-c"), "allow_login_shell=false"), "-c"), "shell_environment_policy.inherit=\"core\""), "-c"), $$bc$str("shell_environment_policy.set=", tomlStringMap(providerGet(shellEnvironmentPolicy, "set")), "")), "-c"), $$bc$str("mcp_servers.north.command=", jsonStringify(managedcodexnorthserver_command(managedcodexappserveroptions_north(options))), "")), "-c"), $$bc$str("mcp_servers.north.args=", jsonStringify(managedcodexnorthserver_args(managedcodexappserveroptions_north(options))), "")), "-c"), $$bc$str("mcp_servers.north.env=", tomlStringMap(northEnv), "")), "-c"), "mcp_servers.north.enabled=true"), "-c"), "mcp_servers.north.required=true"), "-c"), $$bc$str("mcp_servers.north.enabled_tools=", jsonStringify(managedcodexappserveroptions_surface(options).northEnabledTools), "")), "-c"), $$bc$str("web_search=", jsonStringify(managedcodexappserveroptions_surface(options).web), "")), MANAGED__CODEX__ENABLED__FEATURES.flatMap((name) => ["--enable", name])), managedCodexNetworkArguments(managedcodexappserveroptions_surface(options))), MANAGED__CODEX__DISABLED__FEATURES.flatMap((name) => ["--disable", name])), "app-server"), "--stdio"), "--strict-config");
  return LaunchContract(args, expectedSessionConfig, executable, codexHome, sqliteHome, cwd, projectRoot, writableRoots, network, installedManagedHookFailureMode);
}

function externalLaunchContract(contract) {
  return $$bh$js_obj("args", launchcontract_args(contract), "expectedSessionConfig", $$bh$clj_to_js(launchcontract_expectedSessionConfig(contract)), "executable", launchcontract_executable(contract), "codexHome", launchcontract_codexHome(contract), "sqliteHome", launchcontract_sqliteHome(contract), "cwd", launchcontract_cwd(contract), "projectRoot", launchcontract_projectRoot(contract), "writableRoots", launchcontract_writableRoots(contract), "network", launchcontract_network(contract), "installedManagedHookFailureMode", launchcontract_installedManagedHookFailureMode(contract));
}

function managedCodexAppServerLaunchExternal_bang(options) {
  return externalLaunchContract(managedCodexAppServerLaunch_bang(options));
}

const managedCodexAppServerLaunch = managedCodexAppServerLaunchExternal_bang;

function Pending(method, timer, resolve, reject) {
  return $$bc$record_value("north.providers.codex-app-server/Pending", {_tag: "Pending", method, timer, resolve, reject});
}

function pending_method(r) { return r.method; }

function pending_timer(r) { return r.timer; }

function pending_resolve(r) { return r.resolve; }

function pending_reject(r) { return r.reject; }

function ManagedAppServerStdin(write) {
  return $$bc$record_value("north.providers.codex-app-server/ManagedAppServerStdin", {_tag: "ManagedAppServerStdin", write});
}

function managedappserverstdin_write(r) { return r.write; }

function ManagedEventEmitter(on, once) {
  return $$bc$record_value("north.providers.codex-app-server/ManagedEventEmitter", {_tag: "ManagedEventEmitter", on, once});
}

function managedeventemitter_on(r) { return r.on; }

function managedeventemitter_once(r) { return r.once; }

function ManagedSupervisorStatusStream(on, removeListener, resume) {
  return $$bc$record_value("north.providers.codex-app-server/ManagedSupervisorStatusStream", {_tag: "ManagedSupervisorStatusStream", on, removeListener, resume});
}

function managedsupervisorstatusstream_on(r) { return r.on; }

function managedsupervisorstatusstream_removeListener(r) { return r.removeListener; }

function managedsupervisorstatusstream_resume(r) { return r.resume; }

function ManagedStrictJsonlMessages(push) {
  return $$bc$record_value("north.providers.codex-app-server/ManagedStrictJsonlMessages", {_tag: "ManagedStrictJsonlMessages", push});
}

function managedstrictjsonlmessages_push(r) { return r.push; }

function ManagedBufferSlice(toString) {
  return $$bc$record_value("north.providers.codex-app-server/ManagedBufferSlice", {_tag: "ManagedBufferSlice", toString});
}

function managedbufferslice_toString(r) { return r.toString; }

function ManagedUtf8Buffer(subarray) {
  return $$bc$record_value("north.providers.codex-app-server/ManagedUtf8Buffer", {_tag: "ManagedUtf8Buffer", subarray});
}

function managedutf8buffer_subarray(r) { return r.subarray; }

function ManagedTimer(unref) {
  return $$bc$record_value("north.providers.codex-app-server/ManagedTimer", {_tag: "ManagedTimer", unref});
}

function managedtimer_unref(r) { return r.unref; }

function ManagedProviderStderrRing(push, finish, add, tail) {
  return $$bc$record_value("north.providers.codex-app-server/ManagedProviderStderrRing", {_tag: "ManagedProviderStderrRing", push, finish, add, tail});
}

function managedproviderstderrring_push(r) { return r.push; }

function managedproviderstderrring_finish(r) { return r.finish; }

function managedproviderstderrring_add(r) { return r.add; }

function managedproviderstderrring_tail(r) { return r.tail; }

function stderrChunkText(chunk) {
  return ((typeof chunk === "string") ? chunk : (chunk).toString("utf8"));
}

function onManagedEvent_bang(emitter, event, listener) {
  emitter.on(event, listener);
  return null;
}

function onceManagedEvent_bang(emitter, event, listener) {
  emitter.once(event, listener);
  return null;
}

function SupervisorControl(path, connected, writeLine, close) {
  return $$bc$record_value("north.providers.codex-app-server/SupervisorControl", {_tag: "SupervisorControl", path, connected, writeLine, close});
}

function supervisorcontrol_path(r) { return r.path; }

function supervisorcontrol_connected(r) { return r.connected; }

function supervisorcontrol_writeLine(r) { return r.writeLine; }

function supervisorcontrol_close(r) { return r.close; }

function createSupervisorControl_bang() {
  const directory = mkdtempSync(join(tmpdir(), "north-codex-control-"));
  const sequence = ({value: 0, watches: {}});
  const closed = ({value: false, watches: {}});
  return SupervisorControl(directory, Promise.resolve(), (line, callback) => { if ((closed.value || (Buffer.byteLength(line, "utf8") > MAX__LINE__BYTES))) {
  callback(new Error("managed Codex supervisor control is unavailable"));
  return null;
} else {
  (() => { const _a = sequence; const _old = _a.value; _a.value = (((_a, _b) => _a + _b))(_old, 1); for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _a.value); return _a.value; })();
  const stem = String(sequence.value).padStart(12, "0");
  const temporary = join(directory, $$bc$str(".", stem, ".", process.pid, ".tmp"));
  const request = join(directory, $$bc$str(stem, ".req"));
  const fd = ({value: null, watches: {}});
  (() => { try {
    (() => { const _a = fd, _v = openSync(temporary, (FS__O__CREAT + FS__O__EXCL + FS__O__WRONLY + (() => { const nofollow = FS__O__NOFOLLOW; return ((nofollow == null) ? 0 : nofollow); })()), 384); const _old = _a.value; _a.value = _v; for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _v); return _v; })();
  const payloadLength = Buffer.byteLength(line, "utf8");
  const digest = createHash("sha256").update(line, "utf8").digest("hex");
  const framed = $$bc$str(SUPERVISOR__MESSAGE__PREFIX, payloadLength, " ", digest, "\n", line);
  writeStringToFdSync(fd.value, framed);
  syncFdSync(fd.value);
  closeFdSync(fd.value);
  (() => { const _a = fd, _v = null; const _old = _a.value; _a.value = _v; for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _v); return _v; })();
  renameSync(temporary, request);
  return callback(null);
  } catch (_catch_6) {
    switch ($$bd$catch_dispatch(_catch_6, [$$bd$default_catch])) {
      case 0: {
        const error = _catch_6;
        if ((!(fd.value == null))) {
          (() => { try {
    return closeFdSync(fd.value);
  } catch (_catch_7) {
    switch ($$bd$catch_dispatch(_catch_7, [$$bd$default_catch])) {
      case 0: {
        const ignored = _catch_7;
        return null;
        break;
      }
    }
  } })();
        }
        (() => { try {
    return unlinkSync(temporary);
  } catch (_catch_8) {
    switch ($$bd$catch_dispatch(_catch_8, [$$bd$default_catch])) {
      case 0: {
        const ignored = _catch_8;
        return null;
        break;
      }
    }
  } })();
        return callback(((error instanceof Error) ? error : new Error("managed Codex supervisor control failed")));
        break;
      }
    }
  } })();
  return null;
} }, () => { if (closed.value) {
  return null;
} else {
  (() => { const _a = closed, _v = true; const _old = _a.value; _a.value = _v; for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _v); return _v; })();
  (() => { try {
    return removeTreeSync(directory, SupervisorRemovalOptions(true, true));
  } catch (_catch_9) {
    switch ($$bd$catch_dispatch(_catch_9, [$$bd$default_catch])) {
      case 0: {
        const ignored = _catch_9;
        return null;
        break;
      }
    }
  } })();
  return null;
} });
}

const SAFE__NOTIFICATIONS = new Set(["configWarning", "deprecationNotice", "remoteControl/status/changed", "mcpServer/startupStatus/updated", "model/safetyBuffering/updated", "account/rateLimits/updated", "serverRequest/resolved", "thread/started", "thread/status/changed", "thread/tokenUsage/updated", "turn/started", "turn/completed", "item/started", "item/completed", "item/agentMessage/delta", "item/plan/delta", "item/reasoning/summaryTextDelta", "item/reasoning/summaryPartAdded", "item/reasoning/textDelta", "item/commandExecution/outputDelta", "item/commandExecution/terminalInteraction", "item/fileChange/outputDelta", "item/fileChange/patchUpdated", "item/mcpToolCall/progress", "turn/diff/updated", "turn/plan/updated", "hook/started", "hook/completed"]);

function AppServerRpc(nextId, pending, messages, terminal, terminalFromProcessDeath, closed, terminalListeners, unsupported, stderr, inboundQueue, inboundDraining, inboundIdle, deferredInboundFailure, stdoutEnded, child, timeoutMs, onNotification, onServerRequest, writeLine, ownsStderr) {
  return $$bc$record_value("north.providers.codex-app-server/AppServerRpc", {_tag: "AppServerRpc", nextId, pending, messages, terminal, terminalFromProcessDeath, closed, terminalListeners, unsupported, stderr, inboundQueue, inboundDraining, inboundIdle, deferredInboundFailure, stdoutEnded, child, timeoutMs, onNotification, onServerRequest, writeLine, ownsStderr});
}

function appserverrpc_nextId(r) { return r.nextId; }

function appserverrpc_pending(r) { return r.pending; }

function appserverrpc_messages(r) { return r.messages; }

function appserverrpc_terminal(r) { return r.terminal; }

function appserverrpc_terminalFromProcessDeath(r) { return r.terminalFromProcessDeath; }

function appserverrpc_closed(r) { return r.closed; }

function appserverrpc_terminalListeners(r) { return r.terminalListeners; }

function appserverrpc_unsupported(r) { return r.unsupported; }

function appserverrpc_stderr(r) { return r.stderr; }

function appserverrpc_inboundQueue(r) { return r.inboundQueue; }

function appserverrpc_inboundDraining(r) { return r.inboundDraining; }

function appserverrpc_inboundIdle(r) { return r.inboundIdle; }

function appserverrpc_deferredInboundFailure(r) { return r.deferredInboundFailure; }

function appserverrpc_stdoutEnded(r) { return r.stdoutEnded; }

function appserverrpc_child(r) { return r.child; }

function appserverrpc_timeoutMs(r) { return r.timeoutMs; }

function appserverrpc_onNotification(r) { return r.onNotification; }

function appserverrpc_onServerRequest(r) { return r.onServerRequest; }

function appserverrpc_writeLine(r) { return r.writeLine; }

function appserverrpc_ownsStderr(r) { return r.ownsStderr; }

function new_AppServerRpc_bang(...$beagle$args) {
  if (arguments.length === 4) {
    const child = $beagle$args[0];
    const timeoutMs = $beagle$args[1];
    const onNotification = $beagle$args[2];
    const onServerRequest = $beagle$args[3];
    return new_AppServerRpc_bang(child, timeoutMs, onNotification, onServerRequest, (line, callback) => { (child.stdin).write(line, callback);
return null; }, true);
  }
  if (arguments.length === 5) {
    const child = $beagle$args[0];
    const timeoutMs = $beagle$args[1];
    const onNotification = $beagle$args[2];
    const onServerRequest = $beagle$args[3];
    const writeLine = $beagle$args[4];
    return new_AppServerRpc_bang(child, timeoutMs, onNotification, onServerRequest, writeLine, true);
  }
  if (arguments.length === 6) {
    const child = $beagle$args[0];
    const timeoutMs = $beagle$args[1];
    const onNotification = $beagle$args[2];
    const onServerRequest = $beagle$args[3];
    const writeLine = $beagle$args[4];
    const ownsStderr = $beagle$args[5];
    const self = AppServerRpc(({value: 0, watches: {}}), ({value: new Map(), watches: {}}), ({value: new StrictJsonlMessages(strictJsonlLimits("managed Codex app-server", MAX__LINE__BYTES, MAX__TOTAL__BYTES, MAX__MESSAGES)), watches: {}}), ({value: null, watches: {}}), ({value: false, watches: {}}), ({value: false, watches: {}}), ({value: new Set([]), watches: {}}), ({value: new Map(), watches: {}}), ({value: new ProviderStderrRing(), watches: {}}), ({value: [], watches: {}}), ({value: false, watches: {}}), ({value: null, watches: {}}), ({value: null, watches: {}}), ({value: false, watches: {}}), ({value: child, watches: {}}), ({value: timeoutMs, watches: {}}), ({value: onNotification, watches: {}}), ({value: onServerRequest, watches: {}}), ({value: writeLine, watches: {}}), ({value: ownsStderr, watches: {}}));
    onManagedEvent_bang(child.stdout, "data", (chunk) => appserverrpc_onData_bang(self, chunk));
    onManagedEvent_bang(child.stdout, "end", () => { (() => { const _a = appserverrpc_stdoutEnded(self), _v = true; const _old = _a.value; _a.value = _v; for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _v); return _v; })();
return (() => { try {
    return appserverrpc_messages(self).value.finish();
  } catch (_catch_10) {
    switch ($$bd$catch_dispatch(_catch_10, [$$bd$default_catch])) {
      case 0: {
        const cause = _catch_10;
        return appserverrpc_queueInboundFailure_bang(self, new Error("managed Codex closed with a partial message", TypeScriptStructuralObjectV2(cause)), true);
        break;
      }
    }
  } })(); });
    onManagedEvent_bang(child.stdout, "error", () => appserverrpc_failFromDeath_bang(self, new Error("managed Codex stdout failed")));
    if (appserverrpc_ownsStderr(self).value) {
      onManagedEvent_bang(child.stderr, "data", (chunk) => (() => { try {
    appserverrpc_stderr(self).value.push(stderrChunkText(chunk));
  return null;
  } catch (_catch_11) {
    switch ($$bd$catch_dispatch(_catch_11, [$$bd$default_catch])) {
      case 0: {
        const typescript_error = _catch_11;
        return null;
        break;
      }
    }
  } })());
      onManagedEvent_bang(child.stderr, "end", () => (() => { try {
    appserverrpc_stderr(self).value.finish();
  return null;
  } catch (_catch_12) {
    switch ($$bd$catch_dispatch(_catch_12, [$$bd$default_catch])) {
      case 0: {
        const typescript_error = _catch_12;
        return null;
        break;
      }
    }
  } })());
      onManagedEvent_bang(child.stderr, "error", () => null);
    }
    onManagedEvent_bang(child.stdin, "error", () => appserverrpc_failFromDeath_bang(self, new Error("managed Codex stdin failed")));
    onManagedEvent_bang(child, "error", () => appserverrpc_failFromDeath_bang(self, new Error("managed Codex supervisor failed")));
    onManagedEvent_bang(child, "exit", () => { if ((!appserverrpc_closed(self).value)) {
  appserverrpc_queueInboundFailure_bang(self, new Error("managed Codex app-server exited unexpectedly"), true);
}
return null; });
    return self;
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

function appserverrpc_failFromDeath_bang(self, error) {
  if ((!((_truthy) => _truthy !== false && _truthy != null)(appserverrpc_terminal(self).value))) {
    (() => { const _a = appserverrpc_terminalFromProcessDeath(self), _v = true; const _old = _a.value; _a.value = _v; for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _v); return _v; })();
  }
  return appserverrpc_fail_bang(self, error);
}

function appserverrpc_fail_bang(self, error) {
  if (((_truthy) => _truthy !== false && _truthy != null)(appserverrpc_terminal(self).value)) {
    return null;
  } else {
    (() => { const _a = appserverrpc_terminal(self), _v = error; const _old = _a.value; _a.value = _v; for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _v); return _v; })();
    const for_break_23 = ({value: false, watches: {}});
    $$bc$eager_seq(appserverrpc_pending(self).value.values()).forEach((pending) => {
  if ((!for_break_23.value)) {
    const for_continue_24 = ({value: false, watches: {}});
    clearTimeout(pending_timer(pending));
    (pending_reject(pending))(error);
  }
});
    appserverrpc_pending(self).value.clear();
    const for_break_25 = ({value: false, watches: {}});
    $$bc$eager_seq(appserverrpc_terminalListeners(self).value).forEach((listener) => {
  if ((!for_break_25.value)) {
    const for_continue_26 = ({value: false, watches: {}});
    listener(error);
  }
});
    return null;
  }
}

function appserverrpc_onTerminal(self, listener) {
  if (((_truthy) => _truthy !== false && _truthy != null)(appserverrpc_terminal(self).value)) {
    listener(appserverrpc_terminal(self).value);
    return () => null;
  } else {
    appserverrpc_terminalListeners(self).value.add(listener);
    return () => { appserverrpc_terminalListeners(self).value.delete(listener);
return null; };
  }
}

function appserverrpc_rejectServerRequest_bang(self, id) {
  (appserverrpc_writeLine(self).value)($$bc$str("", jsonStringify(Object.assign({}, {}, {[$$bc$property_key("id")]: id}, {[$$bc$property_key("error")]: Object.assign({}, {}, {[$$bc$property_key("code")]: (-32601)}, {[$$bc$property_key("message")]: "North does not grant app-server callback authority"})})), "\n"), (error) => null);
  return appserverrpc_fail_bang(self, new Error("managed Codex requested ungranted client authority"));
}

function appserverrpc_onData_bang(self, chunk) {
  return (() => { try {
    const lines = $$bc$into_value([], appserverrpc_messages(self).value.push(chunk));
  if (((appserverrpc_inboundQueue(self).value.length + lines.length) > MAX__PENDING__RPC__MESSAGES)) {
    appserverrpc_queueInboundFailure_bang(self, new Error("managed Codex inbound message queue exceeded its bound"), false);
    return null;
  } else {
    (() => { $$bc$eager_seq(lines).forEach((line) => {
  appserverrpc_inboundQueue(self).value.push(line);
}); })();
    if ((lines.length > 0)) {
      appserverrpc_startInboundDrain_bang(self);
    }
    return null;
  }
  } catch (_catch_13) {
    switch ($$bd$catch_dispatch(_catch_13, [$$bd$default_catch])) {
      case 0: {
        const cause = _catch_13;
        return appserverrpc_queueInboundFailure_bang(self, new Error("managed Codex emitted invalid JSONL", TypeScriptStructuralObjectV2(cause)), false);
        break;
      }
    }
  } })();
}

async function appserverrpc_runInboundDrain_bang(self) {
  const while_break_28 = ({value: false, watches: {}});
  return (async () => {  while (true) {
    if (((!((_truthy) => _truthy !== false && _truthy != null)(appserverrpc_terminal(self).value)) && (appserverrpc_inboundQueue(self).value.length > 0))) { await (async () => { const while_continue_29 = ({value: false, watches: {}}); return await appserverrpc_onLine_bang(self, appserverrpc_inboundQueue(self).value.shift()); })(); if ((!while_break_28.value)) {  continue; } else { null; return (() => { if (((_truthy) => _truthy !== false && _truthy != null)(((!((_truthy) => _truthy !== false && _truthy != null)(appserverrpc_terminal(self).value)) && appserverrpc_deferredInboundFailure(self).value))) {
  const failure = appserverrpc_deferredInboundFailure(self).value;
  if (((!failure.processDeath) || appserverrpc_stdoutEnded(self).value)) {
    (() => { const _a = appserverrpc_deferredInboundFailure(self), _v = null; const _old = _a.value; _a.value = _v; for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _v); return _v; })();
    if (failure.processDeath) {
      appserverrpc_failFromDeath_bang(self, failure.error);
    } else {
      appserverrpc_fail_bang(self, failure.error);
    }
  }
  null;
}
return null; })(); } } else { null; return (() => { if (((_truthy) => _truthy !== false && _truthy != null)(((!((_truthy) => _truthy !== false && _truthy != null)(appserverrpc_terminal(self).value)) && appserverrpc_deferredInboundFailure(self).value))) {
  const failure = appserverrpc_deferredInboundFailure(self).value;
  if (((!failure.processDeath) || appserverrpc_stdoutEnded(self).value)) {
    (() => { const _a = appserverrpc_deferredInboundFailure(self), _v = null; const _old = _a.value; _a.value = _v; for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _v); return _v; })();
    if (failure.processDeath) {
      appserverrpc_failFromDeath_bang(self, failure.error);
    } else {
      appserverrpc_fail_bang(self, failure.error);
    }
  }
  null;
}
return null; })(); }
  } })();
}

function appserverrpc_startInboundDrain_bang(self) {
  if (((_truthy) => _truthy !== false && _truthy != null)((appserverrpc_inboundDraining(self).value || appserverrpc_terminal(self).value))) {
    return null;
  } else {
    (() => { const _a = appserverrpc_inboundDraining(self), _v = true; const _old = _a.value; _a.value = _v; for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _v); return _v; })();
    const coalesce_field_27 = appserverrpc_inboundIdle(self).value;
    if ((coalesce_field_27 == null)) {
      (() => { const _a = appserverrpc_inboundIdle(self), _v = Promise.withResolvers(); const _old = _a.value; _a.value = _v; for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _v); return _v; })();
    } else {
      coalesce_field_27;
    }
    appserverrpc_child(self).value.stdout.pause();
    appserverrpc_runInboundDrain_bang(self).catch((error) => appserverrpc_fail_bang(self, ((error instanceof Error) ? error : new Error("managed Codex inbound processing failed")))).finally(() => { (() => { const _a = appserverrpc_inboundDraining(self), _v = false; const _old = _a.value; _a.value = _v; for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _v); return _v; })();
if (((_truthy) => _truthy !== false && _truthy != null)(((!((_truthy) => _truthy !== false && _truthy != null)(appserverrpc_terminal(self).value)) && ((appserverrpc_inboundQueue(self).value.length > 0) || ((_logical) => (_logical !== false && _logical != null ? ((!(appserverrpc_deferredInboundFailure(self).value).processDeath) || appserverrpc_stdoutEnded(self).value) : _logical))(appserverrpc_deferredInboundFailure(self).value))))) {
  appserverrpc_startInboundDrain_bang(self);
  return null;
} else {
  (() => { const _a = appserverrpc_inboundQueue(self), _v = []; const _old = _a.value; _a.value = _v; for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _v); return _v; })();
  if ((!((_truthy) => _truthy !== false && _truthy != null)(appserverrpc_terminal(self).value))) {
    appserverrpc_child(self).value.stdout.resume();
  }
  const idle = appserverrpc_inboundIdle(self).value;
  (() => { const _a = appserverrpc_inboundIdle(self), _v = null; const _old = _a.value; _a.value = _v; for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _v); return _v; })();
  const optional_call_30 = idle;
  return ((optional_call_30 == null) ? null : optional_call_30.resolve());
} });
    return null;
  }
}

function appserverrpc_queueInboundFailure_bang(self, error, processDeath) {
  if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? _logical : appserverrpc_deferredInboundFailure(self).value))(appserverrpc_terminal(self).value))) {
    return null;
  } else {
    (() => { const _a = appserverrpc_deferredInboundFailure(self), _v = TypeScriptAnonymousObjectV5(error, processDeath); const _old = _a.value; _a.value = _v; for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _v); return _v; })();
    if (((!processDeath) || appserverrpc_stdoutEnded(self).value)) {
      appserverrpc_startInboundDrain_bang(self);
    }
    return null;
  }
}

async function appserverrpc_drainInbound(self) {
  return await (() => { const optional_value_31 = appserverrpc_inboundIdle(self).value; return ((optional_value_31 == null) ? Promise.resolve() : optional_value_31.promise); })();
}

async function appserverrpc_onLine_bang(self, line) {
  let value = null;
  (() => { try {
    return (value = parseStrictJson(line, "managed Codex JSONL", strictJsonLimits(MAX__LINE__BYTES, null, null)));
  } catch (_catch_14) {
    switch ($$bd$catch_dispatch(_catch_14, [$$bd$default_catch])) {
      case 0: {
        const cause = _catch_14;
        return appserverrpc_fail_bang(self, new Error("managed Codex emitted malformed JSONL", TypeScriptStructuralObjectV2(cause)));
        break;
      }
    }
  } })();
  if (((_truthy) => _truthy !== false && _truthy != null)(appserverrpc_terminal(self).value)) {
    return null;
  } else {
    const message = (() => { try {
    return foreignRecord(value, "managed Codex message");
  } catch (_catch_15) {
    switch ($$bd$catch_dispatch(_catch_15, [$$bd$default_catch])) {
      case 0: {
        const error = _catch_15;
        appserverrpc_fail_bang(self, error);
        return null;
        break;
      }
    }
  } })();
    if ((message == null)) {
      return null;
    } else {
      const message_object = message;
      const method_value = Reflect.get(message_object, "method");
      if ((typeof method_value === "string")) {
        const method = method_value;
        if (((_truthy) => _truthy !== false && _truthy != null)(Object.hasOwn(message_object, "id"))) {
          if ((!((_truthy) => _truthy !== false && _truthy != null)(hostObjectKeys(message_object).every((key) => ["id", "method", "params"].includes(key))))) {
            return appserverrpc_fail_bang(self, new Error("managed Codex server request envelope is invalid"));
          } else {
            const id_value = Reflect.get(message_object, "id");
            if (((!(typeof id_value === "number")) && (!(typeof id_value === "string")))) {
              return appserverrpc_fail_bang(self, new Error("managed Codex server request has invalid id"));
            } else {
              const id = id_value;
              let result = null;
              (() => { try {
    return (result = (appserverrpc_onServerRequest(self).value)(id, method, decodeJsonValue(Reflect.get(message_object, "params"))));
  } catch (_catch_16) {
    switch ($$bd$catch_dispatch(_catch_16, [$$bd$default_catch])) {
      case 0: {
        const error = _catch_16;
        return appserverrpc_fail_bang(self, ((error instanceof Error) ? error : new Error("managed Codex callback invalid")));
        break;
      }
    }
  } })();
              if (((_truthy) => _truthy !== false && _truthy != null)(appserverrpc_terminal(self).value)) {
                return null;
              } else {
                if ((result == null)) {
                  return appserverrpc_rejectServerRequest_bang(self, id);
                } else {
                  (appserverrpc_writeLine(self).value)($$bc$str("", jsonStringify(Object.assign({}, {}, {[$$bc$property_key("id")]: id}, {[$$bc$property_key("result")]: result})), "\n"), (error) => { if (((_truthy) => _truthy !== false && _truthy != null)(error)) {
  appserverrpc_fail_bang(self, new Error("managed Codex callback response failed", TypeScriptStructuralObjectV2(error)));
}
return null; });
                  return null;
                }
              }
            }
          }
        } else {
          (() => { try {
    onlyKeys(message, (((_truthy) => _truthy !== false && _truthy != null)(Object.hasOwn(message_object, "emittedAtMs")) ? ["method", "params", "emittedAtMs"] : ["method", "params"]), "managed Codex notification");
  if (((_truthy) => _truthy !== false && _truthy != null)(Object.hasOwn(message_object, "emittedAtMs"))) {
    const emitted_at_ms = Reflect.get(message_object, "emittedAtMs");
    if (((!(typeof emitted_at_ms === "number")) || ((!Number.isSafeInteger(emitted_at_ms)) || (emitted_at_ms < 0)))) {
      return (() => { throw new Error("managed Codex notification emittedAtMs is invalid"); })();
    }
  }
  } catch (_catch_17) {
    switch ($$bd$catch_dispatch(_catch_17, [$$bd$default_catch])) {
      case 0: {
        const error = _catch_17;
        return appserverrpc_fail_bang(self, error);
        break;
      }
    }
  } })();
          if (((_truthy) => _truthy !== false && _truthy != null)(appserverrpc_terminal(self).value)) {
            return null;
          } else {
            if ((!((_truthy) => _truthy !== false && _truthy != null)(SAFE__NOTIFICATIONS.has(method)))) {
              const current = appserverrpc_unsupported(self).value.get(method);
              const seen = (((current == null) ? 0 : current) + 1);
              appserverrpc_unsupported(self).value.set(method, seen);
              if (((appserverrpc_unsupported(self).value.size > MAX__UNSUPPORTED__NOTIFICATION__METHODS) || (seen > MAX__UNSUPPORTED__NOTIFICATIONS__PER__METHOD))) {
                return appserverrpc_fail_bang(self, new Error($$bc$str("managed Codex flooded unsupported notification ", method)));
              } else {
                if ((seen === 1)) {
                  console.error($$bc$str("[codex] ignoring unsupported managed notification ", method));
                }
                return null;
              }
            } else {
              return (async () => { try {
    const notification_result = (appserverrpc_onNotification(self).value)(method, decodeJsonValue(Reflect.get(message_object, "params")));
  if (((_truthy) => _truthy !== false && _truthy != null)(notification_result)) {
    await notification_result;
  }
  return null;
  } catch (_catch_18) {
    switch ($$bd$catch_dispatch(_catch_18, [$$bd$default_catch])) {
      case 0: {
        const error = _catch_18;
        return appserverrpc_fail_bang(self, ((error instanceof Error) ? error : new Error("managed Codex notification invalid")));
        break;
      }
    }
  } })();
            }
          }
        }
      } else {
        if ((!(typeof method_value === "string"))) {
          const id_value = Reflect.get(message_object, "id");
          if (((!(typeof id_value === "number")) && (!(typeof id_value === "string")))) {
            return appserverrpc_fail_bang(self, new Error("managed Codex response has invalid id"));
          } else {
            const id = id_value;
            const pending = appserverrpc_pending(self).value.get(id);
            if ((pending == null)) {
              return appserverrpc_fail_bang(self, new Error("managed Codex response id is unknown"));
            } else {
              appserverrpc_pending(self).value.delete(id);
              clearTimeout(pending_timer(pending));
              const has_result = Object.hasOwn(message_object, "result");
              const has_error = Object.hasOwn(message_object, "error");
              if ((has_result === has_error)) {
                const error = new Error($$bc$str("managed Codex ", pending_method(pending), " response is malformed"));
                (pending_reject(pending))(error);
                return appserverrpc_fail_bang(self, error);
              } else {
                return (() => { try {
    onlyKeys(message, ["id", (has_result ? "result" : "error")], "managed Codex response");
  if (has_error) {
    const error_payload = Reflect.get(message_object, "error");
    const error = new Error($$bc$str("managed Codex ", pending_method(pending), " failed"), TypeScriptStructuralObjectV2(new Error($$bc$str("provider error response: ", jsonStringify(canonical(error_payload)).slice(0, 600)))));
    (pending_reject(pending))(error);
    return appserverrpc_fail_bang(self, error);
  } else {
    return (pending_resolve(pending))(decodeJsonValue(Reflect.get(message_object, "result")));
  }
  } catch (_catch_19) {
    switch ($$bd$catch_dispatch(_catch_19, [$$bd$default_catch])) {
      case 0: {
        const error = _catch_19;
        (pending_reject(pending))(error);
        return appserverrpc_fail_bang(self, error);
        break;
      }
    }
  } })();
              }
            }
          }
        }
      }
    }
  }
}

function appserverrpc_request_bang(self, method, params) {
  if (((_truthy) => _truthy !== false && _truthy != null)(appserverrpc_terminal(self).value)) {
    (() => { throw appserverrpc_terminal(self).value; })();
  }
  const id = (() => { const _a = appserverrpc_nextId(self); const _old = _a.value; _a.value = ((value) => (value + 1))(_old); for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _a.value); return _a.value; })();
  const envelope = ((params === null) ? Object.assign({}, {}, {[$$bc$property_key("id")]: id}, {[$$bc$property_key("method")]: method}) : Object.assign({}, {}, {[$$bc$property_key("id")]: id}, {[$$bc$property_key("method")]: method}, {[$$bc$property_key("params")]: params}));
  const line = $$bc$str("", jsonStringify(envelope), "\n");
  if ((Buffer.byteLength(line, "utf8") > MAX__LINE__BYTES)) {
    (() => { throw new $$be$ExceptionInfo($$bc$str("managed Codex ", method, " request is oversized"), {}); })();
  }
  const deferred = Promise.withResolvers();
  const timer = setTimeout(() => { const current = appserverrpc_pending(self).value.get(id);
if ((!((_truthy) => _truthy !== false && _truthy != null)(current))) {
  return null;
} else {
  appserverrpc_pending(self).value.delete(id);
  const error = new Error($$bc$str("managed Codex ", method, " timed out"));
  (pending_reject(current))(error);
  return appserverrpc_fail_bang(self, error);
} }, appserverrpc_timeoutMs(self).value);
  (timer).unref();
  appserverrpc_pending(self).value.set(id, Pending(method, timer, deferred.resolve, deferred.reject));
  (appserverrpc_writeLine(self).value)(line, (error) => { if (((_truthy) => _truthy !== false && _truthy != null)(error)) {
  appserverrpc_fail_bang(self, new Error($$bc$str("managed Codex ", method, " write failed")));
}
return null; });
  return deferred.promise;
}

function appserverrpc_notify_bang(self, method, params) {
  if (((_truthy) => _truthy !== false && _truthy != null)(appserverrpc_terminal(self).value)) {
    (() => { throw appserverrpc_terminal(self).value; })();
  }
  const line = $$bc$str("", jsonStringify(((params === null) ? Object.assign({}, {}, {[$$bc$property_key("method")]: method}) : Object.assign({}, {}, {[$$bc$property_key("method")]: method}, {[$$bc$property_key("params")]: params}))), "\n");
  return (appserverrpc_writeLine(self).value)(line, (error) => { if (((_truthy) => _truthy !== false && _truthy != null)(error)) {
  appserverrpc_fail_bang(self, new Error($$bc$str("managed Codex ", method, " notification failed")));
}
return null; });
}

function appserverrpc_assertHealthy(self) {
  if (((_truthy) => _truthy !== false && _truthy != null)(appserverrpc_terminal(self).value)) {
    (() => { throw appserverrpc_terminal(self).value; })();
  }
  return null;
}

function appserverrpc_unsupportedNotifications(self) {
  return Object.fromEntries($$bc$into_value([], appserverrpc_unsupported(self).value.entries()).sort(($beagle$param$0, $beagle$param$1) => { let left = $beagle$param$0[0]; let right = $beagle$param$1[0]; return left.localeCompare(right); }));
}

function appserverrpc_diedFromProcessDeath(self) {
  return appserverrpc_terminalFromProcessDeath(self).value;
}

function appserverrpc_stderrTail(self, count) {
  return (appserverrpc_ownsStderr(self).value ? appserverrpc_stderr(self).value.tail(count) : []);
}

function appserverrpc_markClosing_bang(self) {
  (() => { const _a = appserverrpc_closed(self), _v = true; const _old = _a.value; _a.value = _v; for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _v); return _v; })();
  return null;
}

function configFingerprint(response) {
  const body = record(response, "Codex config/read response");
  if ((!Array.isArray(providerGet(body, "layers")))) {
    (() => { throw new $$be$ExceptionInfo("Codex config/read omitted layers", {}); })();
  }
  return jsonStringify(canonical(providerGet(body, "layers").map((raw) => { const layer = record(raw, "Codex config layer");
const disabledReason = providerGet(layer, "disabledReason");
return TypeScriptStructuralObjectV3(((disabledReason == null) ? null : disabledReason), providerGet(layer, "name"), providerGet(layer, "version"), providerGet(layer, "config")); })));
}

function validateDisabledProjectConfig(value) {
  const serialized = jsonStringify(value);
  if ((!(typeof serialized === "string"))) {
    (() => { throw new $$be$ExceptionInfo("Codex disabled project layer is not JSON-serializable", {}); })();
  }
  parseStrictJson(serialized, "Codex disabled project layer", strictJsonLimits(MAX__DISABLED__PROJECT__CONFIG__BYTES, MAX__DISABLED__PROJECT__CONFIG__DEPTH, MAX__DISABLED__PROJECT__CONFIG__NODES));
  const allowed = new Set(REVIEWED__DISABLED__PROJECT__CONFIG__KEYS);
  const widened = hostObjectKeys(value).filter((key) => (!((_truthy) => _truthy !== false && _truthy != null)(allowed.has(key)))).sort();
  if ((!($$bc$count(widened) === 0))) {
    (() => { throw new $$be$ExceptionInfo($$bc$str($$bc$str("Codex disabled project config widened authority: ", (($beagle$apply$fn, $beagle$apply$tail) => $beagle$apply$fn.call($beagle$apply$fn, ...($beagle$apply$tail ?? [])))(((..._xs) => "".concat(..._xs)), $$bc$interpose(", ", widened)), ""), $$bc$str(" (allowed: ", (($beagle$apply$fn, $beagle$apply$tail) => $beagle$apply$fn.call($beagle$apply$fn, ...($beagle$apply$tail ?? [])))(((..._xs) => "".concat(..._xs)), $$bc$interpose(", ", $$bc$into_value([], allowed).sort())), ")")), {}); })();
  }
  return null;
}

function expectedProjectDisabledReason(contract) {
  return $$bc$str($$bc$str("", launchcontract_projectRoot(contract), " is marked as untrusted in ", launchcontract_codexHome(contract), "/config.toml. "), "To load project-local config, hooks, and exec policies, mark it trusted.");
}

function projectConfigWarningCorrelates(text, project_config, user_config) {
  return ((_logical) => (_logical !== false && _logical != null ? ((_logical) => (_logical !== false && _logical != null ? _logical : ((_logical) => (_logical !== false && _logical != null ? text.includes("effective configuration") : _logical))(text.includes("marked as untrusted"))))(text.includes(user_config)) : _logical))(text.includes(project_config));
}

function validateProjectConfigWarning(value, contract) {
  const warning = record(value, "Codex config warning");
  const summary = boundedProviderProse(providerGet(warning, "summary"), "Codex config warning summary", 8192.0);
  const details = ((providerGet(warning, "details") == null) ? "" : boundedProviderProse(providerGet(warning, "details"), "Codex config warning details", 8192.0));
  const text = $$bc$str(summary, "\n", details);
  const project_config = resolve(launchcontract_projectRoot(contract), ".codex");
  const user_config = resolve(launchcontract_codexHome(contract), "config.toml");
  if ((!projectConfigWarningCorrelates(text, project_config, user_config))) {
    (() => { throw new $$be$ExceptionInfo("Codex config warning omitted the exact project or trust authority", {}); })();
  }
  return null;
}

function validateConfig_bang(...$beagle$args) {
  if (arguments.length === 2) {
    const response = $beagle$args[0];
    const contract = $beagle$args[1];
    return validateConfig_bang(response, contract, false);
  }
  if (arguments.length === 3) {
    const response = $beagle$args[0];
    const contract = $beagle$args[1];
    const projectWarningSeen = $beagle$args[2];
    const body = record(response, "Codex config/read response");
    const config = record(providerGet(body, "config"), "Codex effective config");
    if ((!Array.isArray(providerGet(body, "layers")))) {
      (() => { throw new $$be$ExceptionInfo("Codex config/read omitted layers", {}); })();
    }
    const layers = providerGet(body, "layers").map((raw) => record(raw, "Codex config layer"));
    const seen = new Map();
    let projectWarningRequired = false;
    const for_break_38 = ({value: false, watches: {}});
    $$bc$eager_seq(layers).forEach((layer) => {
  if ((!for_break_38.value)) {
    const for_continue_39 = ({value: false, watches: {}});
    const name = record(providerGet(layer, "name"), "Codex config layer name");
    const type = boundedString(providerGet(name, "type"), "Codex config layer type", 64);
    seen.set(type, ((() => { const coalesce_value_40 = seen.get(type); return ((coalesce_value_40 == null) ? 0 : coalesce_value_40); })() + 1));
    if (((!(typeof providerGet(layer, "version") === "string")) || (!((_truthy) => _truthy !== false && _truthy != null)(/^sha256:[0-9a-f]{64}$/.test(providerGet(layer, "version")))))) {
      (() => { throw new $$be$ExceptionInfo("Codex config layer has invalid version", {}); })();
    }
    const layerConfig = record(providerGet(layer, "config"), "Codex config layer payload");
    if ((type === "sessionFlags")) {
      exactDiagnosable(providerGet(record(providerGet(layerConfig, "features"), "Codex session authority feature set"), "network_proxy"), providerGet(record(providerGet(launchcontract_expectedSessionConfig(contract), "features"), "Codex expected session authority feature set"), "network_proxy"), "Codex session network proxy policy");
      exactNamingKeys(layerConfig, launchcontract_expectedSessionConfig(contract), "Codex session authority layer");
    } else {
      if ((type === "project")) {
        onlyKeys(layer, ((providerGet(layer, "disabledReason") === null) ? ["name", "version", "config"] : ["name", "version", "config", "disabledReason"]), "Codex project layer");
        onlyKeys(name, ["type", "dotCodexFolder"], "Codex project layer name");
        if ((!(providerGet(layer, "disabledReason") === null))) {
          boundedString(providerGet(layer, "disabledReason"), "Codex project layer disabled reason", 4096);
        }
        validateDisabledProjectConfig(layerConfig);
        if ((hostObjectKeys(layerConfig).length > 0)) {
          if ((!(providerGet(layer, "disabledReason") === expectedProjectDisabledReason(contract)))) {
            (() => { throw new $$be$ExceptionInfo("Codex populated project layer lacks its exact structured disabled reason", {}); })();
          }
          (projectWarningRequired = true);
        }
        if ((!(boundedString(providerGet(name, "dotCodexFolder"), "Codex project layer folder", 4096) === join(launchcontract_projectRoot(contract), ".codex")))) {
          (() => { throw new $$be$ExceptionInfo("Codex project layer names an invalid config folder", {}); })();
        }
        null;
      } else {
        if ((type === "user")) {
          mustBeEmptyLayer(layerConfig, "Codex user layer");
          if (((!(providerGet(name, "profile") === null)) || (!(providerGet(name, "file") === resolve(launchcontract_codexHome(contract), "config.toml"))))) {
            (() => { throw new $$be$ExceptionInfo("Codex user layer names the wrong account", {}); })();
          }
          null;
        } else {
          if ((((((type === "system") || (type === "mdm")) || (type === "enterpriseManaged")) || (type === "legacyManagedConfigTomlFromFile")) || (type === "legacyManagedConfigTomlFromMdm"))) {
            mustBeEmptyLayer(layerConfig, $$bc$str("Codex ", type, " layer"));
          } else {
            (() => { throw new $$be$ExceptionInfo($$bc$str("Codex exposed unknown config layer ", type, ""), {}); })();
          }
        }
      }
    }
  }
});
    if (((!(seen.get("sessionFlags") === 1)) || (!(seen.get("user") === 1)))) {
      (() => { throw new $$be$ExceptionInfo("Codex config layer authority is incomplete", {}); })();
    }
    if ((projectWarningRequired && (!projectWarningSeen))) {
      (() => { throw new $$be$ExceptionInfo("Codex tracked project layer lacks its correlated disabled warning", {}); })();
    }
    const expectedFeatures = $$bc$into_value({}, $$bc$conj_value($$bc$conj_value($$bc$into_value($$bc$into_value([], MANAGED__CODEX__ENABLED__FEATURES.map((name) => { const tuple_value_41 = [name, true];
return tuple_value_41; })), MANAGED__CODEX__DISABLED__FEATURES.map((name) => { const tuple_value_42 = [name, false];
return tuple_value_42; })), (() => { const tuple_value_43 = ["network_proxy", (managedcodexnetworkpolicy_networkProxyEnabled(launchcontract_network(contract)) ? Object.assign({}, {}, {[$$bc$property_key("enabled")]: true}, {[$$bc$property_key("domains")]: managedcodexnetworkpolicy_domains(launchcontract_network(contract))}) : false)]; return tuple_value_43; })()), (() => { const tuple_value_44 = ["remote_control", false]; return tuple_value_44; })()));
    exactDiagnosable(providerGet(record(providerGet(config, "features"), "Codex effective feature set"), "network_proxy"), providerGet(expectedFeatures, "network_proxy"), "Codex effective network proxy policy");
    exactNamingKeys(providerGet(config, "features"), expectedFeatures, "Codex effective feature set");
    const sessionMcp = record(providerGet(launchcontract_expectedSessionConfig(contract), "mcp_servers"), "Codex expected MCP session set");
    const expectedEffectiveMcp = Object.fromEntries(hostObjectEntries(sessionMcp).map(($beagle$param$0) => { let name = $beagle$param$0[0]; let raw = $beagle$param$0[1]; const tuple_value_47 = [name, Object.assign({}, {}, record(raw, $$bc$str("Codex expected MCP session ", name)), {[$$bc$property_key("environment_id")]: "local"}, {[$$bc$property_key("tool_timeout_sec")]: null})];
return tuple_value_47; }));
    exactNamingKeys(providerGet(config, "mcp_servers"), expectedEffectiveMcp, "Codex effective MCP set");
    exactNamingKeys(providerGet(config, "projects"), providerGet(launchcontract_expectedSessionConfig(contract), "projects"), "Codex project trust set");
    const sessionShellEnvironmentPolicy = record(providerGet(launchcontract_expectedSessionConfig(contract), "shell_environment_policy"), "Codex expected shell environment policy");
    exactNamingKeys(providerGet(config, "shell_environment_policy"), Object.assign({}, {}, sessionShellEnvironmentPolicy, {[$$bc$property_key("ignore_default_excludes")]: null}, {[$$bc$property_key("exclude")]: null}, {[$$bc$property_key("include_only")]: null}, {[$$bc$property_key("filters")]: null}, {[$$bc$property_key("experimental_use_profile")]: null}), "Codex effective shell environment policy");
    if ((((((((((!(providerGet(config, "project_doc_max_bytes") === 0)) || (!(providerGet(config, "model_provider") === "openai"))) || (!(providerGet(config, "cli_auth_credentials_store") === "file"))) || (!(providerGet(config, "forced_login_method") === "chatgpt"))) || (!(providerGet(config, "sqlite_home") === launchcontract_sqliteHome(contract)))) || (!(providerGet(config, "allow_login_shell") === false))) || (!(providerGet(config, "apps") === null))) || (!(jsonStringify(providerGet(config, "plugins")) === "{}"))) || (!(jsonStringify(providerGet(config, "marketplaces")) === "{}")))) {
      (() => { throw new $$be$ExceptionInfo("Codex effective authority surface is not closed", {}); })();
    }
    return configFingerprint(response);
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

function camelEvent(value) {
  return $$bc$str(value.slice(0, 1).toLowerCase(), value.slice(1));
}

function expectedHookRows() {
  const rows = [];
  const for_break_48 = ({value: false, watches: {}});
  $$bc$eager_seq(hostObjectEntries(expectedManagedCodexHooks())).forEach(($beagle$item) => {
  let event = $beagle$item[0];
  let groups = $beagle$item[1];
  if ((!for_break_48.value)) {
    const for_continue_49 = ({value: false, watches: {}});
    const for_break_50 = ({value: false, watches: {}});
    $$bc$eager_seq(groups).forEach((group) => {
  if ((!for_break_50.value)) {
    const for_continue_51 = ({value: false, watches: {}});
    const for_break_52 = ({value: false, watches: {}});
    $$bc$eager_seq(group.hooks).forEach((hook) => {
  if ((!for_break_52.value)) {
    const for_continue_53 = ({value: false, watches: {}});
    rows.push(ManagedHookRow(camelEvent(event), "command", (() => { const coalesce_value_54 = group.matcher; return ((coalesce_value_54 == null) ? null : coalesce_value_54); })(), hook.command, hook.timeout, "/etc/codex/hooks", "system", true, true, "managed"));
  }
});
    null;
  }
});
    null;
  }
});
  return rows.sort((left, right) => jsonStringify(left).localeCompare(jsonStringify(right)));
}

function validateRequirements(response, contract) {
  const body = record(response, "Codex requirements response");
  const requirements = record(providerGet(body, "requirements"), "Codex requirements");
  if ((((!(providerGet(requirements, "allowManagedHooksOnly") === true)) || (!(providerGet(requirements, "allowRemoteControl") === false))) || ((providerGet(requirements, "managedHookFailureMode") === null) ? (!(launchcontract_installedManagedHookFailureMode(contract) === "block")) : (!(providerGet(requirements, "managedHookFailureMode") === "block"))))) {
    (() => { throw new $$be$ExceptionInfo("Codex requirements do not close managed hooks, failures, and remote control", {}); })();
  }
  exact(providerGet(requirements, "featureRequirements"), Object.assign({}, {}, {[$$bc$property_key("hooks")]: true}), "Codex feature requirements");
  const hooks = record(providerGet(requirements, "hooks"), "Codex managed hook requirements");
  if ((!(providerGet(hooks, "managedDir") === "/etc/codex/hooks"))) {
    (() => { throw new $$be$ExceptionInfo("Codex managed hook requirements name the wrong directory", {}); })();
  }
  return null;
}

function validateHooks(response, cwd) {
  const body = record(response, "Codex hooks/list response");
  if (((!Array.isArray(providerGet(body, "data"))) || (!($$bc$count(providerGet(body, "data")) === 1)))) {
    (() => { throw new $$be$ExceptionInfo("Codex hooks/list returned the wrong cwd cardinality", {}); })();
  }
  const entry = record(providerGet(providerGet(body, "data"), 0), "Codex hook cwd entry");
  if (((((((!(providerGet(entry, "cwd") === cwd)) || (!Array.isArray(providerGet(entry, "hooks")))) || (!Array.isArray(providerGet(entry, "warnings")))) || (!($$bc$count(providerGet(entry, "warnings")) === 0))) || (!Array.isArray(providerGet(entry, "errors")))) || (!($$bc$count(providerGet(entry, "errors")) === 0)))) {
    (() => { throw new $$be$ExceptionInfo($$bc$str("Codex hook inventory invalid: cwd=", jsonStringify(providerGet(entry, "cwd")), " expected=", jsonStringify(cwd), " hooks-array=", Array.isArray(providerGet(entry, "hooks")), " warnings=", jsonStringify(providerGet(entry, "warnings")), " errors=", jsonStringify(providerGet(entry, "errors"))), {}); })();
  }
  const rows = providerGet(entry, "hooks").map((raw) => { const hook = record(raw, "Codex hook metadata");
return ManagedHookRow(providerGet(hook, "eventName"), providerGet(hook, "handlerType"), providerGet(hook, "matcher"), providerGet(hook, "command"), providerGet(hook, "timeoutSec"), providerGet(hook, "sourcePath"), providerGet(hook, "source"), providerGet(hook, "enabled"), providerGet(hook, "isManaged"), providerGet(hook, "trustStatus")); }).sort((left, right) => jsonStringify(left).localeCompare(jsonStringify(right)));
  return exact(rows, expectedHookRows(), "Codex managed hook inventory");
}

function ExpectedMcpServer(name, tools, version) {
  return $$bc$record_value("north.providers.codex-app-server/ExpectedMcpServer", {_tag: "ExpectedMcpServer", name, tools, version});
}

function expectedmcpserver_name(r) { return r.name; }

function expectedmcpserver_tools(r) { return r.tools; }

function expectedmcpserver_version(r) { return r.version; }

function expectedMcpInventory(surface) {
  return Object.freeze([ExpectedMcpServer("north", surface.northEnabledTools, "0.1.0")]);
}

async function validateMcp_bang(...$beagle$args) {
  if (arguments.length === 2) {
    const rpc = $beagle$args[0];
    const expected = $beagle$args[1];
    return validateMcp_bang(rpc, expected, null);
  }
  if (arguments.length === 3) {
    const rpc = $beagle$args[0];
    const expected = $beagle$args[1];
    const threadId = $beagle$args[2];
    const servers = [];
    const cursors = new Set([]);
    let cursor = null;
    let page = 0;
    const for_break_54 = ({value: false, watches: {}});
    await (async () => {  while (true) {
    if ((page < MAX__INVENTORY__PAGES)) { await (async () => { const for_continue_55 = ({value: false, watches: {}}); const response = record(await appserverrpc_request_bang(rpc, "mcpServerStatus/list", Object.assign({}, {}, {[$$bc$property_key("detail")]: "full"}, {[$$bc$property_key("limit")]: 32}, (((_truthy) => _truthy !== false && _truthy != null)(cursor) ? Object.assign({}, {}, {[$$bc$property_key("cursor")]: cursor}) : Object.assign({}, {})), (((_truthy) => _truthy !== false && _truthy != null)(threadId) ? Object.assign({}, {}, {[$$bc$property_key("threadId")]: threadId}) : Object.assign({}, {})))), "Codex MCP inventory");
if ((!Array.isArray(providerGet(response, "data")))) {
  (() => { throw new $$be$ExceptionInfo("Codex MCP inventory data is invalid", {}); })();
}
const for_break_56 = ({value: false, watches: {}});
$$bc$eager_seq(providerGet(response, "data")).forEach((raw) => {
  if ((!for_break_56.value)) {
    const for_continue_57 = ({value: false, watches: {}});
    servers.push(record(raw, "Codex MCP server"));
    if (($$bc$count(servers) > MAX__MCP__SERVERS)) {
      (() => { throw new $$be$ExceptionInfo("Codex MCP inventory is oversized", {}); })();
    }
    null;
  }
});
if ((providerGet(response, "nextCursor") == null)) {
  return (() => { const _a = for_break_54, _v = true; const _old = _a.value; _a.value = _v; for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _v); return _v; })();
} else {
  (cursor = boundedString(providerGet(response, "nextCursor"), "Codex MCP cursor", 4096));
  if (((_truthy) => _truthy !== false && _truthy != null)(cursors.has(cursor))) {
    (() => { throw new $$be$ExceptionInfo("Codex MCP cursor repeated", {}); })();
  }
  cursors.add(cursor);
  if ((page === (MAX__INVENTORY__PAGES - 1))) {
    (() => { throw new $$be$ExceptionInfo("Codex MCP inventory did not terminate", {}); })();
  }
  return null;
} })(); if ((!for_break_54.value)) { (page = (page + 1));  continue; } else { return null; } } else { return null; }
  } })();
    const expectedNames = expected.map((server) => expectedmcpserver_name(server));
    const observed = new Map();
    const for_break_58 = ({value: false, watches: {}});
    $$bc$eager_seq(servers).forEach((server) => {
  if ((!for_break_58.value)) {
    const for_continue_59 = ({value: false, watches: {}});
    const name = boundedString(providerGet(server, "name"), "Codex MCP server name");
    if (((_truthy) => _truthy !== false && _truthy != null)(observed.has(name))) {
      (() => { throw new $$be$ExceptionInfo("Codex MCP inventory repeated a server", {}); })();
    }
    observed.set(name, server);
  }
});
    if (((!(observed.size === expected.length)) || (!(((_pred, _coll) => { if (_coll == null) return null; for (const _item of _coll) { const _value = _pred(_item); if (_value !== false && _value != null) return _value; } return null; })((name) => (!((_truthy) => _truthy !== false && _truthy != null)(observed.has(name))), expectedNames) == null)))) {
      (() => { throw new $$be$ExceptionInfo($$bc$str($$bc$str("Codex MCP inventory is not exactly ", (($beagle$apply$fn, $beagle$apply$tail) => $beagle$apply$fn.call($beagle$apply$fn, ...($beagle$apply$tail ?? [])))(((..._xs) => "".concat(..._xs)), $$bc$interpose("+", expectedNames)), ": "), $$bc$str("observed ", ((_logical) => (_logical !== false && _logical != null ? _logical : "(none)"))((($beagle$apply$fn, $beagle$apply$tail) => $beagle$apply$fn.call($beagle$apply$fn, ...($beagle$apply$tail ?? [])))(((..._xs) => "".concat(..._xs)), $$bc$interpose("+", $$bc$into_value([], observed.keys()).sort()))), "")), {}); })();
    }
    const for_break_60 = ({value: false, watches: {}});
    $$bc$eager_seq(expected).forEach((spec) => {
  if ((!for_break_60.value)) {
    const for_continue_61 = ({value: false, watches: {}});
    const label = $$bc$str("Codex ", expectedmcpserver_name(spec), " MCP server");
    const server = observed.get(expectedmcpserver_name(spec));
    onlyKeys(server, ["name", "serverInfo", "tools", "resources", "resourceTemplates", "authStatus"], label);
    const observedRuntimeStatus = providerGet(server, "runtimeStatus");
    if (((!(observedRuntimeStatus == null)) || (!(providerGet(server, "pluginId") == null)))) {
      (() => { throw new $$be$ExceptionInfo($$bc$str("", label, " unexpectedly carries runtime or plugin authority"), {}); })();
    }
    const identity = record(providerGet(server, "serverInfo"), $$bc$str("", label, " identity"));
    exact(identity, Object.assign({}, {}, {[$$bc$property_key("name")]: expectedmcpserver_name(spec)}, {[$$bc$property_key("title")]: null}, {[$$bc$property_key("version")]: (() => { const coalesce_value_62 = expectedmcpserver_version(spec); return ((coalesce_value_62 == null) ? boundedString(providerGet(identity, "version"), $$bc$str("", label, " version"), 64) : coalesce_value_62); })()}, {[$$bc$property_key("description")]: null}, {[$$bc$property_key("icons")]: null}, {[$$bc$property_key("websiteUrl")]: null}), $$bc$str("", label, " identity"));
    if ((!(providerGet(server, "authStatus") === "unsupported"))) {
      (() => { throw new $$be$ExceptionInfo($$bc$str("", label, " unexpectedly carries authentication authority"), {}); })();
    }
    exact(providerGet(server, "resources"), [], $$bc$str("", label, " resource surface"));
    exact(providerGet(server, "resourceTemplates"), [], $$bc$str("", label, " resource-template surface"));
    const tools = record(providerGet(server, "tools"), $$bc$str("", label, " tools"));
    exact(hostObjectKeys(tools).sort(), $$bc$into_value([], expectedmcpserver_tools(spec)).sort(), $$bc$str("", label, " tool surface"));
  }
});
    return null;
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

function validateAccount(response) {
  const body = record(response, "Codex account/read response");
  const account = record(providerGet(body, "account"), "Codex authenticated account");
  if (((!(providerGet(account, "type") === "chatgpt")) || (!(providerGet(body, "requiresOpenaiAuth") === true)))) {
    (() => { throw new $$be$ExceptionInfo("Codex selected account is not authenticated ChatGPT", {}); })();
  }
  return null;
}

function validateInitialize(response, contract) {
  const initialized = record(response, "Codex initialize response");
  onlyKeys(initialized, ["userAgent", "codexHome", "platformFamily", "platformOs"], "Codex initialize response");
  const expectedPlatformOs = ((process.platform === "darwin") ? "macos" : ((process.platform === "linux") ? "linux" : null));
  const userAgent = ((typeof providerGet(initialized, "userAgent") === "string") ? providerGet(initialized, "userAgent") : "");
  const expectedUserAgent = $$bc$str("north/", MANAGED__CODEX__VERSION, "");
  if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? _logical : ((!(userAgent === expectedUserAgent)) && (!((_truthy) => _truthy !== false && _truthy != null)(userAgent.startsWith($$bc$str("", expectedUserAgent, " ")))))))(((((((!(providerGet(initialized, "codexHome") === launchcontract_codexHome(contract))) || (!((_truthy) => _truthy !== false && _truthy != null)(expectedPlatformOs))) || (!(providerGet(initialized, "platformFamily") === "unix"))) || (!(providerGet(initialized, "platformOs") === expectedPlatformOs))) || (Buffer.byteLength(userAgent, "utf8") > 512)) || /[\u0000-\u001f\u007f]/.test(userAgent))))) {
    (() => { throw new $$be$ExceptionInfo("Codex initialize did not attest the pinned provider runtime", {}); })();
  }
  return null;
}

function expectedSandbox(surface, contract) {
  const network = managedCodexNetworkPolicy(surface);
  return ((surface.sandbox === "read-only") ? Object.assign({}, {}, {[$$bc$property_key("type")]: "readOnly"}, {[$$bc$property_key("networkAccess")]: false}) : Object.assign({}, {}, {[$$bc$property_key("type")]: "workspaceWrite"}, {[$$bc$property_key("writableRoots")]: launchcontract_writableRoots(contract)}, {[$$bc$property_key("networkAccess")]: network.networkAccess}, {[$$bc$property_key("excludeTmpdirEnvVar")]: false}, {[$$bc$property_key("excludeSlashTmp")]: false}));
}

function expectedRuntimeWorkspaceRoots(contract) {
  return $$bc$into_value([], new Set($$bc$into_value($$bc$conj_value([], launchcontract_cwd(contract)), launchcontract_writableRoots(contract)))).sort();
}

function comparableRootList(value) {
  return (((_truthy) => _truthy !== false && _truthy != null)((Array.isArray(value) && (value).every((entry) => (typeof entry === "string")))) ? $$bc$into_value([], value).sort() : value);
}

function validateStartedThread_bang(response, contract, options) {
  const started = response;
  const thread = record(providerGet(started, "thread"), "Codex started thread");
  const threadId = providerThreadId_bang(started, thread, "Codex thread/start response");
  if ((((((((((((((!(providerGet(started, "model") === managedcodexappserveroptions_model(options))) || (!(providerGet(started, "modelProvider") === "openai"))) || (!(providerGet(started, "serviceTier") === null))) || (!(providerGet(started, "cwd") === launchcontract_cwd(contract)))) || (!(providerGet(thread, "ephemeral") === true))) || (!(providerGet(thread, "modelProvider") === "openai"))) || (!(providerGet(thread, "cwd") === launchcontract_cwd(contract)))) || (!(providerGet(thread, "parentThreadId") === null))) || (!(providerGet(started, "approvalPolicy") === "never"))) || (!(providerGet(started, "approvalsReviewer") === "user"))) || (!(providerGet(started, "activePermissionProfile") === null))) || (!(providerGet(started, "reasoningEffort") === (() => { const coalesce_value_63 = managedcodexappserveroptions_effort(options); return ((coalesce_value_63 == null) ? null : coalesce_value_63); })()))) || (!(providerGet(started, "multiAgentMode") === "explicitRequestOnly")))) {
    (() => { throw new $$be$ExceptionInfo("Codex thread/start resolved different execution authority", {}); })();
  }
  exactDiagnosable(comparableRootList(providerGet(started, "runtimeWorkspaceRoots")), expectedRuntimeWorkspaceRoots(contract), "Codex thread runtime workspace roots");
  exactDiagnosable(providerGet(started, "instructionSources"), [resolve(launchcontract_codexHome(contract), "AGENTS.md")], "Codex thread instruction sources");
  exactDiagnosable(providerGet(started, "sandbox"), expectedSandbox(managedcodexappserveroptions_surface(options), contract), "Codex thread sandbox");
  return threadId;
}

function validateStartedTurn(response) {
  const started = response;
  const turn = record(providerGet(started, "turn"), "Codex started turn");
  const turnId = protocolId(providerGet(turn, "id"), "Codex turn id");
  if (((((!(providerGet(turn, "status") === "inProgress")) || (!(providerGet(turn, "error") === null))) || (!Array.isArray(providerGet(turn, "items")))) || (!($$bc$count(providerGet(turn, "items")) === 0)))) {
    (() => { throw new $$be$ExceptionInfo("Codex turn did not start with the exact managed lifecycle", {}); })();
  }
  return turnId;
}

function RuntimeNotificationState(threadId, cwd, model, turnId, hookRuns, text, usage, providerDurationMs, terminalSeen, toolItems, invocationObservations, openItems, mcpActivity, nativeCommands, mcpServerNames) {
  return $$bc$record_value("north.providers.codex-app-server/RuntimeNotificationState", {_tag: "RuntimeNotificationState", threadId, cwd, model, turnId, hookRuns, text, usage, providerDurationMs, terminalSeen, toolItems, invocationObservations, openItems, mcpActivity, nativeCommands, mcpServerNames});
}

function runtimenotificationstate_threadId(r) { return r.threadId; }

function runtimenotificationstate_cwd(r) { return r.cwd; }

function runtimenotificationstate_model(r) { return r.model; }

function runtimenotificationstate_turnId(r) { return r.turnId; }

function runtimenotificationstate_hookRuns(r) { return r.hookRuns; }

function runtimenotificationstate_text(r) { return r.text; }

function runtimenotificationstate_usage(r) { return r.usage; }

function runtimenotificationstate_providerDurationMs(r) { return r.providerDurationMs; }

function runtimenotificationstate_terminalSeen(r) { return r.terminalSeen; }

function runtimenotificationstate_toolItems(r) { return r.toolItems; }

function runtimenotificationstate_invocationObservations(r) { return r.invocationObservations; }

function runtimenotificationstate_openItems(r) { return r.openItems; }

function runtimenotificationstate_mcpActivity(r) { return r.mcpActivity; }

function runtimenotificationstate_nativeCommands(r) { return r.nativeCommands; }

function runtimenotificationstate_mcpServerNames(r) { return r.mcpServerNames; }

function externalInvocationObservation(observation) {
  return $$bh$js_obj("count", managedcodexinvocationobservation_count(observation), "schema", managedcodexinvocationobservation_schema(observation), "hook", managedcodexinvocationobservation_hook(observation), "operation", managedcodexinvocationobservation_operation(observation), "classification", managedcodexinvocationobservation_classification(observation), "decision", managedcodexinvocationobservation_decision(observation));
}

function managedCodexResultValue(state, threadId, turnId, invocationObservations) {
  const usage = runtimenotificationstate_usage(state);
  const result = $$bh$js_obj("text", runtimenotificationstate_text(state), "usage", $$bh$js_obj("input_tokens", usage.input_tokens, "cached_input_tokens", usage.cached_input_tokens, "output_tokens", usage.output_tokens, "reasoning_output_tokens", usage.reasoning_output_tokens), "providerDurationMs", runtimenotificationstate_providerDurationMs(state), "toolItems", runtimenotificationstate_toolItems(state), "providerJoin", providerJoinEvidence("openai", $$bh$js_obj("sessionId", threadId, "turnIds", [turnId], "sessionPersistence", "ephemeral")));
  if (((_truthy) => _truthy !== false && _truthy != null)(invocationObservations)) {
    Reflect.set(result, "invocationObservations", invocationObservations.map(externalInvocationObservation));
  }
  return result;
}

function recordInvocationObservation_bang(state, observation) {
  const key = invocationObservationKey(observation);
  const current = runtimenotificationstate_invocationObservations(state).get(key);
  if (((_truthy) => _truthy !== false && _truthy != null)(current)) {
    if ((managedcodexinvocationobservation_count(current) < Number.MAX_SAFE_INTEGER)) {
      runtimenotificationstate_invocationObservations(state).set(key, Object.freeze(ManagedCodexInvocationObservation((managedcodexinvocationobservation_count(current) + 1), (() => { const spread_value_65 = current; return (providerContains_p(spread_value_65, "schema") ? providerGet(spread_value_65, "schema") : null); })(), (() => { const spread_value_66 = current; return (providerContains_p(spread_value_66, "hook") ? providerGet(spread_value_66, "hook") : null); })(), (() => { const spread_value_67 = current; return (providerContains_p(spread_value_67, "operation") ? providerGet(spread_value_67, "operation") : null); })(), (() => { const spread_value_68 = current; return (providerContains_p(spread_value_68, "classification") ? providerGet(spread_value_68, "classification") : null); })(), (() => { const spread_value_69 = current; return (providerContains_p(spread_value_69, "decision") ? providerGet(spread_value_69, "decision") : null); })())));
    }
    return null;
  } else {
    return runtimenotificationstate_invocationObservations(state).set(key, Object.freeze(ManagedCodexInvocationObservation(1, (() => { const spread_value_71 = observation; return (providerContains_p(spread_value_71, "schema") ? providerGet(spread_value_71, "schema") : null); })(), (() => { const spread_value_72 = observation; return (providerContains_p(spread_value_72, "hook") ? providerGet(spread_value_72, "hook") : null); })(), (() => { const spread_value_73 = observation; return (providerContains_p(spread_value_73, "operation") ? providerGet(spread_value_73, "operation") : null); })(), (() => { const spread_value_74 = observation; return (providerContains_p(spread_value_74, "classification") ? providerGet(spread_value_74, "classification") : null); })(), (() => { const spread_value_75 = observation; return (providerContains_p(spread_value_75, "decision") ? providerGet(spread_value_75, "decision") : null); })())));
  }
}

function invocationObservationInventory(state) {
  return ((runtimenotificationstate_invocationObservations(state).size === 0) ? null : Object.freeze($$bc$into_value([], runtimenotificationstate_invocationObservations(state).values()).sort((left, right) => invocationObservationKey(left).localeCompare(invocationObservationKey(right)))));
}

function pendingItemSnapshot_bang(state) {
  const groups = new Map();
  const for_break_76 = ({value: false, watches: {}});
  $$bc$eager_seq(runtimenotificationstate_openItems(state).values()).forEach(($beagle$item) => {
  let pending = $beagle$item["pending"];
  if ((!for_break_76.value)) {
    const for_continue_77 = ({value: false, watches: {}});
    if ((!((_truthy) => _truthy !== false && _truthy != null)(pending))) {
      (() => { const _a = for_continue_77, _v = true; const _old = _a.value; _a.value = _v; for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _v); return _v; })();
    } else {
      const key = $$bc$str("", pending.kind, "\x00", pending.name, "");
      const current = groups.get(key);
      groups.set(key, Object.freeze(ManagedCodexPendingItemSummary(pending.kind, pending.name, ((() => { const coalesce_value_79 = (() => { const optional_value_78 = current; return ((optional_value_78 == null) ? null : optional_value_78.count); })(); return ((coalesce_value_79 == null) ? 0 : coalesce_value_79); })() + 1))));
    }
  }
});
  const pendingItems = $$bc$into_value([], groups.values()).sort((left, right) => ((managedcodexpendingitemsummary_kind(left) === managedcodexpendingitemsummary_kind(right)) ? ((managedcodexpendingitemsummary_name(left).localeCompare(managedcodexpendingitemsummary_name(right)) < 0) ? (-1) : ((managedcodexpendingitemsummary_name(left).localeCompare(managedcodexpendingitemsummary_name(right)) > 0) ? 1.0 : 0.0)) : ((managedcodexpendingitemsummary_kind(left).localeCompare(managedcodexpendingitemsummary_kind(right)) < 0) ? (-1) : 1.0))).slice(0, MAX__PENDING__ITEM__SUMMARIES);
  const pendingItemCount = $$bc$into_value([], groups.values()).reduce((total, item) => (total + managedcodexpendingitemsummary_count(item)), 0.0);
  return TypeScriptAnonymousObjectV10(pendingItemCount, Object.freeze(pendingItems));
}

function mergePendingItems(left, right) {
  const groups = new Map();
  const for_break_80 = ({value: false, watches: {}});
  $$bc$eager_seq($$bc$into_value($$bc$into_value([], left), right)).forEach((pending) => {
  if ((!for_break_80.value)) {
    const for_continue_81 = ({value: false, watches: {}});
    const key = $$bc$str("", managedcodexpendingitemsummary_kind(pending), "\x00", managedcodexpendingitemsummary_name(pending), "");
    const current = groups.get(key);
    groups.set(key, Object.freeze(ManagedCodexPendingItemSummary(managedcodexpendingitemsummary_kind(pending), managedcodexpendingitemsummary_name(pending), ((() => { const coalesce_value_83 = (() => { const optional_value_82 = current; return ((optional_value_82 == null) ? null : optional_value_82.count); })(); return ((coalesce_value_83 == null) ? 0 : coalesce_value_83); })() + managedcodexpendingitemsummary_count(pending)))));
  }
});
  return Object.freeze($$bc$into_value([], groups.values()).sort((a, b) => ((managedcodexpendingitemsummary_kind(a) === managedcodexpendingitemsummary_kind(b)) ? ((managedcodexpendingitemsummary_name(a).localeCompare(managedcodexpendingitemsummary_name(b)) < 0) ? (-1) : ((managedcodexpendingitemsummary_name(a).localeCompare(managedcodexpendingitemsummary_name(b)) > 0) ? 1.0 : 0.0)) : ((managedcodexpendingitemsummary_kind(a).localeCompare(managedcodexpendingitemsummary_kind(b)) < 0) ? (-1) : 1.0))).slice(0, MAX__PENDING__ITEM__SUMMARIES));
}

function commandText(...$beagle$args) {
  if (arguments.length === 2) {
    const value = $beagle$args[0];
    const label = $beagle$args[1];
    return commandText(value, label, MAX__LINE__BYTES);
  }
  if (arguments.length === 3) {
    const value = $beagle$args[0];
    const label = $beagle$args[1];
    const maxBytes = $beagle$args[2];
    if ((((!(typeof value === "string")) || (value === "")) || (Buffer.byteLength(value, "utf8") > maxBytes))) {
      (() => { throw new $$be$ExceptionInfo($$bc$str("", label, " is invalid"), {}); })();
    }
    return value;
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

function nativeCommandCwd(value, label) {
  const observed = commandText(value, $$bc$str("", label, " cwd"), MAX__CWD__BYTES);
  const segments = observed.split("/");
  return (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (!((_truthy) => _truthy !== false && _truthy != null)(segments.includes(".."))) : _logical))(((_logical) => (_logical !== false && _logical != null ? (!((_truthy) => _truthy !== false && _truthy != null)(segments.includes("."))) : _logical))(((_logical) => (_logical !== false && _logical != null ? (!((_truthy) => _truthy !== false && _truthy != null)(observed.includes("\u0000"))) : _logical))(observed.startsWith("/"))))) ? resolve(observed) : (() => { throw new Error($$bc$str("", label, " cwd is not an absolute traversal-free path"), TypeScriptStructuralObjectV2(new Error($$bc$str($$bc$str("observed=", jsonStringify(observed).slice(0, 600), " "), "expected an absolute filesystem path with no \".\"/\"..\" segments")))); })());
}

function nullableCommandText(value, label) {
  if ((value === null)) {
    return "";
  } else {
    if (((!(typeof value === "string")) || (Buffer.byteLength(value, "utf8") > MAX__LINE__BYTES))) {
      (() => { throw new $$be$ExceptionInfo($$bc$str("", label, " is invalid"), {}); })();
    }
    return value;
  }
}

function validateCommandAction(value) {
  const action = record(value, "Codex command action");
  const type = boundedString(providerGet(action, "type"), "Codex command action type", 32);
  if ((type === "read")) {
    onlyKeys(action, ["type", "command", "name", "path"], "Codex read command action");
    commandText(providerGet(action, "command"), "Codex read command action command");
    commandText(providerGet(action, "name"), "Codex read command action name", 4096);
    commandText(providerGet(action, "path"), "Codex read command action path");
    return null;
  } else {
    if ((type === "listFiles")) {
      onlyKeys(action, ["type", "command", "path"], "Codex list-files command action");
      commandText(providerGet(action, "command"), "Codex list-files command action command");
      if ((!(providerGet(action, "path") === null))) {
        commandText(providerGet(action, "path"), "Codex list-files command action path");
      }
      return null;
    } else {
      if ((type === "search")) {
        onlyKeys(action, ["type", "command", "query", "path"], "Codex search command action");
        commandText(providerGet(action, "command"), "Codex search command action command");
        if ((!(providerGet(action, "query") === null))) {
          commandText(providerGet(action, "query"), "Codex search command action query");
        }
        if ((!(providerGet(action, "path") === null))) {
          commandText(providerGet(action, "path"), "Codex search command action path");
        }
        return null;
      } else {
        if ((type === "unknown")) {
          onlyKeys(action, ["type", "command"], "Codex unknown command action");
          commandText(providerGet(action, "command"), "Codex unknown command action command");
          return null;
        } else {
          return (() => { throw new $$be$ExceptionInfo("Codex command action type is invalid", {}); })();
        }
      }
    }
  }
}

function assertNoPluginProvenance(item, label) {
  if (((!(providerGet(item, "pluginId") === null)) || (!(providerGet(item, "scriptPath") === null)))) {
    (() => { throw new Error($$bc$str("", label, " was attributed to a plugin script"), TypeScriptStructuralObjectV2(new Error($$bc$str($$bc$str("pluginId=", (() => { const optional_call_84 = jsonStringify(providerGet(item, "pluginId")); return ((optional_call_84 == null) ? null : optional_call_84.slice(0, 200)); })(), " "), $$bc$str("scriptPath=", (() => { const optional_call_85 = jsonStringify(providerGet(item, "scriptPath")); return ((optional_call_85 == null) ? null : optional_call_85.slice(0, 200)); })(), ""))))); })();
  }
  return null;
}

function nativeCommandCompletionValue(id, command, cwd, source, status, aggregatedOutput, exitCode, durationMs) {
  return $$bh$js_obj("id", id, "command", command, "cwd", cwd, "source", source, "status", status, "aggregatedOutput", aggregatedOutput, "exitCode", exitCode, "durationMs", durationMs);
}

function completedNativeCommand(item, state) {
  onlyKeys(item, ["id", "type", "command", "cwd", "processId", "source", "status", "commandActions", "aggregatedOutput", "exitCode", "durationMs", "pluginId", "scriptPath"], "Codex completed command execution");
  assertNoPluginProvenance(item, "Codex completed command execution");
  const id = protocolId(providerGet(item, "id"), "Codex completed command execution id");
  if ((!(providerGet(item, "type") === "commandExecution"))) {
    (() => { throw new $$be$ExceptionInfo("Codex completed command execution changed authority", {}); })();
  }
  const cwd = nativeCommandCwd(providerGet(item, "cwd"), "Codex completed command execution");
  const command = commandText(providerGet(item, "command"), "Codex completed command execution command");
  if ((!(providerGet(item, "processId") === null))) {
    protocolId(providerGet(item, "processId"), "Codex command process id");
  }
  const source = String(providerGet(item, "source"));
  if ((!((_truthy) => _truthy !== false && _truthy != null)(["agent", "userShell", "unifiedExecStartup", "unifiedExecInteraction"].includes(source)))) {
    (() => { throw new $$be$ExceptionInfo("Codex completed command execution source is invalid", {}); })();
  }
  const status = String(providerGet(item, "status"));
  if ((!((_truthy) => _truthy !== false && _truthy != null)(["completed", "failed", "declined"].includes(status)))) {
    (() => { throw new $$be$ExceptionInfo("Codex completed command execution status is not terminal", {}); })();
  }
  if (((!Array.isArray(providerGet(item, "commandActions"))) || ($$bc$count(providerGet(item, "commandActions")) > 256))) {
    (() => { throw new $$be$ExceptionInfo("Codex completed command actions are invalid", {}); })();
  }
  const for_break_86 = ({value: false, watches: {}});
  $$bc$eager_seq(providerGet(item, "commandActions")).forEach((action) => {
  if ((!for_break_86.value)) {
    const for_continue_87 = ({value: false, watches: {}});
    validateCommandAction(action);
  }
});
  const aggregatedOutput = nullableCommandText(providerGet(item, "aggregatedOutput"), "Codex completed command execution output");
  if ((((!Number.isSafeInteger(providerGet(item, "exitCode"))) || (providerGet(item, "exitCode") < (-2147483648))) || (providerGet(item, "exitCode") > 2147483647))) {
    (() => { throw new $$be$ExceptionInfo("Codex completed command execution exit code is invalid", {}); })();
  }
  if (((!Number.isSafeInteger(providerGet(item, "durationMs"))) || (providerGet(item, "durationMs") < 0))) {
    (() => { throw new $$be$ExceptionInfo("Codex completed command execution duration is invalid", {}); })();
  }
  if ((!((_truthy) => _truthy !== false && _truthy != null)(runtimenotificationstate_nativeCommands(state).observe(nativeCommandCompletionValue($$bc$str("", runtimenotificationstate_threadId(state), ":", runtimenotificationstate_turnId(state), ":", id, ""), command, cwd, source, status, aggregatedOutput, providerGet(item, "exitCode"), providerGet(item, "durationMs")))))) {
    (() => { throw new $$be$ExceptionInfo("Codex command completion is missing its exact start", {}); })();
  }
  return null;
}

function startedNativeCommand(item, state) {
  onlyKeys(item, ["id", "type", "command", "cwd", "processId", "source", "status", "commandActions", "aggregatedOutput", "exitCode", "durationMs", "pluginId", "scriptPath"], "Codex started command execution");
  assertNoPluginProvenance(item, "Codex started command execution");
  const id = protocolId(providerGet(item, "id"), "Codex started command execution id");
  nativeCommandCwd(providerGet(item, "cwd"), "Codex started command execution");
  if ((((((!(providerGet(item, "type") === "commandExecution")) || (!(providerGet(item, "status") === "inProgress"))) || (!(providerGet(item, "aggregatedOutput") === null))) || (!(providerGet(item, "exitCode") === null))) || (!(providerGet(item, "durationMs") === null)))) {
    (() => { throw new Error("Codex started command execution lifecycle is invalid", TypeScriptStructuralObjectV2(new Error($$bc$str($$bc$str($$bc$str($$bc$str("expected type \"commandExecution\", status \"inProgress\", null ", "aggregatedOutput/exitCode/durationMs; observed "), $$bc$str("type=", jsonStringify(providerGet(item, "type")), " status=", jsonStringify(providerGet(item, "status")), " ")), $$bc$str("aggregatedOutput=", (() => { const optional_call_88 = jsonStringify(providerGet(item, "aggregatedOutput")); return ((optional_call_88 == null) ? null : optional_call_88.slice(0, 200)); })(), " ")), $$bc$str("exitCode=", jsonStringify(providerGet(item, "exitCode")), " durationMs=", jsonStringify(providerGet(item, "durationMs")), ""))))); })();
  }
  commandText(providerGet(item, "command"), "Codex started command execution command");
  if ((!(providerGet(item, "processId") === null))) {
    protocolId(providerGet(item, "processId"), "Codex command process id");
  }
  if ((!((_truthy) => _truthy !== false && _truthy != null)(["agent", "userShell", "unifiedExecStartup", "unifiedExecInteraction"].includes(String(providerGet(item, "source")))))) {
    (() => { throw new $$be$ExceptionInfo("Codex started command execution source is invalid", {}); })();
  }
  if (((!Array.isArray(providerGet(item, "commandActions"))) || ($$bc$count(providerGet(item, "commandActions")) > 256))) {
    (() => { throw new $$be$ExceptionInfo("Codex started command actions are invalid", {}); })();
  }
  const for_break_89 = ({value: false, watches: {}});
  $$bc$eager_seq(providerGet(item, "commandActions")).forEach((action) => {
  if ((!for_break_89.value)) {
    const for_continue_90 = ({value: false, watches: {}});
    validateCommandAction(action);
  }
});
  if ((!((_truthy) => _truthy !== false && _truthy != null)(runtimenotificationstate_nativeCommands(state).start($$bc$str("", runtimenotificationstate_threadId(state), ":", runtimenotificationstate_turnId(state), ":", id, ""))))) {
    (() => { throw new $$be$ExceptionInfo("Codex command start lifecycle is invalid", {}); })();
  }
  return null;
}

function validateMcpStartupNotification_bang(...$beagle$args) {
  if (arguments.length === 3) {
    const value = $beagle$args[0];
    const expectedThreadId = $beagle$args[1];
    const expectedNames = $beagle$args[2];
    return validateMcpStartupNotification_bang(value, expectedThreadId, expectedNames, false);
  }
  if (arguments.length === 4) {
    const value = $beagle$args[0];
    const expectedThreadId = $beagle$args[1];
    const expectedNames = $beagle$args[2];
    const allowPendingThreadId = $beagle$args[3];
    const params = record(value, "Codex MCP startup notification");
    onlyKeys(params, ["threadId", "name", "status", "error", "failureReason"], "Codex MCP startup notification");
    let validThreadId = (providerGet(params, "threadId") === null);
    if ((typeof providerGet(params, "threadId") === "string")) {
      (() => { try {
    protocolId(providerGet(params, "threadId"), "Codex MCP startup thread id");
  return (validThreadId = ((expectedThreadId === null) ? allowPendingThreadId : (providerGet(params, "threadId") === expectedThreadId)));
  } catch (_catch_20) {
    switch ($$bd$catch_dispatch(_catch_20, [$$bd$default_catch])) {
      case 0: {
        const typescript_error = _catch_20;
        return (validThreadId = false);
        break;
      }
    }
  } })();
    }
    if ((((((!validThreadId) || (!((_truthy) => _truthy !== false && _truthy != null)(expectedNames.includes(String(providerGet(params, "name")))))) || (!((_truthy) => _truthy !== false && _truthy != null)(["starting", "ready"].includes(String(providerGet(params, "status")))))) || (!(providerGet(params, "error") === null))) || (!(providerGet(params, "failureReason") === null)))) {
      const expected = ((expectedThreadId === null) ? (allowPendingThreadId ? "null or the pending thread/start protocol id" : "null") : $$bc$str("null or ", jsonStringify(expectedThreadId), ""));
      (() => { throw new $$be$ExceptionInfo($$bc$str($$bc$str($$bc$str($$bc$str("Codex managed MCP startup status is invalid: expected threadId ", expected, ", "), $$bc$str("name ", (($beagle$apply$fn, $beagle$apply$tail) => $beagle$apply$fn.call($beagle$apply$fn, ...($beagle$apply$tail ?? [])))(((..._xs) => "".concat(..._xs)), $$bc$interpose("|", expectedNames.map((name) => jsonStringify(name)))), ", ")), "status \"starting\"|\"ready\", error null, failureReason null; "), $$bc$str("observed ", jsonStringify(canonical(params)), "")), {}); })();
    }
    return params;
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

function validateSafetyBufferingNotification(value, state) {
  const params = record(value, "Codex safety-buffering notification");
  const keys = ["threadId", "turnId", "model", "useCases", "reasons", "showBufferingUi"];
  if (providerContains_p(params, "fasterModel")) {
    keys.push("fasterModel");
  }
  onlyKeys(params, keys, "Codex safety-buffering notification");
  exactRuntimeIds(params, state, "Codex safety-buffering notification");
  if ((!(boundedString(providerGet(params, "model"), "Codex safety-buffering model") === runtimenotificationstate_model(state)))) {
    (() => { throw new $$be$ExceptionInfo("Codex safety-buffering notification changed the active model", {}); })();
  }
  const for_break_91 = ({value: false, watches: {}});
  $$bc$eager_seq((() => { const tuple_value_95 = [(() => { const tuple_value_93 = ["useCases", "use case"]; return tuple_value_93; })(), (() => { const tuple_value_94 = ["reasons", "reason"]; return tuple_value_94; })()]; return tuple_value_95; })()).forEach(($beagle$item) => {
  let key = $beagle$item[0];
  let label = $beagle$item[1];
  if ((!for_break_91.value)) {
    const for_continue_92 = ({value: false, watches: {}});
    const values = providerGet(params, key);
    if (((!Array.isArray(values)) || (values.length > MAX__SAFETY__BUFFERING__VALUES))) {
      (() => { throw new $$be$ExceptionInfo($$bc$str("Codex safety-buffering ", key, " are invalid"), {}); })();
    }
    (values).forEach((entry, index) => boundedString(entry, $$bc$str("Codex safety-buffering ", label, " ", index, ""), MAX__SAFETY__BUFFERING__VALUE__BYTES));
  }
});
  if ((!(typeof providerGet(params, "showBufferingUi") === "boolean"))) {
    (() => { throw new $$be$ExceptionInfo("Codex safety-buffering UI flag is invalid", {}); })();
  }
  if (providerContains_p(params, "fasterModel")) {
    optionalBoundedString(providerGet(params, "fasterModel"), "Codex safety-buffering faster model");
  }
  return null;
}

function validateNotifiedTurn(value, expectedId, expectedStatus, label) {
  const turn = record(value, label);
  const turnId = protocolId(providerGet(turn, "id"), $$bc$str("", label, " id"));
  if ((!(providerGet(turn, "error") === null))) {
    (() => { throw new Error($$bc$str("", label, " reported a provider-side turn error"), TypeScriptStructuralObjectV2(new Error($$bc$str("provider turn error: ", jsonStringify(canonical(providerGet(turn, "error"))).slice(0, 600), "")))); })();
  }
  const invalid = (reasons) => { const named = reasons.filter((reason) => (!(reason === false))).map((reason) => reason);
return ((named.length === 0) ? null : (() => { throw new Error($$bc$str("", label, " is invalid"), TypeScriptStructuralObjectV2(new Error((($beagle$apply$fn, $beagle$apply$tail) => $beagle$apply$fn.call($beagle$apply$fn, ...($beagle$apply$tail ?? [])))(((..._xs) => "".concat(..._xs)), $$bc$interpose(", ", named))))); })()); };
  invalid([((!((_truthy) => _truthy !== false && _truthy != null)(expectedId)) && "no expected turn id"), (((!(expectedId === null)) && (!(turnId === expectedId))) && "turn id is not the started turn"), ((!(providerGet(turn, "status") === expectedStatus)) && $$bc$str("status ", jsonStringify(providerGet(turn, "status")), " is not ", jsonStringify(expectedStatus), "")), ((!Array.isArray(providerGet(turn, "items"))) && "items is not an array"), (((!(providerGet(turn, "itemsView") === "notLoaded")) && (!(providerGet(turn, "itemsView") === "summary"))) && $$bc$str("itemsView ", jsonStringify(providerGet(turn, "itemsView")), "")), (((!Number.isSafeInteger(providerGet(turn, "startedAt"))) || (providerGet(turn, "startedAt") < 0)) && $$bc$str("startedAt ", jsonStringify(providerGet(turn, "startedAt")), ""))]);
  if ((expectedStatus === "inProgress")) {
    invalid([((!(providerGet(turn, "completedAt") === null)) && $$bc$str("completedAt ", jsonStringify(providerGet(turn, "completedAt")), "")), ((!(providerGet(turn, "durationMs") === null)) && $$bc$str("durationMs ", jsonStringify(providerGet(turn, "durationMs")), "")), ((!(providerGet(turn, "items").length === 0)) && $$bc$str("items carries ", providerGet(turn, "items").length, ""))]);
    return null;
  } else {
    return invalid([(((!Number.isSafeInteger(providerGet(turn, "completedAt"))) || (providerGet(turn, "completedAt") < 0)) && $$bc$str("completedAt ", jsonStringify(providerGet(turn, "completedAt")), "")), (((!Number.isSafeInteger(providerGet(turn, "durationMs"))) || (providerGet(turn, "durationMs") < 0)) && $$bc$str("durationMs ", jsonStringify(providerGet(turn, "durationMs")), ""))]);
  }
}

function exactRuntimeIds(...$beagle$args) {
  if (arguments.length === 3) {
    const params = $beagle$args[0];
    const state = $beagle$args[1];
    const label = $beagle$args[2];
    return exactRuntimeIds(params, state, label, true);
  }
  if (arguments.length === 4) {
    const params = $beagle$args[0];
    const state = $beagle$args[1];
    const label = $beagle$args[2];
    const requireTurn = $beagle$args[3];
    if ((!(providerGet(params, "threadId") === runtimenotificationstate_threadId(state)))) {
      (() => { throw new $$be$ExceptionInfo($$bc$str("", label, " belongs to another thread"), {}); })();
    }
    if ((requireTurn && ((!((_truthy) => _truthy !== false && _truthy != null)(runtimenotificationstate_turnId(state))) || (!(providerGet(params, "turnId") === runtimenotificationstate_turnId(state)))))) {
      (() => { throw new $$be$ExceptionInfo($$bc$str("", label, " belongs to another turn"), {}); })();
    }
    return null;
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

function validateHookNotification_bang(method, value, state) {
  const params = record(value, "Codex hook notification");
  onlyKeys(params, ["threadId", "turnId", "run"], "Codex hook notification");
  const run = record(providerGet(params, "run"), "Codex hook run");
  onlyKeys(run, ["id", "eventName", "handlerType", "executionMode", "scope", "sourcePath", "source", "displayOrder", "status", "statusMessage", "startedAt", "completedAt", "durationMs", "entries"], "Codex hook run");
  const id = boundedString(providerGet(run, "id"), "Codex hook run id", 512);
  const allowedEvents = new Set(hostObjectKeys(expectedManagedCodexHooks()).map(camelEvent));
  const eventName = boundedString(providerGet(run, "eventName"), "Codex hook event", 64);
  const threadScoped = (eventName === "sessionStart");
  if ((!(providerGet(params, "threadId") === runtimenotificationstate_threadId(state)))) {
    (() => { throw new $$be$ExceptionInfo("Codex hook belongs to another thread", {}); })();
  }
  if (threadScoped) {
    if ((!(providerGet(params, "turnId") === null))) {
      protocolId(providerGet(params, "turnId"), "Codex session hook turn id");
    }
  } else {
    if (((!((_truthy) => _truthy !== false && _truthy != null)(runtimenotificationstate_turnId(state))) || (!(providerGet(params, "turnId") === runtimenotificationstate_turnId(state))))) {
      (() => { throw new $$be$ExceptionInfo("Codex hook belongs to another turn", {}); })();
    }
  }
  if (((!((_truthy) => _truthy !== false && _truthy != null)(allowedEvents.has(eventName))) || ((!(providerGet(run, "handlerType") === "command")) || ((!(providerGet(run, "executionMode") === "sync")) || ((!(providerGet(run, "scope") === (threadScoped ? "thread" : "turn"))) || ((!(providerGet(run, "sourcePath") === "/etc/codex/hooks")) || ((!(providerGet(run, "source") === "system")) || ((!Number.isSafeInteger(providerGet(run, "displayOrder"))) || ((providerGet(run, "displayOrder") < 0) || ((!Number.isSafeInteger(providerGet(run, "startedAt"))) || ((providerGet(run, "startedAt") < 0) || ((!Array.isArray(providerGet(run, "entries"))) || (providerGet(run, "entries").length > 64))))))))))))) {
    (() => { throw new $$be$ExceptionInfo("Codex hook run provenance is invalid", {}); })();
  }
  let hasNonemptyFeedback = false;
  const observations = [];
  $$bc$eager_seq(providerGet(run, "entries")).forEach((raw) => {
  const entry = record(raw, "Codex hook output entry");
  onlyKeys(entry, ["kind", "text"], "Codex hook output entry");
  if ((!((_truthy) => _truthy !== false && _truthy != null)(["warning", "stop", "feedback", "context", "error"].includes(String(providerGet(entry, "kind")))))) {
    (() => { throw new $$be$ExceptionInfo("Codex hook output kind is invalid", {}); })();
  }
  if (((!(typeof providerGet(entry, "text") === "string")) || (Buffer.byteLength(providerGet(entry, "text"), "utf8") > (64 * 1024)))) {
    (() => { throw new $$be$ExceptionInfo("Codex hook output is invalid", {}); })();
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(((providerGet(entry, "kind") === "feedback") && String(providerGet(entry, "text")).trim()))) {
    (hasNonemptyFeedback = true);
  }
  if ((providerGet(entry, "kind") === "context")) {
    const receipt = parseInvocationObservationReceipt(providerGet(entry, "text"));
    if (((_truthy) => _truthy !== false && _truthy != null)(receipt)) {
      observations.push(providerGet(receipt, "observation"));
    }
  }
});
  if ((method === "hook/started")) {
    if (((_truthy) => _truthy !== false && _truthy != null)(((!(providerGet(run, "status") === "running")) || ((!(providerGet(run, "statusMessage") === null)) || ((!(providerGet(run, "completedAt") === null)) || ((!(providerGet(run, "durationMs") === null)) || ((!(providerGet(run, "entries").length === 0)) || runtimenotificationstate_hookRuns(state).has(id)))))))) {
      (() => { throw new $$be$ExceptionInfo("Codex hook start lifecycle is invalid", {}); })();
    }
    runtimenotificationstate_hookRuns(state).add(id);
    return null;
  } else {
    if ((!((_truthy) => _truthy !== false && _truthy != null)(runtimenotificationstate_hookRuns(state).has(id)))) {
      (() => { throw new $$be$ExceptionInfo("Codex hook completion is missing its start", {}); })();
    }
    runtimenotificationstate_hookRuns(state).delete(id);
    const validCompletion = ((providerGet(run, "status") === "completed") || ((eventName === "preToolUse") && ((providerGet(run, "status") === "blocked") && hasNonemptyFeedback)));
    if (((!validCompletion) || ((providerGet(run, "completedAt") === null) || ((providerGet(run, "durationMs") === null) || ((!Number.isSafeInteger(providerGet(run, "completedAt"))) || ((!Number.isSafeInteger(providerGet(run, "durationMs"))) || ((providerGet(run, "completedAt") < providerGet(run, "startedAt")) || ((providerGet(run, "durationMs") < 0) || (!(providerGet(run, "statusMessage") === null)))))))))) {
      (() => { throw new $$be$ExceptionInfo("Codex managed hook did not complete successfully", {}); })();
    }
    if (((eventName === "preToolUse") && ($$bc$count(observations) === 1))) {
      const observation = observations[0];
      if (((providerGet(observation, "decision") === "deny") === (providerGet(run, "status") === "blocked"))) {
        recordInvocationObservation_bang(state, observation);
      }
    }
    return null;
  }
}

const validateHookNotification = validateHookNotification_bang;

function validateProgressNotificationBase_bang(method, value, state) {
  const params = record(value, $$bc$str("Codex ", method, " notification"));
  if ((method === "thread/started")) {
    onlyKeys(params, ["thread"], "Codex thread/started notification");
    const thread = record(providerGet(params, "thread"), "Codex thread/started thread");
    if ((((((!(providerThreadId_bang(params, thread, "Codex thread/started notification") === runtimenotificationstate_threadId(state))) || (!(providerGet(thread, "ephemeral") === true))) || (!(providerGet(thread, "modelProvider") === "openai"))) || (!(providerGet(thread, "cwd") === runtimenotificationstate_cwd(state)))) || (!(providerGet(thread, "parentThreadId") === null)))) {
      (() => { throw new $$be$ExceptionInfo("Codex thread/started notification changed authority", {}); })();
    }
    return null;
  } else {
    if ((method === "thread/status/changed")) {
      onlyKeys(params, ["threadId", "status"], "Codex thread status notification");
      exactRuntimeIds(params, state, "Codex thread status", false);
      const status = record(providerGet(params, "status"), "Codex thread status");
      if ((!((_truthy) => _truthy !== false && _truthy != null)(["idle", "active"].includes(String(providerGet(status, "type")))))) {
        (() => { throw new $$be$ExceptionInfo("Codex thread entered an invalid managed status", {}); })();
      }
      return null;
    } else {
      if ((method === "turn/started")) {
        onlyKeys(params, ["threadId", "turn"], "Codex turn/started notification");
        if ((!(providerGet(params, "threadId") === runtimenotificationstate_threadId(state)))) {
          (() => { throw new $$be$ExceptionInfo("Codex turn belongs to another thread", {}); })();
        }
        validateNotifiedTurn(providerGet(params, "turn"), runtimenotificationstate_turnId(state), "inProgress", "Codex turn/started notification");
        return null;
      } else {
        if ((method === "thread/tokenUsage/updated")) {
          (state.usage = usageFromNotification(params, runtimenotificationstate_threadId(state), runtimenotificationstate_turnId(state)));
          return null;
        } else {
          if (((method === "item/started") || (method === "item/completed"))) {
            onlyKeys(params, ["item", "threadId", "turnId", ((method === "item/started") ? "startedAtMs" : "completedAtMs")], $$bc$str("Codex ", method, ""));
            exactRuntimeIds(params, state, $$bc$str("Codex ", method, ""));
            const timestamp = providerGet(params, ((method === "item/started") ? "startedAtMs" : "completedAtMs"));
            if (((!Number.isSafeInteger(timestamp)) || (timestamp < 0))) {
              (() => { throw new $$be$ExceptionInfo($$bc$str("Codex ", method, " timestamp is invalid"), {}); })();
            }
            const item = record(providerGet(params, "item"), $$bc$str("Codex ", method, " item"));
            const itemId = protocolId(providerGet(item, "id"), $$bc$str("Codex ", method, " item id"));
            const itemType = boundedString(providerGet(item, "type"), $$bc$str("Codex ", method, " item type"), 128);
            const started = ((method === "item/started") ? null : runtimenotificationstate_openItems(state).get(itemId));
            if ((method === "item/started")) {
              let pending = null;
              if ((!((_truthy) => _truthy !== false && _truthy != null)(openAIWireItemIsPassive(itemType)))) {
                (() => { try {
    return (pending = (() => { const coalesce_value_96 = openAIWireToolIdentity(publicJsValue($$bh$clj_to_js(item))); return ((coalesce_value_96 == null) ? Object.freeze(TypeScriptAnonymousObjectV9("unknown", "provider-item")) : coalesce_value_96); })());
  } catch (_catch_21) {
    switch ($$bd$catch_dispatch(_catch_21, [$$bd$default_catch])) {
      case 0: {
        const typescript_error = _catch_21;
        return (pending = Object.freeze(TypeScriptAnonymousObjectV9("unknown", "provider-item")));
        break;
      }
    }
  } })();
              }
              return runtimenotificationstate_openItems(state).set(itemId, TypeScriptAnonymousObjectV8(itemType, Date.now(), pending, timestamp));
            } else {
              if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (!(started.kind === itemType)) : _logical))(started))) {
                (() => { throw new $$be$ExceptionInfo("Codex item completion changed its started kind", {}); })();
              }
              return null;
            }
          } else {
            if ((method === "turn/completed")) {
              onlyKeys(params, ["threadId", "turn"], "Codex turn completion");
              if (runtimenotificationstate_terminalSeen(state)) {
                (() => { throw new $$be$ExceptionInfo("Codex emitted multiple turn terminals", {}); })();
              }
              if ((!(providerGet(params, "threadId") === runtimenotificationstate_threadId(state)))) {
                (() => { throw new $$be$ExceptionInfo("Codex turn terminal is for another thread", {}); })();
              }
              validateNotifiedTurn(providerGet(params, "turn"), runtimenotificationstate_turnId(state), "completed", "Codex completed turn");
              const completedTurn = record(providerGet(params, "turn"), "Codex completed turn");
              (state.providerDurationMs = providerGet(completedTurn, "durationMs"));
              if ((!(runtimenotificationstate_hookRuns(state).size === 0))) {
                (() => { throw new $$be$ExceptionInfo("Codex turn completed with unfinished managed hooks", {}); })();
              }
              (state.terminalSeen = true);
              return null;
            } else {
              if (((method === "hook/started") || (method === "hook/completed"))) {
                validateHookNotification(method, value, state);
                return null;
              } else {
                if ((method === "account/rateLimits/updated")) {
                  onlyKeys(params, ["rateLimits"], "Codex rate-limit notification");
                  record(providerGet(params, "rateLimits"), "Codex rate-limit snapshot");
                  return null;
                } else {
                  if ((method === "mcpServer/startupStatus/updated")) {
                    validateMcpStartupNotification_bang(value, runtimenotificationstate_threadId(state), runtimenotificationstate_mcpServerNames(state));
                    return null;
                  } else {
                    if ((method === "model/safetyBuffering/updated")) {
                      validateSafetyBufferingNotification(value, state);
                      return null;
                    } else {
                      const deltaMethods = new Set(["item/agentMessage/delta", "item/plan/delta", "item/reasoning/summaryTextDelta", "item/reasoning/textDelta", "item/commandExecution/outputDelta", "item/fileChange/outputDelta"]);
                      if (((_truthy) => _truthy !== false && _truthy != null)(deltaMethods.has(method))) {
                        const keys = ["threadId", "turnId", "itemId", "delta"];
                        if ((method === "item/reasoning/summaryTextDelta")) {
                          keys.push("summaryIndex");
                        }
                        if ((method === "item/reasoning/textDelta")) {
                          keys.push("contentIndex");
                        }
                        onlyKeys(params, keys, $$bc$str("Codex ", method, ""));
                        exactRuntimeIds(params, state, $$bc$str("Codex ", method, ""));
                        protocolId(providerGet(params, "itemId"), $$bc$str("Codex ", method, " item id"));
                        if ((!(typeof providerGet(params, "delta") === "string"))) {
                          (() => { throw new $$be$ExceptionInfo($$bc$str("Codex ", method, " delta is invalid"), {}); })();
                        }
                        const for_break_101 = ({value: false, watches: {}});
                        $$bc$eager_seq(["summaryIndex", "contentIndex"]).forEach((key) => {
  if ((!for_break_101.value)) {
    const for_continue_102 = ({value: false, watches: {}});
    if (((($beagle$jst$receiver, $beagle$jst$key) => ($beagle$jst$key in $beagle$jst$receiver))(params, key) && ((!Number.isSafeInteger(providerGet(params, key))) || (providerGet(params, key) < 0)))) {
      (() => { throw new $$be$ExceptionInfo($$bc$str("Codex ", method, " index is invalid"), {}); })();
    }
    null;
  }
});
                        return null;
                      } else {
                        if ((method === "item/reasoning/summaryPartAdded")) {
                          onlyKeys(params, ["threadId", "turnId", "itemId", "summaryIndex"], $$bc$str("Codex ", method, ""));
                          exactRuntimeIds(params, state, $$bc$str("Codex ", method, ""));
                          protocolId(providerGet(params, "itemId"), $$bc$str("Codex ", method, " item id"));
                          if (((!Number.isSafeInteger(providerGet(params, "summaryIndex"))) || (providerGet(params, "summaryIndex") < 0))) {
                            (() => { throw new $$be$ExceptionInfo("Codex reasoning summary index is invalid", {}); })();
                          }
                          return null;
                        } else {
                          if ((method === "item/commandExecution/terminalInteraction")) {
                            onlyKeys(params, ["threadId", "turnId", "itemId", "processId", "stdin"], $$bc$str("Codex ", method, ""));
                            exactRuntimeIds(params, state, $$bc$str("Codex ", method, ""));
                            protocolId(providerGet(params, "itemId"), $$bc$str("Codex ", method, " item id"));
                            protocolId(providerGet(params, "processId"), $$bc$str("Codex ", method, " process id"));
                            if ((!(typeof providerGet(params, "stdin") === "string"))) {
                              (() => { throw new $$be$ExceptionInfo("Codex terminal interaction is invalid", {}); })();
                            }
                            return null;
                          } else {
                            if ((method === "item/fileChange/patchUpdated")) {
                              onlyKeys(params, ["threadId", "turnId", "itemId", "changes"], $$bc$str("Codex ", method, ""));
                              exactRuntimeIds(params, state, $$bc$str("Codex ", method, ""));
                              protocolId(providerGet(params, "itemId"), $$bc$str("Codex ", method, " item id"));
                              if ((!Array.isArray(providerGet(params, "changes")))) {
                                (() => { throw new $$be$ExceptionInfo("Codex file patch changes are invalid", {}); })();
                              }
                              return null;
                            } else {
                              if ((method === "item/mcpToolCall/progress")) {
                                onlyKeys(params, ["threadId", "turnId", "itemId", "message"], $$bc$str("Codex ", method, ""));
                                exactRuntimeIds(params, state, $$bc$str("Codex ", method, ""));
                                protocolId(providerGet(params, "itemId"), $$bc$str("Codex ", method, " item id"));
                                boundedString(providerGet(params, "message"), $$bc$str("Codex ", method, " message"), (64 * 1024));
                                return null;
                              } else {
                                if ((method === "turn/diff/updated")) {
                                  onlyKeys(params, ["threadId", "turnId", "diff"], $$bc$str("Codex ", method, ""));
                                  exactRuntimeIds(params, state, $$bc$str("Codex ", method, ""));
                                  if ((!(typeof providerGet(params, "diff") === "string"))) {
                                    (() => { throw new $$be$ExceptionInfo("Codex turn diff is invalid", {}); })();
                                  }
                                  return null;
                                } else {
                                  if ((method === "turn/plan/updated")) {
                                    onlyKeys(params, ["threadId", "turnId", "explanation", "plan"], $$bc$str("Codex ", method, ""));
                                    exactRuntimeIds(params, state, $$bc$str("Codex ", method, ""));
                                    if ((((!(providerGet(params, "explanation") === null)) && (!(typeof providerGet(params, "explanation") === "string"))) || (!Array.isArray(providerGet(params, "plan"))))) {
                                      (() => { throw new $$be$ExceptionInfo("Codex turn plan is invalid", {}); })();
                                    }
                                    return null;
                                  } else {
                                    return (() => { throw new $$be$ExceptionInfo($$bc$str("managed Codex emitted unsupported notification ", method, ""), {}); })();
                                  }
                                }
                              }
                            }
                          }
                        }
                      }
                    }
                  }
                }
              }
            }
          }
        }
      }
    }
  }
}

function projectRuntimeItemEffects_bang(method, value, state) {
  const params = record(value, $$bc$str("Codex ", method, " notification"));
  const item = record(providerGet(params, "item"), $$bc$str("Codex ", method, " item"));
  const itemId = protocolId(providerGet(item, "id"), $$bc$str("Codex ", method, " item id"));
  const itemType = boundedString(providerGet(item, "type"), $$bc$str("Codex ", method, " item type"), 128);
  const completed = (method === "item/completed");
  const started = (completed ? runtimenotificationstate_openItems(state).get(itemId) : null);
  const timestamp = providerGet(params, (completed ? "completedAtMs" : "startedAtMs"));
  if (((_truthy) => _truthy !== false && _truthy != null)((completed && openAIWireCountsAsToolItem(itemType)))) {
    (state.toolItems = (runtimenotificationstate_toolItems(state) + 1));
  }
  if (((!completed) && (itemType === "commandExecution"))) {
    startedNativeCommand(item, state);
  }
  if ((completed && (itemType === "agentMessage"))) {
    if ((!(typeof providerGet(item, "text") === "string"))) {
      (() => { throw new $$be$ExceptionInfo("Codex agent message text is invalid", {}); })();
    }
    (state.text = providerGet(item, "text"));
  }
  if ((completed && (itemType === "mcpToolCall"))) {
    runtimenotificationstate_mcpActivity(state).observe($$bc$str(runtimenotificationstate_threadId(state), ":", runtimenotificationstate_turnId(state), ":", itemId), normalizeCodexMcpIdentity(providerGet(item, "server"), providerGet(item, "tool")), mcpReceiptMetadata(providerGet(item, "arguments"), providerGet(item, "result"), (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (started.kind === "mcpToolCall") : _logical))(started)) ? (timestamp - started.startedAtMs) : null)));
  }
  if ((completed && (itemType === "commandExecution"))) {
    completedNativeCommand(item, state);
  }
  const result = (completed ? itemId : null);
  return result;
}

function validateProgressNotification_bang(method, value, state) {
  if (((method === "item/started") || (method === "item/completed"))) {
    validateProgressNotificationBase_bang(method, value, state);
    return projectRuntimeItemEffects_bang(method, value, state);
  } else {
    return validateProgressNotificationBase_bang(method, value, state);
  }
}

function providerExecutionActivityKind(method, value) {
  if ((method === "turn/started")) {
    return "provider.codex.turn.started";
  } else {
    if ((method === "turn/completed")) {
      return "provider.codex.turn.completed";
    } else {
      if ((method === "item/started")) {
        return "provider.codex.item.started";
      } else {
        if ((method === "item/completed")) {
          return "provider.codex.item.completed";
        } else {
          if (((_truthy) => _truthy !== false && _truthy != null)(["item/agentMessage/delta", "item/plan/delta", "item/reasoning/summaryTextDelta", "item/reasoning/textDelta", "item/commandExecution/outputDelta", "item/fileChange/outputDelta"].includes(method))) {
            const params = record(value, $$bc$str("Codex ", method, " activity"));
            return (((typeof providerGet(params, "delta") === "string") && ($$bc$count(providerGet(params, "delta")) > 0)) ? "provider.codex.item.delta" : null);
          } else {
            if ((method === "item/commandExecution/terminalInteraction")) {
              return "provider.codex.command.interaction";
            } else {
              if ((method === "item/fileChange/patchUpdated")) {
                const params = record(value, "Codex file patch activity");
                return ((Array.isArray(providerGet(params, "changes")) && ($$bc$count(providerGet(params, "changes")) > 0)) ? "provider.codex.file.patch" : null);
              } else {
                if ((method === "item/mcpToolCall/progress")) {
                  return "provider.codex.mcp.progress";
                } else {
                  if ((method === "turn/diff/updated")) {
                    const params = record(value, "Codex turn diff activity");
                    return (((typeof providerGet(params, "diff") === "string") && ($$bc$count(providerGet(params, "diff")) > 0)) ? "provider.codex.turn.diff" : null);
                  } else {
                    if ((method === "turn/plan/updated")) {
                      const params = record(value, "Codex turn plan activity");
                      return ((((typeof providerGet(params, "explanation") === "string") && ($$bc$count(providerGet(params, "explanation")) > 0)) || (Array.isArray(providerGet(params, "plan")) && ($$bc$count(providerGet(params, "plan")) > 0))) ? "provider.codex.turn.plan" : null);
                    } else {
                      return null;
                    }
                  }
                }
              }
            }
          }
        }
      }
    }
  }
}

function usageFromNotification(value, threadId, turnId) {
  const params = record(value, "Codex token usage notification");
  if (((!(providerGet(params, "threadId") === threadId)) || (!(providerGet(params, "turnId") === turnId)))) {
    (() => { throw new $$be$ExceptionInfo("Codex token usage belongs to another turn", {}); })();
  }
  const tokenUsage = record(providerGet(params, "tokenUsage"), "Codex token usage");
  const total = record(providerGet(tokenUsage, "total"), "Codex cumulative token usage");
  const counter = (name) => { const number = providerGet(total, name);
if ((((!(typeof number === "number")) || (!Number.isSafeInteger(number))) || (number < 0))) {
  (() => { throw new $$be$ExceptionInfo($$bc$str("Codex token usage ", name, " is invalid"), {}); })();
}
return number; };
  const result = TypeScriptAnonymousObjectV1(counter("cachedInputTokens"), counter("inputTokens"), counter("outputTokens"), counter("reasoningOutputTokens"));
  if ((((!(counter("totalTokens") === (result.input_tokens + result.output_tokens))) || (result.cached_input_tokens > result.input_tokens)) || (result.reasoning_output_tokens > result.output_tokens))) {
    (() => { throw new $$be$ExceptionInfo("Codex cumulative token usage is incoherent", {}); })();
  }
  return result;
}

function SupervisorStatusChannel(failure, settled, stderrTail, exitCode, close) {
  return $$bc$record_value("north.providers.codex-app-server/SupervisorStatusChannel", {_tag: "SupervisorStatusChannel", failure, settled, stderrTail, exitCode, close});
}

function supervisorstatuschannel_failure(r) { return r.failure; }

function supervisorstatuschannel_settled(r) { return r.settled; }

function supervisorstatuschannel_stderrTail(r) { return r.stderrTail; }

function supervisorstatuschannel_exitCode(r) { return r.exitCode; }

function supervisorstatuschannel_close(r) { return r.close; }

function supervisorStatusChannel_bang(child) {
  const status = child.stderr;
  if ((!((_truthy) => _truthy !== false && _truthy != null)(status))) {
    const absent = (() => { const rejected = Promise.withResolvers(); rejected.reject(new Error("Codex supervisor status pipe is absent"));
return rejected.promise; })();
    absent.catch(() => null);
    null;
    return SupervisorStatusChannel(absent, () => null, function(...$beagle$args) {
  if (arguments.length === 0) {
    return [];
  }
  if (arguments.length === 1) {
    const count = $beagle$args[0];
    return [];
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}, () => null, () => null);
  } else {
    const messages = new StrictJsonlMessages(strictJsonlLimits("Codex supervisor", SUPERVISOR__STATUS__MAX__LINE__BYTES, SUPERVISOR__STATUS__MAX__TOTAL__BYTES, SUPERVISOR__STATUS__MAX__MESSAGES));
    const ring = new ProviderStderrRing();
    let preflight = true;
    let closed = false;
    let observedExit = null;
    let malformedNoted = false;
    const failureDeferred = Promise.withResolvers();
    const failure = failureDeferred.promise;
    failure.catch(() => null);
    null;
    const failPreflight = (error) => { if ((!preflight)) {
  return null;
} else {
  (preflight = false);
  return failureDeferred.reject(error);
} };
    const onLine = (line) => { const statusLine = (((_truthy) => _truthy !== false && _truthy != null)(line.startsWith(CODEX__SUPERVISOR__STATUS__PREFIX)) ? line.slice($$bc$count(CODEX__SUPERVISOR__STATUS__PREFIX)) : null);
const forwarded = ((statusLine === null) ? null : codexSupervisorStderrLine(statusLine));
if ((forwarded != null)) {
  ring.add(forwarded);
  return null;
} else {
  if ((statusLine === "STARTED")) {
    return null;
  } else {
    if ((statusLine === "UNAVAILABLE")) {
      failPreflight(new Error("Codex executable unavailable"));
      return null;
    } else {
      const exit = ((statusLine === null) ? null : /^EXIT (0|[1-9][0-9]{0,2})$/.exec(statusLine));
      const code = (((_truthy) => _truthy !== false && _truthy != null)(exit) ? Number(providerGet(exit, 1)) : NaN);
      if ((Number.isInteger(code) && (code <= 255))) {
        if ((observedExit == null)) {
          (observedExit = code);
        } else {
          observedExit;
        }
        failPreflight(new Error($$bc$str("Codex supervisor exited before authority preflight (exit ", code, ")")));
        return null;
      } else {
        failPreflight(new Error("Codex supervisor emitted invalid start receipt"));
        if ((!malformedNoted)) {
          (malformedNoted = true);
          ring.add("<supervisor emitted an invalid status message>");
        }
        return null;
      }
    }
  }
} };
    const onData = (chunk) => (closed ? null : (() => { try {
    const for_break_103 = ({value: false, watches: {}});
  (() => { $$bc$eager_seq(messages.push(chunk)).forEach((line) => {
  if ((!for_break_103.value)) {
    const for_continue_104 = ({value: false, watches: {}});
    onLine(line);
  }
}); })();
  return null;
  } catch (_catch_22) {
    switch ($$bd$catch_dispatch(_catch_22, [$$bd$default_catch])) {
      case 0: {
        const error = _catch_22;
        (closed = true);
        failPreflight(error);
        ring.add($$bc$str("<supervisor status channel bound exceeded: ", (error).message, ">"));
        return null;
        break;
      }
    }
  } })());
    const onEnd = () => failPreflight(new Error("Codex supervisor closed before authority preflight"));
    status.on("data", onData);
    null;
    status.on("end", onEnd);
    null;
    status.on("error", onEnd);
    null;
    return SupervisorStatusChannel(failure, () => (preflight = false), function(...$beagle$args) {
  if (arguments.length === 0) {
    return ring.tail(STDERR__TAIL__LINES);
  }
  if (arguments.length === 1) {
    const count = $beagle$args[0];
    return ring.tail(count);
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}, () => observedExit, () => { (closed = true);
(preflight = false);
status.removeListener("data", onData);
status.removeListener("end", onEnd);
status.removeListener("error", onEnd);
return (() => { try {
    status.resume();
  return null;
  } catch (_catch_23) {
    switch ($$bd$catch_dispatch(_catch_23, [$$bd$default_catch])) {
      case 0: {
        const typescript_error = _catch_23;
        return null;
        break;
      }
    }
  } })(); });
  }
}

function destroyManagedProcessStreams_bang(child) {
  $$bc$eager_seq([child.stdin, child.stdout, child.stderr]).forEach((stream) => {
  if (((_truthy) => _truthy !== false && _truthy != null)(stream)) {
    (() => { try {
    return stream.destroy();
  } catch (_catch_24) {
    switch ($$bd$catch_dispatch(_catch_24, [$$bd$default_catch])) {
      case 0: {
        const typescript_error = _catch_24;
        return null;
        break;
      }
    }
  } })();
  }
});
  return null;
}

async function closeProcess_bang(...$beagle$args) {
  if (arguments.length === 1) {
    const child = $beagle$args[0];
    return closeProcess_bang(child, null, null);
  }
  if (arguments.length === 2) {
    const child = $beagle$args[0];
    const rpc = $beagle$args[1];
    return closeProcess_bang(child, rpc, null);
  }
  if (arguments.length === 3) {
    const child = $beagle$args[0];
    const rpc = $beagle$args[1];
    const control = $beagle$args[2];
    await (() => { const active = rpc; return ((active == null) ? Promise.resolve() : appserverrpc_drainInbound(active)); })();
    const activeRpc = rpc;
    if (((_truthy) => _truthy !== false && _truthy != null)(activeRpc)) {
      appserverrpc_markClosing_bang(activeRpc);
    }
    const activeControl = control;
    if (((_truthy) => _truthy !== false && _truthy != null)(activeControl)) {
      activeControl.close();
    }
    const closedDeferred = Promise.withResolvers();
    onceManagedEvent_bang(child, "close", () => closedDeferred.resolve(true));
    (() => { try {
    return child.stdin.end();
  } catch (_catch_25) {
    switch ($$bd$catch_dispatch(_catch_25, [$$bd$default_catch])) {
      case 0: {
        const typescript_error = _catch_25;
        return null;
        break;
      }
    }
  } })();
    if (((!(child.exitCode === null)) || (!(child.signalCode === null)))) {
      destroyManagedProcessStreams_bang(child);
      return await Promise.resolve();
    } else {
      const closed = closedDeferred.promise;
      const settled = await Promise.race((() => { const tuple_value = [closed, Bun.sleep(2280).then(() => false)]; return tuple_value; })());
      let reaped = settled;
      if ((!settled)) {
        (() => { try {
    return child.kill("SIGKILL");
  } catch (_catch_26) {
    switch ($$bd$catch_dispatch(_catch_26, [$$bd$default_catch])) {
      case 0: {
        const typescript_error = _catch_26;
        return null;
        break;
      }
    }
  } })();
        (reaped = await Promise.race((() => { const tuple_value = [closed, Bun.sleep(750).then(() => false)]; return tuple_value; })()));
      }
      destroyManagedProcessStreams_bang(child);
      if ((!reaped)) {
        (() => { throw new Error("managed Codex supervisor exceeded its teardown bound"); })();
      }
      return await Promise.resolve();
    }
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

function awaitChildSpawn_bang(child, timeoutMs) {
  if ((!(child.pid === null))) {
    return Promise.resolve();
  } else {
    const deferred = Promise.withResolvers();
    const timer = setTimeout(() => deferred.reject(new Error("managed Codex process spawn timed out")), timeoutMs);
    onceManagedEvent_bang(child, "spawn", () => { clearTimeout(timer);
return deferred.resolve(); });
    onceManagedEvent_bang(child, "error", () => { clearTimeout(timer);
return deferred.reject(new Error("managed Codex process unavailable")); });
    return deferred.promise;
  }
}

function boundedMs(...$beagle$args) {
  if (arguments.length === 2) {
    const name = $beagle$args[0];
    const fallback = $beagle$args[1];
    return boundedMs(name, fallback, null);
  }
  if (arguments.length === 3) {
    const name = $beagle$args[0];
    const fallback = $beagle$args[1];
    const override = $beagle$args[2];
    const candidate = (() => { const coalesce_value_108 = override; return ((coalesce_value_108 == null) ? Number(foreign_string_property(typescriptprocessv1_env(process), name)) : coalesce_value_108); })();
    return ((Number.isSafeInteger(candidate) && (candidate > 0)) ? candidate : fallback);
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

function boundedRespawns(...$beagle$args) {
  if (arguments.length === 0) {
    return boundedRespawns(null);
  }
  if (arguments.length === 1) {
    const override = $beagle$args[0];
    if ((!(override == null))) {
      return ((Number.isSafeInteger(override) && (override >= 0)) ? override : MAX__RESPAWNS);
    } else {
      const configured = foreign_string_property(typescriptprocessv1_env(process), "NORTH_CODEX_MAX_RESPAWNS");
      if ((configured == null)) {
        return MAX__RESPAWNS;
      } else {
        const candidate = Number(configured);
        return ((Number.isSafeInteger(candidate) && (candidate >= 0)) ? candidate : MAX__RESPAWNS);
      }
    }
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

function clip(value, maxBytes) {
  if ((Buffer.byteLength(value, "utf8") <= maxBytes)) {
    return value;
  } else {
    const bytes = Buffer.from(value, "utf8");
    const kept = bytes.subarray(0, maxBytes).toString("utf8");
    return $$bc$str("", kept, "\n… (truncated)");
  }
}

function managedCodexRecoveredContext(brief, completedTurnTexts, harvest) {
  const parts = [brief, "", "=== recovered context from a crashed provider session ===", $$bc$str($$bc$str($$bc$str("The provider process running this lane died and North started a new one.", " Nothing below was re-executed: it is a record of work YOUR OWN earlier"), " turns already performed in this same working tree. Verify it on disk"), " before redoing it, then continue the brief above from there."), "recovery cause: provider_process_died", $$bc$str("completed turns before the crash: ", completedTurnTexts.length, "")];
  completedTurnTexts.forEach((text, index) => ((text.trim() === "") ? null : parts.push($$bc$str("--- your result from recovered turn ", (index + 1), " ---"), clip(text, MAX__RECOVERED__TEXT__BYTES))));
  if ((!(managedcodexharvest_text(harvest).trim() === ""))) {
    parts.push("--- partial output of the turn that was interrupted by the crash ---", clip(managedcodexharvest_text(harvest), MAX__RECOVERED__TEXT__BYTES));
  }
  const tools = [];
  const commands = (() => { const coalesce_value_110 = managedcodexharvest_nativeCommands(harvest).totalCommands; return ((coalesce_value_110 == null) ? 0 : coalesce_value_110); })();
  if ((!(commands === 0))) {
    tools.push($$bc$str($$bc$str($$bc$str("", commands, " native command(s)"), $$bc$str(" (", (() => { const coalesce_value_111 = managedcodexharvest_nativeCommands(harvest).successfulCommands; return ((coalesce_value_111 == null) ? 0 : coalesce_value_111); })(), " succeeded,")), $$bc$str(" ", (() => { const coalesce_value_112 = managedcodexharvest_nativeCommands(harvest).failedCommands; return ((coalesce_value_112 == null) ? 0 : coalesce_value_112); })(), " failed)")));
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(managedcodexharvest_mcp(harvest).totalCalls)) {
    tools.push($$bc$str($$bc$str("", managedcodexharvest_mcp(harvest).totalCalls, " MCP tool call(s): "), (($beagle$apply$fn, $beagle$apply$tail) => $beagle$apply$fn.call($beagle$apply$fn, ...($beagle$apply$tail ?? [])))(((..._xs) => "".concat(..._xs)), $$bc$interpose(", ", managedcodexharvest_mcp(harvest).tools.map((tool) => $$bc$str("", tool.server, "/", tool.tool, "×", tool.count, ""))))));
  }
  if ((!((() => { const harvestedToolItems = managedcodexharvest_toolItems(harvest); return ((harvestedToolItems == null) ? 0 : harvestedToolItems); })() === 0))) {
    tools.push($$bc$str("", managedcodexharvest_toolItems(harvest), " completed work item(s)"));
  }
  parts.push("--- tool work observed before the crash ---", ((!($$bc$count(tools) === 0)) ? (($beagle$apply$fn, $beagle$apply$tail) => $beagle$apply$fn.call($beagle$apply$fn, ...($beagle$apply$tail ?? [])))(((..._xs) => "".concat(..._xs)), $$bc$interpose("; ", tools)) : "none observed"));
  if (((() => { const coalesce_value_113 = managedcodexharvest_pendingItemCount(harvest); return ((coalesce_value_113 == null) ? 0 : coalesce_value_113); })() > 0)) {
    const summaries = (($beagle$apply$fn, $beagle$apply$tail) => $beagle$apply$fn.call($beagle$apply$fn, ...($beagle$apply$tail ?? [])))(((..._xs) => "".concat(..._xs)), $$bc$interpose(", ", (() => { const coalesce_value_114 = managedcodexharvest_pendingItems(harvest); return ((coalesce_value_114 == null) ? [] : coalesce_value_114); })().map((item) => $$bc$str("", managedcodexpendingitemsummary_kind(item), "/", managedcodexpendingitemsummary_name(item), "×", managedcodexpendingitemsummary_count(item), ""))));
    const summarizedCount = ((() => { const coalesce_value_115 = managedcodexharvest_pendingItems(harvest); return ((coalesce_value_115 == null) ? [] : coalesce_value_115); })()).reduce((total, item) => (total + managedcodexpendingitemsummary_count(item)), 0.0);
    const omitted = (managedcodexharvest_pendingItemCount(harvest) - summarizedCount);
    parts.push("--- provider items with no observed completion ---", $$bc$str($$bc$str($$bc$str("", managedcodexharvest_pendingItemCount(harvest), " item(s) were open when the provider process died. "), "North did not observe their completion, so success is unknown; they may have "), "completed before the crash."), ((!(summaries === "")) ? $$bc$str("normalized kinds: ", summaries, "", ((omitted > 0) ? $$bc$str("; ", omitted, " other item(s)") : ""), "") : $$bc$str("", managedcodexharvest_pendingItemCount(harvest), " item(s) omitted from the bounded summary")), "Inspect current state before deciding whether to retry any of this work.");
  }
  parts.push("=== end recovered context ===");
  return clip((($beagle$apply$fn, $beagle$apply$tail) => $beagle$apply$fn.call($beagle$apply$fn, ...($beagle$apply$tail ?? [])))(((..._xs) => "".concat(..._xs)), $$bc$interpose("\n", parts)), MAX__RECOVERED__CONTEXT__BYTES);
}

function providerLiveness(child, supervisorExit) {
  const exitSignal = (() => { const coalesce_value_116 = child.signalCode; return ((coalesce_value_116 == null) ? null : coalesce_value_116); })();
  const exitCode = (() => { const coalesce_value_118 = supervisorExit; if ((coalesce_value_118 == null)) {
  const coalesce_value_117 = child.exitCode;
  return ((coalesce_value_117 == null) ? null : coalesce_value_117);
} else {
  return coalesce_value_118;
} })();
  return TypeScriptAnonymousObjectV14((((child.exitCode === null) && (child.signalCode === null)) && (supervisorExit === null)), exitCode, exitSignal);
}

function ManagedCodexAppServerRunState(child, rpc, control, threadStarted, mcp, nativeCommands, respawns, private_retainedPendingItemCount, private_retainedPendingItems, laneCompletedTurns, attemptDeath, attemptFailure, interrupted, activeTurnInterrupt, private_replacementTurnPending, private_pendingReplacementTurnInterrupt, options) {
  return $$bc$record_value("north.providers.codex-app-server/ManagedCodexAppServerRunState", {_tag: "ManagedCodexAppServerRunState", child, rpc, control, threadStarted, mcp, nativeCommands, respawns, private_retainedPendingItemCount, private_retainedPendingItems, laneCompletedTurns, attemptDeath, attemptFailure, interrupted, activeTurnInterrupt, private_replacementTurnPending, private_pendingReplacementTurnInterrupt, options});
}

function managedcodexappserverrunstate_child(r) { return r.child; }

function managedcodexappserverrunstate_rpc(r) { return r.rpc; }

function managedcodexappserverrunstate_control(r) { return r.control; }

function managedcodexappserverrunstate_threadStarted(r) { return r.threadStarted; }

function managedcodexappserverrunstate_mcp(r) { return r.mcp; }

function managedcodexappserverrunstate_nativeCommands(r) { return r.nativeCommands; }

function managedcodexappserverrunstate_respawns(r) { return r.respawns; }

function managedcodexappserverrunstate_private_retainedPendingItemCount(r) { return r.private_retainedPendingItemCount; }

function managedcodexappserverrunstate_private_retainedPendingItems(r) { return r.private_retainedPendingItems; }

function managedcodexappserverrunstate_laneCompletedTurns(r) { return r.laneCompletedTurns; }

function managedcodexappserverrunstate_attemptDeath(r) { return r.attemptDeath; }

function managedcodexappserverrunstate_attemptFailure(r) { return r.attemptFailure; }

function managedcodexappserverrunstate_interrupted(r) { return r.interrupted; }

function managedcodexappserverrunstate_activeTurnInterrupt(r) { return r.activeTurnInterrupt; }

function managedcodexappserverrunstate_private_replacementTurnPending(r) { return r.private_replacementTurnPending; }

function managedcodexappserverrunstate_private_pendingReplacementTurnInterrupt(r) { return r.private_pendingReplacementTurnInterrupt; }

function managedcodexappserverrunstate_options(r) { return r.options; }

function new_ManagedCodexAppServerRunState(options) {
  const self = ManagedCodexAppServerRunState(({value: null, watches: {}}), ({value: null, watches: {}}), ({value: null, watches: {}}), ({value: false, watches: {}}), new McpActivityAccumulator("codex-app-server:item-completed"), ({value: null, watches: {}}), [], ({value: 0, watches: {}}), ({value: [], watches: {}}), ({value: 0, watches: {}}), ({value: null, watches: {}}), ({value: null, watches: {}}), ({value: false, watches: {}}), ({value: null, watches: {}}), ({value: false, watches: {}}), ({value: null, watches: {}}), ({value: options, watches: {}}));
  null;
  return self;
}

function managed_codex_app_server_run_new_bang(options) {
  const state = new_ManagedCodexAppServerRunState(options);
  const run = $$bh$js_obj();
  (run.mcpActivity = () => managedcodexappserverrunstate_mcpActivity(state));
  (run.nativeCommandActivity = () => managedcodexappserverrunstate_nativeCommandActivity(state));
  (run.respawnRecord = () => managedcodexappserverrunstate_respawnRecord(state));
  (run.interrupt = () => managedcodexappserverrunstate_interrupt_bang(state));
  (run.interruptTurn = () => managedcodexappserverrunstate_interruptTurn_bang(state));
  (run.execute = () => managedcodexappserverrunstate_execute_bang(state));
  (run.session = (nextInput) => managedcodexappserverrunstate_session_bang(state, nextInput));
  Object.setPrototypeOf(run, managed_codex_app_server_run_new_bang.prototype);
  return Object.freeze(run);
}

const ManagedCodexAppServerRun = managed_codex_app_server_run_new_bang;

function managedcodexappserverrunstate_mcpActivity(self) {
  return managedcodexappserverrunstate_mcp(self).snapshot();
}

function managedcodexappserverrunstate_nativeCommandActivity(self) {
  const coalesce_value_126 = (() => { const optional_call_125 = managedcodexappserverrunstate_nativeCommands(self).value; return ((optional_call_125 == null) ? null : optional_call_125.snapshot()); })();
  return ((coalesce_value_126 == null) ? unknownNativeCommandActivity("codex-app-server:not-started") : coalesce_value_126);
}

function managedcodexappserverrunstate_respawnRecord(self) {
  return ManagedCodexRespawnRecord(managedcodexappserverrunstate_respawns(self).length, managedcodexappserverrunstate_laneCompletedTurns(self).value, managedcodexappserverrunstate_respawns(self).map((attempt) => ManagedCodexRespawnAttempt(managedcodexrespawnattempt_attempt(attempt), managedcodexrespawnattempt_reason(attempt), managedcodexrespawnattempt_threadId(attempt), managedcodexrespawnattempt_completedTurns(attempt), managedcodexrespawnattempt_stderrTail(attempt), managedcodexrespawnattempt_exitCode(attempt), managedcodexrespawnattempt_exitSignal(attempt))));
}

function managedcodexappserverrunstate_takeAttemptDeath_bang(self, failure) {
  const death = managedcodexappserverrunstate_attemptDeath(self).value;
  const matched = ((!(death === null)) && (failure === managedcodexappserverrunstate_attemptFailure(self).value));
  (() => { const _a = managedcodexappserverrunstate_attemptDeath(self), _v = null; const _old = _a.value; _a.value = _v; for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _v); return _v; })();
  (() => { const _a = managedcodexappserverrunstate_attemptFailure(self), _v = null; const _old = _a.value; _a.value = _v; for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _v); return _v; })();
  return (matched ? death : null);
}

function managedcodexappserverrunstate_harvest(self, partial) {
  const mcp = managedcodexappserverrunstate_mcp(self).harvest();
  const nativeCommands = (() => { const candidate = (() => { const activity = managedcodexappserverrunstate_nativeCommands(self).value; return ((activity == null) ? null : activity.harvest()); })(); return ((candidate == null) ? unknownNativeCommandActivity("codex-app-server:not-started") : candidate); })();
  const observedPending = ((!(partial.pendingItemCount === null)) || (managedcodexappserverrunstate_private_retainedPendingItemCount(self).value > 0));
  const pendingItemCount = (managedcodexappserverrunstate_private_retainedPendingItemCount(self).value + (() => { const count = partial.pendingItemCount; return ((count == null) ? 0 : count); })());
  const pendingItems = mergePendingItems(managedcodexappserverrunstate_private_retainedPendingItems(self).value, (() => { const items = partial.pendingItems; return ((items == null) ? [] : items); })());
  const respawnCount = managedcodexappserverrunstate_respawns(self).length;
  const respawnRecord = managedcodexappserverrunstate_respawnRecord(self);
  return ManagedCodexHarvest(partial.threadId, partial.turnIds, partial.completedTurns, partial.text, partial.toolItems, (observedPending ? pendingItemCount : partial.pendingItemCount), (observedPending ? pendingItems : partial.pendingItems), partial.usage, partial.invocationObservations, mcp, nativeCommands, partial.unsupportedNotifications, ((partial.completedTurns > 0) || ((managedcodexappserverrunstate_laneCompletedTurns(self).value > 0) || ((respawnCount > 0) || ((!(partial.text.trim() === "")) || ((pendingItemCount > 0) || (((() => { const calls = mcp.totalCalls; return ((calls == null) ? 0 : calls); })() > 0) || ((() => { const commands = nativeCommands.totalCommands; return ((commands == null) ? 0 : commands); })() > 0))))))), partial.stderrTail, partial.exitCode, partial.exitSignal, ((respawnCount > 0) ? respawnCount : partial.respawnCount), ((respawnCount > 0) ? managedcodexrespawnrecord_respawns(respawnRecord) : partial.respawns), partial.interrupt);
}

async function managedcodexappserverrunstate_interrupt_bang(self) {
  (() => { const _a = managedcodexappserverrunstate_interrupted(self), _v = true; const _old = _a.value; _a.value = _v; for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _v); return _v; })();
  managedcodexappserverrunstate_private_rejectPendingReplacementTurnInterrupt_bang(self);
  const child = managedcodexappserverrunstate_child(self).value;
  if ((child == null)) {
    null;
  } else {
    await closeProcess_bang(child, managedcodexappserverrunstate_rpc(self).value, managedcodexappserverrunstate_control(self).value);
  }
  return undefined;
}

function managedcodexappserverrunstate_interruptTurn_bang(self) {
  const pending = managedcodexappserverrunstate_private_pendingReplacementTurnInterrupt(self).value;
  if (((_truthy) => _truthy !== false && _truthy != null)(pending)) {
    return pendingreplacementturninterrupt_settlement(pending).promise;
  } else {
    const interrupt = managedcodexappserverrunstate_activeTurnInterrupt(self).value;
    if (((_truthy) => _truthy !== false && _truthy != null)(interrupt)) {
      return interrupt().catch((cause) => (() => { throw new Error(PROVIDER__TURN__INTERRUPT__FAILED, TypeScriptStructuralObjectV2(cause)); })());
    } else {
      if ((!managedcodexappserverrunstate_private_replacementTurnPending(self).value)) {
        const rejected = Promise.withResolvers();
        rejected.reject(new Error(PROVIDER__HAS__NO__ACTIVE__TURN));
        return rejected.promise;
      } else {
        const settlement = Promise.withResolvers();
        settlement.promise.catch(() => null);
        null;
        (() => { const _a = managedcodexappserverrunstate_private_pendingReplacementTurnInterrupt(self), _v = $$bh$js_obj("settlement", settlement, "dispatched", false); const _old = _a.value; _a.value = _v; for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _v); return _v; })();
        return settlement.promise;
      }
    }
  }
}

function managedcodexappserverrunstate_private_beginReplacementTurn_bang(self) {
  (() => { const _a = managedcodexappserverrunstate_private_replacementTurnPending(self), _v = true; const _old = _a.value; _a.value = _v; for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _v); return _v; })();
  return null;
}

function managedcodexappserverrunstate_private_dispatchPendingReplacementTurnInterrupt_bang(self, interrupt) {
  (() => { const _a = managedcodexappserverrunstate_private_replacementTurnPending(self), _v = false; const _old = _a.value; _a.value = _v; for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _v); return _v; })();
  const pending = managedcodexappserverrunstate_private_pendingReplacementTurnInterrupt(self).value;
  if (((!((_truthy) => _truthy !== false && _truthy != null)(pending)) || pendingreplacementturninterrupt_dispatched(pending))) {
    return null;
  } else {
    (pending.dispatched = true);
    interrupt().then(() => managedcodexappserverrunstate_private_resolvePendingReplacementTurnInterrupt_bang(self, pending), (cause) => managedcodexappserverrunstate_private_rejectPendingReplacementTurnInterrupt_bang(self, new Error(PROVIDER__TURN__INTERRUPT__FAILED, TypeScriptStructuralObjectV2(cause)), pending));
    return null;
  }
}

function managedcodexappserverrunstate_private_resolvePendingReplacementTurnInterrupt_bang(self, pending) {
  if ((!(managedcodexappserverrunstate_private_pendingReplacementTurnInterrupt(self).value === pending))) {
    return null;
  } else {
    (() => { const _a = managedcodexappserverrunstate_private_pendingReplacementTurnInterrupt(self), _v = null; const _old = _a.value; _a.value = _v; for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _v); return _v; })();
    return pendingreplacementturninterrupt_settlement(pending).resolve();
  }
}

function managedcodexappserverrunstate_private_rejectPendingReplacementTurnInterrupt_bang(...$beagle$args) {
  if (arguments.length === 1) {
    const self = $beagle$args[0];
    return managedcodexappserverrunstate_private_rejectPendingReplacementTurnInterrupt_bang(self, new Error(REPLACEMENT__TURN__INTERRUPT__UNAVAILABLE), managedcodexappserverrunstate_private_pendingReplacementTurnInterrupt(self).value);
  }
  if (arguments.length === 2) {
    const self = $beagle$args[0];
    const error = $beagle$args[1];
    return managedcodexappserverrunstate_private_rejectPendingReplacementTurnInterrupt_bang(self, error, managedcodexappserverrunstate_private_pendingReplacementTurnInterrupt(self).value);
  }
  if (arguments.length === 3) {
    const self = $beagle$args[0];
    const error = $beagle$args[1];
    const pending = $beagle$args[2];
    (() => { const _a = managedcodexappserverrunstate_private_replacementTurnPending(self), _v = false; const _old = _a.value; _a.value = _v; for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _v); return _v; })();
    if (((!((_truthy) => _truthy !== false && _truthy != null)(pending)) || (!(managedcodexappserverrunstate_private_pendingReplacementTurnInterrupt(self).value === pending)))) {
      return null;
    } else {
      (() => { const _a = managedcodexappserverrunstate_private_pendingReplacementTurnInterrupt(self), _v = null; const _old = _a.value; _a.value = _v; for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _v); return _v; })();
      return pendingreplacementturninterrupt_settlement(pending).reject(error);
    }
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

async function managedcodexappserverrunstate_execute_bang(self) {
  const session = managedcodexappserverrunstate_session_bang(self, () => null);
  const first = await session.next();
  if (((_logical) => (_logical !== false && _logical != null ? _logical : (!((_truthy) => _truthy !== false && _truthy != null)(first.value))))(first.done)) {
    (() => { throw new Error("openai_provider_execution_failed", TypeScriptStructuralObjectV2(new Error($$bc$str("codex app-server session produced no result on first turn ", $$bc$str("(done=", String(first.done), ", value=", ((first.value === null) ? "undefined" : "empty"), ")"))))); })();
  }
  await session.return(first.value);
  return first.value;
}

async function* managedcodexappserverrunstate_session_bang(self, nextInput) {
  const maxRespawns = boundedRespawns(managedcodexappserveroptions_maxRespawns(managedcodexappserverrunstate_options(self).value));
  const completedTurnTexts = [];
  let launchPrompt = managedcodexappserveroptions_prompt(managedcodexappserverrunstate_options(self).value);
  try {
    const while_break_198 = ({value: false, watches: {}});
  {  while (true) {
    if (((_truthy) => _truthy !== false && _truthy != null)(true)) { const while_continue_199 = ({value: false, watches: {}});
try {
    for await (const result of managedcodexappserverrunstate_attempt_bang(self, nextInput, launchPrompt)) {
  (() => { const _a = managedcodexappserverrunstate_laneCompletedTurns(self); const _old = _a.value; _a.value = (((_a, _b) => _a + _b))(_old, 1); for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _a.value); return _a.value; })();
  completedTurnTexts.push(managedcodexresult_text(result));
  yield result;
}
  return;
  } catch (_catch_27) {
    switch ($$bd$catch_dispatch(_catch_27, [$$bd$default_catch])) {
      case 0: {
        const error = _catch_27;
        const death = managedcodexappserverrunstate_takeAttemptDeath_bang(self, error);
        if ((((!((_truthy) => _truthy !== false && _truthy != null)(death)) || managedcodexappserverrunstate_interrupted(self).value) || (managedcodexappserverrunstate_respawns(self).length >= maxRespawns))) {
          (() => { throw error; })();
        }
        const harvest = (error).harvest;
        (() => { const _a = managedcodexappserverrunstate_private_retainedPendingItemCount(self), _v = (() => { const coalesce_value_200 = managedcodexharvest_pendingItemCount(harvest); return ((coalesce_value_200 == null) ? 0.0 : coalesce_value_200); })(); const _old = _a.value; _a.value = _v; for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _v); return _v; })();
        (() => { const _a = managedcodexappserverrunstate_private_retainedPendingItems(self), _v = (() => { const coalesce_value_201 = managedcodexharvest_pendingItems(harvest); return ((coalesce_value_201 == null) ? [] : coalesce_value_201); })(); const _old = _a.value; _a.value = _v; for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _v); return _v; })();
        managedcodexappserverrunstate_respawns(self).push(ManagedCodexRespawnAttempt((() => { const spread_value_205 = ((managedcodexdiagnostics_exitSignal(death.diagnostics) === null) ? Object.assign({}, {}) : TypeScriptAnonymousObjectV25(managedcodexdiagnostics_exitSignal(death.diagnostics))); if (providerContains_p(spread_value_205, "attempt")) {
  return providerGet(spread_value_205, "attempt");
} else {
  const spread_value_204 = ((managedcodexdiagnostics_exitCode(death.diagnostics) === null) ? Object.assign({}, {}) : TypeScriptAnonymousObjectV24(managedcodexdiagnostics_exitCode(death.diagnostics)));
  if (providerContains_p(spread_value_204, "attempt")) {
    return providerGet(spread_value_204, "attempt");
  } else {
    const spread_value_203 = ((!(managedcodexdiagnostics_stderrTail(death.diagnostics).length === 0)) ? TypeScriptAnonymousObjectV29($$bc$into_value([], managedcodexdiagnostics_stderrTail(death.diagnostics))) : Object.assign({}, {}));
    if (providerContains_p(spread_value_203, "attempt")) {
      return providerGet(spread_value_203, "attempt");
    } else {
      const spread_value_202 = (((_truthy) => _truthy !== false && _truthy != null)(managedcodexharvest_threadId(harvest)) ? TypeScriptAnonymousObjectV28(managedcodexharvest_threadId(harvest)) : Object.assign({}, {}));
      return (providerContains_p(spread_value_202, "attempt") ? providerGet(spread_value_202, "attempt") : (managedcodexappserverrunstate_respawns(self).length + 1));
    }
  }
} })(), (() => { const spread_value_209 = ((managedcodexdiagnostics_exitSignal(death.diagnostics) === null) ? Object.assign({}, {}) : TypeScriptAnonymousObjectV25(managedcodexdiagnostics_exitSignal(death.diagnostics))); if (providerContains_p(spread_value_209, "reason")) {
  return providerGet(spread_value_209, "reason");
} else {
  const spread_value_208 = ((managedcodexdiagnostics_exitCode(death.diagnostics) === null) ? Object.assign({}, {}) : TypeScriptAnonymousObjectV24(managedcodexdiagnostics_exitCode(death.diagnostics)));
  if (providerContains_p(spread_value_208, "reason")) {
    return providerGet(spread_value_208, "reason");
  } else {
    const spread_value_207 = ((!(managedcodexdiagnostics_stderrTail(death.diagnostics).length === 0)) ? TypeScriptAnonymousObjectV29($$bc$into_value([], managedcodexdiagnostics_stderrTail(death.diagnostics))) : Object.assign({}, {}));
    if (providerContains_p(spread_value_207, "reason")) {
      return providerGet(spread_value_207, "reason");
    } else {
      const spread_value_206 = (((_truthy) => _truthy !== false && _truthy != null)(managedcodexharvest_threadId(harvest)) ? TypeScriptAnonymousObjectV28(managedcodexharvest_threadId(harvest)) : Object.assign({}, {}));
      return (providerContains_p(spread_value_206, "reason") ? providerGet(spread_value_206, "reason") : death.reason);
    }
  }
} })(), (() => { const spread_value_213 = ((managedcodexdiagnostics_exitSignal(death.diagnostics) === null) ? Object.assign({}, {}) : TypeScriptAnonymousObjectV25(managedcodexdiagnostics_exitSignal(death.diagnostics))); if (providerContains_p(spread_value_213, "threadId")) {
  return providerGet(spread_value_213, "threadId");
} else {
  const spread_value_212 = ((managedcodexdiagnostics_exitCode(death.diagnostics) === null) ? Object.assign({}, {}) : TypeScriptAnonymousObjectV24(managedcodexdiagnostics_exitCode(death.diagnostics)));
  if (providerContains_p(spread_value_212, "threadId")) {
    return providerGet(spread_value_212, "threadId");
  } else {
    const spread_value_211 = ((!(managedcodexdiagnostics_stderrTail(death.diagnostics).length === 0)) ? TypeScriptAnonymousObjectV29($$bc$into_value([], managedcodexdiagnostics_stderrTail(death.diagnostics))) : Object.assign({}, {}));
    if (providerContains_p(spread_value_211, "threadId")) {
      return providerGet(spread_value_211, "threadId");
    } else {
      const spread_value_210 = (((_truthy) => _truthy !== false && _truthy != null)(managedcodexharvest_threadId(harvest)) ? TypeScriptAnonymousObjectV28(managedcodexharvest_threadId(harvest)) : Object.assign({}, {}));
      return (providerContains_p(spread_value_210, "threadId") ? providerGet(spread_value_210, "threadId") : null);
    }
  }
} })(), (() => { const spread_value_217 = ((managedcodexdiagnostics_exitSignal(death.diagnostics) === null) ? Object.assign({}, {}) : TypeScriptAnonymousObjectV25(managedcodexdiagnostics_exitSignal(death.diagnostics))); if (providerContains_p(spread_value_217, "completedTurns")) {
  return providerGet(spread_value_217, "completedTurns");
} else {
  const spread_value_216 = ((managedcodexdiagnostics_exitCode(death.diagnostics) === null) ? Object.assign({}, {}) : TypeScriptAnonymousObjectV24(managedcodexdiagnostics_exitCode(death.diagnostics)));
  if (providerContains_p(spread_value_216, "completedTurns")) {
    return providerGet(spread_value_216, "completedTurns");
  } else {
    const spread_value_215 = ((!(managedcodexdiagnostics_stderrTail(death.diagnostics).length === 0)) ? TypeScriptAnonymousObjectV29($$bc$into_value([], managedcodexdiagnostics_stderrTail(death.diagnostics))) : Object.assign({}, {}));
    return (providerContains_p(spread_value_215, "completedTurns") ? providerGet(spread_value_215, "completedTurns") : managedcodexharvest_completedTurns(harvest));
  }
} })(), (() => { const spread_value_221 = ((managedcodexdiagnostics_exitSignal(death.diagnostics) === null) ? Object.assign({}, {}) : TypeScriptAnonymousObjectV25(managedcodexdiagnostics_exitSignal(death.diagnostics))); if (providerContains_p(spread_value_221, "stderrTail")) {
  return providerGet(spread_value_221, "stderrTail");
} else {
  const spread_value_220 = ((managedcodexdiagnostics_exitCode(death.diagnostics) === null) ? Object.assign({}, {}) : TypeScriptAnonymousObjectV24(managedcodexdiagnostics_exitCode(death.diagnostics)));
  if (providerContains_p(spread_value_220, "stderrTail")) {
    return providerGet(spread_value_220, "stderrTail");
  } else {
    const spread_value_219 = ((!(managedcodexdiagnostics_stderrTail(death.diagnostics).length === 0)) ? TypeScriptAnonymousObjectV29($$bc$into_value([], managedcodexdiagnostics_stderrTail(death.diagnostics))) : Object.assign({}, {}));
    if (providerContains_p(spread_value_219, "stderrTail")) {
      return providerGet(spread_value_219, "stderrTail");
    } else {
      const spread_value_218 = (((_truthy) => _truthy !== false && _truthy != null)(managedcodexharvest_threadId(harvest)) ? TypeScriptAnonymousObjectV28(managedcodexharvest_threadId(harvest)) : Object.assign({}, {}));
      return (providerContains_p(spread_value_218, "stderrTail") ? providerGet(spread_value_218, "stderrTail") : null);
    }
  }
} })(), (() => { const spread_value_225 = ((managedcodexdiagnostics_exitSignal(death.diagnostics) === null) ? Object.assign({}, {}) : TypeScriptAnonymousObjectV25(managedcodexdiagnostics_exitSignal(death.diagnostics))); if (providerContains_p(spread_value_225, "exitCode")) {
  return providerGet(spread_value_225, "exitCode");
} else {
  const spread_value_224 = ((managedcodexdiagnostics_exitCode(death.diagnostics) === null) ? Object.assign({}, {}) : TypeScriptAnonymousObjectV24(managedcodexdiagnostics_exitCode(death.diagnostics)));
  if (providerContains_p(spread_value_224, "exitCode")) {
    return providerGet(spread_value_224, "exitCode");
  } else {
    const spread_value_223 = ((!(managedcodexdiagnostics_stderrTail(death.diagnostics).length === 0)) ? TypeScriptAnonymousObjectV29($$bc$into_value([], managedcodexdiagnostics_stderrTail(death.diagnostics))) : Object.assign({}, {}));
    if (providerContains_p(spread_value_223, "exitCode")) {
      return providerGet(spread_value_223, "exitCode");
    } else {
      const spread_value_222 = (((_truthy) => _truthy !== false && _truthy != null)(managedcodexharvest_threadId(harvest)) ? TypeScriptAnonymousObjectV28(managedcodexharvest_threadId(harvest)) : Object.assign({}, {}));
      return (providerContains_p(spread_value_222, "exitCode") ? providerGet(spread_value_222, "exitCode") : null);
    }
  }
} })(), (() => { const spread_value_229 = ((managedcodexdiagnostics_exitSignal(death.diagnostics) === null) ? Object.assign({}, {}) : TypeScriptAnonymousObjectV25(managedcodexdiagnostics_exitSignal(death.diagnostics))); if (providerContains_p(spread_value_229, "exitSignal")) {
  return providerGet(spread_value_229, "exitSignal");
} else {
  const spread_value_228 = ((managedcodexdiagnostics_exitCode(death.diagnostics) === null) ? Object.assign({}, {}) : TypeScriptAnonymousObjectV24(managedcodexdiagnostics_exitCode(death.diagnostics)));
  if (providerContains_p(spread_value_228, "exitSignal")) {
    return providerGet(spread_value_228, "exitSignal");
  } else {
    const spread_value_227 = ((!(managedcodexdiagnostics_stderrTail(death.diagnostics).length === 0)) ? TypeScriptAnonymousObjectV29($$bc$into_value([], managedcodexdiagnostics_stderrTail(death.diagnostics))) : Object.assign({}, {}));
    if (providerContains_p(spread_value_227, "exitSignal")) {
      return providerGet(spread_value_227, "exitSignal");
    } else {
      const spread_value_226 = (((_truthy) => _truthy !== false && _truthy != null)(managedcodexharvest_threadId(harvest)) ? TypeScriptAnonymousObjectV28(managedcodexharvest_threadId(harvest)) : Object.assign({}, {}));
      return (providerContains_p(spread_value_226, "exitSignal") ? providerGet(spread_value_226, "exitSignal") : null);
    }
  }
} })()));
        managedcodexappserverrunstate_mcp(self).retireSession();
        const optional_call_230 = managedcodexappserverrunstate_nativeCommands(self).value;
        if ((optional_call_230 == null)) {
          null;
        } else {
          optional_call_230.retireSession();
        }
        managedcodexappserverrunstate_private_beginReplacementTurn_bang(self);
        const onRespawn = managedcodexappserveroptions_onRespawn(managedcodexappserverrunstate_options(self).value);
        if (((_truthy) => _truthy !== false && _truthy != null)(onRespawn)) {
          const pendingRespawn = onRespawn();
          if (((_truthy) => _truthy !== false && _truthy != null)(pendingRespawn)) {
            await pendingRespawn;
          }
        }
        if (managedcodexappserverrunstate_interrupted(self).value) {
          (() => { throw error; })();
        }
        (launchPrompt = managedCodexRecoveredContext(managedcodexappserveroptions_prompt(managedcodexappserverrunstate_options(self).value), completedTurnTexts, harvest));
        console.error($$bc$str($$bc$str($$bc$str("[codex] managed provider session died (", death.reason, ") — respawning "), $$bc$str("", managedcodexappserverrunstate_respawns(self).length, "/", maxRespawns, " with ", $$bc$count(completedTurnTexts), " ")), "completed turn(s) of recovered context"));
        break;
      }
    }
  } if ((!while_break_198.value)) {  continue; } else { null; break; } } else { null; break; }
  } };
  null;
  } finally {
    managedcodexappserverrunstate_private_rejectPendingReplacementTurnInterrupt_bang(self);
  }
}

async function* managedcodexappserverrunstate_attempt_bang(self, nextInput, launchPrompt) {
  const contract = await (async () => { try {
    const beforeLaunch = managedcodexappserveroptions_beforeLaunch(managedcodexappserverrunstate_options(self).value);
  if (((_truthy) => _truthy !== false && _truthy != null)(beforeLaunch)) {
    await beforeLaunch();
  }
  return managedCodexAppServerLaunch_bang(managedcodexappserverrunstate_options(self).value);
  } catch (_catch_28) {
    switch ($$bd$catch_dispatch(_catch_28, [$$bd$default_catch])) {
      case 0: {
        const error = _catch_28;
        const failure = (managedCodexPreThreadError_p(error) ? error : new_ManagedCodexPreThreadError("openai_codex_launch_contract_invalid", TypeScriptStructuralObjectV2(error)));
        if ((!managedcodexappserverrunstate_threadStarted(self).value)) {
          (() => { throw failure; })();
        }
        return (() => { throw new_ManagedCodexHarvestError(managedcodexappserverrunstate_harvest(self, TypeScriptStructuralObjectV11(0.0, null, null, null, null, null, null, null, null, null, "", null, null, [], Object.assign({}, {}), null)), TypeScriptStructuralObjectV2(failure)); })();
        break;
      }
    }
  } })();
  const coalesce_field_240 = managedcodexappserverrunstate_nativeCommands(self).value;
  if ((coalesce_field_240 == null)) {
    (() => { const _a = managedcodexappserverrunstate_nativeCommands(self), _v = new NativeCommandActivityAccumulator(launchcontract_cwd(contract), ENGINE); const _old = _a.value; _a.value = _v; for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _v); return _v; })();
  } else {
    coalesce_field_240;
  }
  const inventory = expectedMcpInventory(managedcodexappserveroptions_surface(managedcodexappserverrunstate_options(self).value));
  const mcpServerNames = Object.freeze(inventory.map((server) => expectedmcpserver_name(server)));
  const supervised = (!(managedcodexappserveroptions_useSupervisor(managedcodexappserverrunstate_options(self).value) === false));
  const spawnProcess = (() => { const coalesce_value_241 = managedcodexappserveroptions_spawnProcess(managedcodexappserverrunstate_options(self).value); return ((coalesce_value_241 == null) ? spawn : coalesce_value_241); })();
  const control = (supervised ? createSupervisorControl_bang() : null);
  (() => { const _a = managedcodexappserverrunstate_control(self), _v = control; const _old = _a.value; _a.value = _v; for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _v); return _v; })();
  let child = null;
  try {
    (child = externalized_spawn(spawnProcess, (supervised ? process.execPath : launchcontract_executable(contract)), (supervised ? $$bc$into_value($$bc$into_value($$bc$conj_value($$bc$conj_value($$bc$conj_value($$bc$conj_value($$bc$conj_value([], SUPERVISOR), "--duplex"), supervisorcontrol_path(control)), CODEX__SUPERVISOR__STDERR__FLAG), launchcontract_executable(contract)), (() => { const coalesce_value_243 = managedcodexappserveroptions_commandPrefix(managedcodexappserverrunstate_options(self).value); return ((coalesce_value_243 == null) ? [] : coalesce_value_243); })()), launchcontract_args(contract)) : $$bc$into_value($$bc$into_value([], (() => { const coalesce_value_244 = managedcodexappserveroptions_commandPrefix(managedcodexappserverrunstate_options(self).value); return ((coalesce_value_244 == null) ? [] : coalesce_value_244); })()), launchcontract_args(contract))), Object.assign({}, {}, {[$$bc$property_key("stdio")]: ["pipe", "pipe", "pipe"]}, {[$$bc$property_key("shell")]: false}, {[$$bc$property_key("cwd")]: launchcontract_cwd(contract)}, {[$$bc$property_key("env")]: managedcodexappserveroptions_env(managedcodexappserverrunstate_options(self).value)})));
  } catch (_catch_29) {
    switch ($$bd$catch_dispatch(_catch_29, [$$bd$default_catch])) {
      case 0: {
        const cause = _catch_29;
        const optional_call_242 = control;
        if ((optional_call_242 == null)) {
          null;
        } else {
          optional_call_242.close();
        }
        (() => { const _a = managedcodexappserverrunstate_control(self), _v = null; const _old = _a.value; _a.value = _v; for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _v); return _v; })();
        (() => { throw new_ManagedCodexPreThreadError("openai_codex_supervisor_unavailable", TypeScriptStructuralObjectV2(cause)); })();
        break;
      }
    }
  }
  (() => { const _a = managedcodexappserverrunstate_child(self), _v = child; const _old = _a.value; _a.value = _v; for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _v); return _v; })();
  let remoteDisabled = false;
  let threadId = null;
  let turnId = null;
  let completedTurns = 0;
  const settledTurnIds = [];
  let runtimeState = null;
  const approvedServerRequests = new Set([]);
  const queuedNotifications = [];
  let terminalDeferred = Promise.withResolvers();
  let terminalResolve = terminalDeferred.resolve;
  let terminalReject = terminalDeferred.reject;
  let terminal = terminalDeferred.promise;
  terminal.catch(() => null);
  null;
  const turnDeadlineMs = boundedMs("NORTH_CODEX_TURN_DEADLINE_MS", TURN__DEADLINE__MS, managedcodexappserveroptions_turnDeadlineMs(managedcodexappserverrunstate_options(self).value));
  const turnDeadlineInactivityMs = boundedMs("NORTH_CODEX_TURN_DEADLINE_INACTIVITY_MS", TURN__DEADLINE__INACTIVITY__MS, managedcodexappserveroptions_turnDeadlineInactivityMs(managedcodexappserverrunstate_options(self).value));
  const inFlightItemCeilingMs = boundedMs("NORTH_CODEX_IN_FLIGHT_ITEM_CEILING_MS", IN__FLIGHT__ITEM__CEILING__MS, managedcodexappserveroptions_inFlightItemCeilingMs(managedcodexappserverrunstate_options(self).value));
  const postToolQuietMs = boundedMs("NORTH_CODEX_POST_TOOL_QUIET_MS", POST__TOOL__QUIET__MS, managedcodexappserveroptions_postToolQuietMs(managedcodexappserverrunstate_options(self).value));
  let deadlineTimer = null;
  let quietTimer = null;
  let watchdogReason = null;
  let interruptEvidence = null;
  let turnStartedAt = 0;
  let lastTurnActivityAt = 0;
  let turnEventCount = 0;
  let turnEventCounts = Object.assign({}, {});
  const clearQuietWatchdog = () => { if (((_truthy) => _truthy !== false && _truthy != null)(quietTimer)) {
  clearTimeout(quietTimer);
}
return (quietTimer = null); };
  const clearWatchdogs = () => { clearQuietWatchdog();
if (((_truthy) => _truthy !== false && _truthy != null)(deadlineTimer)) {
  clearTimeout(deadlineTimer);
}
return (deadlineTimer = null); };
  {
    function armQuietWatchdog() { clearQuietWatchdog();
if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? _logical : (!(runtimenotificationstate_openItems(runtimeState).size === 0))))(((_logical) => (_logical !== false && _logical != null ? _logical : runtimenotificationstate_terminalSeen(runtimeState)))(((_logical) => (_logical !== false && _logical != null ? _logical : (!((_truthy) => _truthy !== false && _truthy != null)((() => { const optional_value_245 = runtimeState; return ((optional_value_245 == null) ? null : optional_value_245.turnId); })()))))(watchdogReason))))) {
  return null;
} else {
  (quietTimer = setTimeout(() => expireTurn("post_tool_silence", $$bc$str("codex went silent for ", postToolQuietMs, "ms after a completed tool item"), postToolQuietMs, postToolQuietMs), postToolQuietMs));
  return quietTimer.unref();
} } function oldestOpenItem() { let oldest = null;
const for_break_246 = ({value: false, watches: {}});
$$bc$eager_seq((() => { const coalesce_value_252 = (() => { const optional_value_251 = runtimeState; return ((optional_value_251 == null) ? null : optional_value_251.openItems); })(); return ((coalesce_value_252 == null) ? [] : coalesce_value_252); })()).forEach(($beagle$item) => {
  let id = $beagle$item[0];
  let item = $beagle$item[1];
  if ((!for_break_246.value)) {
    const for_continue_247 = ({value: false, watches: {}});
    if (((!((_truthy) => _truthy !== false && _truthy != null)(oldest)) || (item.observedAtMs < oldest.observedAtMs))) {
      (oldest = TypeScriptAnonymousObjectV31((() => { const spread_value_248 = item; return (providerContains_p(spread_value_248, "id") ? providerGet(spread_value_248, "id") : id); })(), (() => { const spread_value_249 = item; return (providerContains_p(spread_value_249, "kind") ? providerGet(spread_value_249, "kind") : null); })(), (() => { const spread_value_250 = item; return (providerContains_p(spread_value_250, "observedAtMs") ? providerGet(spread_value_250, "observedAtMs") : null); })()));
    }
    null;
  }
});
return oldest; } function armTurnDeadline() { if (((_truthy) => _truthy !== false && _truthy != null)(deadlineTimer)) {
  clearTimeout(deadlineTimer);
}
const now = Date.now();
const openItem = oldestOpenItem();
const wallRemaining = Math.max(0.0, ((turnStartedAt + turnDeadlineMs) - now));
const inactivityRemaining = Math.max(0.0, ((lastTurnActivityAt + turnDeadlineInactivityMs) - now));
const remaining = (((_truthy) => _truthy !== false && _truthy != null)(openItem) ? Math.max(0.0, ((openItem.observedAtMs + inFlightItemCeilingMs) - now)) : Math.max(wallRemaining, inactivityRemaining));
(deadlineTimer = setTimeout(() => { const checkedAt = Date.now();
const liveOpenItem = oldestOpenItem();
if (((_truthy) => _truthy !== false && _truthy != null)(liveOpenItem)) {
  const openAgeMs = (checkedAt - liveOpenItem.observedAtMs);
  if ((openAgeMs < inFlightItemCeilingMs)) {
    armTurnDeadline();
    return null;
  } else {
    expireTurn("in_flight_item_ceiling", $$bc$str("codex in-flight ", liveOpenItem.kind, " item ", liveOpenItem.id, " exceeded its ", inFlightItemCeilingMs, "ms ceiling"), inFlightItemCeilingMs, turnDeadlineInactivityMs);
    return null;
  }
} else {
  if ((((checkedAt - turnStartedAt) < turnDeadlineMs) || ((checkedAt - lastTurnActivityAt) < turnDeadlineInactivityMs))) {
    armTurnDeadline();
    return null;
  } else {
    return expireTurn("turn_deadline", $$bc$str("codex turn exceeded its ", turnDeadlineMs, "ms deadline"), turnDeadlineMs, turnDeadlineInactivityMs);
  }
} }, Math.max(1.0, remaining)));
return deadlineTimer.unref(); } function recordTurnActivity(kind) { (lastTurnActivityAt = Date.now());
(turnEventCount = (turnEventCount + 1));
(turnEventCounts = $$bc$assoc_value(turnEventCounts, kind, ((() => { const coalesce_value_253 = providerGet(turnEventCounts, kind); return ((coalesce_value_253 == null) ? 0.0 : coalesce_value_253); })() + 1)));
if ((turnStartedAt > 0)) {
  armTurnDeadline();
}
return null; } async function interruptTurn() { if (((!((_truthy) => _truthy !== false && _truthy != null)(threadId)) || (!((_truthy) => _truthy !== false && _truthy != null)(turnId)))) {
  (() => { throw new $$be$ExceptionInfo("Codex has no active turn to interrupt", {}); })();
}
record(await Promise.race((() => { const tuple_value_254 = [appserverrpc_request_bang(rpc, "turn/interrupt", Object.assign({}, {}, {[$$bc$property_key("threadId")]: threadId}, {[$$bc$property_key("turnId")]: turnId})), Bun.sleep(TURN__INTERRUPT__MS).then(() => (() => { throw new $$be$ExceptionInfo("turn/interrupt timed out", {}); })())]; return tuple_value_254; })()), "Codex turn/interrupt response");
return null; } function expireTurn(reason, bound, deadlineMs, inactivityThresholdMs) { if (((_truthy) => _truthy !== false && _truthy != null)(watchdogReason)) {
  return null;
} else {
  clearWatchdogs();
  const now = Date.now();
  const openItem = oldestOpenItem();
  (interruptEvidence = ManagedCodexInterruptEvidence(reason, deadlineMs, inactivityThresholdMs, Math.max(0.0, (now - lastTurnActivityAt)), (() => { const coalesce_value_257 = (() => { const optional_value_256 = (() => { const optional_value_255 = runtimeState; return ((optional_value_255 == null) ? null : optional_value_255.openItems); })(); return ((optional_value_256 == null) ? null : optional_value_256.size); })(); return ((coalesce_value_257 == null) ? 0.0 : coalesce_value_257); })(), (((_truthy) => _truthy !== false && _truthy != null)(openItem) ? TypeScriptAnonymousObjectV2(Math.max(0.0, (now - openItem.observedAtMs)), openItem.id, openItem.kind) : null), turnEventCount, Object.assign({}, {}, turnEventCounts)));
  const liveness = providerLiveness(child, (supervised ? (supervisorstatuschannel_exitCode(supervisor))() : null));
  const cause = new Error($$bc$str(bound, "; provider ", (liveness.alive ? "still running" : "not running"), ((liveness.exitCode === null) ? "" : $$bc$str(" (exit ", liveness.exitCode, ")")), ((liveness.exitSignal === null) ? "" : $$bc$str(" (signal ", liveness.exitSignal, ")"))));
  (watchdogReason = new Error("openai_codex_turn_interrupted", TypeScriptStructuralObjectV2(cause)));
  const settle = terminalReject;
  (async () => { let outcome = "provider already gone";
if (liveness.alive) {
  await (async () => { try {
    await interruptTurn();
  return (outcome = "turn/interrupt accepted");
  } catch (_catch_30) {
    switch ($$bd$catch_dispatch(_catch_30, [$$bd$default_catch])) {
      case 0: {
        const error = _catch_30;
        return (outcome = $$bc$str("turn/interrupt refused: ", ((error instanceof Error) ? (error).message : String(error)), ""));
        break;
      }
    }
  } })();
}
(cause.message = $$bc$str("", cause.message, "; ", outcome, ""));
return settle(watchdogReason); })();
  return null;
} } let projectWarningSeen = false;
    const validateConnectionNotification = (method, value) => { if ((method === "configWarning")) {
  validateProjectConfigWarning(value, contract);
  (projectWarningSeen = true);
  return true;
} else {
  if ((method === "deprecationNotice")) {
    const params = record(value, "Codex deprecation notice");
    onlyKeys(params, ["summary", "details"], "Codex deprecation notice");
    boundedString(providerGet(params, "summary"), "Codex deprecation summary", 2048);
    boundedString(providerGet(params, "details"), "Codex deprecation details", 4096);
    return true;
  } else {
    if ((method === "remoteControl/status/changed")) {
      const params = record(value, "Codex remote-control status");
      onlyKeys(params, ["status", "serverName", "installationId", "environmentId"], "Codex remote-control status");
      if (((((((((remoteDisabled || (!(providerGet(params, "status") === "disabled"))) || (!(typeof providerGet(params, "serverName") === "string"))) || (providerGet(params, "serverName") === "")) || (Buffer.byteLength(providerGet(params, "serverName"), "utf8") > 256)) || (!(typeof providerGet(params, "installationId") === "string"))) || (providerGet(params, "installationId") === "")) || (Buffer.byteLength(providerGet(params, "installationId"), "utf8") > 256)) || ((!(providerGet(params, "environmentId") === null)) && (((!(typeof providerGet(params, "environmentId") === "string")) || (providerGet(params, "environmentId") === "")) || (Buffer.byteLength(providerGet(params, "environmentId"), "utf8") > 256))))) {
        (() => { throw new $$be$ExceptionInfo("Codex remote control is not exactly disabled", {}); })();
      }
      (remoteDisabled = true);
      return true;
    } else {
      if ((method === "mcpServer/startupStatus/updated")) {
        const pendingThreadStart = ((threadId === null) && managedcodexappserverrunstate_threadStarted(self).value);
        const params = validateMcpStartupNotification_bang(value, threadId, mcpServerNames, pendingThreadStart);
        return (((!(providerGet(params, "threadId") === null)) && (threadId === null)) ? false : true);
      } else {
        if ((method === "account/rateLimits/updated")) {
          const params = record(value, "Codex rate-limit notification");
          onlyKeys(params, ["rateLimits"], "Codex rate-limit notification");
          record(providerGet(params, "rateLimits"), "Codex rate-limit snapshot");
          return true;
        } else {
          if ((method === "serverRequest/resolved")) {
            const params = record(value, "Codex server request resolution");
            onlyKeys(params, ["threadId", "requestId"], "Codex server request resolution");
            const requestId = providerGet(params, "requestId");
            if ((((!(providerGet(params, "threadId") === threadId)) || ((!(typeof requestId === "number")) && (!(typeof requestId === "string")))) || (!((_truthy) => _truthy !== false && _truthy != null)(approvedServerRequests.delete(requestId))))) {
              (() => { throw new $$be$ExceptionInfo("Codex resolved an unknown server request", {}); })();
            }
            return true;
          } else {
            return false;
          }
        }
      }
    }
  }
} };
    const canProcessWithoutTurn = (entry) => { if ((((entry.method === "thread/started") || (entry.method === "thread/status/changed")) || (entry.method === "mcpServer/startupStatus/updated"))) {
  return true;
} else {
  if (((entry.method === "hook/started") || (entry.method === "hook/completed"))) {
    (() => { try {
    const params = record(entry.value, "Codex hook notification");
  const run = record(providerGet(params, "run"), "Codex hook run");
  return (providerGet(run, "eventName") === "sessionStart");
  } catch (_catch_31) {
    switch ($$bd$catch_dispatch(_catch_31, [$$bd$default_catch])) {
      case 0: {
        const typescript_error = _catch_31;
        return true;
        break;
      }
    }
  } })();
  }
  return false;
} };
    const processRuntime = async (entry) => { if ((!((_truthy) => _truthy !== false && _truthy != null)(runtimeState))) {
  (() => { throw new $$be$ExceptionInfo("Codex runtime notification preceded thread authority", {}); })();
}
const wasTerminal = runtimenotificationstate_terminalSeen(runtimeState);
const toolItemsBefore = runtimenotificationstate_toolItems(runtimeState);
const completedItemId = validateProgressNotification_bang(entry.method, entry.value, runtimeState);
const onEvent = managedcodexappserveroptions_onEvent(managedcodexappserverrunstate_options(self).value);
if (((_truthy) => _truthy !== false && _truthy != null)(onEvent)) {
  const pendingEvent = onEvent(entry.method, publicJsValue($$bh$clj_to_js(entry.value)));
  if (((_truthy) => _truthy !== false && _truthy != null)(pendingEvent)) {
    await pendingEvent;
  }
}
if ((!(completedItemId === null))) {
  runtimenotificationstate_openItems(runtimeState).delete(completedItemId);
}
const activity = providerExecutionActivityKind(entry.method, entry.value);
if (((_truthy) => _truthy !== false && _truthy != null)(activity)) {
  recordTurnActivity(activity);
  reportManagedActivity_bang(managedcodexappserverrunstate_options(self).value, activity);
}
if ((runtimenotificationstate_toolItems(runtimeState) > toolItemsBefore)) {
  armQuietWatchdog();
} else {
  clearQuietWatchdog();
}
if (((!wasTerminal) && runtimenotificationstate_terminalSeen(runtimeState))) {
  return terminalResolve();
} };
    const drainQueued = async (withTurn) => { let index = 0;
const for_break_258 = ({value: false, watches: {}});
await (async () => {  while (true) {
    if ((index < $$bc$count(queuedNotifications))) { await (async () => { const for_continue_259 = ({value: false, watches: {}}); const entry = providerGet(queuedNotifications, index);
if (((!withTurn) && (!canProcessWithoutTurn(entry)))) {
  (index = (index + 1));
  return (() => { const _a = for_continue_259, _v = true; const _old = _a.value; _a.value = _v; for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _v); return _v; })();
} else {
  queuedNotifications.splice(index, 1);
  return await processRuntime(entry);
} })(); if ((!for_break_258.value)) {  continue; } else { return null; } } else { return null; }
  } })();
return null; };
    const onNotification = async (method, value) => { if (validateConnectionNotification(method, value)) {
  const onEvent = managedcodexappserveroptions_onEvent(managedcodexappserverrunstate_options(self).value);
  if (((_truthy) => _truthy !== false && _truthy != null)(onEvent)) {
    const pendingEvent = onEvent(method, publicJsValue($$bh$clj_to_js(value)));
    if (((_truthy) => _truthy !== false && _truthy != null)(pendingEvent)) {
      await pendingEvent;
    }
  }
  return null;
} else {
  const entry = TypeScriptAnonymousObjectV30(method, value);
  if (((!((_truthy) => _truthy !== false && _truthy != null)(runtimeState)) || ((!((_truthy) => _truthy !== false && _truthy != null)(runtimenotificationstate_turnId(runtimeState))) && (!canProcessWithoutTurn(entry))))) {
    if (($$bc$count(queuedNotifications) >= MAX__QUEUED__NOTIFICATIONS)) {
      (() => { throw new $$be$ExceptionInfo("Codex queued too many pre-authority notifications", {}); })();
    }
    queuedNotifications.push(entry);
    return null;
  } else {
    return await processRuntime(entry);
  }
} };
    const onServerRequest = (id, method, value) => { if ((!(method === "item/tool/requestUserInput"))) {
  return null;
} else {
  const params = record(value, "Codex tool input request");
  onlyKeys(params, ["threadId", "turnId", "itemId", "questions", "autoResolutionMs"], "Codex tool input request");
  if ((((!(providerGet(params, "threadId") === threadId)) || (!(providerGet(params, "turnId") === turnId))) || (!(providerGet(params, "autoResolutionMs") === null)))) {
    (() => { throw new $$be$ExceptionInfo("Codex tool input request belongs to another execution", {}); })();
  }
  const itemId = protocolId(providerGet(params, "itemId"), "Codex tool input item id");
  if (((!Array.isArray(providerGet(params, "questions"))) || (!($$bc$count(providerGet(params, "questions")) === 1)))) {
    (() => { throw new $$be$ExceptionInfo("Codex tool input request must contain one approval question", {}); })();
  }
  const question = record(providerGet(providerGet(params, "questions"), 0), "Codex managed MCP approval question");
  onlyKeys(question, ["id", "header", "question", "isOther", "isSecret", "options"], "Codex managed MCP approval question");
  const questionId = $$bc$str("mcp_tool_call_approval_", itemId, "");
  const prompt = boundedString(providerGet(question, "question"), "Codex managed MCP approval prompt", 512);
  const match = /^Allow the ([a-z][a-z0-9-]*) MCP server to run tool \"([a-z][a-z0-9_-]*)\"\?$/.exec(prompt);
  const granted = (((_truthy) => _truthy !== false && _truthy != null)(match) ? (() => { const optional_value_260 = inventory.find((server) => (expectedmcpserver_name(server) === providerGet(match, 1))); return ((optional_value_260 == null) ? null : optional_value_260.tools); })() : null);
  if (((((((!(providerGet(question, "id") === questionId)) || (!(providerGet(question, "header") === "Approve app tool call?"))) || (!(providerGet(question, "isOther") === false))) || (!(providerGet(question, "isSecret") === false))) || (!((_truthy) => _truthy !== false && _truthy != null)(match))) || (!((_truthy) => _truthy !== false && _truthy != null)((() => { const optional_call_261 = granted; return ((optional_call_261 == null) ? null : optional_call_261.includes(providerGet(match, 2))); })())))) {
    (() => { throw new $$be$ExceptionInfo("Codex requested approval outside North's sealed MCP grant", {}); })();
  }
  exact(providerGet(question, "options"), [Object.assign({}, {}, {[$$bc$property_key("label")]: "Allow"}, {[$$bc$property_key("description")]: "Run the tool and continue."}), Object.assign({}, {}, {[$$bc$property_key("label")]: "Allow for this session"}, {[$$bc$property_key("description")]: "Run the tool and remember this choice for this session."}), Object.assign({}, {}, {[$$bc$property_key("label")]: "Cancel"}, {[$$bc$property_key("description")]: "Cancel this tool call."})], "Codex managed MCP approval options");
  recordTurnActivity("provider.codex.mcp.request");
  reportManagedActivity_bang(managedcodexappserverrunstate_options(self).value, "provider.codex.mcp.request");
  approvedServerRequests.add(id);
  return Object.assign({}, {}, {[$$bc$property_key("answers")]: Object.assign({}, {}, $$bc$assoc_value({}, questionId, Object.assign({}, {}, {[$$bc$property_key("answers")]: ["Allow"]})))});
} };
    const rpc = new_AppServerRpc_bang(child, (() => { const coalesce_value_262 = managedcodexappserveroptions_timeoutMs(managedcodexappserverrunstate_options(self).value); return ((coalesce_value_262 == null) ? RPC__TIMEOUT__MS : coalesce_value_262); })(), onNotification, onServerRequest, (() => { const optional_value_263 = control; return ((optional_value_263 == null) ? (line, callback) => { (child.stdin).write(line, callback);
return null; } : optional_value_263.writeLine); })(), (!supervised));
    (() => { const _a = managedcodexappserverrunstate_rpc(self), _v = rpc; const _old = _a.value; _a.value = _v; for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _v); return _v; })();
    const removeTerminal = appserverrpc_onTerminal(rpc, (error) => terminalReject(error));
    const supervisor = (supervised ? supervisorStatusChannel_bang(child) : (() => { const inactive_supervisor = Promise.withResolvers(); return SupervisorStatusChannel(inactive_supervisor.promise, () => null, function(...$beagle$args) {
  if (arguments.length === 0) {
    return [];
  }
  if (arguments.length === 1) {
    const count = $beagle$args[0];
    return [];
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}, () => null, () => null); })());
    const providerDiagnostics = () => { const liveness = providerLiveness(child, (supervised ? (supervisorstatuschannel_exitCode(supervisor))() : null));
return ManagedCodexDiagnostics((supervised ? (supervisorstatuschannel_stderrTail(supervisor))() : appserverrpc_stderrTail(rpc, STDERR__TAIL__LINES)), liveness.exitCode, liveness.exitSignal, liveness.alive); };
    let failure = null;
    let primaryFailed = false;
    let protocolSucceeded = false;
    try {
    await awaitChildSpawn_bang(child, (() => { const coalesce_value_348 = managedcodexappserveroptions_timeoutMs(managedcodexappserverrunstate_options(self).value); return ((coalesce_value_348 == null) ? RPC__TIMEOUT__MS : coalesce_value_348); })());
  if (((_truthy) => _truthy !== false && _truthy != null)(control)) {
    await Promise.race((() => { const tuple_value_349 = [supervisorcontrol_connected(control), supervisorstatuschannel_failure(supervisor)]; return tuple_value_349; })());
  }
  const initialized = await Promise.race((() => { const tuple_value_350 = [appserverrpc_request_bang(rpc, "initialize", Object.assign({}, {}, {[$$bc$property_key("clientInfo")]: Object.assign({}, {}, {[$$bc$property_key("name")]: "north"}, {[$$bc$property_key("title")]: "North"}, {[$$bc$property_key("version")]: "1"})}, {[$$bc$property_key("capabilities")]: Object.assign({}, {}, {[$$bc$property_key("experimentalApi")]: true})})), supervisorstatuschannel_failure(supervisor)]; return tuple_value_350; })());
  (supervisorstatuschannel_settled(supervisor))();
  validateInitialize(initialized, contract);
  appserverrpc_notify_bang(rpc, "initialized", Object.assign({}, {}));
  validateAccount(await appserverrpc_request_bang(rpc, "account/read", Object.assign({}, {})));
  const config = await appserverrpc_request_bang(rpc, "config/read", Object.assign({}, {}, {[$$bc$property_key("includeLayers")]: true}, {[$$bc$property_key("cwd")]: launchcontract_cwd(contract)}));
  const fingerprint = validateConfig_bang(config, contract, projectWarningSeen);
  validateRequirements(await appserverrpc_request_bang(rpc, "configRequirements/read", null), contract);
  validateHooks(await appserverrpc_request_bang(rpc, "hooks/list", Object.assign({}, {}, {[$$bc$property_key("cwds")]: [launchcontract_cwd(contract)]})), launchcontract_cwd(contract));
  await validateMcp_bang(rpc, inventory);
  if ((!remoteDisabled)) {
    (() => { throw new $$be$ExceptionInfo("Codex did not prove remote control disabled", {}); })();
  }
  assertNoFilesystemAuthority_bang(launchcontract_codexHome(contract));
  const shellPolicy = record(providerGet(launchcontract_expectedSessionConfig(contract), "shell_environment_policy"), "Codex managed shell policy");
  const shellEnvironment = record(providerGet(shellPolicy, "set"), "Codex managed shell environment");
  validateShellPreflight(await appserverrpc_request_bang(rpc, "command/exec", Object.assign({}, {}, {[$$bc$property_key("command")]: $$bc$into_value([], CODEX__SHELL__PREFLIGHT__COMMAND)}, {[$$bc$property_key("processId")]: null}, {[$$bc$property_key("tty")]: false}, {[$$bc$property_key("streamStdin")]: false}, {[$$bc$property_key("streamStdoutStderr")]: false}, {[$$bc$property_key("outputBytesCap")]: CODEX__SHELL__PREFLIGHT__OUTPUT__BYTES}, {[$$bc$property_key("disableOutputCap")]: false}, {[$$bc$property_key("disableTimeout")]: false}, {[$$bc$property_key("timeoutMs")]: CODEX__SHELL__PREFLIGHT__TIMEOUT__MS}, {[$$bc$property_key("cwd")]: launchcontract_cwd(contract)}, {[$$bc$property_key("env")]: Object.assign({}, {}, {[$$bc$property_key("PATH")]: providerGet(shellEnvironment, "PATH")}, {[$$bc$property_key("NORTH_BIN")]: providerGet(shellEnvironment, "NORTH_BIN")})}, {[$$bc$property_key("size")]: null}, {[$$bc$property_key("sandboxPolicy")]: Object.assign({}, {}, {[$$bc$property_key("type")]: "readOnly"}, {[$$bc$property_key("networkAccess")]: false})}, {[$$bc$property_key("permissionProfile")]: null})));
  (() => { const _a = managedcodexappserverrunstate_threadStarted(self), _v = true; const _old = _a.value; _a.value = _v; for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _v); return _v; })();
  const started = record(await appserverrpc_request_bang(rpc, "thread/start", Object.assign({}, {}, {[$$bc$property_key("model")]: managedcodexappserveroptions_model(managedcodexappserverrunstate_options(self).value)}, {[$$bc$property_key("modelProvider")]: "openai"}, {[$$bc$property_key("approvalPolicy")]: "never"}, {[$$bc$property_key("approvalsReviewer")]: "user"}, {[$$bc$property_key("sandbox")]: managedcodexappserveroptions_surface(managedcodexappserverrunstate_options(self).value).sandbox}, {[$$bc$property_key("config")]: (((_truthy) => _truthy !== false && _truthy != null)(managedcodexappserveroptions_effort(managedcodexappserverrunstate_options(self).value)) ? Object.assign({}, {}, {[$$bc$property_key("model_reasoning_effort")]: managedcodexappserveroptions_effort(managedcodexappserverrunstate_options(self).value)}) : Object.assign({}, {}))}, {[$$bc$property_key("developerInstructions")]: managedcodexappserveroptions_developerInstructions(managedcodexappserverrunstate_options(self).value)}, {[$$bc$property_key("ephemeral")]: true})), "Codex thread/start response");
  (threadId = validateStartedThread_bang(started, contract, managedcodexappserverrunstate_options(self).value));
  (runtimeState = $$bh$js_obj("threadId", threadId, "cwd", launchcontract_cwd(contract), "model", managedcodexappserveroptions_model(managedcodexappserverrunstate_options(self).value), "turnId", null, "hookRuns", new Set([]), "text", "", "usage", null, "providerDurationMs", null, "terminalSeen", false, "toolItems", 0.0, "invocationObservations", new Map(), "openItems", new Map(), "mcpActivity", managedcodexappserverrunstate_mcp(self), "nativeCommands", managedcodexappserverrunstate_nativeCommands(self).value, "mcpServerNames", mcpServerNames));
  await drainQueued(false);
  if (((!(runtimenotificationstate_hookRuns(runtimeState).size === 0)) || (!($$bc$count(queuedNotifications) === 0)))) {
    (() => { throw new $$be$ExceptionInfo("Codex thread/start left unresolved lifecycle notifications", {}); })();
  }
  let input = launchPrompt;
  const while_break_351 = ({value: false, watches: {}});
  {  while (true) {
    if (((_truthy) => _truthy !== false && _truthy != null)(true)) { const while_continue_352 = ({value: false, watches: {}});
const repeated = await appserverrpc_request_bang(rpc, "config/read", Object.assign({}, {}, {[$$bc$property_key("includeLayers")]: true}, {[$$bc$property_key("cwd")]: launchcontract_cwd(contract)}));
if ((!(validateConfig_bang(repeated, contract, projectWarningSeen) === fingerprint))) {
  (() => { throw new $$be$ExceptionInfo("Codex config authority changed after thread/start", {}); })();
}
validateHooks(await appserverrpc_request_bang(rpc, "hooks/list", Object.assign({}, {}, {[$$bc$property_key("cwds")]: [launchcontract_cwd(contract)]})), launchcontract_cwd(contract));
await validateMcp_bang(rpc, inventory, threadId);
assertNoFilesystemAuthority_bang(launchcontract_codexHome(contract));
((runtimeState).text = "");
((runtimeState).usage = null);
((runtimeState).providerDurationMs = null);
((runtimeState).turnId = null);
((runtimeState).terminalSeen = false);
((runtimeState).toolItems = 0.0);
runtimenotificationstate_invocationObservations(runtimeState).clear();
runtimenotificationstate_openItems(runtimeState).clear();
(interruptEvidence = null);
(turnStartedAt = 0.0);
(lastTurnActivityAt = 0.0);
(turnEventCount = 0.0);
(turnEventCounts = Object.assign({}, {}));
(terminalDeferred = Promise.withResolvers());
(terminalResolve = terminalDeferred.resolve);
(terminalReject = terminalDeferred.reject);
(terminal = terminalDeferred.promise);
terminal.catch(() => null);
null;
(protocolSucceeded = false);
const turnStart = record(await appserverrpc_request_bang(rpc, "turn/start", Object.assign({}, {}, {[$$bc$property_key("threadId")]: threadId}, {[$$bc$property_key("input")]: [Object.assign({}, {}, {[$$bc$property_key("type")]: "text"}, {[$$bc$property_key("text")]: input})]}, (((_truthy) => _truthy !== false && _truthy != null)(managedcodexappserveroptions_effort(managedcodexappserverrunstate_options(self).value)) ? Object.assign({}, {}, {[$$bc$property_key("effort")]: managedcodexappserveroptions_effort(managedcodexappserverrunstate_options(self).value)}) : Object.assign({}, {})))), "Codex turn/start response");
(turnId = validateStartedTurn(turnStart));
((runtimeState).turnId = turnId);
(() => { const _a = managedcodexappserverrunstate_activeTurnInterrupt(self), _v = interruptTurn; const _old = _a.value; _a.value = _v; for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _v); return _v; })();
managedcodexappserverrunstate_private_dispatchPendingReplacementTurnInterrupt_bang(self, interruptTurn);
(turnStartedAt = Date.now());
(lastTurnActivityAt = turnStartedAt);
armTurnDeadline();
await drainQueued(true);
try {
    await terminal;
  } catch (_catch_32) {
    switch ($$bd$catch_dispatch(_catch_32, [$$bd$default_catch])) {
      case 0: {
        const error = _catch_32;
        (() => { throw (() => { const coalesce_value_353 = watchdogReason; return ((coalesce_value_353 == null) ? error : coalesce_value_353); })(); })();
        break;
      }
    }
  } finally {
    if ((managedcodexappserverrunstate_activeTurnInterrupt(self).value === interruptTurn)) {
      (() => { const _a = managedcodexappserverrunstate_activeTurnInterrupt(self), _v = null; const _old = _a.value; _a.value = _v; for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _v); return _v; })();
    }
    null;
  }
clearWatchdogs();
if ((((((!runtimenotificationstate_terminalSeen(runtimeState)) || (!((_truthy) => _truthy !== false && _truthy != null)(runtimenotificationstate_usage(runtimeState)))) || (runtimenotificationstate_providerDurationMs(runtimeState) === null)) || (!(runtimenotificationstate_hookRuns(runtimeState).size === 0))) || (!($$bc$count(queuedNotifications) === 0)))) {
  (() => { throw new $$be$ExceptionInfo("Codex closed without exact terminal usage and lifecycle", {}); })();
}
let settlementDefect = null;
try {
    const terminalConfig = await appserverrpc_request_bang(rpc, "config/read", Object.assign({}, {}, {[$$bc$property_key("includeLayers")]: true}, {[$$bc$property_key("cwd")]: launchcontract_cwd(contract)}));
  if ((!(validateConfig_bang(terminalConfig, contract, projectWarningSeen) === fingerprint))) {
    (() => { throw new $$be$ExceptionInfo("Codex config authority changed at terminal settlement", {}); })();
  }
  appserverrpc_assertHealthy(rpc);
  } catch (_catch_33) {
    switch ($$bd$catch_dispatch(_catch_33, [$$bd$default_catch])) {
      case 0: {
        const error = _catch_33;
        (settlementDefect = ((error instanceof Error) ? error : new Error(String(error))));
        console.error($$bc$str("[codex] managed thread settlement defect after a completed turn: ", $$bc$str("", settlementDefect.message, " — delivering the turn, refusing continuation")));
        break;
      }
    }
  }
(protocolSucceeded = true);
managedcodexappserverrunstate_mcp(self).complete();
if ((!((_truthy) => _truthy !== false && _truthy != null)(managedcodexappserverrunstate_nativeCommands(self).value.complete()))) {
  (() => { throw new $$be$ExceptionInfo("Codex turn completed with unfinished native commands", {}); })();
}
const invocationObservations = invocationObservationInventory(runtimeState);
yield managedCodexResultValue(runtimeState, threadId, turnId, invocationObservations);
(completedTurns = (completedTurns + 1));
settledTurnIds.push(turnId);
(input = await nextInput());
if ((input == null)) {
  (() => { const _a = while_break_351, _v = true; const _old = _a.value; _a.value = _v; for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _v); return _v; })();
} else {
  if (((_truthy) => _truthy !== false && _truthy != null)(settlementDefect)) {
    (() => { throw new Error("Codex refused continuation after a terminal settlement defect", TypeScriptStructuralObjectV2(settlementDefect)); })();
  }
  managedcodexappserverrunstate_mcp(self).reopen();
  managedcodexappserverrunstate_nativeCommands(self).value.reopen();
} if ((!while_break_351.value)) {  continue; } else { null; break; } } else { null; break; }
  } };
  null;
  } catch (_catch_34) {
    switch ($$bd$catch_dispatch(_catch_34, [$$bd$default_catch])) {
      case 0: {
        const error = _catch_34;
        (primaryFailed = true);
        const failedInvocationObservations = (((_truthy) => _truthy !== false && _truthy != null)(runtimeState) ? invocationObservationInventory(runtimeState) : null);
        (failure = (managedcodexappserverrunstate_threadStarted(self).value ? new_ManagedCodexHarvestError(managedcodexappserverrunstate_harvest(self, TypeScriptStructuralObjectV11((() => { const spread_value_275 = (((_truthy) => _truthy !== false && _truthy != null)(interruptEvidence) ? TypeScriptAnonymousObjectV36(interruptEvidence) : Object.assign({}, {})); if (providerContains_p(spread_value_275, "completedTurns")) {
  return providerGet(spread_value_275, "completedTurns");
} else {
  const spread_value_274 = (((_truthy) => _truthy !== false && _truthy != null)(runtimeState) ? pendingItemSnapshot_bang(runtimeState) : Object.assign({}, {}));
  if (providerContains_p(spread_value_274, "completedTurns")) {
    return providerGet(spread_value_274, "completedTurns");
  } else {
    const spread_value_273 = (((_truthy) => _truthy !== false && _truthy != null)(failedInvocationObservations) ? TypeScriptAnonymousObjectV35(failedInvocationObservations) : Object.assign({}, {}));
    if (providerContains_p(spread_value_273, "completedTurns")) {
      return providerGet(spread_value_273, "completedTurns");
    } else {
      const spread_value_272 = (((_truthy) => _truthy !== false && _truthy != null)(runtimeState) ? TypeScriptAnonymousObjectV34(runtimenotificationstate_toolItems(runtimeState)) : Object.assign({}, {}));
      return (providerContains_p(spread_value_272, "completedTurns") ? providerGet(spread_value_272, "completedTurns") : completedTurns);
    }
  }
} })(), (() => { const spread_value_279 = (((_truthy) => _truthy !== false && _truthy != null)(interruptEvidence) ? TypeScriptAnonymousObjectV36(interruptEvidence) : Object.assign({}, {})); if (providerContains_p(spread_value_279, "exitCode")) {
  return providerGet(spread_value_279, "exitCode");
} else {
  const spread_value_278 = (((_truthy) => _truthy !== false && _truthy != null)(runtimeState) ? pendingItemSnapshot_bang(runtimeState) : Object.assign({}, {}));
  if (providerContains_p(spread_value_278, "exitCode")) {
    return providerGet(spread_value_278, "exitCode");
  } else {
    const spread_value_277 = (((_truthy) => _truthy !== false && _truthy != null)(failedInvocationObservations) ? TypeScriptAnonymousObjectV35(failedInvocationObservations) : Object.assign({}, {}));
    if (providerContains_p(spread_value_277, "exitCode")) {
      return providerGet(spread_value_277, "exitCode");
    } else {
      const spread_value_276 = (((_truthy) => _truthy !== false && _truthy != null)(runtimeState) ? TypeScriptAnonymousObjectV34(runtimenotificationstate_toolItems(runtimeState)) : Object.assign({}, {}));
      return (providerContains_p(spread_value_276, "exitCode") ? providerGet(spread_value_276, "exitCode") : null);
    }
  }
} })(), (() => { const spread_value_283 = (((_truthy) => _truthy !== false && _truthy != null)(interruptEvidence) ? TypeScriptAnonymousObjectV36(interruptEvidence) : Object.assign({}, {})); if (providerContains_p(spread_value_283, "exitSignal")) {
  return providerGet(spread_value_283, "exitSignal");
} else {
  const spread_value_282 = (((_truthy) => _truthy !== false && _truthy != null)(runtimeState) ? pendingItemSnapshot_bang(runtimeState) : Object.assign({}, {}));
  if (providerContains_p(spread_value_282, "exitSignal")) {
    return providerGet(spread_value_282, "exitSignal");
  } else {
    const spread_value_281 = (((_truthy) => _truthy !== false && _truthy != null)(failedInvocationObservations) ? TypeScriptAnonymousObjectV35(failedInvocationObservations) : Object.assign({}, {}));
    if (providerContains_p(spread_value_281, "exitSignal")) {
      return providerGet(spread_value_281, "exitSignal");
    } else {
      const spread_value_280 = (((_truthy) => _truthy !== false && _truthy != null)(runtimeState) ? TypeScriptAnonymousObjectV34(runtimenotificationstate_toolItems(runtimeState)) : Object.assign({}, {}));
      return (providerContains_p(spread_value_280, "exitSignal") ? providerGet(spread_value_280, "exitSignal") : null);
    }
  }
} })(), (() => { const spread_value_287 = (((_truthy) => _truthy !== false && _truthy != null)(interruptEvidence) ? TypeScriptAnonymousObjectV36(interruptEvidence) : Object.assign({}, {})); if (providerContains_p(spread_value_287, "interrupt")) {
  return providerGet(spread_value_287, "interrupt");
} else {
  const spread_value_286 = (((_truthy) => _truthy !== false && _truthy != null)(runtimeState) ? pendingItemSnapshot_bang(runtimeState) : Object.assign({}, {}));
  if (providerContains_p(spread_value_286, "interrupt")) {
    return providerGet(spread_value_286, "interrupt");
  } else {
    const spread_value_285 = (((_truthy) => _truthy !== false && _truthy != null)(failedInvocationObservations) ? TypeScriptAnonymousObjectV35(failedInvocationObservations) : Object.assign({}, {}));
    if (providerContains_p(spread_value_285, "interrupt")) {
      return providerGet(spread_value_285, "interrupt");
    } else {
      const spread_value_284 = (((_truthy) => _truthy !== false && _truthy != null)(runtimeState) ? TypeScriptAnonymousObjectV34(runtimenotificationstate_toolItems(runtimeState)) : Object.assign({}, {}));
      return (providerContains_p(spread_value_284, "interrupt") ? providerGet(spread_value_284, "interrupt") : null);
    }
  }
} })(), (() => { const spread_value_291 = (((_truthy) => _truthy !== false && _truthy != null)(interruptEvidence) ? TypeScriptAnonymousObjectV36(interruptEvidence) : Object.assign({}, {})); if (providerContains_p(spread_value_291, "invocationObservations")) {
  return providerGet(spread_value_291, "invocationObservations");
} else {
  const spread_value_290 = (((_truthy) => _truthy !== false && _truthy != null)(runtimeState) ? pendingItemSnapshot_bang(runtimeState) : Object.assign({}, {}));
  if (providerContains_p(spread_value_290, "invocationObservations")) {
    return providerGet(spread_value_290, "invocationObservations");
  } else {
    const spread_value_289 = (((_truthy) => _truthy !== false && _truthy != null)(failedInvocationObservations) ? TypeScriptAnonymousObjectV35(failedInvocationObservations) : Object.assign({}, {}));
    if (providerContains_p(spread_value_289, "invocationObservations")) {
      return providerGet(spread_value_289, "invocationObservations");
    } else {
      const spread_value_288 = (((_truthy) => _truthy !== false && _truthy != null)(runtimeState) ? TypeScriptAnonymousObjectV34(runtimenotificationstate_toolItems(runtimeState)) : Object.assign({}, {}));
      return (providerContains_p(spread_value_288, "invocationObservations") ? providerGet(spread_value_288, "invocationObservations") : null);
    }
  }
} })(), (() => { const spread_value_295 = (((_truthy) => _truthy !== false && _truthy != null)(interruptEvidence) ? TypeScriptAnonymousObjectV36(interruptEvidence) : Object.assign({}, {})); if (providerContains_p(spread_value_295, "pendingItemCount")) {
  return providerGet(spread_value_295, "pendingItemCount");
} else {
  const spread_value_294 = (((_truthy) => _truthy !== false && _truthy != null)(runtimeState) ? pendingItemSnapshot_bang(runtimeState) : Object.assign({}, {}));
  if (providerContains_p(spread_value_294, "pendingItemCount")) {
    return providerGet(spread_value_294, "pendingItemCount");
  } else {
    const spread_value_293 = (((_truthy) => _truthy !== false && _truthy != null)(failedInvocationObservations) ? TypeScriptAnonymousObjectV35(failedInvocationObservations) : Object.assign({}, {}));
    if (providerContains_p(spread_value_293, "pendingItemCount")) {
      return providerGet(spread_value_293, "pendingItemCount");
    } else {
      const spread_value_292 = (((_truthy) => _truthy !== false && _truthy != null)(runtimeState) ? TypeScriptAnonymousObjectV34(runtimenotificationstate_toolItems(runtimeState)) : Object.assign({}, {}));
      return (providerContains_p(spread_value_292, "pendingItemCount") ? providerGet(spread_value_292, "pendingItemCount") : null);
    }
  }
} })(), (() => { const spread_value_299 = (((_truthy) => _truthy !== false && _truthy != null)(interruptEvidence) ? TypeScriptAnonymousObjectV36(interruptEvidence) : Object.assign({}, {})); if (providerContains_p(spread_value_299, "pendingItems")) {
  return providerGet(spread_value_299, "pendingItems");
} else {
  const spread_value_298 = (((_truthy) => _truthy !== false && _truthy != null)(runtimeState) ? pendingItemSnapshot_bang(runtimeState) : Object.assign({}, {}));
  if (providerContains_p(spread_value_298, "pendingItems")) {
    return providerGet(spread_value_298, "pendingItems");
  } else {
    const spread_value_297 = (((_truthy) => _truthy !== false && _truthy != null)(failedInvocationObservations) ? TypeScriptAnonymousObjectV35(failedInvocationObservations) : Object.assign({}, {}));
    if (providerContains_p(spread_value_297, "pendingItems")) {
      return providerGet(spread_value_297, "pendingItems");
    } else {
      const spread_value_296 = (((_truthy) => _truthy !== false && _truthy != null)(runtimeState) ? TypeScriptAnonymousObjectV34(runtimenotificationstate_toolItems(runtimeState)) : Object.assign({}, {}));
      return (providerContains_p(spread_value_296, "pendingItems") ? providerGet(spread_value_296, "pendingItems") : null);
    }
  }
} })(), (() => { const spread_value_303 = (((_truthy) => _truthy !== false && _truthy != null)(interruptEvidence) ? TypeScriptAnonymousObjectV36(interruptEvidence) : Object.assign({}, {})); if (providerContains_p(spread_value_303, "respawnCount")) {
  return providerGet(spread_value_303, "respawnCount");
} else {
  const spread_value_302 = (((_truthy) => _truthy !== false && _truthy != null)(runtimeState) ? pendingItemSnapshot_bang(runtimeState) : Object.assign({}, {}));
  if (providerContains_p(spread_value_302, "respawnCount")) {
    return providerGet(spread_value_302, "respawnCount");
  } else {
    const spread_value_301 = (((_truthy) => _truthy !== false && _truthy != null)(failedInvocationObservations) ? TypeScriptAnonymousObjectV35(failedInvocationObservations) : Object.assign({}, {}));
    if (providerContains_p(spread_value_301, "respawnCount")) {
      return providerGet(spread_value_301, "respawnCount");
    } else {
      const spread_value_300 = (((_truthy) => _truthy !== false && _truthy != null)(runtimeState) ? TypeScriptAnonymousObjectV34(runtimenotificationstate_toolItems(runtimeState)) : Object.assign({}, {}));
      return (providerContains_p(spread_value_300, "respawnCount") ? providerGet(spread_value_300, "respawnCount") : null);
    }
  }
} })(), (() => { const spread_value_307 = (((_truthy) => _truthy !== false && _truthy != null)(interruptEvidence) ? TypeScriptAnonymousObjectV36(interruptEvidence) : Object.assign({}, {})); if (providerContains_p(spread_value_307, "respawns")) {
  return providerGet(spread_value_307, "respawns");
} else {
  const spread_value_306 = (((_truthy) => _truthy !== false && _truthy != null)(runtimeState) ? pendingItemSnapshot_bang(runtimeState) : Object.assign({}, {}));
  if (providerContains_p(spread_value_306, "respawns")) {
    return providerGet(spread_value_306, "respawns");
  } else {
    const spread_value_305 = (((_truthy) => _truthy !== false && _truthy != null)(failedInvocationObservations) ? TypeScriptAnonymousObjectV35(failedInvocationObservations) : Object.assign({}, {}));
    if (providerContains_p(spread_value_305, "respawns")) {
      return providerGet(spread_value_305, "respawns");
    } else {
      const spread_value_304 = (((_truthy) => _truthy !== false && _truthy != null)(runtimeState) ? TypeScriptAnonymousObjectV34(runtimenotificationstate_toolItems(runtimeState)) : Object.assign({}, {}));
      return (providerContains_p(spread_value_304, "respawns") ? providerGet(spread_value_304, "respawns") : null);
    }
  }
} })(), (() => { const spread_value_311 = (((_truthy) => _truthy !== false && _truthy != null)(interruptEvidence) ? TypeScriptAnonymousObjectV36(interruptEvidence) : Object.assign({}, {})); if (providerContains_p(spread_value_311, "stderrTail")) {
  return providerGet(spread_value_311, "stderrTail");
} else {
  const spread_value_310 = (((_truthy) => _truthy !== false && _truthy != null)(runtimeState) ? pendingItemSnapshot_bang(runtimeState) : Object.assign({}, {}));
  if (providerContains_p(spread_value_310, "stderrTail")) {
    return providerGet(spread_value_310, "stderrTail");
  } else {
    const spread_value_309 = (((_truthy) => _truthy !== false && _truthy != null)(failedInvocationObservations) ? TypeScriptAnonymousObjectV35(failedInvocationObservations) : Object.assign({}, {}));
    if (providerContains_p(spread_value_309, "stderrTail")) {
      return providerGet(spread_value_309, "stderrTail");
    } else {
      const spread_value_308 = (((_truthy) => _truthy !== false && _truthy != null)(runtimeState) ? TypeScriptAnonymousObjectV34(runtimenotificationstate_toolItems(runtimeState)) : Object.assign({}, {}));
      return (providerContains_p(spread_value_308, "stderrTail") ? providerGet(spread_value_308, "stderrTail") : null);
    }
  }
} })(), (() => { const spread_value_317 = (((_truthy) => _truthy !== false && _truthy != null)(interruptEvidence) ? TypeScriptAnonymousObjectV36(interruptEvidence) : Object.assign({}, {})); if (providerContains_p(spread_value_317, "text")) {
  return providerGet(spread_value_317, "text");
} else {
  const spread_value_316 = (((_truthy) => _truthy !== false && _truthy != null)(runtimeState) ? pendingItemSnapshot_bang(runtimeState) : Object.assign({}, {}));
  if (providerContains_p(spread_value_316, "text")) {
    return providerGet(spread_value_316, "text");
  } else {
    const spread_value_315 = (((_truthy) => _truthy !== false && _truthy != null)(failedInvocationObservations) ? TypeScriptAnonymousObjectV35(failedInvocationObservations) : Object.assign({}, {}));
    if (providerContains_p(spread_value_315, "text")) {
      return providerGet(spread_value_315, "text");
    } else {
      const spread_value_314 = (((_truthy) => _truthy !== false && _truthy != null)(runtimeState) ? TypeScriptAnonymousObjectV34(runtimenotificationstate_toolItems(runtimeState)) : Object.assign({}, {}));
      if (providerContains_p(spread_value_314, "text")) {
        return providerGet(spread_value_314, "text");
      } else {
        const coalesce_value_313 = (() => { const optional_value_312 = runtimeState; return ((optional_value_312 == null) ? null : optional_value_312.text); })();
        return ((coalesce_value_313 == null) ? "" : coalesce_value_313);
      }
    }
  }
} })(), (() => { const spread_value_321 = (((_truthy) => _truthy !== false && _truthy != null)(interruptEvidence) ? TypeScriptAnonymousObjectV36(interruptEvidence) : Object.assign({}, {})); if (providerContains_p(spread_value_321, "threadId")) {
  return providerGet(spread_value_321, "threadId");
} else {
  const spread_value_320 = (((_truthy) => _truthy !== false && _truthy != null)(runtimeState) ? pendingItemSnapshot_bang(runtimeState) : Object.assign({}, {}));
  if (providerContains_p(spread_value_320, "threadId")) {
    return providerGet(spread_value_320, "threadId");
  } else {
    const spread_value_319 = (((_truthy) => _truthy !== false && _truthy != null)(failedInvocationObservations) ? TypeScriptAnonymousObjectV35(failedInvocationObservations) : Object.assign({}, {}));
    if (providerContains_p(spread_value_319, "threadId")) {
      return providerGet(spread_value_319, "threadId");
    } else {
      const spread_value_318 = (((_truthy) => _truthy !== false && _truthy != null)(runtimeState) ? TypeScriptAnonymousObjectV34(runtimenotificationstate_toolItems(runtimeState)) : Object.assign({}, {}));
      return (providerContains_p(spread_value_318, "threadId") ? providerGet(spread_value_318, "threadId") : threadId);
    }
  }
} })(), (() => { const spread_value_325 = (((_truthy) => _truthy !== false && _truthy != null)(interruptEvidence) ? TypeScriptAnonymousObjectV36(interruptEvidence) : Object.assign({}, {})); if (providerContains_p(spread_value_325, "toolItems")) {
  return providerGet(spread_value_325, "toolItems");
} else {
  const spread_value_324 = (((_truthy) => _truthy !== false && _truthy != null)(runtimeState) ? pendingItemSnapshot_bang(runtimeState) : Object.assign({}, {}));
  if (providerContains_p(spread_value_324, "toolItems")) {
    return providerGet(spread_value_324, "toolItems");
  } else {
    const spread_value_323 = (((_truthy) => _truthy !== false && _truthy != null)(failedInvocationObservations) ? TypeScriptAnonymousObjectV35(failedInvocationObservations) : Object.assign({}, {}));
    if (providerContains_p(spread_value_323, "toolItems")) {
      return providerGet(spread_value_323, "toolItems");
    } else {
      const spread_value_322 = (((_truthy) => _truthy !== false && _truthy != null)(runtimeState) ? TypeScriptAnonymousObjectV34(runtimenotificationstate_toolItems(runtimeState)) : Object.assign({}, {}));
      return (providerContains_p(spread_value_322, "toolItems") ? providerGet(spread_value_322, "toolItems") : null);
    }
  }
} })(), (() => { const spread_value_329 = (((_truthy) => _truthy !== false && _truthy != null)(interruptEvidence) ? TypeScriptAnonymousObjectV36(interruptEvidence) : Object.assign({}, {})); if (providerContains_p(spread_value_329, "turnIds")) {
  return providerGet(spread_value_329, "turnIds");
} else {
  const spread_value_328 = (((_truthy) => _truthy !== false && _truthy != null)(runtimeState) ? pendingItemSnapshot_bang(runtimeState) : Object.assign({}, {}));
  if (providerContains_p(spread_value_328, "turnIds")) {
    return providerGet(spread_value_328, "turnIds");
  } else {
    const spread_value_327 = (((_truthy) => _truthy !== false && _truthy != null)(failedInvocationObservations) ? TypeScriptAnonymousObjectV35(failedInvocationObservations) : Object.assign({}, {}));
    if (providerContains_p(spread_value_327, "turnIds")) {
      return providerGet(spread_value_327, "turnIds");
    } else {
      const spread_value_326 = (((_truthy) => _truthy !== false && _truthy != null)(runtimeState) ? TypeScriptAnonymousObjectV34(runtimenotificationstate_toolItems(runtimeState)) : Object.assign({}, {}));
      return (providerContains_p(spread_value_326, "turnIds") ? providerGet(spread_value_326, "turnIds") : (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (!((_truthy) => _truthy !== false && _truthy != null)(settledTurnIds.includes(turnId))) : _logical))(turnId)) ? $$bc$conj_value($$bc$into_value([], settledTurnIds), turnId) : $$bc$into_value([], settledTurnIds)));
    }
  }
} })(), (() => { const spread_value_333 = (((_truthy) => _truthy !== false && _truthy != null)(interruptEvidence) ? TypeScriptAnonymousObjectV36(interruptEvidence) : Object.assign({}, {})); return (providerContains_p(spread_value_333, "unsupportedNotifications") ? providerGet(spread_value_333, "unsupportedNotifications") : appserverrpc_unsupportedNotifications(rpc)); })(), (() => { const spread_value_338 = (((_truthy) => _truthy !== false && _truthy != null)(interruptEvidence) ? TypeScriptAnonymousObjectV36(interruptEvidence) : Object.assign({}, {})); if (providerContains_p(spread_value_338, "usage")) {
  return providerGet(spread_value_338, "usage");
} else {
  const optional_value_337 = runtimeState;
  return ((optional_value_337 == null) ? null : optional_value_337.usage);
} })())), TypeScriptStructuralObjectV2(error)) : new_ManagedCodexPreThreadError("openai_codex_authority_preflight_failed", TypeScriptStructuralObjectV2(error))));
        const diagnostics = providerDiagnostics();
        attachDiagnostics_bang(failure, diagnostics);
        if (((managedCodexHarvestError_p(failure) && (!((_truthy) => _truthy !== false && _truthy != null)(watchdogReason))) && (appserverrpc_diedFromProcessDeath(rpc) || (managedcodexdiagnostics_providerAlive(diagnostics) === false)))) {
          (() => { const _a = managedcodexappserverrunstate_attemptDeath(self), _v = TypeScriptAnonymousObjectV15(diagnostics, ((error instanceof Error) ? (error).message : String(error))); const _old = _a.value; _a.value = _v; for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _v); return _v; })();
          (() => { const _a = managedcodexappserverrunstate_attemptFailure(self), _v = failure; const _old = _a.value; _a.value = _v; for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _v); return _v; })();
          managedcodexappserverrunstate_private_beginReplacementTurn_bang(self);
        }
        (() => { throw failure; })();
        break;
      }
    }
  } finally {
    clearWatchdogs();
    (supervisorstatuschannel_settled(supervisor))();
    removeTerminal();
    try {
    await closeProcess_bang(child, rpc, control);
  if (protocolSucceeded) {
    appserverrpc_assertHealthy(rpc);
  }
  null;
  } catch (_catch_35) {
    switch ($$bd$catch_dispatch(_catch_35, [$$bd$default_catch])) {
      case 0: {
        const error = _catch_35;
        if ((!primaryFailed)) {
          (() => { throw new Error("openai_provider_execution_failed", TypeScriptStructuralObjectV2(error)); })();
        }
        null;
        break;
      }
    }
  }
    if (((_truthy) => _truthy !== false && _truthy != null)(failure)) {
      const alive = (() => { const optional_value_339 = failure.diagnostics; return ((optional_value_339 == null) ? null : optional_value_339.providerAlive); })();
      const baseDiagnostics = providerDiagnostics();
      attachDiagnostics_bang(failure, ManagedCodexDiagnostics(managedcodexdiagnostics_stderrTail(baseDiagnostics), managedcodexdiagnostics_exitCode(baseDiagnostics), managedcodexdiagnostics_exitSignal(baseDiagnostics), ((alive === null) ? managedcodexdiagnostics_providerAlive(baseDiagnostics) : alive)));
      if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? failure.diagnostics : _logical))(managedcodexappserverrunstate_attemptDeath(self).value))) {
        const death = managedcodexappserverrunstate_attemptDeath(self).value;
        (() => { const _a = managedcodexappserverrunstate_attemptDeath(self), _v = TypeScriptAnonymousObjectV15(failure.diagnostics, death.reason); const _old = _a.value; _a.value = _v; for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _v); return _v; })();
      }
      null;
    }
    (supervisorstatuschannel_close(supervisor))();
    (() => { const _a = managedcodexappserverrunstate_child(self), _v = null; const _old = _a.value; _a.value = _v; for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _v); return _v; })();
    (() => { const _a = managedcodexappserverrunstate_rpc(self), _v = null; const _old = _a.value; _a.value = _v; for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _v); return _v; })();
    (() => { const _a = managedcodexappserverrunstate_control(self), _v = null; const _old = _a.value; _a.value = _v; for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _v); return _v; })();
    (() => { const _a = managedcodexappserverrunstate_activeTurnInterrupt(self), _v = null; const _old = _a.value; _a.value = _v; for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _v); return _v; })();
  }
  }
}

export { MANAGED__CODEX__DISABLED__FEATURES as "MANAGED_CODEX_DISABLED_FEATURES" };
export { MANAGED__CODEX__ENABLED__FEATURES as "MANAGED_CODEX_ENABLED_FEATURES" };
export { MANAGED__CODEX__VERSION as "MANAGED_CODEX_VERSION" };
export { ManagedCodexAppServerRun as "ManagedCodexAppServerRun" };
export { ManagedCodexHarvestError as "ManagedCodexHarvestError" };
export { ManagedCodexPreThreadError as "ManagedCodexPreThreadError" };
export { managedCodexAppServerLaunch as "managedCodexAppServerLaunch" };
export { managedCodexRecoveredContext as "managedCodexRecoveredContext" };
export { managedCodexWritableRoots as "managedCodexWritableRoots" };
export { projectConfigWarningCorrelates as "projectConfigWarningCorrelates" };
