import { spawn, spawnSync } from 'child_process';
import { createHash } from 'crypto';
import { linkSync, mkdirSync, readFileSync, readdirSync, renameSync, rmdirSync, statSync, unlinkSync, writeFileSync } from 'fs';
import { basename, dirname, join, resolve } from 'path';
import { keyword as $$bc$keyword, property_key as $$bc$property_key, str as $$bc$str } from '../sdk/src/bridge/generated/beagle/core.js';
import { catch_dispatch as $$bd$catch_dispatch } from '../sdk/src/bridge/generated/beagle/exception-dispatch.js';

const ACTOR_KEY_PATTERN = "^[A-Za-z0-9._:-]+$";

function text_value(value) {
  return ((typeof value === "string") ? value : "");
}

function env_value(name) {
  return text_value(Reflect.get(process.env, name));
}

function text_or(value, fallback) {
  return ((value === "") ? fallback : value);
}

function valid_actor_p(raw) {
  return ((raw.length > 0) && ((raw.length <= 512) && new RegExp(ACTOR_KEY_PATTERN, "u").test(raw)));
}

function actor_key(namespace, raw) {
  return (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? valid_actor_p(raw) : _logical))(["agent", "session", "managed"].includes(namespace))) ? createHash("sha256").update($$bc$str("north-actor-key-v1\x00", namespace, "\x00", raw), "utf8").digest("hex") : null);
}

function runtime_root() {
  return text_or(env_value("XDG_RUNTIME_DIR"), "/tmp");
}

function readable_p(path) {
  return (() => { try {
    readFileSync(path);
  return true;
  } catch (_catch_0) {
    switch ($$bd$catch_dispatch(_catch_0, [Error])) {
      case 0: {
        const __ = _catch_0;
        return false;
        break;
      }
    }
  } })();
}

function read_text(path) {
  return (() => { try {
    return text_value(readFileSync(path, "utf8"));
  } catch (_catch_1) {
    switch ($$bd$catch_dispatch(_catch_1, [Error])) {
      case 0: {
        const __ = _catch_1;
        return "";
        break;
      }
    }
  } })();
}

function first_line(path) {
  return (() => { const _x = read_text(path).split("\n"), _i = 0; return _x[_i] != null ? _x[_i] : ""; })();
}

function route_lines(path) {
  const lines = read_text(path).split("\n");
  return [(() => { const _x = lines, _i = 0; return _x[_i] != null ? _x[_i] : ""; })().trim(), (() => { const _x = lines, _i = 1; return _x[_i] != null ? _x[_i] : ""; })().trim(), (() => { const _x = lines, _i = 2; return _x[_i] != null ? _x[_i] : ""; })().trim(), (() => { const _x = lines, _i = 3; return _x[_i] != null ? _x[_i] : ""; })().trim(), (() => { const _x = lines, _i = 4; return _x[_i] != null ? _x[_i] : ""; })().trim(), (() => { const _x = lines, _i = 5; return _x[_i] != null ? _x[_i] : ""; })().trim()];
}

function route_body(provider, model, effort, role, role_alias, repo_name) {
  return $$bc$str(provider, "\n", model, "\n", effort, "\n", role, "\n", role_alias, "\n", repo_name, "\n");
}

function random_token() {
  return Math.random().toString(36).replace(new RegExp("^[.]", "u"), "");
}

function ensure_dir_bang(path) {
  return (() => { try {
    mkdirSync(path, {[$$bc$property_key($$bc$keyword("recursive"))]: true, [$$bc$property_key($$bc$keyword("mode"))]: 448});
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

function unlink_quiet_bang(path) {
  (() => { try {
    return unlinkSync(path);
  } catch (_catch_3) {
    switch ($$bd$catch_dispatch(_catch_3, [Error])) {
      case 0: {
        const __ = _catch_3;
        return null;
        break;
      }
    }
  } })();
  return null;
}

function rmdir_quiet_bang(path) {
  (() => { try {
    return rmdirSync(path);
  } catch (_catch_4) {
    switch ($$bd$catch_dispatch(_catch_4, [Error])) {
      case 0: {
        const __ = _catch_4;
        return null;
        break;
      }
    }
  } })();
  return null;
}

function write_atomic_bang(path, body) {
  const parent = dirname(path);
  const temp = join(parent, $$bc$str(".", basename(path), ".", process.pid, ".", random_token(), ".tmp"));
  return ((!ensure_dir_bang(parent)) ? false : (() => { try {
    writeFileSync(temp, body, {[$$bc$property_key($$bc$keyword("encoding"))]: "utf8", [$$bc$property_key($$bc$keyword("mode"))]: 384});
  renameSync(temp, path);
  return true;
  } catch (_catch_5) {
    switch ($$bd$catch_dispatch(_catch_5, [Error])) {
      case 0: {
        const cause = _catch_5;
        if ((env_value("NORTH_LIFECYCLE_DEBUG") === "1")) {
          console.error(cause);
        }
        unlink_quiet_bang(temp);
        return false;
        break;
      }
    }
  } })());
}

function parse_envelope(raw) {
  return (() => { try {
    const value = JSON.parse(raw);
  return (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? ((typeof value === "object") && (!Array.isArray(value))) : _logical))(value)) ? value : null);
  } catch (_catch_6) {
    switch ($$bd$catch_dispatch(_catch_6, [Error])) {
      case 0: {
        const __ = _catch_6;
        return null;
        break;
      }
    }
  } })();
}

function number_value(value) {
  return value;
}

function int_value(value) {
  return value;
}

function command_result(command, args, timeout_ms, env) {
  return (() => { try {
    return spawnSync(command, args, {[$$bc$property_key($$bc$keyword("encoding"))]: "utf8", [$$bc$property_key($$bc$keyword("timeout"))]: timeout_ms, [$$bc$property_key($$bc$keyword("env"))]: env, [$$bc$property_key($$bc$keyword("stdio"))]: ["ignore", "pipe", "ignore"]});
  } catch (_catch_7) {
    switch ($$bd$catch_dispatch(_catch_7, [Error])) {
      case 0: {
        const __ = _catch_7;
        return null;
        break;
      }
    }
  } })();
}

function command_output(command, args, timeout_ms) {
  const result = command_result(command, args, timeout_ms, process.env);
  return (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (result.status === 0) : _logical))(result)) ? text_value(result.stdout).trim() : "");
}

