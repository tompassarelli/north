import { "spawn" as spawn } from "node:child_process";
import { "Socket" as Socket } from "node:net";
import { "resolve" as resolve } from "node:path";
import { "run-northbridge-app!" as run_northbridge_app_bang } from "./app.js";
import { "prepare-managed-bridge-app-launch!" as prepare_managed_bridge_app_launch_bang } from "./app-launch-reservation.js";
import { "bridge-journal-root" as bridge_journal_root, "bridge-socket-path" as bridge_socket_path, "bridge-source-identity" as bridge_source_identity, "parse-bridge-launch-attempt-id!" as parse_bridge_launch_attempt_id_bang, "parse-bridge-launch-effort!" as parse_bridge_launch_effort_bang, "parse-bridge-launch-model!" as parse_bridge_launch_model_bang, "parse-bridge-launch-provider!" as parse_bridge_launch_provider_bang, "parse-bridge-launch-role!" as parse_bridge_launch_role_bang, "parse-bridge-launch-tier!" as parse_bridge_launch_tier_bang, "pinning-executions" as pinning_executions } from "./protocol.js";
import { "acquireFileLease" as acquireFileLease } from "north-sdk/internal/file-lease";
import { "runBridgeAcceptance" as runBridgeAcceptance } from "north-sdk/internal/bridge-accept";
import { "markLaneConsumed" as markLaneConsumed, "pendingLanes" as pendingLanes } from "north-sdk/internal/bridge-pending";
import { keyword as $$bc$keyword, property_key as $$bc$property_key, record_value as $$bc$record_value, str as $$bc$str } from '../../beagle/core.js';
import { aset as $$bh$aset, host_object as $$bh$host_object } from '../../beagle/host.js';
import { catch_dispatch as $$bd$catch_dispatch } from '../../beagle/exception-dispatch.js';

function LaunchArgumentsModel(role, attemptId, provider, tier, model, effort, promptArguments) {
  return $$bc$record_value("north.bridge.cli/LaunchArgumentsModel", {_tag: "LaunchArgumentsModel", role, attemptId, provider, tier, model, effort, promptArguments});
}

function launchargumentsmodel_role(r) { return r.role; }

function launchargumentsmodel_attemptId(r) { return r.attemptId; }

function launchargumentsmodel_provider(r) { return r.provider; }

function launchargumentsmodel_tier(r) { return r.tier; }

function launchargumentsmodel_model(r) { return r.model; }

function launchargumentsmodel_effort(r) { return r.effort; }

function launchargumentsmodel_promptArguments(r) { return r.promptArguments; }

const USAGE = $$bc$str("usage: north bridge [route flags] [--view-id agents|goals|all]  (opens the app)", " | north bridge --attempt @attempt:<sha256> [--role director|implementer] [route flags] <prompt>", " | north bridge dashboard [--once] [--ids]", " | north bridge accept <messaged-attempt-id> <interrupted-attempt-id>", " | north bridge restart  (retire the control daemon now)", " | north bridge pending [--json | --consume <execution-id>]", " | north bridge attach <execution-id> [--cursor N]", " | north bridge msg <execution-id> <text> | north bridge interrupt <execution-id>", "\nroute flags: --provider anthropic|openai | --claude | --openai", " --tier economy|standard|senior|frontier --model ID", " --effort low|medium|high|xhigh|max", "\napp launches support Store-authorized OpenAI routes only", "\nlaunch requires a reserved attempt id; role defaults to implementer");

function usage_bang() {
  console.error(USAGE);
  return process.exit(2);
}

const BRIDGE_VIEW_IDS = ["agents", "goals", "all"];

const NORTH_MAIN_TITLE = "North Main";

function parse_bridge_view_id_bang(value) {
  if ((value == null)) {
    return null;
  } else {
    const candidate = ((typeof value === "string") ? value : "");
    if ((!((_truthy) => _truthy !== false && _truthy != null)(BRIDGE_VIEW_IDS.includes(candidate)))) {
      (() => { throw new Error("bridge --view-id requires exactly agents, goals, or all"); })();
    }
    return candidate;
  }
}

function sleep_bang(milliseconds) {
  return Bun.sleep(milliseconds);
}

async function run_north_command_bang(arguments$) {
  const executable = ((_logical) => (_logical !== false && _logical != null ? _logical : "north"))(process.env.NORTH_BIN);
  const child = Bun.spawn({[$$bc$property_key($$bc$keyword("cmd"))]: [executable].concat(arguments$), [$$bc$property_key($$bc$keyword("stdout"))]: "pipe", [$$bc$property_key($$bc$keyword("stderr"))]: "pipe"});
  const results = await Promise.all([new Response(child.stdout).text(), new Response(child.stderr).text(), child.exited]);
  const stdout = $$bc$str(results[0]);
  const stderr = $$bc$str(results[1]);
  const exit_code = results[2];
  if ((!(exit_code === 0))) {
    (() => { throw new Error($$bc$str("north ", arguments$.join(" "), " failed (", exit_code, "): ", (() => { const detail = (((stderr === "") ? stdout : stderr)).trim(); return ((detail === "") ? "no diagnostic" : detail); })())); })();
  }
  return stdout;
}

function parse_command_json_bang(source) {
  return (() => { try {
    return JSON.parse(source.trim());
  } catch (_catch_0) {
    switch ($$bd$catch_dispatch(_catch_0, [Error])) {
      case 0: {
        const __ = _catch_0;
        return (() => { throw new Error("North returned malformed JSON while preparing Main"); })();
        break;
      }
    }
  } })();
}

function catalog_main_id_bang(catalog) {
  const rows = (Array.isArray(catalog.trackedThings) ? catalog.trackedThings : []);
  const matches = rows.filter((row) => ($$bc$str(row.title).trim() === NORTH_MAIN_TITLE));
  if ((matches.length > 1)) {
    (() => { throw new Error("North contains more than one Main identity"); })();
  }
  if ((matches.length === 0)) {
    return "";
  } else {
    const identity = $$bc$str(matches[0].id).trim();
    if ((identity === "")) {
      (() => { throw new Error("North Main has no identity"); })();
    }
    return identity;
  }
}

