import { spawnSync } from 'child_process';
import { homedir } from 'os';
import { join, resolve } from 'path';
import { keyword as $$bc$keyword, property_key as $$bc$property_key, record_value as $$bc$record_value, str as $$bc$str } from '../../beagle/core.js';
import { aset as $$bh$aset, host_object as $$bh$host_object } from '../../beagle/host.js';

// BridgeRequestModel = LaunchRequest | AttachRequest | SubmitInputRequest | InterruptTurnRequest | RedirectNowRequest | TerminateSessionRequest | RetireRequest
function LaunchRequest(prompt, cwd, role, attemptId, executionId, provider, tier, model, effort) { return $$bc$record_value("north.bridge.protocol/LaunchRequest", { _tag: "LaunchRequest", prompt: prompt, cwd: cwd, role: role, attemptId: attemptId, executionId: executionId, provider: provider, tier: tier, model: model, effort: effort }); }

function launchrequest_prompt(r) { return r.prompt; }

function launchrequest_cwd(r) { return r.cwd; }

function launchrequest_role(r) { return r.role; }

function launchrequest_attemptId(r) { return r.attemptId; }

function launchrequest_executionId(r) { return r.executionId; }

function launchrequest_provider(r) { return r.provider; }

function launchrequest_tier(r) { return r.tier; }

function launchrequest_model(r) { return r.model; }

function launchrequest_effort(r) { return r.effort; }
function AttachRequest(executionId, cursor) { return $$bc$record_value("north.bridge.protocol/AttachRequest", { _tag: "AttachRequest", executionId: executionId, cursor: cursor }); }

function attachrequest_executionId(r) { return r.executionId; }

function attachrequest_cursor(r) { return r.cursor; }
function SubmitInputRequest(executionId, input) { return $$bc$record_value("north.bridge.protocol/SubmitInputRequest", { _tag: "SubmitInputRequest", executionId: executionId, input: input }); }

function submitinputrequest_executionId(r) { return r.executionId; }

function submitinputrequest_input(r) { return r.input; }
function InterruptTurnRequest(executionId) { return $$bc$record_value("north.bridge.protocol/InterruptTurnRequest", { _tag: "InterruptTurnRequest", executionId: executionId }); }

function interruptturnrequest_executionId(r) { return r.executionId; }
function RedirectNowRequest(executionId, input) { return $$bc$record_value("north.bridge.protocol/RedirectNowRequest", { _tag: "RedirectNowRequest", executionId: executionId, input: input }); }

function redirectnowrequest_executionId(r) { return r.executionId; }

function redirectnowrequest_input(r) { return r.input; }
function TerminateSessionRequest(executionId) { return $$bc$record_value("north.bridge.protocol/TerminateSessionRequest", { _tag: "TerminateSessionRequest", executionId: executionId }); }

function terminatesessionrequest_executionId(r) { return r.executionId; }
function RetireRequest(op) { return $$bc$record_value("north.bridge.protocol/RetireRequest", { _tag: "RetireRequest", op: op }); }

function retirerequest_op(r) { return r.op; }

function BridgeHelloModel(identity, liveExecutions, pinningExecutions, pid) {
  return $$bc$record_value("north.bridge.protocol/BridgeHelloModel", {_tag: "BridgeHelloModel", identity, liveExecutions, pinningExecutions, pid});
}

function bridgehellomodel_identity(r) { return r.identity; }

function bridgehellomodel_liveExecutions(r) { return r.liveExecutions; }

function bridgehellomodel_pinningExecutions(r) { return r.pinningExecutions; }

function bridgehellomodel_pid(r) { return r.pid; }

const SEMANTIC_TIERS = ["economy", "standard", "senior", "frontier"];

const REASONING_LEVELS = ["low", "medium", "high", "xhigh", "max"];

function error_bang(message) {
  return (() => { throw new Error(message); })();
}

function matches_p(pattern, value) {
  return new RegExp(pattern, "i").test(value);
}

function bridge_source_identity() {
  const repo = resolve(import.meta.dir, "../../../../../..");
  const configured = process.env.NORTH_GIT_BIN;
  const git = (((_truthy) => _truthy !== false && _truthy != null)(((typeof configured === "string") && (!(configured === "")))) ? configured : "git");
  const result = spawnSync(git, ["-C", repo, "rev-parse", "HEAD"], {[$$bc$property_key($$bc$keyword("encoding"))]: "utf8"});
  const stdout = result.stdout;
  if (((_truthy) => _truthy !== false && _truthy != null)(((result.status === 0) && (typeof stdout === "string")))) {
    const revision = stdout.trim();
    return (matches_p("^[0-9a-f]{40}([0-9a-f]{24})?$", revision) ? revision : null);
  } else {
    return null;
  }
}

function pinning_executions(hello) {
  const pinning = hello.pinningExecutions;
  return ((typeof pinning === "number") ? pinning : hello.liveExecutions);
}

function parse_bridge_launch_provider_bang(value) {
  return (((value == null)) ? null : (((_truthy) => _truthy !== false && _truthy != null)(((value === "anthropic") || (value === "openai")))) ? value : error_bang("bridge launch provider must be anthropic or openai"));
}