function supervised_result(command, args, deadline, timeout_ms, env) {
  return command_result("timeout", ["--signal=TERM", "--kill-after=0.2s", deadline, command].concat(args), timeout_ms, env);
}

function supervised_ok_p(command, args, deadline, timeout_ms, env) {
  const result = supervised_result(command, args, deadline, timeout_ms, env);
  return ((_logical) => (_logical !== false && _logical != null ? (result.status === 0) : _logical))(result);
}

function supervised_captured(command, args, deadline, timeout_ms) {
  const result = supervised_result(command, args, deadline, timeout_ms, process.env);
  return (((_truthy) => _truthy !== false && _truthy != null)(result) ? text_value(result.stdout).trimEnd() : "");
}

function north_home() {
  return text_or(env_value("NORTH_HOME"), resolve(dirname(text_value((() => { const _x = process.argv, _i = 1; return _x[_i] != null ? _x[_i] : ""; })())), ".."));
}

function stable_bin(name) {
  const home = north_home();
  return (((_truthy) => _truthy !== false && _truthy != null)(home.startsWith("/nix/store/")) ? ((name === "north") ? $$bc$str(env_value("HOME"), "/code/north/main/bin/north") : name) : join(home, "bin", name));
}

function repo_info(cwd) {
  const requested = ((cwd === "") ? text_value(process.cwd) : cwd);
  const root_output = command_output("git", ["-C", requested, "rev-parse", "--show-toplevel"], 300);
  const root = ((root_output === "") ? requested : root_output);
  const common = command_output("git", ["-C", requested, "rev-parse", "--path-format=absolute", "--git-common-dir"], 300);
  const remote = command_output("git", ["-C", requested, "remote", "get-url", "origin"], 300);
  const initial_name = (((_truthy) => _truthy !== false && _truthy != null)(((!(common === "")) && (basename(common) === ".git"))) ? basename(dirname(common)) : basename(root));
  const trimmed = remote.replace(new RegExp("/+$", "u"), "").replace(new RegExp("[.]git$", "u"), "");
  const slash_name = (() => { const _x = trimmed.split("/"), _i = (int_value(trimmed.split("/").length) - 1); return _x[_i] != null ? _x[_i] : ""; })();
  const colon_name = (() => { const _x = slash_name.split(":"), _i = (int_value(slash_name.split(":").length) - 1); return _x[_i] != null ? _x[_i] : ""; })();
  return {[$$bc$property_key($$bc$keyword("root"))]: root, [$$bc$property_key($$bc$keyword("name"))]: ((colon_name === "") ? initial_name : colon_name)};
}

function slug(value) {
  const lower = value.toLowerCase();
  const collapsed = lower.replace(new RegExp("[^a-z0-9]+", "gu"), "-");
  return collapsed.replace(new RegExp("^-", "u"), "").replace(new RegExp("-$", "u"), "");
}

function provider_observation() {
  const explicit = env_value("AGENT_PROVIDER");
  return ((((_truthy) => _truthy !== false && _truthy != null)(["anthropic", "openai"].includes(explicit))) ? explicit : (((_truthy) => _truthy !== false && _truthy != null)(((!(env_value("CODEX_THREAD_ID") === "")) || (!(env_value("CODEX_CI") === ""))))) ? "openai" : ((!(env_value("CLAUDECODE") === ""))) ? "anthropic" : "");
}

function dispatch_mode_at_start() {
  const state_path = text_or(env_value("NORTH_HARNESS_STATE"), join(env_value("HOME"), ".local", "state", "north", "harness.conf"));
  const mode = read_text(state_path).split("\n").reduce((current, line) => (((_truthy) => _truthy !== false && _truthy != null)(line.startsWith("dispatch=")) ? line.slice("dispatch=".length) : current), "managed");
  return (((_truthy) => _truthy !== false && _truthy != null)(["native", "managed", "auto"].includes(mode)) ? mode : "invalid");
}