async function ensure_main_identity_bang() {
  const catalog = parse_command_json_bang(await run_north_command_bang(["work", "catalog", "--json"]));
  const existing = catalog_main_id_bang(catalog);
  const identity = ((existing === "") ? await (async () => { const receipt = parse_command_json_bang(await run_north_command_bang(["work", "track", NORTH_MAIN_TITLE, "--tracked-by", "tom_passarelli", "--json"])); const created = $$bc$str(receipt.referent).trim(); if ((created === "")) {
  (() => { throw new Error("North did not return the Main identity"); })();
}
return created; })() : existing);
  await run_north_command_bang(["tell", identity, "referent_role", "agent"]);
  return identity;
}

function acquire_launch_lease_bang(path) {
  return acquireFileLease(path);
}

function release_launch_lease_bang(lease) {
  return lease.release();
}

function stdin_text_bang() {
  return Bun.stdin.text();
}

function optional_string_field_bang(target, key, value) {
  if (((_truthy) => _truthy !== false && _truthy != null)(value)) {
    $$bh$aset(target, key, value);
  }
  return target;
}

function launch_arguments_wire_bang(launch) {
  const wire = $$bh$host_object($$bc$keyword("role"), launchargumentsmodel_role(launch), $$bc$keyword("promptArguments"), launchargumentsmodel_promptArguments(launch));
  optional_string_field_bang(wire, "attemptId", launchargumentsmodel_attemptId(launch));
  optional_string_field_bang(wire, "provider", launchargumentsmodel_provider(launch));
  optional_string_field_bang(wire, "tier", launchargumentsmodel_tier(launch));
  optional_string_field_bang(wire, "model", launchargumentsmodel_model(launch));
  optional_string_field_bang(wire, "effort", launchargumentsmodel_effort(launch));
  return wire;
}

function parse_bridge_route_arguments_bang(args, attempt_mode) {
  const state = {[$$bc$property_key($$bc$keyword("role"))]: "implementer", [$$bc$property_key($$bc$keyword("provider"))]: null, [$$bc$property_key($$bc$keyword("tier"))]: null, [$$bc$property_key($$bc$keyword("model"))]: null, [$$bc$property_key($$bc$keyword("effort"))]: null, [$$bc$property_key($$bc$keyword("attemptId"))]: null, [$$bc$property_key($$bc$keyword("index"))]: 0, [$$bc$property_key($$bc$keyword("done"))]: false};
  (() => {  while (true) {
    const index = state.index; if (((index < args.length) && (!((_truthy) => _truthy !== false && _truthy != null)(state.done)))) { (() => { const argument = args[index]; return (((argument === "--role")) ? (() => { if (((index + 1) >= args.length)) {
  (() => { throw new Error("bridge --role requires director or implementer"); })();
}
(state.role = parse_bridge_launch_role_bang(args[(index + 1)]));
return (state.index = (index + 2)); })() : ((argument === "--attempt")) ? (() => { if ((attempt_mode === "forbidden")) {
  (() => { throw new Error("bridge app-launch reserves its own attempt"); })();
}
if (((index + 1) >= args.length)) {
  (() => { throw new Error("bridge --attempt requires a canonical reserved attempt id"); })();
}
(state.attemptId = parse_bridge_launch_attempt_id_bang(args[(index + 1)]));
return (state.index = (index + 2)); })() : (((argument === "--claude") || (argument === "--anthropic"))) ? (() => { (state.provider = "anthropic");
return (state.index = (index + 1)); })() : (((argument === "--openai") || (argument === "--codex"))) ? (() => { (state.provider = "openai");
return (state.index = (index + 1)); })() : (((_truthy) => _truthy !== false && _truthy != null)(["--provider", "--tier", "--model", "--effort"].includes(argument))) ? (() => { if (((index + 1) >= args.length)) {
  (() => { throw new Error($$bc$str("bridge ", argument, " requires a value")); })();
}
const value = args[(index + 1)];
if ((argument === "--provider")) {
  (state.provider = parse_bridge_launch_provider_bang(value));
} else if ((argument === "--tier")) {
  (state.tier = parse_bridge_launch_tier_bang(value));
} else if ((argument === "--model")) {
  (state.model = parse_bridge_launch_model_bang(value));
} else {
  (state.effort = parse_bridge_launch_effort_bang(value));
}
return (state.index = (index + 2)); })() : (state.done = true)); })(); if ((!((_truthy) => _truthy !== false && _truthy != null)(state.done))) {  continue; } else { return null; } } else { return null; }
  } })();
  if (((attempt_mode === "required") && (state.attemptId == null))) {
    (() => { throw new Error("bridge launch requires --attempt with a reserved attempt id"); })();
  }
  return launch_arguments_wire_bang(LaunchArgumentsModel(state.role, state.attemptId, state.provider, state.tier, state.model, state.effort, args.slice(state.index)));
}

function parse_bridge_launch_arguments_bang(args) {
  const parsed = parse_bridge_route_arguments_bang(args, "required");
  if ((parsed.attemptId == null)) {
    (() => { throw new Error("bridge launch requires --attempt with a reserved attempt id"); })();
  }
  return parsed;
}