function parse_bridge_launch_tier_bang(value) {
  return (((value == null)) ? null : (((_truthy) => _truthy !== false && _truthy != null)(((typeof value === "string") && SEMANTIC_TIERS.includes(value)))) ? value : error_bang($$bc$str("bridge launch tier must be one of: ", SEMANTIC_TIERS.join(", "))));
}

function parse_bridge_launch_model_bang(value) {
  return (((value == null)) ? null : (((_truthy) => _truthy !== false && _truthy != null)(((typeof value === "string") && matches_p("^[A-Za-z0-9][A-Za-z0-9._:/-]{0,199}$", value)))) ? value : error_bang("bridge launch model must be a model id without whitespace"));
}

function parse_bridge_launch_effort_bang(value) {
  return (((value == null)) ? null : (((_truthy) => _truthy !== false && _truthy != null)(((typeof value === "string") && REASONING_LEVELS.includes(value)))) ? value : error_bang($$bc$str("bridge launch effort must be one of: ", REASONING_LEVELS.join(", "))));
}

function parse_bridge_launch_role_bang(value) {
  return (((value == null)) ? "implementer" : (((_truthy) => _truthy !== false && _truthy != null)(((value === "director") || (value === "implementer")))) ? value : error_bang("bridge launch role must be director or implementer"));
}

function resolve_bridge_state_directory(env) {
  const configured = env.NORTH_BRIDGE_STATE_DIR;
  const trimmed = ((typeof configured === "string") ? configured.trim() : "");
  return ((trimmed === "") ? join(homedir(), ".local/state/north/bridge") : resolve(trimmed));
}