function acquire_singleflight_bang(lock, stale_after_s) {
  return ((!ensure_dir_bang(dirname(lock))) ? false : (() => { try {
    mkdirSync(lock, {[$$bc$property_key($$bc$keyword("mode"))]: 448});
  return true;
  } catch (_catch_8) {
    switch ($$bd$catch_dispatch(_catch_8, [Error])) {
      case 0: {
        const __ = _catch_8;
        return (() => { try {
    const modified = number_value(statSync(lock).mtimeMs);
  const age = (number_value(Date.now()) - modified);
  if ((age < (stale_after_s * 1000))) {
    return false;
  } else {
    rmdirSync(lock);
    mkdirSync(lock, {[$$bc$property_key($$bc$keyword("mode"))]: 448});
    return true;
  }
  } catch (_catch_9) {
    switch ($$bd$catch_dispatch(_catch_9, [Error])) {
      case 0: {
        const __ = _catch_9;
        return false;
        break;
      }
    }
  } })();
        break;
      }
    }
  } })());
}

function spawn_worker_bang(mode, payload) {
  (() => { try {
    const child = spawn(text_value(process.execPath), [text_value((() => { const _x = process.argv, _i = 1; return _x[_i] != null ? _x[_i] : ""; })()), mode, JSON.stringify(payload)], {[$$bc$property_key($$bc$keyword("env"))]: process.env, [$$bc$property_key($$bc$keyword("stdio"))]: "ignore", [$$bc$property_key($$bc$keyword("detached"))]: true});
  return child.unref();
  } catch (_catch_10) {
    switch ($$bd$catch_dispatch(_catch_10, [Error])) {
      case 0: {
        const __ = _catch_10;
        return null;
        break;
      }
    }
  } })();
  return null;
}

function projection_env(payload) {
  return Object.assign({}, process.env, {[$$bc$property_key($$bc$keyword("NORTH_HOME"))]: north_home(), [$$bc$property_key($$bc$keyword("NORTH_NATIVE_PORT"))]: text_or(env_value("NORTH_PORT"), "7977"), [$$bc$property_key($$bc$keyword("NORTH_NATIVE_SUBJECT"))]: $$bc$str("@agent:", payload.id), [$$bc$property_key($$bc$keyword("NORTH_NATIVE_REPO"))]: payload.repoName, [$$bc$property_key($$bc$keyword("NORTH_NATIVE_PROVIDER"))]: payload.provider, [$$bc$property_key($$bc$keyword("NORTH_NATIVE_MODEL"))]: payload.model, [$$bc$property_key($$bc$keyword("NORTH_NATIVE_EFFORT"))]: payload.effort, [$$bc$property_key($$bc$keyword("NORTH_NATIVE_DISPLAY"))]: payload.display, [$$bc$property_key($$bc$keyword("NORTH_NATIVE_ROLE"))]: payload.role, [$$bc$property_key($$bc$keyword("NORTH_NATIVE_ROLE_ALIAS"))]: payload.roleAlias, [$$bc$property_key($$bc$keyword("NORTH_NATIVE_ACTOR_KIND"))]: ((_logical) => (_logical !== false && _logical != null ? _logical : ""))(payload.actorKind), [$$bc$property_key($$bc$keyword("NORTH_NATIVE_DEPTH"))]: ((_logical) => (_logical !== false && _logical != null ? _logical : ""))(payload.depth), [$$bc$property_key($$bc$keyword("NORTH_NATIVE_DISPATCH_MODE_AT_START"))]: ((_logical) => (_logical !== false && _logical != null ? _logical : ""))(payload.dispatchMode), [$$bc$property_key($$bc$keyword("NORTH_NATIVE_PARENT_ACTOR_KEY"))]: ((_logical) => (_logical !== false && _logical != null ? _logical : ""))(payload.parentActorKey), [$$bc$property_key($$bc$keyword("NORTH_NATIVE_PROVIDER_SESSION_KEY"))]: ((_logical) => (_logical !== false && _logical != null ? _logical : ""))(payload.providerSessionKey)});
}

function spawn_maintenance_bang(payload) {
  const lock = payload.lock;
  (() => { try {
    const presence_ok = supervised_ok_p("bb", [join(north_home(), "cli", "presence-cli.clj"), text_or(env_value("NORTH_PORT"), "7977"), "register", payload.id, payload.repoRoot, payload.id], "3s", 3500, process.env);
  const projection_ok = supervised_ok_p("bb", [join(north_home(), "cli", "provider-native-session-projection.clj"), "spawn"], "6s", 6500, projection_env(payload));
  presence_ok;
  if (projection_ok) {
    return write_atomic_bang(payload.routeCache, route_body(payload.provider, payload.model, payload.effort, payload.role, payload.roleAlias, payload.repoName));
  }
  } catch (_catch_11) {
    switch ($$bd$catch_dispatch(_catch_11, [Error])) {
      case 0: {
        const __ = _catch_11;
        return null;
        break;
      }
    }
  } })();
  rmdir_quiet_bang(lock);
  return null;
}