function parse_bridge_app_launch_arguments_bang(args) {
  const state = {[$$bc$property_key($$bc$keyword("selectedThreadId"))]: null, [$$bc$property_key($$bc$keyword("launchArguments"))]: [], [$$bc$property_key($$bc$keyword("index"))]: 0, [$$bc$property_key($$bc$keyword("done"))]: false};
  const valued_flags = new Set(["--role", "--provider", "--tier", "--model", "--effort"]);
  (() => {  while (true) {
    const index = state.index; if (((index < args.length) && (!((_truthy) => _truthy !== false && _truthy != null)(state.done)))) { const argument = args[index]; (((argument === "--attempt")) ? (() => { throw new Error("bridge app-launch reserves its own attempt"); })() : ((argument === "--thread")) ? (() => { const value = (() => { const _x = args, _i = (index + 1); return _x[_i] != null ? _x[_i] : null; })(); if ((!((_truthy) => _truthy !== false && _truthy != null)(value))) {
  (() => { throw new Error("bridge app-launch --thread requires an exact thread id"); })();
}
if (((_truthy) => _truthy !== false && _truthy != null)(state.selectedThreadId)) {
  (() => { throw new Error("bridge app-launch accepts exactly one --thread"); })();
}
(state.selectedThreadId = value);
return (state.index = (index + 2)); })() : (((argument === "--claude") || (argument === "--anthropic"))) ? (() => { throw new Error("bridge app-launch requires a Store-authorized OpenAI route"); })() : (((argument === "--openai") || (argument === "--codex"))) ? (() => { state.launchArguments.push(argument);
return (state.index = (index + 1)); })() : (((_truthy) => _truthy !== false && _truthy != null)(valued_flags.has(argument))) ? (() => { const value = (() => { const _x = args, _i = (index + 1); return _x[_i] != null ? _x[_i] : null; })(); if ((!((_truthy) => _truthy !== false && _truthy != null)(value))) {
  (() => { throw new Error($$bc$str("bridge app-launch ", argument, " requires a value")); })();
}
state.launchArguments.push(argument, value);
return (state.index = (index + 2)); })() : (() => { args.slice(index).forEach((value) => state.launchArguments.push(value));
return (state.done = true); })()); if ((!((_truthy) => _truthy !== false && _truthy != null)(state.done))) {  continue; } else { return null; } } else { return null; }
  } })();
  if ((!((_truthy) => _truthy !== false && _truthy != null)(state.selectedThreadId))) {
    (() => { throw new Error("bridge app-launch requires --thread with an exact Store thread id"); })();
  }
  const parsed = parse_bridge_route_arguments_bang(state.launchArguments, "forbidden");
  if ((parsed.promptArguments.length === 0)) {
    (() => { throw new Error("bridge app-launch requires a prompt"); })();
  }
  (parsed.selectedThreadId = state.selectedThreadId);
  return parsed;
}

function set_route_environment_bang(argument, value) {
  return (((argument === "--provider")) ? (process.env.NORTH_BRIDGE_PROVIDER = parse_bridge_launch_provider_bang(value)) : ((argument === "--tier")) ? (process.env.NORTH_BRIDGE_TIER = parse_bridge_launch_tier_bang(value)) : ((argument === "--model")) ? (process.env.NORTH_BRIDGE_MODEL = parse_bridge_launch_model_bang(value)) : (process.env.NORTH_BRIDGE_EFFORT = parse_bridge_launch_effort_bang(value)));
}

async function run_app_bang(args) {
  const state = {[$$bc$property_key($$bc$keyword("rest"))]: [], [$$bc$property_key($$bc$keyword("index"))]: 0, [$$bc$property_key($$bc$keyword("refused"))]: false};
  (() => {  while (true) {
    const index = state.index; if ((index < args.length)) { (() => { const argument = args[index]; return ((((argument === "--claude") || (argument === "--anthropic"))) ? (() => { (state.refused = true);
return (state.index = (index + 1)); })() : (((argument === "--openai") || (argument === "--codex"))) ? (() => { (process.env.NORTH_BRIDGE_PROVIDER = "openai");
return (state.index = (index + 1)); })() : (((_truthy) => _truthy !== false && _truthy != null)(["--provider", "--tier", "--model", "--effort"].includes(argument))) ? (() => { const value = (() => { const _x = args, _i = (index + 1); return _x[_i] != null ? _x[_i] : null; })(); if ((!((_truthy) => _truthy !== false && _truthy != null)(value))) {
  usage_bang();
}
if (((argument === "--provider") && (!(parse_bridge_launch_provider_bang(value) === "openai")))) {
  (state.refused = true);
} else {
  set_route_environment_bang(argument, value);
}
return (state.index = (index + 2)); })() : (() => { state.rest.push(argument);
return (state.index = (index + 1)); })()); })();  continue; } else { return null; }
  } })();
  if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? _logical : (((_logical) => (_logical !== false && _logical != null ? _logical : ""))(process.env.NORTH_BRIDGE_PROVIDER).trim() === "anthropic")))(state.refused))) {
    console.error("north bridge: the app requires a Store-authorized OpenAI route");
    return 1;
  } else {
    const rest = state.rest;
    const view_id = parse_bridge_view_id_bang(((rest.length === 0) ? null : (() => { if (((!(rest.length === 2)) || ((!(rest[0] === "--view-id")) || (!((_truthy) => _truthy !== false && _truthy != null)((() => { const _x = rest, _i = 1; return _x[_i] != null ? _x[_i] : null; })()))))) {
  usage_bang();
}
return rest[1]; })()));
    if ((process.env.NORTH_BIN == null)) {
      (process.env.NORTH_BIN = resolve(import.meta.dir, "../../../../../../bin/north"));
    }
    (process.env.NORTH_BRIDGE_CONTROL_THREAD = await ensure_main_identity_bang());
    const connection = await verified_socket_bang(bridge_socket_path(), CONSOLE_CONNECTION_OUTPUT, {[$$bc$property_key($$bc$keyword("replacePinned"))]: true});
    connection.socket.destroy();
    await run_northbridge_app_bang({[$$bc$property_key($$bc$keyword("viewId"))]: view_id, [$$bc$property_key($$bc$keyword("sourceIdentity"))]: bridge_source_identity()});
    return 0;
  }
}

function pending_value(value) {
  return (((typeof value === "string") && (!(value === ""))) ? value : null);
}

function render_pending_lane(lane) {
  const terminal_data = lane.terminal.data;
  const harvest_data = (((_truthy) => _truthy !== false && _truthy != null)(lane.harvest) ? lane.harvest.data : null);
  const process_outcome = ((_logical) => (_logical !== false && _logical != null ? _logical : "unknown"))(pending_value(terminal_data.processOutcome));
  const delivery_outcome = ((_logical) => (_logical !== false && _logical != null ? _logical : "unknown"))(pending_value(terminal_data.deliveryOutcome));
  const branch = pending_value((((_truthy) => _truthy !== false && _truthy != null)(harvest_data) ? harvest_data.branch : null));
  const sha = pending_value((((_truthy) => _truthy !== false && _truthy != null)(harvest_data) ? harvest_data.sha : null));
  const parts = [lane.executionId, $$bc$str("process=", process_outcome), $$bc$str("delivery=", delivery_outcome)];
  if (((_truthy) => _truthy !== false && _truthy != null)(branch)) {
    parts.push($$bc$str("branch=", branch));
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(sha)) {
    parts.push($$bc$str("sha=", sha));
  }
  return parts.join(" ");
}