function bridge_state_directory(...$beagle$args) {
  if (arguments.length === 0) {
    return resolve_bridge_state_directory(process.env);
  }
  if (arguments.length === 1) {
    const env = $beagle$args[0];
    return resolve_bridge_state_directory(env);
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

function bridge_socket_path(...$beagle$args) {
  if (arguments.length === 0) {
    return join(resolve_bridge_state_directory(process.env), "northd.sock");
  }
  if (arguments.length === 1) {
    const env = $beagle$args[0];
    return join(resolve_bridge_state_directory(env), "northd.sock");
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

function bridge_journal_root(...$beagle$args) {
  if (arguments.length === 0) {
    return join(resolve_bridge_state_directory(process.env), "journal");
  }
  if (arguments.length === 1) {
    const env = $beagle$args[0];
    return join(resolve_bridge_state_directory(env), "journal");
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

function parse_execution_id_bang(value, operation) {
  return (((_truthy) => _truthy !== false && _truthy != null)(((typeof value === "string") && (matches_p("^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$", value) && ((!(value === ".")) && (!(value === "..")))))) ? value : error_bang($$bc$str("bridge ", operation, " requires a safe execution id")));
}

function parse_bridge_launch_attempt_id_bang(value) {
  return (((_truthy) => _truthy !== false && _truthy != null)(((typeof value === "string") && matches_p("^@attempt:[0-9a-f]{64}$", value))) ? value : error_bang("bridge launch requires a canonical reserved attempt id"));
}

function parse_bridge_launch_execution_id_bang(value) {
  return (((_truthy) => _truthy !== false && _truthy != null)(((typeof value === "string") && matches_p("^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$", value))) ? value.toLowerCase() : error_bang("bridge launch execution id must be a UUIDv4"));
}

function launch_request_bang(request) {
  const prompt = request.prompt;
  const cwd = request.cwd;
  if (((_truthy) => _truthy !== false && _truthy != null)(((!(typeof prompt === "string")) || (prompt.trim() === "")))) {
    error_bang("bridge launch requires a non-empty prompt");
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(((!(typeof cwd === "string")) || (cwd === "")))) {
    error_bang("bridge launch requires cwd");
  }
  const provider = parse_bridge_launch_provider_bang(request.provider);
  const tier = parse_bridge_launch_tier_bang(request.tier);
  const model = parse_bridge_launch_model_bang(request.model);
  const effort = parse_bridge_launch_effort_bang(request.effort);
  const execution_id = ((request.executionId == null) ? null : parse_bridge_launch_execution_id_bang(request.executionId));
  const role = parse_bridge_launch_role_bang(request.role);
  const attempt_id = parse_bridge_launch_attempt_id_bang(request.attemptId);
  return LaunchRequest(prompt, cwd, role, attempt_id, execution_id, provider, tier, model, effort);
}

function request_model_bang(request) {
  const op = request.op;
  return (((op === "launch")) ? launch_request_bang(request) : ((op === "retire")) ? RetireRequest("retire") : ((op === "attach")) ? (() => { const cursor = request.cursor; if (((_truthy) => _truthy !== false && _truthy != null)(((!Number.isSafeInteger(cursor)) || (cursor < 0)))) {
  error_bang("bridge attach cursor must be a non-negative integer");
}
return AttachRequest(parse_execution_id_bang(request.executionId, "attach"), cursor); })() : ((op === "submitInput")) ? (() => { const input = request.input; if (((_truthy) => _truthy !== false && _truthy != null)(((!(typeof input === "string")) || (input.trim() === "")))) {
  error_bang("bridge submitInput requires non-empty input");
}
return SubmitInputRequest(parse_execution_id_bang(request.executionId, "submitInput"), input); })() : ((op === "redirectNow")) ? (() => { const input = request.input; if (((_truthy) => _truthy !== false && _truthy != null)(((!(typeof input === "string")) || (input.trim() === "")))) {
  error_bang("bridge redirectNow requires non-empty input");
}
return RedirectNowRequest(parse_execution_id_bang(request.executionId, "redirectNow"), input); })() : ((op === "interruptTurn")) ? InterruptTurnRequest(parse_execution_id_bang(request.executionId, "interruptTurn")) : ((op === "terminateSession")) ? TerminateSessionRequest(parse_execution_id_bang(request.executionId, "terminateSession")) : error_bang("unknown bridge request"));
}

function optional_field_bang(target, key, value) {
  if (((_truthy) => _truthy !== false && _truthy != null)(value)) {
    $$bh$aset(target, key, value);
  }
  return target;
}

function request_wire_bang(request) {
  return (() => { const _match_0 = request; if (_match_0._tag === "LaunchRequest") { const prompt = _match_0.prompt; const cwd = _match_0.cwd; const role = _match_0.role; const attemptId = _match_0.attemptId; const executionId = _match_0.executionId; const provider = _match_0.provider; const tier = _match_0.tier; const model = _match_0.model; const effort = _match_0.effort; return (() => { const wire = $$bh$host_object($$bc$keyword("op"), "launch", $$bc$keyword("prompt"), prompt, $$bc$keyword("cwd"), cwd, $$bc$keyword("role"), role, $$bc$keyword("attemptId"), attemptId); optional_field_bang(wire, "executionId", executionId);
optional_field_bang(wire, "provider", provider);
optional_field_bang(wire, "tier", tier);
optional_field_bang(wire, "model", model);
optional_field_bang(wire, "effort", effort);
return wire; })(); } else if (_match_0._tag === "AttachRequest") { const executionId = _match_0.executionId; const cursor = _match_0.cursor; return {[$$bc$property_key($$bc$keyword("op"))]: "attach", [$$bc$property_key($$bc$keyword("executionId"))]: executionId, [$$bc$property_key($$bc$keyword("cursor"))]: cursor}; } else if (_match_0._tag === "SubmitInputRequest") { const executionId = _match_0.executionId; const input = _match_0.input; return {[$$bc$property_key($$bc$keyword("op"))]: "submitInput", [$$bc$property_key($$bc$keyword("executionId"))]: executionId, [$$bc$property_key($$bc$keyword("input"))]: input}; } else if (_match_0._tag === "InterruptTurnRequest") { const executionId = _match_0.executionId; return {[$$bc$property_key($$bc$keyword("op"))]: "interruptTurn", [$$bc$property_key($$bc$keyword("executionId"))]: executionId}; } else if (_match_0._tag === "RedirectNowRequest") { const executionId = _match_0.executionId; const input = _match_0.input; return {[$$bc$property_key($$bc$keyword("op"))]: "redirectNow", [$$bc$property_key($$bc$keyword("executionId"))]: executionId, [$$bc$property_key($$bc$keyword("input"))]: input}; } else if (_match_0._tag === "TerminateSessionRequest") { const executionId = _match_0.executionId; return {[$$bc$property_key($$bc$keyword("op"))]: "terminateSession", [$$bc$property_key($$bc$keyword("executionId"))]: executionId}; } else if (_match_0._tag === "RetireRequest") { const __op = _match_0.op; return {[$$bc$property_key($$bc$keyword("op"))]: "retire"}; } else { return null; } })();
}

function parse_bridge_request_bang(value) {
  if (((_truthy) => _truthy !== false && _truthy != null)(((!((_truthy) => _truthy !== false && _truthy != null)(value)) || ((!(typeof value === "object")) || Array.isArray(value))))) {
    error_bang("bridge request must be an object");
  }
  return request_wire_bang(request_model_bang(value));
}

export { bridge_journal_root as "bridge-journal-root" };
export { bridge_socket_path as "bridge-socket-path" };
export { bridge_source_identity as "bridge-source-identity" };
export { bridge_state_directory as "bridge-state-directory" };
export { parse_bridge_launch_attempt_id_bang as "parse-bridge-launch-attempt-id!" };
export { parse_bridge_launch_effort_bang as "parse-bridge-launch-effort!" };
export { parse_bridge_launch_execution_id_bang as "parse-bridge-launch-execution-id!" };
export { parse_bridge_launch_model_bang as "parse-bridge-launch-model!" };
export { parse_bridge_launch_provider_bang as "parse-bridge-launch-provider!" };
export { parse_bridge_launch_role_bang as "parse-bridge-launch-role!" };
export { parse_bridge_launch_tier_bang as "parse-bridge-launch-tier!" };
export { parse_bridge_request_bang as "parse-bridge-request!" };
export { pinning_executions as "pinning-executions" };