function repair_maintenance_bang(payload) {
  const lock = payload.lock;
  (() => { try {
    const current = route_lines(payload.routeCache);
  const desired = route_body(payload.provider, payload.model, payload.effort, payload.role, payload.roleAlias, payload.repoName);
  if ((route_body(current[0], current[1], current[2], current[3], current[4], current[5]) === desired)) {
    return null;
  } else {
    if (supervised_ok_p("bb", [join(north_home(), "cli", "provider-native-session-projection.clj"), "repair"], "6s", 6500, projection_env(payload))) {
      return write_atomic_bang(payload.routeCache, desired);
    }
  }
  } catch (_catch_12) {
    switch ($$bd$catch_dispatch(_catch_12, [Error])) {
      case 0: {
        const __ = _catch_12;
        return null;
        break;
      }
    }
  } })();
  rmdir_quiet_bang(lock);
  return null;
}

function renew_maintenance_bang(payload) {
  const lock = payload.lock;
  const environment = Object.assign({}, process.env, {[$$bc$property_key($$bc$keyword("NORTH_COMMS_DIR"))]: payload.repoRoot, [$$bc$property_key($$bc$keyword("NORTH_COMMS_SESSION_ID"))]: payload.id, [$$bc$property_key($$bc$keyword("NORTH_COMMS_ALIAS"))]: payload.roleAlias});
  (() => { try {
    if (supervised_ok_p(join(north_home(), "bin", "north-comms"), ["lease", payload.id, "--announce"], "3s", 3500, environment)) {
    return write_atomic_bang(payload.mark, $$bc$str(payload.now));
  }
  } catch (_catch_13) {
    switch ($$bd$catch_dispatch(_catch_13, [Error])) {
      case 0: {
        const __ = _catch_13;
        return null;
        break;
      }
    }
  } })();
  rmdir_quiet_bang(lock);
  return null;
}

function clean_field(value) {
  return text_value(value).replace(new RegExp("[\\r\\n]", "gu"), "").replace(new RegExp("\\u0000", "gu"), "");
}

function validated_pin() {
  const pin = env_value("NORTH_AGENT_ID");
  return (((_truthy) => _truthy !== false && _truthy != null)(actor_key("managed", pin)) ? pin : "");
}

function resolved_identity(session_id, provider_agent_id) {
  const id_dir = join(runtime_root(), "north-agent-ids");
  const agent_key = actor_key("agent", provider_agent_id);
  const pin = validated_pin();
  const session_key = actor_key("session", session_id);
  const pin_key = actor_key("managed", pin);
  const identity_key = ((((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? readable_p(join(id_dir, agent_key)) : _logical))(agent_key))) ? agent_key : (((_truthy) => _truthy !== false && _truthy != null)(session_key)) ? session_key : (((_truthy) => _truthy !== false && _truthy != null)(pin_key)) ? pin_key : null);
  if ((identity_key == null)) {
    return null;
  } else {
    const cached = read_text(join(id_dir, identity_key)).trim();
    const id = ((valid_actor_p(cached)) ? cached : ((!(pin === ""))) ? pin : $$bc$str("native-", identity_key));
    return {[$$bc$property_key($$bc$keyword("identityKey"))]: identity_key, [$$bc$property_key($$bc$keyword("id"))]: id};
  }
}

function mark_delegated_bang(envelope) {
  const session_id = clean_field(envelope.session_id);
  const provider_agent_id = clean_field(envelope.agent_id);
  const identity = resolved_identity(session_id, provider_agent_id);
  if (((_truthy) => _truthy !== false && _truthy != null)(identity)) {
    write_atomic_bang(join(runtime_root(), "north-delegated", identity.identityKey), $$bc$str(identity.id, "\n", Math.floor((Date.now() / 1000)), "\n"));
  }
  return null;
}

function claim_pin_bang(id_dir, identity_key, pin, pin_key) {
  const owners = join(id_dir, ".pin-owners");
  if ((!((_truthy) => _truthy !== false && _truthy != null)((ensure_dir_bang(id_dir) && ensure_dir_bang(owners))))) {
    return "";
  } else {
    const matching = readdirSync(id_dir).filter((name) => ((!((_truthy) => _truthy !== false && _truthy != null)(name.startsWith("."))) && (read_text(join(id_dir, name)).trim() === pin)));
    const claim_owner = (((matching.length === 0)) ? identity_key : ((matching.length === 1)) ? matching[0] : "__conflicting_legacy_owners__");
    const claim_file = join(owners, pin_key);
    const temp = join(owners, $$bc$str(".", pin_key, ".", process.pid, ".", random_token(), ".tmp"));
    (() => { try {
    writeFileSync(temp, claim_owner, {[$$bc$property_key($$bc$keyword("encoding"))]: "utf8", [$$bc$property_key($$bc$keyword("mode"))]: 384});
  (() => { try {
    return linkSync(temp, claim_file);
  } catch (_catch_14) {
    switch ($$bd$catch_dispatch(_catch_14, [Error])) {
      case 0: {
        const __ = _catch_14;
        return null;
        break;
      }
    }
  } })();
  return unlink_quiet_bang(temp);
  } catch (_catch_15) {
    switch ($$bd$catch_dispatch(_catch_15, [Error])) {
      case 0: {
        const __ = _catch_15;
        return unlink_quiet_bang(temp);
        break;
      }
    }
  } })();
    return ((read_text(claim_file).trim() === identity_key) ? pin : "");
  }
}