function run_pending_bang(args) {
  return (((args.length === 0)) ? (() => { pendingLanes().forEach((lane) => console.log(render_pending_lane(lane)));
return 0; })() : (((args.length === 1) && (args[0] === "--json"))) ? (() => { console.log(JSON.stringify(pendingLanes()));
return 0; })() : (((args.length === 2) && ((args[0] === "--consume") || (args[0] === "consume")))) ? (() => { const execution_id = args[1]; const created = markLaneConsumed(execution_id); console.log($$bc$str((((_truthy) => _truthy !== false && _truthy != null)(created) ? "consumed" : "already consumed"), " ", execution_id));
return 0; })() : usage_bang());
}

async function run_dashboard_bang(args) {
  const dashboard = resolve(import.meta.dir, "../../../../../../cli/dashboard-cli.clj");
  const bb = ((_logical) => (_logical !== false && _logical != null ? _logical : "bb"))(process.env.NORTH_BB);
  const child = spawn(bb, [dashboard, "dashboard"].concat(args), {[$$bc$property_key($$bc$keyword("stdio"))]: "inherit"});
  const result = Promise.withResolvers();
  const exited = result.promise;
  child.once("error", result.reject);
  child.once("exit", (code) => (result.resolve)(((typeof code === "number") ? code : 1)));
  return await exited;
}

function open_socket_bang(path) {
  const result = Promise.withResolvers();
  const socket = new Socket();
  const on_error = (error) => { socket.destroy();
return (result.reject)(error); };
  socket.once("error", on_error);
  socket.once("connect", () => { socket.off("error", on_error);
return (result.resolve)(socket); });
  socket.connect(path);
  return result.promise;
}

async function wait_for_socket_bang(path, attempt, last_error) {
  return ((attempt >= 100) ? (() => { throw new Error($$bc$str("northd did not open ", path), {[$$bc$property_key($$bc$keyword("cause"))]: last_error}); })() : await (async () => { try {
    return await open_socket_bang(path);
  } catch (_catch_1) {
    switch ($$bd$catch_dispatch(_catch_1, [Error])) {
      case 0: {
        const error = _catch_1;
        await sleep_bang(20);
        return await wait_for_socket_bang(path, (attempt + 1), error);
        break;
      }
    }
  } })());
}

async function connected_socket_bang(path) {
  return (async () => { try {
    return await open_socket_bang(path);
  } catch (_catch_2) {
    switch ($$bd$catch_dispatch(_catch_2, [Error])) {
      case 0: {
        const __ = _catch_2;
        const lease = await acquire_launch_lease_bang($$bc$str(path, ".launch.lock"));
        return (async () => { try {
    return (async () => { try {
    return await open_socket_bang(path);
  } catch (_catch_3) {
    switch ($$bd$catch_dispatch(_catch_3, [Error])) {
      case 0: {
        const __ = _catch_3;
        const northd = resolve(import.meta.dir, "../../../northd.ts");
        const bun = $$bc$str(process.execPath);
        const child = spawn(bun, [northd], {[$$bc$property_key($$bc$keyword("detached"))]: true, [$$bc$property_key($$bc$keyword("stdio"))]: "ignore"});
        child.unref();
        return await wait_for_socket_bang(path, 0, null);
        break;
      }
    }
  } })();
  } finally {
    await release_launch_lease_bang(lease);
  } })();
        break;
      }
    }
  } })();
}

function read_hello_bang(socket, timeout_ms) {
  const result = Promise.withResolvers();
  const state = {[$$bc$property_key($$bc$keyword("buffer"))]: "", [$$bc$property_key($$bc$keyword("finished"))]: false, [$$bc$property_key($$bc$keyword("timer"))]: null, [$$bc$property_key($$bc$keyword("onData"))]: null, [$$bc$property_key($$bc$keyword("onClose"))]: null, [$$bc$property_key($$bc$keyword("onError"))]: null};
  const finish = (value) => { if ((!((_truthy) => _truthy !== false && _truthy != null)(state.finished))) {
  (state.finished = true);
  clearTimeout(state.timer);
  socket.off("data", state.onData);
  socket.off("close", state.onClose);
  socket.off("error", state.onError);
  return (result.resolve)(value);
} };
  const on_close = () => finish(null);
  const on_error = () => finish(null);
  const on_data = (chunk) => { (state.buffer = $$bc$str(state.buffer, chunk));
const newline = state.buffer.indexOf("\n");
if ((newline >= 0)) {
  return (() => { try {
    const message = JSON.parse(state.buffer.slice(0, newline));
  return finish(((message.type === "hello") ? message : null));
  } catch (_catch_4) {
    switch ($$bd$catch_dispatch(_catch_4, [Error])) {
      case 0: {
        const __ = _catch_4;
        return finish(null);
        break;
      }
    }
  } })();
} };
  (state.onData = on_data);
  (state.onClose = on_close);
  (state.onError = on_error);
  (state.timer = setTimeout(() => finish(null), timeout_ms));
  socket.setEncoding("utf8");
  socket.on("data", on_data);
  socket.once("close", on_close);
  socket.once("error", on_error);
  if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? _logical : socket.readableEnded))(socket.destroyed))) {
    finish(null);
  }
  return result.promise;
}

function socket_closed_bang(socket) {
  const result = Promise.withResolvers();
  socket.once("close", () => (result.resolve)());
  return result.promise;
}

const CONSOLE_CONNECTION_OUTPUT = {[$$bc$property_key($$bc$keyword("info"))]: (message) => console.log(message), [$$bc$property_key($$bc$keyword("error"))]: (message) => console.error(message)};

function short_identity(identity) {
  return (((_truthy) => _truthy !== false && _truthy != null)(identity) ? identity.slice(0, 8) : "unknown");
}

const DAEMON_RETIRE_TIMEOUT_MS = 5000;

function process_alive_p(pid) {
  return (() => { try {
    process.kill(pid, 0);
  return true;
  } catch (_catch_5) {
    switch ($$bd$catch_dispatch(_catch_5, [Error])) {
      case 0: {
        const error = _catch_5;
        return (error.code === "EPERM");
        break;
      }
    }
  } })();
}