function spawn_hook_bang(envelope) {
  const event = (() => { const candidate = clean_field(envelope.hook_event_name); return ((candidate === "") ? "SessionStart" : candidate); })();
  if (((_truthy) => _truthy !== false && _truthy != null)(["SessionStart", "SubagentStart", "SubagentSessionStart"].includes(event))) {
    const cwd = (() => { const candidate = clean_field(envelope.cwd); return ((candidate === "") ? text_value(process.cwd) : candidate); })();
    const session_id = clean_field(envelope.session_id);
    const provider_agent_id = clean_field(envelope.agent_id);
    const pin = validated_pin();
    const identity_kind = ((((_truthy) => _truthy !== false && _truthy != null)(((event === "SubagentStart") && (!(provider_agent_id === ""))))) ? "agent" : ((!(session_id === ""))) ? "session" : "managed");
    const identity_source = (((identity_kind === "agent")) ? provider_agent_id : ((identity_kind === "session")) ? session_id : pin);
    const identity_key = actor_key(identity_kind, identity_source);
    if (((_truthy) => _truthy !== false && _truthy != null)(identity_key)) {
      const id_dir = join(runtime_root(), "north-agent-ids");
      const route_dir = join(runtime_root(), "north-agent-routes");
      const pin_key = actor_key("managed", pin);
      const claimed = (((_truthy) => _truthy !== false && _truthy != null)(pin_key) ? claim_pin_bang(id_dir, identity_key, pin, pin_key) : "");
      const id = ((claimed === "") ? $$bc$str("native-", identity_key) : claimed);
      const cache = join(id_dir, identity_key);
      const repo = repo_info(cwd);
      const repo_root = repo.root;
      const repo_name = repo.name;
      const actor_kind = (((_truthy) => _truthy !== false && _truthy != null)(["SubagentStart", "SubagentSessionStart"].includes(event)) ? "subagent" : "root");
      const depth = ((actor_kind === "subagent") ? "1" : "0");
      const parent_key = ((actor_kind === "subagent") ? ((_logical) => (_logical !== false && _logical != null ? _logical : "unobserved"))(actor_key("session", session_id)) : "");
      const provider_session_key = (((_truthy) => _truthy !== false && _truthy != null)(((actor_kind === "root") && (!(session_id === "")))) ? ((_logical) => (_logical !== false && _logical != null ? _logical : ""))(actor_key("session", session_id)) : "");
      const provider = text_or(provider_observation(), "unobserved");
      const model = text_or(clean_field(envelope.model), text_or(env_value("AGENT_MODEL"), "unobserved"));
      const effort_field = envelope.effort;
      const effort = text_or(clean_field((((_truthy) => _truthy !== false && _truthy != null)(effort_field) ? effort_field.level : null)), text_or(env_value("CLAUDE_EFFORT"), text_or(env_value("AGENT_REASONING"), "unobserved")));
      const role = slug(env_value("NORTH_ORCHESTRATION_ROLE"));
      const repo_segment = slug(repo_name);
      const role_alias = (((_truthy) => _truthy !== false && _truthy != null)(((!(role === "")) && (!(repo_segment === "")))) ? $$bc$str(repo_segment, "-", role) : "");
      const dispatch_mode = dispatch_mode_at_start();
      const display = $$bc$str((() => { const v = slug(provider); return ((v === "") ? "unobserved" : v); })(), "-", (() => { const v = slug(model); return ((v === "") ? "unobserved" : v); })(), "-", (() => { const v = slug(effort); return ((v === "") ? "unobserved" : v); })(), "-native-", (() => { const _x = id.split("-"), _i = (int_value(id.split("-").length) - 1); return _x[_i] != null ? _x[_i] : ""; })());
      const route_cache = join(route_dir, identity_key);
      const route_seed = $$bc$str(route_cache, ".seed");
      const role_context = ((role_alias === "") ? "This session has no declared durable role alias; ask for the peer's repo-role alias before sending." : $$bc$str("Your durable role alias is \"", role_alias, "\"; peers should address that alias, not this session id."));
      const north_bin = stable_bin("north");
      const context = $$bc$str("[north coordination active — you are agent \"", id, "\" in repo \"", repo_name, "\"]\n", "FIRST ACT: arm your real-time interrupt listener as a BACKGROUND task now:\n  ", north_bin, " listen ", id, "\n", "Eight deliveries sat unlanded when coordinators were unarmed. Dormant (no tokens) until a peer\n", "addresses you; when it returns you have mail — read it, act, re-arm. Watchers match invariants,\n", "never cohort names; a hand-rolled poller never substitutes for the designed channel.\n\n", "Other agents may be working here concurrently. Coordinate through North threads, lane\n", "registrations, and mail. Inspect `", north_bin, " board` and `", north_bin, " agents`,\n", "then keep the current thread and affected peers updated as the work changes or lands.\n\n", "Internal notes / status / scratch / handoffs -> docs/private/ (gitignored), NEVER public docs/.\n", "Run `", stable_bin("ensure-private-docs"), "` to set up the ignore in a repo before writing there.\n", "Need a system rebuild? Commit your own changes, then run `firn rebuild` directly.\n\n", role_context, "\n", "Address peers ALIAS-FIRST using their stable <repo>-<role> name. Direct agent ids are\n", "session-scoped and should be used only when that exact current session is intended.\n", "`north-comms send` resolves aliases at send time and FAILS LOUDLY when the concrete recipient\n", "has neither a live lease nor an armed listener; do not guess or retry another session hash.\n", "`--dead-drop` is only for a deliberate send to an absent identity.\n", "Ping a peer:  ", stable_bin("north-comms"), " send ", id, " <peer-role-alias> \"URGENT\" \"<message>\"");
      write_atomic_bang(cache, id);
      write_atomic_bang(route_seed, route_body(provider, model, effort, role, role_alias, repo_name));
      emit_json_bang({[$$bc$property_key($$bc$keyword("hookSpecificOutput"))]: {[$$bc$property_key($$bc$keyword("hookEventName"))]: event, [$$bc$property_key($$bc$keyword("additionalContext"))]: context}});
      const lock = $$bc$str(route_cache, ".spawn.lock");
      if (acquire_singleflight_bang(lock, 10)) {
        spawn_worker_bang("__spawn-maintenance", {[$$bc$property_key($$bc$keyword("lock"))]: lock, [$$bc$property_key($$bc$keyword("routeCache"))]: route_cache, [$$bc$property_key($$bc$keyword("id"))]: id, [$$bc$property_key($$bc$keyword("repoRoot"))]: repo_root, [$$bc$property_key($$bc$keyword("repoName"))]: repo_name, [$$bc$property_key($$bc$keyword("provider"))]: provider, [$$bc$property_key($$bc$keyword("model"))]: model, [$$bc$property_key($$bc$keyword("effort"))]: effort, [$$bc$property_key($$bc$keyword("display"))]: display, [$$bc$property_key($$bc$keyword("role"))]: role, [$$bc$property_key($$bc$keyword("roleAlias"))]: role_alias, [$$bc$property_key($$bc$keyword("actorKind"))]: actor_kind, [$$bc$property_key($$bc$keyword("depth"))]: depth, [$$bc$property_key($$bc$keyword("dispatchMode"))]: dispatch_mode, [$$bc$property_key($$bc$keyword("parentActorKey"))]: parent_key, [$$bc$property_key($$bc$keyword("providerSessionKey"))]: provider_session_key});
      }
    }
  }
  return null;
}

function schedule_repair_bang(route_cache, id, repo_name, provider, model, effort, display, role, role_alias) {
  const lock = $$bc$str(route_cache, ".repair.lock");
  if (acquire_singleflight_bang(lock, 10)) {
    spawn_worker_bang("__repair-maintenance", {[$$bc$property_key($$bc$keyword("lock"))]: lock, [$$bc$property_key($$bc$keyword("routeCache"))]: route_cache, [$$bc$property_key($$bc$keyword("id"))]: id, [$$bc$property_key($$bc$keyword("repoName"))]: repo_name, [$$bc$property_key($$bc$keyword("provider"))]: provider, [$$bc$property_key($$bc$keyword("model"))]: model, [$$bc$property_key($$bc$keyword("effort"))]: effort, [$$bc$property_key($$bc$keyword("display"))]: display, [$$bc$property_key($$bc$keyword("role"))]: role, [$$bc$property_key($$bc$keyword("roleAlias"))]: role_alias});
  }
  return null;
}

function numeric_file(path) {
  const value = read_text(path).trim();
  return (((_truthy) => _truthy !== false && _truthy != null)(new RegExp("^[0-9]+$", "u").test(value)) ? parseInt(value, 10) : 0);
}

function tooluse_hook_bang(envelope) {
  const cwd = (() => { const candidate = clean_field(envelope.cwd); return ((candidate === "") ? text_value(process.cwd) : candidate); })();
  const session_id = clean_field(envelope.session_id);
  const provider_agent_id = clean_field(envelope.agent_id);
  const identity = resolved_identity(session_id, provider_agent_id);
  if (((_truthy) => _truthy !== false && _truthy != null)(identity)) {
    const identity_key = identity.identityKey;
    const id = identity.id;
    const repo = repo_info(cwd);
    const repo_root = repo.root;
    const repo_name = repo.name;
    const route_cache = join(runtime_root(), "north-agent-routes", identity_key);
    const route_seed = $$bc$str(route_cache, ".seed");
    const cached = (readable_p(route_cache) ? route_lines(route_cache) : ["", "", "", "", "", ""]);
    const seed = (((_truthy) => _truthy !== false && _truthy != null)(((!readable_p(route_cache)) && readable_p(route_seed))) ? route_lines(route_seed) : ["", "", "", "", "", ""]);
    const ambient_role = slug(env_value("NORTH_ORCHESTRATION_ROLE"));
    const ambient_alias = ((ambient_role === "") ? "" : $$bc$str(slug(repo_name), "-", ambient_role));
    const role = text_or((() => { const _x = cached, _i = 3; return _x[_i] != null ? _x[_i] : ""; })(), text_or((() => { const _x = seed, _i = 3; return _x[_i] != null ? _x[_i] : ""; })(), ambient_role));
    const role_alias = text_or((() => { const _x = cached, _i = 4; return _x[_i] != null ? _x[_i] : ""; })(), text_or((() => { const _x = seed, _i = 4; return _x[_i] != null ? _x[_i] : ""; })(), ambient_alias));
    const route_repo = text_or((() => { const _x = cached, _i = 5; return _x[_i] != null ? _x[_i] : ""; })(), text_or((() => { const _x = seed, _i = 5; return _x[_i] != null ? _x[_i] : ""; })(), repo_name));
    const effort_field = envelope.effort;
    const observed_effort = text_or(clean_field((((_truthy) => _truthy !== false && _truthy != null)(effort_field) ? effort_field.level : null)), env_value("CLAUDE_EFFORT"));
    const seed_provider = (() => { const _x = seed, _i = 0; return _x[_i] != null ? _x[_i] : ""; })();
    const exact_provider = provider_observation();
    const provider = (readable_p(route_cache) ? (() => { const _x = cached, _i = 0; return _x[_i] != null ? _x[_i] : ""; })() : text_or(exact_provider, text_or(seed_provider, "unobserved")));
    const model = (readable_p(route_cache) ? (() => { const _x = cached, _i = 1; return _x[_i] != null ? _x[_i] : ""; })() : text_or((() => { const _x = seed, _i = 1; return _x[_i] != null ? _x[_i] : ""; })(), "unobserved"));
    const effort = (readable_p(route_cache) ? ((observed_effort === "") ? (() => { const _x = cached, _i = 2; return _x[_i] != null ? _x[_i] : ""; })() : observed_effort) : text_or(observed_effort, text_or((() => { const _x = seed, _i = 2; return _x[_i] != null ? _x[_i] : ""; })(), "unobserved")));
    const display = $$bc$str(text_or(slug(provider), "unknown"), "-", text_or(slug(model), "unknown"), "-", text_or(slug(effort), "unknown"), "-native-", (() => { const _x = id.split("-"), _i = (int_value(id.split("-").length) - 1); return _x[_i] != null ? _x[_i] : ""; })());
    const cached_body = route_body((() => { const _x = cached, _i = 0; return _x[_i] != null ? _x[_i] : ""; })(), (() => { const _x = cached, _i = 1; return _x[_i] != null ? _x[_i] : ""; })(), (() => { const _x = cached, _i = 2; return _x[_i] != null ? _x[_i] : ""; })(), (() => { const _x = cached, _i = 3; return _x[_i] != null ? _x[_i] : ""; })(), (() => { const _x = cached, _i = 4; return _x[_i] != null ? _x[_i] : ""; })(), (() => { const _x = cached, _i = 5; return _x[_i] != null ? _x[_i] : ""; })());
    const desired_body = route_body(provider, model, effort, role, role_alias, route_repo);
    if (((_truthy) => _truthy !== false && _truthy != null)(((!readable_p(route_cache)) || (!(cached_body === desired_body))))) {
      schedule_repair_bang(route_cache, id, route_repo, provider, model, effort, display, role, role_alias);
    }
    const renew_dir = join(runtime_root(), "north-presence-renew");
    const renew_mark = join(renew_dir, identity_key);
    const now = Math.floor((number_value(Date.now()) / 1000));
    const last = numeric_file(renew_mark);
    if (((_truthy) => _truthy !== false && _truthy != null)(((now === 0) || ((now - last) >= 60)))) {
      const lock = $$bc$str(renew_mark, ".lock");
      if (acquire_singleflight_bang(lock, 8)) {
        const fresh = numeric_file(renew_mark);
        if (((_truthy) => _truthy !== false && _truthy != null)(((!(now === 0)) && ((now - fresh) < 60)))) {
          rmdir_quiet_bang(lock);
        } else {
          spawn_worker_bang("__renew-maintenance", {[$$bc$property_key($$bc$keyword("lock"))]: lock, [$$bc$property_key($$bc$keyword("mark"))]: renew_mark, [$$bc$property_key($$bc$keyword("now"))]: now, [$$bc$property_key($$bc$keyword("id"))]: id, [$$bc$property_key($$bc$keyword("repoRoot"))]: repo_root, [$$bc$property_key($$bc$keyword("roleAlias"))]: role_alias});
        }
      }
    }
    const mail = supervised_captured(join(north_home(), "bin", "north-comms"), ["poll", id], "2s", 2500);
    if ((!(mail === ""))) {
      emit_json_bang({[$$bc$property_key($$bc$keyword("hookSpecificOutput"))]: {[$$bc$property_key($$bc$keyword("hookEventName"))]: "PostToolUse", [$$bc$property_key($$bc$keyword("additionalContext"))]: $$bc$str("[north] new peer message(s):\n", mail)}});
    }
  }
  return null;
}