async function retirement_poll_bang(path, retiring_pid, deadline) {
  if ((Date.now() >= deadline)) {
    return false;
  } else {
    if (((!(retiring_pid === process.pid)) && (!process_alive_p(retiring_pid)))) {
      return true;
    } else {
      const socket = await (async () => { try {
    return await open_socket_bang(path);
  } catch (_catch_6) {
    switch ($$bd$catch_dispatch(_catch_6, [Error])) {
      case 0: {
        const __ = _catch_6;
        return null;
        break;
      }
    }
  } })();
      if ((socket == null)) {
        if ((retiring_pid === process.pid)) {
          return true;
        } else {
          await sleep_bang(20);
          return await retirement_poll_bang(path, retiring_pid, deadline);
        }
      } else {
        const hello = await read_hello_bang(socket, 100);
        socket.destroy();
        if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? ((!(hello.pid === retiring_pid)) && ((retiring_pid === process.pid) || (!process_alive_p(retiring_pid)))) : _logical))(hello))) {
          return true;
        } else {
          await sleep_bang(20);
          return await retirement_poll_bang(path, retiring_pid, deadline);
        }
      }
    }
  }
}

function daemon_retired_bang(path, retiring_pid) {
  return retirement_poll_bang(path, retiring_pid, (Date.now() + DAEMON_RETIRE_TIMEOUT_MS));
}

async function verified_attempt_bang(path, output, options, attempt, replaced_from, replaced) {
  if ((attempt >= 3)) {
    return (() => { throw new Error("northd did not present a fresh identity after replacement"); })();
  } else {
    const socket = await connected_socket_bang(path);
    const hello = await read_hello_bang(socket, 750);
    const disk = bridge_source_identity();
    const fresh = ((_logical) => (_logical !== false && _logical != null ? ((hello.identity == null) || ((disk == null) || (hello.identity === disk))) : _logical))(hello);
    if (((_truthy) => _truthy !== false && _truthy != null)(fresh)) {
      if (replaced) {
        output.info($$bc$str("northd: control daemon was stale — replaced (", short_identity(replaced_from), " → ", short_identity(hello.identity), "); starting fresh"));
      }
      return {[$$bc$property_key($$bc$keyword("socket"))]: socket, [$$bc$property_key($$bc$keyword("hello"))]: hello};
    } else {
      const pinning = (((_truthy) => _truthy !== false && _truthy != null)(hello) ? pinning_executions(hello) : 0);
      if (((pinning > 0) && (!(options.replacePinned === true)))) {
        output.error($$bc$str("north bridge: northd is stale with ", pinning, " live session(s);", " run 'north bridge restart' to replace it now, or new launches are refused", " until it drains"));
        return {[$$bc$property_key($$bc$keyword("socket"))]: socket, [$$bc$property_key($$bc$keyword("hello"))]: hello};
      } else {
        if (((_truthy) => _truthy !== false && _truthy != null)(hello)) {
          const closed = socket_closed_bang(socket);
          const retiring_pid = hello.pid;
          socket.write($$bc$str(JSON.stringify({[$$bc$property_key($$bc$keyword("op"))]: "retire"}), "\n"));
          await closed;
          if ((!await daemon_retired_bang(path, retiring_pid))) {
            (() => { throw new Error($$bc$str("northd ", retiring_pid, " did not finish retirement")); })();
          }
        } else {
          socket.destroy();
          if ((attempt === 2)) {
            output.error($$bc$str("north bridge: northd did not present the identity handshake;", " reap the orphan with: pkill -f bridge/northd"));
          }
          await sleep_bang(50);
        }
        return await verified_attempt_bang(path, output, options, (attempt + 1), (((_truthy) => _truthy !== false && _truthy != null)(hello) ? hello.identity : replaced_from), (replaced || (!(hello == null))));
      }
    }
  }
}