function valid_stop_envelope_p(envelope) {
  const cwd = text_value(envelope.cwd);
  const session_id = text_value(envelope.session_id);
  const agent_id = text_value(envelope.agent_id);
  return ((!((_truthy) => _truthy !== false && _truthy != null)(envelope.stop_hook_active)) && ((cwd.length > 0) && ((cwd.length <= 4096) && ((_logical) => (_logical !== false && _logical != null ? ((!((_truthy) => _truthy !== false && _truthy != null)(new RegExp("[\\u0000\\n\\r\\t]", "u").test(cwd))) && ((_logical) => (_logical !== false && _logical != null ? ((agent_id === "") || valid_actor_p(agent_id)) : _logical))(((session_id === "") || valid_actor_p(session_id)))) : _logical))(cwd.startsWith("/")))));
}

function listener_live_p(id) {
  return (() => { try {
    return readdirSync("/proc").some((pid) => ((_logical) => (_logical !== false && _logical != null ? (() => { const argv = read_text(join("/proc", pid, "cmdline")).split("\u0000"); return ((_logical) => (_logical !== false && _logical != null ? argv.includes(id) : _logical))(argv.some((token) => ((token === "north-listen.clj") || token.endsWith("/north-listen.clj")))); })() : _logical))(new RegExp("^[0-9]+$", "u").test(pid)));
  } catch (_catch_16) {
    switch ($$bd$catch_dispatch(_catch_16, [Error])) {
      case 0: {
        const __ = _catch_16;
        return false;
        break;
      }
    }
  } })();
}