async function verified_socket_bang(...$beagle$args) {
  if (arguments.length === 1) {
    const path = $beagle$args[0];
    return await verified_attempt_bang(path, CONSOLE_CONNECTION_OUTPUT, {}, 0, null, false);
  }
  if (arguments.length === 2) {
    const path = $beagle$args[0];
    const output = $beagle$args[1];
    return await verified_attempt_bang(path, output, {}, 0, null, false);
  }
  if (arguments.length === 3) {
    const path = $beagle$args[0];
    const output = $beagle$args[1];
    const options = $beagle$args[2];
    return await verified_attempt_bang(path, ((output === undefined) ? CONSOLE_CONNECTION_OUTPUT : output), options, 0, null, false);
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

async function run_bridge_restart_bang(path) {
  const socket = await (async () => { try {
    return await open_socket_bang(path);
  } catch (_catch_7) {
    switch ($$bd$catch_dispatch(_catch_7, [Error])) {
      case 0: {
        const __ = _catch_7;
        return null;
        break;
      }
    }
  } })();
  const state = {[$$bc$property_key($$bc$keyword("retiredFrom"))]: null, [$$bc$property_key($$bc$keyword("failed"))]: false};
  if (((_truthy) => _truthy !== false && _truthy != null)(socket)) {
    const hello = await read_hello_bang(socket, 750);
    if ((hello == null)) {
      socket.destroy();
      console.error($$bc$str("north bridge: northd predates the identity handshake;", " reap the orphan with: pkill -f bridge/northd"));
      (state.failed = true);
    } else {
      (state.retiredFrom = hello.identity);
      const closed = socket_closed_bang(socket);
      socket.write($$bc$str(JSON.stringify({[$$bc$property_key($$bc$keyword("op"))]: "retire"}), "\n"));
      await closed;
      if ((!await daemon_retired_bang(path, hello.pid))) {
        console.error($$bc$str("north bridge: the control daemon is still listening at ", path));
        (state.failed = true);
      }
    }
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(state.failed)) {
    return 1;
  } else {
    const successor = await verified_socket_bang(path);
    const now = short_identity(successor.hello.identity);
    successor.socket.destroy();
    console.log(((state.retiredFrom == null) ? $$bc$str("control daemon started (", now, ")") : $$bc$str("control daemon replaced (", short_identity(state.retiredFrom), " → ", now, ")")));
    return 0;
  }
}

function render_record(record) {
  const data = $$bc$str(" ", JSON.stringify(Object.assign({}, record.data, {[$$bc$property_key($$bc$keyword("bridgeRecordAt"))]: record.at})));
  return $$bc$str("[", record.seq, "] ", record.kind, data);
}

function render_wire_event(event) {
  const sequence = event.sequence;
  return $$bc$str("[", (sequence + 1), "] ", event.kind, " ", JSON.stringify(event));
}

function bridge_app_launch_recovery_action(phase, outcome, managed) {
  return ((((_truthy) => _truthy !== false && _truthy != null)(managed.settled)) ? "complete" : (((_truthy) => _truthy !== false && _truthy != null)(managed.providerEffectObserved)) ? "reconnect" : (((_truthy) => _truthy !== false && _truthy != null)(((phase === "launch") && outcome.refused))) ? "prove-unsent" : (((_truthy) => _truthy !== false && _truthy != null)(((phase === "attach") && ((_logical) => (_logical !== false && _logical != null ? outcome.errors.some((message) => message.startsWith("unknown bridge execution ")) : _logical))(outcome.refused)))) ? "prove-unsent" : "reconnect");
}

function run_client_bang(...$beagle$args) {
  if (arguments.length === 2) {
    const socket = $beagle$args[0];
    const request = $beagle$args[1];
    return run_client_bang(socket, request, {});
  }
  if (arguments.length === 3) {
    const socket = $beagle$args[0];
    const request = $beagle$args[1];
    const hooks = $beagle$args[2];
    const result = Promise.withResolvers();
    const state = {[$$bc$property_key($$bc$keyword("buffer"))]: "", [$$bc$property_key($$bc$keyword("exitCode"))]: 0, [$$bc$property_key($$bc$keyword("launched"))]: false, [$$bc$property_key($$bc$keyword("refused"))]: false, [$$bc$property_key($$bc$keyword("errors"))]: [], [$$bc$property_key($$bc$keyword("cursor"))]: 0, [$$bc$property_key($$bc$keyword("observationTail"))]: Promise.resolve(undefined), [$$bc$property_key($$bc$keyword("observationFailed"))]: false};
    socket.setEncoding("utf8");
    socket.on("data", (chunk) => { (state.buffer = $$bc$str(state.buffer, chunk));
return (() => {  while (true) {
    const newline = state.buffer.indexOf("\n"); if ((newline >= 0)) { const line = state.buffer.slice(0, newline); (state.buffer = state.buffer.slice((newline + 1))); ((!(line === "")) ? (() => { const message = JSON.parse(line);
return (((message.type === "hello")) ? null : ((message.type === "launched")) ? (() => { (state.launched = true);
return console.log($$bc$str("execution ", message.executionId)); })() : ((message.type === "controlled")) ? console.log($$bc$str(message.executionId, " ", message.delivery)) : ((message.type === "event")) ? console.log(render_record(message.record)) : ((message.type === "wire")) ? (() => { console.log(render_wire_event(message.event));
return (state.observationTail = state.observationTail.then(async () => { if ((!((_truthy) => _truthy !== false && _truthy != null)(state.observationFailed))) {
  return (async () => { try {
    if (((_truthy) => _truthy !== false && _truthy != null)(hooks.onDurableWireEvent)) {
    await (hooks.onDurableWireEvent)(message.event);
  }
  return (state.cursor = Math.max(state.cursor, (message.event.sequence + 1)));
  } catch (_catch_8) {
    switch ($$bd$catch_dispatch(_catch_8, [Error])) {
      case 0: {
        const error = _catch_8;
        (state.observationFailed = true);
        console.error($$bc$str("north bridge: ", ((_logical) => (_logical !== false && _logical != null ? _logical : "wire settlement failed"))(error.message)));
        return (state.exitCode = 1);
        break;
      }
    }
  } })();
} })); })() : ((message.type === "barrier")) ? (() => { if ((!((_truthy) => _truthy !== false && _truthy != null)(hooks.onDurableWireEvent))) {
  (state.cursor = Math.max(state.cursor, message.cursor));
}
console.log($$bc$str("attached ", message.executionId, " at ", message.cursor));
if (((_truthy) => _truthy !== false && _truthy != null)(message.tornTail)) {
  console.error($$bc$str("torn journal tail at byte ", message.tornTail.offset, ": ", message.tornTail.availableBytes, "/", message.tornTail.requiredBytes, " bytes"));
  return (state.exitCode = 1);
} })() : (() => { (state.refused = (!((_truthy) => _truthy !== false && _truthy != null)(state.launched)));
state.errors.push(message.message);
console.error($$bc$str("north bridge: ", message.message));
return (state.exitCode = 1); })()); })() : null);  continue; } else { return null; }
  } })(); });
    socket.once("error", (error) => { console.error($$bc$str("north bridge: ", error.message));
return (state.exitCode = 1); });
    socket.once("close", () => state.observationTail.then(() => (result.resolve)({[$$bc$property_key($$bc$keyword("code"))]: state.exitCode, [$$bc$property_key($$bc$keyword("launched"))]: state.launched, [$$bc$property_key($$bc$keyword("refused"))]: state.refused, [$$bc$property_key($$bc$keyword("errors"))]: state.errors, [$$bc$property_key($$bc$keyword("cursor"))]: state.cursor})));
    socket.write($$bc$str(JSON.stringify(request), "\n"));
    return result.promise;
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

async function settle_managed_app_launch_refusal_bang(managed) {
  if (((!((_truthy) => _truthy !== false && _truthy != null)(managed.providerEffectObserved)) && (!((_truthy) => _truthy !== false && _truthy != null)(managed.settled)))) {
    return await managed.proveUnsent("daemon-launch-refused");
  }
}

async function terminate_managed_app_launch_bang(execution_id) {
  const connection = await verified_socket_bang(bridge_socket_path());
  await run_client_bang(connection.socket, {[$$bc$property_key($$bc$keyword("op"))]: "terminateSession", [$$bc$property_key($$bc$keyword("executionId"))]: execution_id});
  return null;
}

async function run_managed_app_launch_bang(launch) {
  return (async () => { try {
    const prompt = launch.promptArguments.join(" ").trim();
  const managed = await prepare_managed_bridge_app_launch_bang({[$$bc$property_key($$bc$keyword("role"))]: launch.role, [$$bc$property_key($$bc$keyword("prompt"))]: prompt, [$$bc$property_key($$bc$keyword("cwd"))]: process.cwd(), [$$bc$property_key($$bc$keyword("selectedThreadId"))]: launch.selectedThreadId, [$$bc$property_key($$bc$keyword("provider"))]: launch.provider, [$$bc$property_key($$bc$keyword("tier"))]: launch.tier, [$$bc$property_key($$bc$keyword("model"))]: launch.model, [$$bc$property_key($$bc$keyword("effort"))]: launch.effort});
  const state = {[$$bc$property_key($$bc$keyword("socket"))]: null, [$$bc$property_key($$bc$keyword("leaseFailed"))]: false, [$$bc$property_key($$bc$keyword("cursor"))]: 0, [$$bc$property_key($$bc$keyword("outcome"))]: null};
  const hooks = {[$$bc$property_key($$bc$keyword("onDurableWireEvent"))]: (event) => managed.observeDurableWireEvent(event)};
  const monitored_bang = async (socket, request) => { const client = run_client_bang(socket, request, hooks);
if (((_truthy) => _truthy !== false && _truthy != null)(state.leaseFailed)) {
  return client;
} else {
  const winner = await Promise.race([client.then((outcome) => ({[$$bc$property_key($$bc$keyword("kind"))]: "client", [$$bc$property_key($$bc$keyword("outcome"))]: outcome})), managed.leaseFailure.then((error) => ({[$$bc$property_key($$bc$keyword("kind"))]: "lease", [$$bc$property_key($$bc$keyword("error"))]: error}))]);
  if ((winner.kind === "client")) {
    return winner.outcome;
  } else {
    (state.leaseFailed = true);
    console.error($$bc$str("north bridge: delivery lease renewal failed: ", winner.error.message));
    socket.destroy();
    await (async () => { try {
    return await terminate_managed_app_launch_bang(managed.executionId);
  } catch (_catch_9) {
    switch ($$bd$catch_dispatch(_catch_9, [Error])) {
      case 0: {
        const __ = _catch_9;
        return null;
        break;
      }
    }
  } })();
    return await client;
  }
} };
  await (async () => { try {
    return (state.socket = (await verified_socket_bang(bridge_socket_path())).socket);
  } catch (_catch_10) {
    switch ($$bd$catch_dispatch(_catch_10, [Error])) {
      case 0: {
        const error = _catch_10;
        await managed.proveUnsent("daemon-not-contacted");
        return (() => { throw error; })();
        break;
      }
    }
  } })();
  (state.outcome = await monitored_bang(state.socket, {[$$bc$property_key($$bc$keyword("op"))]: "launch", [$$bc$property_key($$bc$keyword("executionId"))]: managed.executionId, [$$bc$property_key($$bc$keyword("attemptId"))]: managed.attemptId, [$$bc$property_key($$bc$keyword("prompt"))]: prompt, [$$bc$property_key($$bc$keyword("cwd"))]: process.cwd(), [$$bc$property_key($$bc$keyword("role"))]: launch.role, [$$bc$property_key($$bc$keyword("provider"))]: managed.provider, [$$bc$property_key($$bc$keyword("model"))]: managed.model, [$$bc$property_key($$bc$keyword("tier"))]: launch.tier, [$$bc$property_key($$bc$keyword("effort"))]: launch.effort}));
  if ((bridge_app_launch_recovery_action("launch", state.outcome, managed) === "prove-unsent")) {
    await settle_managed_app_launch_refusal_bang(managed);
    return state.outcome.code;
  } else {
    (state.cursor = state.outcome.cursor);
    await (async () => {  while (true) {
    if ((!((_truthy) => _truthy !== false && _truthy != null)(managed.settled))) { if (((_truthy) => _truthy !== false && _truthy != null)(state.leaseFailed)) { { let _loop_try_result_0; try {
    _loop_try_result_0 = await terminate_managed_app_launch_bang(managed.executionId);
  } catch (_catch_11) {
    switch ($$bd$catch_dispatch(_catch_11, [Error])) {
      case 0: {
        const __ = _catch_11;
        await sleep_bang(250);  continue;
        break;
      }
    }
  } _loop_try_result_0; { let _loop_try_result_1; try {
    _loop_try_result_1 = (state.socket = (await verified_socket_bang(bridge_socket_path())).socket);
  } catch (_catch_12) {
    switch ($$bd$catch_dispatch(_catch_12, [Error])) {
      case 0: {
        const __ = _catch_12;
        await sleep_bang(250);  continue;
        break;
      }
    }
  } _loop_try_result_1; (state.outcome = await monitored_bang(state.socket, {[$$bc$property_key($$bc$keyword("op"))]: "attach", [$$bc$property_key($$bc$keyword("executionId"))]: managed.executionId, [$$bc$property_key($$bc$keyword("cursor"))]: state.cursor})); (state.cursor = Math.max(state.cursor, state.outcome.cursor)); ((bridge_app_launch_recovery_action("attach", state.outcome, managed) === "prove-unsent") ? await (async () => { return await settle_managed_app_launch_refusal_bang(managed); })() : null); if ((!((_truthy) => _truthy !== false && _truthy != null)(managed.settled))) { await sleep_bang(250);  continue; } else { return null; } } } } else { null; { let _loop_try_result_2; try {
    _loop_try_result_2 = (state.socket = (await verified_socket_bang(bridge_socket_path())).socket);
  } catch (_catch_13) {
    switch ($$bd$catch_dispatch(_catch_13, [Error])) {
      case 0: {
        const __ = _catch_13;
        await sleep_bang(250);  continue;
        break;
      }
    }
  } _loop_try_result_2; (state.outcome = await monitored_bang(state.socket, {[$$bc$property_key($$bc$keyword("op"))]: "attach", [$$bc$property_key($$bc$keyword("executionId"))]: managed.executionId, [$$bc$property_key($$bc$keyword("cursor"))]: state.cursor})); (state.cursor = Math.max(state.cursor, state.outcome.cursor)); ((bridge_app_launch_recovery_action("attach", state.outcome, managed) === "prove-unsent") ? await (async () => { return await settle_managed_app_launch_refusal_bang(managed); })() : null); if ((!((_truthy) => _truthy !== false && _truthy != null)(managed.settled))) { await sleep_bang(250);  continue; } else { return null; } } } } else { return null; }
  } })();
    return (((_truthy) => _truthy !== false && _truthy != null)(state.leaseFailed) ? 1 : state.outcome.code);
  }
  } catch (_catch_14) {
    switch ($$bd$catch_dispatch(_catch_14, [Error])) {
      case 0: {
        const error = _catch_14;
        console.error($$bc$str("north bridge: ", ((_logical) => (_logical !== false && _logical != null ? _logical : "app launch failed"))(error.message)));
        return 1;
        break;
      }
    }
  } })();
}

function app_arguments_p(args) {
  const app_flags = new Set(["--claude", "--anthropic", "--openai", "--codex", "--view-id", "--provider", "--tier", "--model", "--effort"]);
  const valued = new Set(["--view-id", "--provider", "--tier", "--model", "--effort"]);
  return args.every((argument, index) => ((_logical) => (_logical !== false && _logical != null ? _logical : ((index > 0) && valued.has(args[(index - 1)]))))(app_flags.has(argument)));
}

function launch_arguments_p(args) {
  return args.includes("--attempt");
}

async function main_bang(args) {
  return (((args.length === 0)) ? await run_app_bang(args) : (app_arguments_p(args)) ? await run_app_bang(args) : ((args[0] === "dashboard")) ? await run_dashboard_bang(args.slice(1)) : ((args[0] === "pending")) ? run_pending_bang(args.slice(1)) : ((args[0] === "restart")) ? await (async () => { if ((!(args.length === 1))) {
  usage_bang();
}
return await run_bridge_restart_bang(bridge_socket_path()); })() : ((args[0] === "accept")) ? await (async () => { if ((!(args.length === 3))) {
  usage_bang();
}
return (async () => { try {
    await runBridgeAcceptance({[$$bc$property_key($$bc$keyword("attemptIds"))]: [parse_bridge_launch_attempt_id_bang(args[1]), parse_bridge_launch_attempt_id_bang(args[2])]});
  return 0;
  } catch (_catch_15) {
    switch ($$bd$catch_dispatch(_catch_15, [Error])) {
      case 0: {
        const __ = _catch_15;
        return 1;
        break;
      }
    }
  } })(); })() : ((args[0] === "app-launch")) ? await (async () => { try {
    return await run_managed_app_launch_bang(parse_bridge_app_launch_arguments_bang(args.slice(1)));
  } catch (_catch_16) {
    switch ($$bd$catch_dispatch(_catch_16, [Error])) {
      case 0: {
        const error = _catch_16;
        console.error($$bc$str("north bridge: ", ((_logical) => (_logical !== false && _logical != null ? _logical : "app launch failed"))(error.message)));
        return 1;
        break;
      }
    }
  } })() : await (async () => { const request = (((args[0] === "attach")) ? (() => { const execution_id = (() => { const _x = args, _i = 1; return _x[_i] != null ? _x[_i] : null; })(); if (((!((_truthy) => _truthy !== false && _truthy != null)(execution_id)) || ((!(args.length === 2)) && (!(args.length === 4))))) {
  usage_bang();
}
const cursor = ((args.length === 4) ? (() => { if (((!(args[2] === "--cursor")) || (!((_truthy) => _truthy !== false && _truthy != null)(new RegExp("^[0-9]+$").test(args[3]))))) {
  usage_bang();
}
const parsed = Number(args[3]);
if ((!Number.isSafeInteger(parsed))) {
  usage_bang();
}
return parsed; })() : 0);
return {[$$bc$property_key($$bc$keyword("op"))]: "attach", [$$bc$property_key($$bc$keyword("executionId"))]: execution_id, [$$bc$property_key($$bc$keyword("cursor"))]: cursor}; })() : ((args[0] === "msg")) ? (() => { const execution_id = (() => { const _x = args, _i = 1; return _x[_i] != null ? _x[_i] : null; })(); const input = args.slice(2).join(" ").trim(); if (((!((_truthy) => _truthy !== false && _truthy != null)(execution_id)) || (input === ""))) {
  usage_bang();
}
return {[$$bc$property_key($$bc$keyword("op"))]: "submitInput", [$$bc$property_key($$bc$keyword("executionId"))]: execution_id, [$$bc$property_key($$bc$keyword("input"))]: input}; })() : ((args[0] === "interrupt")) ? (() => { const execution_id = (() => { const _x = args, _i = 1; return _x[_i] != null ? _x[_i] : null; })(); if (((!((_truthy) => _truthy !== false && _truthy != null)(execution_id)) || (!(args.length === 2)))) {
  usage_bang();
}
return {[$$bc$property_key($$bc$keyword("op"))]: "interruptTurn", [$$bc$property_key($$bc$keyword("executionId"))]: execution_id}; })() : (launch_arguments_p(args)) ? await (async () => { const launch = (() => { try {
    return parse_bridge_launch_arguments_bang(args);
  } catch (_catch_17) {
    switch ($$bd$catch_dispatch(_catch_17, [Error])) {
      case 0: {
        const __ = _catch_17;
        return usage_bang();
        break;
      }
    }
  } })(); const prompt_state = {[$$bc$property_key($$bc$keyword("prompt"))]: launch.promptArguments.join(" ").trim()}; if (((prompt_state.prompt === "") && (!((_truthy) => _truthy !== false && _truthy != null)(process.stdin.isTTY)))) {
  (prompt_state.prompt = (await stdin_text_bang()).trim());
}
if ((prompt_state.prompt === "")) {
  usage_bang();
}
const wire = $$bh$host_object($$bc$keyword("op"), "launch", $$bc$keyword("prompt"), prompt_state.prompt, $$bc$keyword("cwd"), process.cwd(), $$bc$keyword("role"), launch.role, $$bc$keyword("attemptId"), launch.attemptId);
optional_string_field_bang(wire, "provider", launch.provider);
optional_string_field_bang(wire, "tier", launch.tier);
optional_string_field_bang(wire, "model", launch.model);
optional_string_field_bang(wire, "effort", launch.effort);
return wire; })() : usage_bang()); const connection = await verified_socket_bang(bridge_socket_path()); return (await run_client_bang(connection.socket, request)).code; })());
}

if (import.meta.main) {
  main_bang(process.argv.slice(2)).then((code) => (process.exitCode = code));
}

export { bridge_app_launch_recovery_action as "bridge-app-launch-recovery-action" };
export { parse_bridge_app_launch_arguments_bang as "parse-bridge-app-launch-arguments!" };
export { parse_bridge_launch_arguments_bang as "parse-bridge-launch-arguments!" };
export { parse_bridge_view_id_bang as "parse-bridge-view-id!" };
export { read_hello_bang as "read-hello!" };
export { render_wire_event as "render-wire-event" };
export { run_bridge_restart_bang as "run-bridge-restart!" };
export { settle_managed_app_launch_refusal_bang as "settle-managed-app-launch-refusal!" };
export { verified_socket_bang as "verified-socket!" };