function shell_quote(value) {
  return value.replace(new RegExp("[^A-Za-z0-9_./:-]", "gu"), (character) => $$bc$str("\\", character));
}

function stop_decision(envelope) {
  if ((!valid_stop_envelope_p(envelope))) {
    return null;
  } else {
    const session_id = text_value(envelope.session_id);
    const provider_agent_id = text_value(envelope.agent_id);
    const identity = resolved_identity(session_id, provider_agent_id);
    if ((identity == null)) {
      return null;
    } else {
      const id = identity.id;
      const marker = join(runtime_root(), "north-delegated", identity.identityKey);
      const marked_id = first_line(marker).trim();
      if (((_truthy) => _truthy !== false && _truthy != null)(((!readable_p(marker)) || ((!(marked_id === id)) || listener_live_p(id))))) {
        return null;
      } else {
        const home = north_home();
        const clear = $$bc$str("rm -f -- ", shell_quote(marker));
        return {[$$bc$property_key($$bc$keyword("decision"))]: "block", [$$bc$property_key($$bc$keyword("reason"))]: $$bc$str("You delegated work this session but no north listener is armed. ", "If you end your turn now, a spawned worker's completion/death ping lands in a dead inbox and this session goes SILENT until a human pokes it. ", "Arm the listener as a BACKGROUND task before yielding:  ", home, "/bin/north listen ", id, "  (run_in_background). When it fires, handle the mail and re-arm. ", "If every delegated worker is already reconciled and you truly want to stop, clear the marker:  ", clear)};
      }
    }
  }
}

function emit_json_bang(value) {
  if (((_truthy) => _truthy !== false && _truthy != null)(value)) {
    process.stdout.write($$bc$str(JSON.stringify(value), "\n"));
  }
  return null;
}

function stdin_text_bounded_bang(milliseconds) {
  const result = Promise.withResolvers();
  const timer = setTimeout(() => process.exit(0), milliseconds);
  Bun.stdin.text().then((raw) => { clearTimeout(timer);
return result.resolve(raw); }, (__) => { clearTimeout(timer);
return result.resolve(null); });
  return result.promise;
}

async function main_bang(args) {
  return (async () => { try {
    const __ = process.umask(63);
  const mode = text_value((() => { const _x = args, _i = 0; return _x[_i] != null ? _x[_i] : ""; })());
  if ((mode === "actor-key")) {
    const key = actor_key(text_value((() => { const _x = args, _i = 1; return _x[_i] != null ? _x[_i] : ""; })()), text_value((() => { const _x = args, _i = 2; return _x[_i] != null ? _x[_i] : ""; })()));
    if (((_truthy) => _truthy !== false && _truthy != null)(key)) {
      process.stdout.write(key);
      return 0;
    } else {
      return 2;
    }
  } else {
    if (((_truthy) => _truthy !== false && _truthy != null)(mode.startsWith("__"))) {
      const payload = parse_envelope(text_value((() => { const _x = args, _i = 1; return _x[_i] != null ? _x[_i] : ""; })()));
      if (((_truthy) => _truthy !== false && _truthy != null)(payload)) {
        if ((mode === "__spawn-maintenance")) {
          spawn_maintenance_bang(payload);
        } else if ((mode === "__repair-maintenance")) {
          repair_maintenance_bang(payload);
        } else if ((mode === "__renew-maintenance")) {
          renew_maintenance_bang(payload);
        } else {
          null;
        }
      }
    } else {
      const raw = await stdin_text_bounded_bang(((mode === "stop") ? 1600 : 1200));
      const envelope = (((_truthy) => _truthy !== false && _truthy != null)(raw) ? parse_envelope(raw) : null);
      if (((_truthy) => _truthy !== false && _truthy != null)(envelope)) {
        if ((mode === "mark-delegated")) {
          mark_delegated_bang(envelope);
        } else if ((mode === "spawn")) {
          spawn_hook_bang(envelope);
        } else if ((mode === "tooluse")) {
          tooluse_hook_bang(envelope);
        } else if ((mode === "stop")) {
          emit_json_bang(stop_decision(envelope));
        } else {
          null;
        }
      }
    }
    return 0;
  }
  } catch (_catch_17) {
    switch ($$bd$catch_dispatch(_catch_17, [Error])) {
      case 0: {
        const cause = _catch_17;
        if ((env_value("NORTH_LIFECYCLE_DEBUG") === "1")) {
          console.error(cause);
        }
        return 0;
        break;
      }
    }
  } })();
}

if (((_truthy) => _truthy !== false && _truthy != null)(import.meta.main)) {
  main_bang(process.argv.slice(2)).then((code) => (process.exitCode = code));
}
