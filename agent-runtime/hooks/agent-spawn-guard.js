import { spawnSync } from 'child_process';
import { readFileSync, readdirSync } from 'fs';
import { join } from 'path';
import { keyword as $$bc$keyword, property_key as $$bc$property_key, str as $$bc$str } from '../../sdk/src/bridge/generated/beagle/core.js';
import { catch_dispatch as $$bd$catch_dispatch } from '../../sdk/src/bridge/generated/beagle/exception-dispatch.js';

const CONTROL = new Set([";", "&&", "||", "|", "|&", "&", "\n", "(", ")"]);

const SHELLS = new Set(["bash", "sh", "dash", "zsh", "fish", "ksh"]);

const SIMPLE_WRAPPERS = new Set(["command", "exec", "nohup"]);

const HELP_VERSION_FLAGS = new Set(["-h", "--help", "-V", "--version"]);

function text_value(value) {
  return ((typeof value === "string") ? value : "");
}

function north_home_bang(environment) {
  const configured = text_value(environment.NORTH_HOME).trim();
  const home = text_value(environment.HOME).trim();
  return (((!(configured === ""))) ? configured : ((!(home === ""))) ? join(home, "code", "north", "main") : (() => { throw new Error("North home is unavailable"); })());
}

function basename_token(token) {
  const parts = token.replace(new RegExp("/+$", "u"), "").split("/");
  return ((_logical) => (_logical !== false && _logical != null ? _logical : ""))((() => { const _x = parts, _i = (int_value(parts.length) - 1); return _x[_i] != null ? _x[_i] : ""; })());
}

function regex_p(pattern, value) {
  return new RegExp(pattern, "u").test(value);
}

function int_value(value) {
  return value;
}

function assignment_p(token) {
  return regex_p("^[A-Za-z_][A-Za-z0-9_]*=[\\s\\S]*$", token);
}

function redirection_p(token) {
  return regex_p("^[0-9]*(?:<>|>>|<<|>|<).*$", token);
}

function diagnostic_probe_p(args) {
  return args.some((token) => HELP_VERSION_FLAGS.has(token));
}

function has_option_p(args, names) {
  return args.some((token) => names.some((name) => ((token === name) || ((_logical) => (_logical !== false && _logical != null ? token.startsWith($$bc$str(name, "=")) : _logical))(name.startsWith("--")))));
}

function shell_tokens_bang(source) {
  const tokens = [];
  const state = {[$$bc$property_key($$bc$keyword("index"))]: 0, [$$bc$property_key($$bc$keyword("quote"))]: "", [$$bc$property_key($$bc$keyword("escaped"))]: false, [$$bc$property_key($$bc$keyword("current"))]: ""};
  (() => { function flush_bang() { if ((!(state.current === ""))) {
  tokens.push(state.current);
  return (state.current = "");
} } (() => {  while (true) {
    if ((state.index < source.length)) { const character = source.charAt(state.index); const next = source.charAt((int_value(state.index) + 1)); ((((_truthy) => _truthy !== false && _truthy != null)(state.escaped)) ? (() => { (state.current = $$bc$str(state.current, character));
return (state.escaped = false); })() : (((_truthy) => _truthy !== false && _truthy != null)(((!(state.quote === "'")) && (character === "\\")))) ? (state.escaped = true) : (((_truthy) => _truthy !== false && _truthy != null)(((character === "'") && (!(state.quote === "\""))))) ? (state.quote = ((state.quote === "'") ? "" : "'")) : (((_truthy) => _truthy !== false && _truthy != null)(((character === "\"") && (!(state.quote === "'"))))) ? (state.quote = ((state.quote === "\"") ? "" : "\"")) : ((!(state.quote === ""))) ? (state.current = $$bc$str(state.current, character)) : (((_truthy) => _truthy !== false && _truthy != null)(new RegExp("[ \\t\\r]", "u").test(character))) ? flush_bang() : (((_truthy) => _truthy !== false && _truthy != null)([";", "|", "&", "\n", "(", ")"].includes(character))) ? (() => { flush_bang();
if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (next === character) : _logical))(["|", "&"].includes(character)))) {
  tokens.push($$bc$str(character, next));
  return (state.index = (int_value(state.index) + 1));
} else {
  if (((_truthy) => _truthy !== false && _truthy != null)(((character === "|") && (next === "&")))) {
    tokens.push("|&");
    return (state.index = (int_value(state.index) + 1));
  } else {
    return tokens.push(character);
  }
} })() : (state.current = $$bc$str(state.current, character))); (state.index = (int_value(state.index) + 1));  continue; } else { return null; }
  } })();
return flush_bang(); })();
  return tokens;
}

function command_segments_bang(source) {
  const segments = [];
  const state = {[$$bc$property_key($$bc$keyword("current"))]: []};
  shell_tokens_bang(source).forEach((token, index, tokens) => { const redirect_amp = ((token === "&") && ((_logical) => (_logical !== false && _logical != null ? _logical : (((index + 1) < tokens.length) && tokens[(index + 1)].startsWith(">"))))(((state.current.length > 0) && state.current[(int_value(state.current.length) - 1)].endsWith(">"))));
if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (!redirect_amp) : _logical))(CONTROL.has(token)))) {
  if ((state.current.length > 0)) {
    segments.push(state.current);
    return (state.current = []);
  }
} else {
  return state.current.push(token);
} });
  if ((state.current.length > 0)) {
    segments.push(state.current);
  }
  return segments;
}

function command_substitutions_bang(source) {
  const bodies = [];
  const state = {[$$bc$property_key($$bc$keyword("index"))]: 0, [$$bc$property_key($$bc$keyword("single"))]: false, [$$bc$property_key($$bc$keyword("double"))]: false};
  (() => {  while (true) {
    if ((state.index < source.length)) { const index = state.index; const character = source.charAt(index); ((((_truthy) => _truthy !== false && _truthy != null)(((character === "\\") && (!((_truthy) => _truthy !== false && _truthy != null)(state.single))))) ? (state.index = (index + 2)) : (((_truthy) => _truthy !== false && _truthy != null)(((character === "'") && (!((_truthy) => _truthy !== false && _truthy != null)(state.double))))) ? (() => { (state.single = (!((_truthy) => _truthy !== false && _truthy != null)(state.single)));
return (state.index = (index + 1)); })() : (((_truthy) => _truthy !== false && _truthy != null)(((character === "\"") && (!((_truthy) => _truthy !== false && _truthy != null)(state.single))))) ? (() => { (state.double = (!((_truthy) => _truthy !== false && _truthy != null)(state.double)));
return (state.index = (index + 1)); })() : (((_truthy) => _truthy !== false && _truthy != null)(((!((_truthy) => _truthy !== false && _truthy != null)(state.single)) && (source.slice(index, (index + 2)) === "$(")))) ? (() => { const inner = {[$$bc$property_key($$bc$keyword("cursor"))]: (index + 2), [$$bc$property_key($$bc$keyword("depth"))]: 1, [$$bc$property_key($$bc$keyword("single"))]: false, [$$bc$property_key($$bc$keyword("double"))]: false, [$$bc$property_key($$bc$keyword("done"))]: false}; (() => {  while (true) {
    if (((_truthy) => _truthy !== false && _truthy != null)(((inner.cursor < source.length) && (!((_truthy) => _truthy !== false && _truthy != null)(inner.done))))) { const cursor = inner.cursor; const item = source.charAt(cursor); ((((_truthy) => _truthy !== false && _truthy != null)(((item === "\\") && (!((_truthy) => _truthy !== false && _truthy != null)(inner.single))))) ? (inner.cursor = (cursor + 2)) : (((_truthy) => _truthy !== false && _truthy != null)(((item === "'") && (!((_truthy) => _truthy !== false && _truthy != null)(inner.double))))) ? (() => { (inner.single = (!((_truthy) => _truthy !== false && _truthy != null)(inner.single)));
return (inner.cursor = (cursor + 1)); })() : (((_truthy) => _truthy !== false && _truthy != null)(((item === "\"") && (!((_truthy) => _truthy !== false && _truthy != null)(inner.single))))) ? (() => { (inner.double = (!((_truthy) => _truthy !== false && _truthy != null)(inner.double)));
return (inner.cursor = (cursor + 1)); })() : (((_truthy) => _truthy !== false && _truthy != null)(((!((_truthy) => _truthy !== false && _truthy != null)(inner.single)) && ((!((_truthy) => _truthy !== false && _truthy != null)(inner.double)) && (item === "("))))) ? (() => { (inner.depth = (int_value(inner.depth) + 1));
return (inner.cursor = (cursor + 1)); })() : (((_truthy) => _truthy !== false && _truthy != null)(((!((_truthy) => _truthy !== false && _truthy != null)(inner.single)) && ((!((_truthy) => _truthy !== false && _truthy != null)(inner.double)) && (item === ")"))))) ? (() => { (inner.depth = (int_value(inner.depth) - 1));
if ((inner.depth === 0)) {
  bodies.push(source.slice((index + 2), cursor));
  return (inner.done = true);
} else {
  return (inner.cursor = (cursor + 1));
} })() : (inner.cursor = (cursor + 1))); if ((!((_truthy) => _truthy !== false && _truthy != null)(inner.done))) {  continue; } else { return null; } } else { return null; }
  } })();
return (state.index = (int_value(inner.cursor) + 1)); })() : (((_truthy) => _truthy !== false && _truthy != null)(((!((_truthy) => _truthy !== false && _truthy != null)(state.single)) && (character === "`")))) ? (() => { const relative = source.slice((index + 1)).indexOf("`"); const close = ((relative >= 0) ? (index + 1 + relative) : -1); if ((close >= 0)) {
  bodies.push(source.slice((index + 1), close));
  return (state.index = (close + 1));
} else {
  return (state.index = (index + 1));
} })() : (state.index = (index + 1)));  continue; } else { return null; }
  } })();
  return bodies;
}

function skip_redirections_bang(tokens, start) {
  const state = {[$$bc$property_key($$bc$keyword("index"))]: start};
  (() => {  while (true) {
    if ((state.index < tokens.length)) { const token = tokens[state.index]; if (((_truthy) => _truthy !== false && _truthy != null)([">", ">>", "<", "<<", "<<<", "<>"].includes(token))) { (state.index = (int_value(state.index) + 2));  continue; } else if (redirection_p(token)) { (state.index = (int_value(state.index) + (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? _logical : token.endsWith("<")))(token.endsWith(">"))) ? 2 : 1)));  continue; } else { return null; } } else { return null; }
  } })();
  return state.index;
}

function initial_command_bang(segment) {
  const state = {[$$bc$property_key($$bc$keyword("index"))]: 0};
  (() => {  while (true) {
    if (((_truthy) => _truthy !== false && _truthy != null)(((state.index < segment.length) && ["then", "do", "else", "elif", "if", "while", "until", "!", "{"].includes(segment[state.index])))) { (state.index = (int_value(state.index) + 1));  continue; } else { return null; }
  } })();
  if (((_truthy) => _truthy !== false && _truthy != null)(((state.index < segment.length) && ["for", "case", "select", "function"].includes(segment[state.index])))) {
    return null;
  } else {
    (() => {  while (true) {
    if (((_truthy) => _truthy !== false && _truthy != null)(((state.index < segment.length) && assignment_p(segment[state.index])))) { (state.index = (int_value(state.index) + 1));  continue; } else { return null; }
  } })();
    (state.index = skip_redirections_bang(segment, state.index));
    return ((state.index >= segment.length) ? null : {[$$bc$property_key($$bc$keyword("command"))]: segment[state.index], [$$bc$property_key($$bc$keyword("args"))]: segment.slice((int_value(state.index) + 1))});
  }
}

function option_value_p(name, values) {
  return values.includes(name);
}

function drop_options_bang(...$beagle$args) {
  if (arguments.length === 1) {
    const args = $beagle$args[0];
    return drop_options_bang(args, []);
  }
  if (arguments.length === 2) {
    const args = $beagle$args[0];
    const value_options = $beagle$args[1];
    const state = {[$$bc$property_key($$bc$keyword("index"))]: 0, [$$bc$property_key($$bc$keyword("done"))]: false};
    (() => {  while (true) {
    if (((_truthy) => _truthy !== false && _truthy != null)(((state.index < args.length) && (!((_truthy) => _truthy !== false && _truthy != null)(state.done))))) { const token = args[state.index]; const name = token.split("=")[0]; (((token === "--")) ? (() => { (state.index = (int_value(state.index) + 1));
return (state.done = true); })() : (((_truthy) => _truthy !== false && _truthy != null)(((!((_truthy) => _truthy !== false && _truthy != null)(token.startsWith("-"))) || (token === "-")))) ? (state.done = true) : (((_truthy) => _truthy !== false && _truthy != null)((option_value_p(name, value_options) && (!((_truthy) => _truthy !== false && _truthy != null)(token.includes("=")))))) ? (state.index = (int_value(state.index) + 2)) : (() => { const short_values = value_options.filter((value) => regex_p("^-[A-Za-z0-9]$", value)); const cluster = token.slice(1); const found = cluster.split("").findIndex((flag) => short_values.includes($$bc$str("-", flag))); return (((_truthy) => _truthy !== false && _truthy != null)(((!((_truthy) => _truthy !== false && _truthy != null)(token.startsWith("--"))) && (found >= 0))) ? (state.index = (int_value(state.index) + ((cluster.length > (found + 1)) ? 1 : 2))) : (state.index = (int_value(state.index) + 1))); })()); if ((!((_truthy) => _truthy !== false && _truthy != null)(state.done))) {  continue; } else { return null; } } else { return null; }
  } })();
    return args.slice(state.index);
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

function shell_command_strings_bang(name, args) {
  const strings = [];
  const state = {[$$bc$property_key($$bc$keyword("index"))]: 0, [$$bc$property_key($$bc$keyword("done"))]: false};
  (() => {  while (true) {
    if (((_truthy) => _truthy !== false && _truthy != null)(((state.index < args.length) && (!((_truthy) => _truthy !== false && _truthy != null)(state.done))))) { const token = args[state.index]; (((token === "--")) ? (state.done = true) : (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? ((_logical) => (_logical !== false && _logical != null ? (name === "fish") : _logical))(((token.split("=")[0] === "--command") || (token.split("=")[0] === "--init-command"))) : _logical))(token.startsWith("--")))) ? (() => { const parts = token.split("="); const option = parts[0]; if ((parts.length > 1)) {
  strings.push(parts.slice(1).join("="));
  (state.index = (int_value(state.index) + 1));
} else {
  if (((int_value(state.index) + 1) < args.length)) {
    strings.push(args[(int_value(state.index) + 1)]);
  }
  (state.index = (int_value(state.index) + 2));
}
if ((option === "--command")) {
  return (state.done = true);
} })() : (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? ["--init-file", "--rcfile"].includes(token) : _logical))(token.startsWith("--")))) ? (state.index = (int_value(state.index) + 2)) : (((_truthy) => _truthy !== false && _truthy != null)(((!((_truthy) => _truthy !== false && _truthy != null)(token.startsWith("-"))) || (token === "-")))) ? (state.done = true) : (() => { const flags = token.slice(1); const c_index = flags.indexOf("c"); const upper_index = ((name === "fish") ? flags.indexOf("C") : -1); return (((c_index >= 0)) ? (() => { if (((_truthy) => _truthy !== false && _truthy != null)(((name === "fish") && (flags.length > (c_index + 1))))) {
  strings.push(flags.slice((c_index + 1)));
} else {
  if (((int_value(state.index) + 1) < args.length)) {
    strings.push(args[(int_value(state.index) + 1)]);
  }
}
return (state.done = true); })() : ((upper_index >= 0)) ? (() => { if ((flags.length > (upper_index + 1))) {
  strings.push(flags.slice((upper_index + 1)));
} else {
  if (((int_value(state.index) + 1) < args.length)) {
    strings.push(args[(int_value(state.index) + 1)]);
  }
}
return (state.index = (int_value(state.index) + 2)); })() : (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? _logical : flags.includes("O")))(flags.includes("o")))) ? (state.index = (int_value(state.index) + 2)) : (state.index = (int_value(state.index) + 1))); })()); if ((!((_truthy) => _truthy !== false && _truthy != null)(state.done))) {  continue; } else { return null; } } else { return null; }
  } })();
  return strings;
}

function split_static_bang(source) {
  return shell_tokens_bang(source);
}

function expand_env_split_bang(args) {
  const state = {[$$bc$property_key($$bc$keyword("args"))]: args.slice(0), [$$bc$property_key($$bc$keyword("round"))]: 0, [$$bc$property_key($$bc$keyword("changed"))]: true, [$$bc$property_key($$bc$keyword("failed"))]: false};
  (() => {  while (true) {
    if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? ((state.round < 12) && (!((_truthy) => _truthy !== false && _truthy != null)(state.failed))) : _logical))(state.changed))) { (state.changed = false); (() => { const scan = {[$$bc$property_key($$bc$keyword("index"))]: 0, [$$bc$property_key($$bc$keyword("done"))]: false}; return (() => {  while (true) {
    if (((_truthy) => _truthy !== false && _truthy != null)(((scan.index < state.args.length) && (!((_truthy) => _truthy !== false && _truthy != null)(scan.done))))) { const token = state.args[scan.index]; const value = ((((_truthy) => _truthy !== false && _truthy != null)(["-S", "--split-string"].includes(token))) ? (() => { const _x = state.args, _i = (int_value(scan.index) + 1); return _x[_i] != null ? _x[_i] : null; })() : (((_truthy) => _truthy !== false && _truthy != null)(token.startsWith("--split-string="))) ? token.slice("--split-string=".length) : (regex_p("^-[i0v]*S.*$", token)) ? (() => { const suffix = token.replace(new RegExp("^-[i0v]*S", "u"), ""); return ((suffix === "") ? (() => { const _x = state.args, _i = (int_value(scan.index) + 1); return _x[_i] != null ? _x[_i] : null; })() : suffix); })() : null); (((_truthy) => _truthy !== false && _truthy != null)(value) ? (() => { const consumed = (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? _logical : (regex_p("^-[i0v]*S$", token) && ((int_value(scan.index) + 1) < state.args.length))))(["-S", "--split-string"].includes(token))) ? 2 : 1); const prefix = token.replace(new RegExp("^-([i0v]*)S.*$", "u"), "$1"); const words = split_static_bang(value); (state.args = state.args.slice(0, scan.index).concat((((_truthy) => _truthy !== false && _truthy != null)(((!(prefix === "")) && (!((_truthy) => _truthy !== false && _truthy != null)(token.startsWith("--"))))) ? [$$bc$str("-", prefix)] : []), words, state.args.slice((int_value(scan.index) + consumed))));
(state.changed = true);
return (scan.done = true); })() : (((_truthy) => _truthy !== false && _truthy != null)(((token === "--") || ((!((_truthy) => _truthy !== false && _truthy != null)(token.startsWith("-"))) || (token === "-")))) ? (scan.done = true) : (scan.index = (int_value(scan.index) + (((_truthy) => _truthy !== false && _truthy != null)(["-a", "--argv0", "-u", "--unset", "-C", "--chdir"].includes(token)) ? 2 : 1))))); if ((!((_truthy) => _truthy !== false && _truthy != null)(scan.done))) {  continue; } else { return null; } } else { return null; }
  } })(); })(); (state.round = (int_value(state.round) + 1)); if (((_truthy) => _truthy !== false && _truthy != null)(state.changed)) {  continue; } else { return null; } } else { return null; }
  } })();
  return state.args;
}

function unwrap_command_bang(command, args) {
  const state = {[$$bc$property_key($$bc$keyword("command"))]: command, [$$bc$property_key($$bc$keyword("args"))]: args, [$$bc$property_key($$bc$keyword("round"))]: 0, [$$bc$property_key($$bc$keyword("done"))]: false};
  (() => {  while (true) {
    if (((_truthy) => _truthy !== false && _truthy != null)(((state.round < 12) && (!((_truthy) => _truthy !== false && _truthy != null)(state.done))))) { const name = basename_token(state.command); ((((_truthy) => _truthy !== false && _truthy != null)(SIMPLE_WRAPPERS.has(name))) ? (state.args = drop_options_bang(state.args, ((name === "exec") ? ["-a", "--argv0"] : []))) : ((name === "env")) ? (() => { (state.args = expand_env_split_bang(state.args));
(state.args = drop_options_bang(state.args, ["-a", "--argv0", "-u", "--unset", "-C", "--chdir"]));
return (() => {  while (true) {
    if (((_truthy) => _truthy !== false && _truthy != null)(((state.args.length > 0) && assignment_p(state.args[0])))) { (state.args = state.args.slice(1));  continue; } else { return null; }
  } })(); })() : ((name === "sudo")) ? (state.args = drop_options_bang(state.args, ["-C", "--close-from", "-D", "--chdir", "-g", "--group", "-h", "--host", "-p", "--prompt", "-R", "--chroot", "-r", "--role", "-t", "--type", "-u", "--user", "-T", "--command-timeout"])) : ((name === "time")) ? (state.args = drop_options_bang(state.args, ["-f", "--format", "-o", "--output"])) : ((name === "nice")) ? (state.args = drop_options_bang(state.args, ["-n", "--adjustment"])) : ((name === "timeout")) ? (() => { (state.args = drop_options_bang(state.args, ["-k", "--kill-after", "-s", "--signal"]));
if ((state.args.length > 0)) {
  return (state.args = state.args.slice(1));
} })() : ((name === "stdbuf")) ? (state.args = drop_options_bang(state.args, ["-i", "--input", "-o", "--output", "-e", "--error"])) : (((_truthy) => _truthy !== false && _truthy != null)(((name === "direnv") && ((() => { const _x = state.args, _i = 0; return _x[_i] != null ? _x[_i] : null; })() === "exec")))) ? (state.args = state.args.slice(2)) : (state.done = true)); if ((!((_truthy) => _truthy !== false && _truthy != null)(state.done))) { (() => {  while (true) {
    if (((_truthy) => _truthy !== false && _truthy != null)(((state.args.length > 0) && assignment_p(state.args[0])))) { (state.args = state.args.slice(1));  continue; } else { return null; }
  } })(); if ((state.args.length === 0)) { return (state.done = true); } else { (state.command = state.args[0]); (state.args = state.args.slice(1)); (state.round = (int_value(state.round) + 1));  continue; } } else { return null; } } else { return null; }
  } })();
  return ((state.command === "") ? null : state);
}

function first_positional_bang(args, value_options) {
  const state = {[$$bc$property_key($$bc$keyword("index"))]: 0, [$$bc$property_key($$bc$keyword("result"))]: null};
  (() => {  while (true) {
    if (((_truthy) => _truthy !== false && _truthy != null)(((state.index < args.length) && (state.result == null)))) { const token = args[state.index]; const name = token.split("=")[0]; (((token === "--")) ? (() => { (state.index = (int_value(state.index) + 1));
return (state.result = (() => { const _x = args, _i = state.index; return _x[_i] != null ? _x[_i] : null; })()); })() : (((_truthy) => _truthy !== false && _truthy != null)(((!((_truthy) => _truthy !== false && _truthy != null)(token.startsWith("-"))) || (token === "-")))) ? (state.result = token) : (state.index = (int_value(state.index) + (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (!((_truthy) => _truthy !== false && _truthy != null)(token.includes("="))) : _logical))(value_options.includes(name))) ? 2 : 1)))); if ((state.result == null)) {  continue; } else { return null; } } else { return null; }
  } })();
  return state.result;
}

function positional_args_bang(args, value_options) {
  const result = [];
  const state = {[$$bc$property_key($$bc$keyword("index"))]: 0, [$$bc$property_key($$bc$keyword("rest"))]: false};
  (() => {  while (true) {
    if ((state.index < args.length)) { const token = args[state.index]; const name = token.split("=")[0]; ((((_truthy) => _truthy !== false && _truthy != null)(state.rest)) ? (() => { result.push(token);
return (state.index = (int_value(state.index) + 1)); })() : ((token === "--")) ? (() => { (state.rest = true);
return (state.index = (int_value(state.index) + 1)); })() : (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (!(token === "-")) : _logical))(token.startsWith("-")))) ? (state.index = (int_value(state.index) + (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (!((_truthy) => _truthy !== false && _truthy != null)(token.includes("="))) : _logical))(value_options.includes(name))) ? 2 : 1))) : (() => { result.push(token);
return (state.index = (int_value(state.index) + 1)); })());  continue; } else { return null; }
  } })();
  return result;
}

function provider_agent_turn_bang(name, args) {
  if ((name === "codex")) {
    const values = ["-c", "--config", "--enable", "--disable", "--remote", "--remote-auth-token-env", "-i", "--image", "-m", "--model", "--local-provider", "-p", "--profile", "-s", "--sandbox", "-C", "--cd", "--add-dir", "-a", "--ask-for-approval"];
    const verb = first_positional_bang(args, values);
    const positionals = positional_args_bang(args, values);
    const commands = new Set(["apply", "archive", "cloud", "completion", "debug", "delete", "doctor", "features", "help", "login", "logout", "mcp", "plugin", "unarchive", "update"]);
    return ((diagnostic_probe_p(args)) ? null : (((_truthy) => _truthy !== false && _truthy != null)(["app-server", "exec", "e", "exec-server", "fork", "mcp-server", "remote-control", "resume", "review", "sandbox"].includes(verb))) ? (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? args.includes("help") : _logical))(["exec", "e"].includes(verb))) ? null : $$bc$str("codex ", verb)) : (((_truthy) => _truthy !== false && _truthy != null)((((() => { const _x = positionals, _i = 0; return _x[_i] != null ? _x[_i] : null; })() === "cloud") && ((() => { const _x = positionals, _i = 1; return _x[_i] != null ? _x[_i] : null; })() === "exec")))) ? "codex cloud exec" : (((_truthy) => _truthy !== false && _truthy != null)((((() => { const _x = positionals, _i = 0; return _x[_i] != null ? _x[_i] : null; })() === "debug") && ((() => { const _x = positionals, _i = 1; return _x[_i] != null ? _x[_i] : null; })() === "app-server")))) ? "codex debug app-server" : ((verb == null)) ? "codex interactive session" : ((!((_truthy) => _truthy !== false && _truthy != null)(commands.has(verb)))) ? "codex prompt" : null);
  } else {
    if ((!(name === "claude"))) {
      return null;
    } else {
      const values = ["--add-dir", "--agent", "--agents", "--allowedTools", "--allowed-tools", "--append-system-prompt", "--betas", "-d", "--debug", "--debug-file", "--disallowedTools", "--disallowed-tools", "--effort", "--fallback-model", "--file", "--from-pr", "--input-format", "--json-schema", "--max-budget-usd", "--mcp-config", "--model", "-n", "--name", "--output-format", "--permission-mode", "--plugin-dir", "--plugin-url", "--prompt-suggestions", "--remote-control", "--session-id", "--setting-sources", "--settings", "--system-prompt", "--tools", "-w", "--worktree"];
      const positionals = positional_args_bang(args, values);
      const verb = (() => { const _x = positionals, _i = 0; return _x[_i] != null ? _x[_i] : null; })();
      const commands = new Set(["auth", "auto-mode", "doctor", "gateway", "help", "install", "mcp", "plugin", "plugins", "project", "setup-token", "status", "update", "upgrade"]);
      return ((((_truthy) => _truthy !== false && _truthy != null)((diagnostic_probe_p(args) || ["auth", "status", "doctor", "help"].includes(verb)))) ? null : (has_option_p(args, ["-p", "--print"])) ? "claude --print" : (has_option_p(args, ["-c", "--continue"])) ? (((_truthy) => _truthy !== false && _truthy != null)(verb) ? "claude --continue prompt" : "claude --continue session") : (has_option_p(args, ["-r", "--resume"])) ? ((positionals.length >= (((_truthy) => _truthy !== false && _truthy != null)(args.some((token) => ((_logical) => (_logical !== false && _logical != null ? _logical : token.startsWith("--resume=")))(token.startsWith("-r=")))) ? 1 : 2)) ? "claude --resume prompt" : "claude --resume session") : ((verb === "ultrareview")) ? "claude ultrareview" : (((_truthy) => _truthy !== false && _truthy != null)(((verb === "auto-mode") && ((() => { const _x = positionals, _i = 1; return _x[_i] != null ? _x[_i] : null; })() === "critique")))) ? "claude auto-mode critique" : (((_truthy) => _truthy !== false && _truthy != null)(((verb === "mcp") && ((() => { const _x = positionals, _i = 1; return _x[_i] != null ? _x[_i] : null; })() === "serve")))) ? "claude mcp serve" : (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? ((() => { const _x = positionals, _i = 1; return _x[_i] != null ? _x[_i] : null; })() === "eval") : _logical))(["plugin", "plugins"].includes(verb)))) ? "claude plugin eval" : ((verb === "agents")) ? (((_truthy) => _truthy !== false && _truthy != null)(args.includes("--json")) ? null : "claude agents") : ((verb == null)) ? "claude interactive session" : ((!((_truthy) => _truthy !== false && _truthy != null)(commands.has(verb)))) ? "claude prompt" : null);
    }
  }
}

function north_config_mutation(args) {
  if ((!((() => { const _x = args, _i = 0; return _x[_i] != null ? _x[_i] : null; })() === "config"))) {
    return null;
  } else {
    const form = args.slice(1);
    const key = form.join("\u0000");
    const read_only = new Set(["", "status", "help", "-h", "--help", "dispatch", "dispatch\x00--canonical", "dispatch\x00--guard-action", "dispatch\x00--managed-admission", "coord", "beagle", "beagle\x00list", "guards", "hooks", "hooks\x00list", "modules", "modules\x00list", "context", "context\x00show", "skills", "skills\x00list", "comms", "comms\x00show", "comms\x00doctor", "routing", "routing\x00show", "learning", "learning\x00show"]);
    return ((((_truthy) => _truthy !== false && _truthy != null)(read_only.has(key))) ? null : (((_truthy) => _truthy !== false && _truthy != null)(((form.length === 3) && ((form[0] === "hooks") && (form[1] === "explain"))))) ? null : (((() => { const _x = form, _i = 0; return _x[_i] != null ? _x[_i] : null; })() === "agents")) ? (() => { const agent_form = form.slice(1).filter((token) => (!(token === "--json"))); const agent_key = agent_form.join("\u0000"); return (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? _logical : ((agent_form.length === 2) && ["path", "inspect"].includes(agent_form[0]))))(new Set(["", "status", "skills", "skills\x00list", "hooks", "hooks\x00list", "modules", "modules\x00list"]).has(agent_key))) ? null : "north config mutation"); })() : "north config mutation");
  }
}

const RECURSION = {[$$bc$property_key($$bc$keyword("forbidden"))]: null};

function direct_spawn_bang(command, args, cwd) {
  const unwrapped = unwrap_command_bang(command, args);
  if ((unwrapped == null)) {
    return null;
  } else {
    const actual = unwrapped.command;
    const actual_args = unwrapped.args;
    const name = basename_token(actual);
    return ((((_truthy) => _truthy !== false && _truthy != null)(SHELLS.has(name))) ? (() => { const matches = []; shell_command_strings_bang(name, actual_args).forEach((source) => (RECURSION.forbidden)(source, cwd).forEach((match) => matches.push(match)));
if ((matches.length > 0)) {
  return matches;
} else {
  const shell_args = drop_options_bang(actual_args);
  return (((_truthy) => _truthy !== false && _truthy != null)(((shell_args.length > 0) && (basename_token(shell_args[0]) === "north"))) ? direct_spawn_bang(shell_args[0], shell_args.slice(1), cwd) : null);
} })() : (((_truthy) => _truthy !== false && _truthy != null)(["mcp__north__spawn", "mcp__north__dispatch"].includes(name))) ? name : (() => { const provider = provider_agent_turn_bang(name, actual_args); return (((_truthy) => _truthy !== false && _truthy != null)(provider) ? provider : ((((_truthy) => _truthy !== false && _truthy != null)(((name === "north") && north_config_mutation(actual_args)))) ? north_config_mutation(actual_args) : (((_truthy) => _truthy !== false && _truthy != null)(((name === "north") && ["spawn", "delegate", "msg", "goal"].includes((() => { const _x = actual_args, _i = 0; return _x[_i] != null ? _x[_i] : null; })())))) ? (() => { const verb = actual_args[0]; const dry_safe = ((_logical) => (_logical !== false && _logical != null ? actual_args.slice(1).includes("--dry-run") : _logical))(["spawn", "delegate", "msg"].includes(verb)); return (((_truthy) => _truthy !== false && _truthy != null)(((actual_args.length > 1) && (!dry_safe))) ? $$bc$str("north ", verb) : null); })() : (((_truthy) => _truthy !== false && _truthy != null)(["bb", "bun"].includes(name))) ? (() => { const runtime = drop_options_bang(actual_args, ["-cp", "--classpath", "--cwd"]); const runtime_args = (((_truthy) => _truthy !== false && _truthy != null)(((name === "bun") && ((() => { const _x = runtime, _i = 0; return _x[_i] != null ? _x[_i] : null; })() === "run"))) ? runtime.slice(1) : runtime); const entry = text_value((() => { const _x = runtime_args, _i = 0; return _x[_i] != null ? _x[_i] : null; })()); const entry_args = runtime_args.slice(1); const normalized = entry.replace(new RegExp("\\\\", "gu"), "/"); return ((((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? _logical : (normalized === "cli/agents-cli.clj")))(normalized.endsWith("/cli/agents-cli.clj")))) ? (() => { const verb = (() => { const _x = entry_args, _i = 0; return _x[_i] != null ? _x[_i] : null; })(); const dry_safe = ((_logical) => (_logical !== false && _logical != null ? entry_args.slice(1).includes("--dry-run") : _logical))(["spawn", "delegate", "msg"].includes(verb)); return (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? ((entry_args.length > 1) && (!dry_safe)) : _logical))(["spawn", "delegate", "msg", "goal"].includes(verb))) ? $$bc$str("agents-cli.clj ", verb) : null); })() : (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? _logical : (normalized === "cli/config-cli.clj")))(normalized.endsWith("/cli/config-cli.clj")))) ? north_config_mutation(["config"].concat(entry_args)) : (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? _logical : (normalized === "cli/msg-cli.clj")))(normalized.endsWith("/cli/msg-cli.clj")))) ? (((_truthy) => _truthy !== false && _truthy != null)(((entry_args.length > 2) && (entry_args[1] === "send-cmd"))) ? "msg-cli.clj send-cmd" : null) : (((_truthy) => _truthy !== false && _truthy != null)((regex_p("(?:^|/)(?:north/)?sdk/src/spawn[.]ts$", normalized) || ((normalized === "sdk/src/spawn.ts") && regex_p("/north/sdk/?$", cwd))))) ? ((entry_args.length > 0) ? "sdk/src/spawn.ts" : null) : (((_truthy) => _truthy !== false && _truthy != null)((regex_p("(?:^|/)(?:north/)?sdk/src/dispatch[.]ts$", normalized) || ((normalized === "sdk/src/dispatch.ts") && regex_p("/north/sdk/?$", cwd))))) ? ((entry_args.length > 0) ? "sdk/src/dispatch.ts" : null) : null); })() : null)); })());
  }
}

function forbidden_shell_matches_bang(command, cwd) {
  if ((!(typeof command === "string"))) {
    return [];
  } else {
    const matches = [];
    command_substitutions_bang(command).forEach((nested) => forbidden_shell_matches_bang(nested, cwd).forEach((match) => matches.push(match)));
    command_segments_bang(command).forEach((segment) => { const initial = initial_command_bang(segment);
if (((_truthy) => _truthy !== false && _truthy != null)(initial)) {
  const match = direct_spawn_bang(initial.command, initial.args, cwd);
  if (((_truthy) => _truthy !== false && _truthy != null)(match)) {
    return (Array.isArray(match) ? match.forEach((nested) => matches.push(nested)) : matches.push(match));
  }
} });
    return matches;
  }
}

(RECURSION.forbidden = forbidden_shell_matches_bang);

function north_lane_launch_p(match) {
  return ((_logical) => (_logical !== false && _logical != null ? ["mcp__north__spawn", "mcp__north__dispatch", "north spawn", "north delegate", "agents-cli.clj spawn", "agents-cli.clj delegate", "sdk/src/spawn.ts", "sdk/src/dispatch.ts"].includes(match) : _logical))(match);
}

function provider_native_turn_p(match) {
  return ((_logical) => (_logical !== false && _logical != null ? ((_logical) => (_logical !== false && _logical != null ? _logical : match.startsWith("claude ")))(match.startsWith("codex ")) : _logical))(match);
}

function deny_wire(reason) {
  return {[$$bc$property_key($$bc$keyword("hookSpecificOutput"))]: {[$$bc$property_key($$bc$keyword("hookEventName"))]: "PreToolUse", [$$bc$property_key($$bc$keyword("permissionDecision"))]: "deny", [$$bc$property_key($$bc$keyword("permissionDecisionReason"))]: reason}};
}

function command_output(command, args) {
  return (() => { try {
    const result = spawnSync(command, args, {[$$bc$property_key($$bc$keyword("encoding"))]: "utf8", [$$bc$property_key($$bc$keyword("timeout"))]: 1500});
  return ((result.status === 0) ? text_value(result.stdout).trim() : null);
  } catch (_catch_0) {
    switch ($$bd$catch_dispatch(_catch_0, [Error])) {
      case 0: {
        const __ = _catch_0;
        return null;
        break;
      }
    }
  } })();
}

function admitted_models(root) {
  return (() => { try {
    const admitted = new Set();
  const names = readdirSync(root).filter((name) => ((_logical) => (_logical !== false && _logical != null ? (!(name === "catalog.schema.json")) : _logical))(name.endsWith(".json"))).sort();
  names.forEach((name) => { const catalog = JSON.parse(readFileSync(join(root, name), "utf8"));
const models = catalog.models;
if (((_truthy) => _truthy !== false && _truthy != null)(((!(typeof catalog.provider === "string")) || ((!((_truthy) => _truthy !== false && _truthy != null)(models)) || (!(typeof models === "object")))))) {
  (() => { throw new Error("invalid provider catalog"); })();
}
return Object.keys(models).forEach((model) => { if ((model === "")) {
  (() => { throw new Error("invalid model"); })();
}
return admitted.add(model); }); });
  return ((admitted.size > 0) ? admitted : null);
  } catch (_catch_1) {
    switch ($$bd$catch_dispatch(_catch_1, [Error])) {
      case 0: {
        const __ = _catch_1;
        return null;
        break;
      }
    }
  } })();
}

function safe_role_p(role) {
  return regex_p("^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$", role);
}

function routing_for(machinery_root, invoked_role) {
  return ((!safe_role_p(invoked_role)) ? null : (() => { try {
    const source = readFileSync(join(machinery_root, "agents", $$bc$str(invoked_role, ".md")), "utf8");
  const match = source.match(new RegExp("<!--\\s*ORCHESTRATION_ROUTING\\s+(\\{[\\s\\S]*?\\})\\s*-->", "u"));
  const routing = (((_truthy) => _truthy !== false && _truthy != null)(match) ? JSON.parse(match[1]) : null);
  const required = ["role", "taskGrade", "domainRequirements", "topology", "tier", "reasoning", "posture", "composition"];
  const keys = (((_truthy) => _truthy !== false && _truthy != null)(routing) ? Object.keys(routing).sort() : []);
  if (((_truthy) => _truthy !== false && _truthy != null)(((!((_truthy) => _truthy !== false && _truthy != null)(routing)) || ((!(JSON.stringify(keys) === JSON.stringify(required.sort()))) || ((!(routing.role === invoked_role)) || ((routing.role === "researcher") || (!safe_role_p(text_value(routing.role))))))))) {
    (() => { throw new Error("invalid routing"); })();
  }
  return routing;
  } catch (_catch_2) {
    switch ($$bd$catch_dispatch(_catch_2, [Error])) {
      case 0: {
        const __ = _catch_2;
        return null;
        break;
      }
    }
  } })());
}

function north_call(routing) {
  return $$bc$str("mcp__north__spawn ", JSON.stringify(Object.assign({[$$bc$property_key($$bc$keyword("prompt"))]: "<paste the same prompt verbatim>", [$$bc$property_key($$bc$keyword("provider"))]: "auto"}, routing)));
}

function redirect_recipe(tool, tool_input) {
  const subagent = (((_truthy) => _truthy !== false && _truthy != null)(["Agent", "Task"].includes(tool)) ? ((_logical) => (_logical !== false && _logical != null ? _logical : text_value(tool_input.subagentType)))(text_value(tool_input.subagent_type)) : "");
  const lower = subagent.trim().toLowerCase();
  const role = (((_truthy) => _truthy !== false && _truthy != null)(lower.startsWith("orchestration:")) ? (() => { const _x = lower.split(":"), _i = 1; return _x[_i] != null ? _x[_i] : ""; })().trim() : lower);
  const home = ((_logical) => (_logical !== false && _logical != null ? _logical : $$bc$str(text_value(process.env.HOME), "/code/agent-machinery/main")))(text_value(process.env.AGENT_MACHINERY_HOME));
  const routing = ((role === "") ? null : routing_for(home, role));
  return (((_truthy) => _truthy !== false && _truthy != null)(routing) ? $$bc$str("Native ", tool, " (", subagent, ") is ephemeral — no claim trail, no steering, no observability. ", "Re-issue the SAME work on north; dials are read from the canonical ", role, " template metadata — just paste your prompt in:\n  ", north_call(routing), "\nFan-out? fire one mcp__north__spawn per lane in the same turn. ", "Observe: north watch/agents/board. Deliberate provider-native pin: north config dispatch native.") : $$bc$str("Native ", tool, " (", ((_logical) => (_logical !== false && _logical != null ? _logical : tool))(subagent), ") is ephemeral — no claim trail, no steering, no observability. ", "Do the SAME work on north:\n  1. Trivial lookup / single file? No agent at all — bash/grep/read inline.\n", "  2. One job: inspect north templates, select an Orchestration template, then use its generated full ", "eight-field routing request with mcp__north__spawn; the CLI forcing form is north spawn <template-id> <prompt>. ", "Override only task grade, domains, tier, reasoning, or posture, with a reason, while its responsibility, ", "deliverable, done criteria, report shape, and fixed topology/capability boundary still match. Any topology/authority ", "change requires a complete bespoke contract.\n  3. Fan-out: N x mcp__north__spawn in parallel.\n", "Provider resolution and concrete model selection belong to North. Pin provider-native execution deliberately ", "with north config dispatch native (or /north-config)."));
}

function decide_bang(envelope, action, admission, north_home) {
  const tool = text_value(envelope.tool_name);
  const input = ((_logical) => (_logical !== false && _logical != null ? _logical : {}))(envelope.tool_input);
  if (((_truthy) => _truthy !== false && _truthy != null)(["Agent", "Task", "Workflow"].includes(tool))) {
    const models = admitted_models($$bc$str(north_home, "/agent-runtime/orchestration/providers"));
    const model = input.model;
    return (((models == null)) ? deny_wire($$bc$str("DENIED by concrete model identity policy: the authoritative provider model catalog is unavailable, ", "so this native agent dispatch cannot prove an exact concrete model identity.")) : (((_truthy) => _truthy !== false && _truthy != null)(((!(typeof model === "string")) || (!((_truthy) => _truthy !== false && _truthy != null)(models.has(model)))))) ? deny_wire($$bc$str("DENIED by concrete model identity policy: every native agent dispatch must explicitly name an exact ", "current model from the provider catalog; omitted, aliased, or placeholder selection is not an identity. ", "Recover the current concrete model from runtime evidence and re-issue the same dispatch with that exact model; never guess.")) : ((action === "allow")) ? null : deny_wire($$bc$str("DENIED by north config dispatch action (", action, "). ", redirect_recipe(tool, input))));
  } else {
    if (((_truthy) => _truthy !== false && _truthy != null)(["Bash", "shell", "exec_command"].includes(tool))) {
      const matches = forbidden_shell_matches_bang(input.command, text_value(envelope.cwd));
      return (((matches.length === 0)) ? null : ((((_logical) => (_logical !== false && _logical != null ? _logical : ""))(process.env.AGENT_TOPOLOGY).trim().toLowerCase() === "worker")) ? (() => { const match = matches[0]; return deny_wire($$bc$str("DENIED by Orchestration worker topology: worker lanes cannot spawn, delegate, dispatch, or command agents (matched ", match, "). Return the subtask, steering request, or escalation to the orchestrator; only an orchestrator owns fan-out and peer control.")); })() : (((_truthy) => _truthy !== false && _truthy != null)(((admission === "deny") && matches.some(north_lane_launch_p)))) ? (() => { const match = matches.find(north_lane_launch_p); return deny_wire($$bc$str("DENIED by north config dispatch: native pins the provider-native surface and does not admit North lane creation (matched ", match, "). Re-issue the same work through the provider-native Agent/Workflow surface; North remains available for coordination.")); })() : (((_truthy) => _truthy !== false && _truthy != null)(((action === "deny") && matches.some(provider_native_turn_p)))) ? (() => { const match = matches.find(provider_native_turn_p); return deny_wire($$bc$str("DENIED by north config dispatch: managed pins the North-managed surface and does not admit provider-native agent turns (matched ", match, "). Re-issue the same work through north spawn or mcp__north__spawn.")); })() : null);
    } else {
      return null;
    }
  }
}

function stdin_text_bang() {
  return Bun.stdin.text();
}

async function main_bang(__args) {
  const raw = await stdin_text_bang();
  return ((raw.length > 1048576) ? 0 : (() => { try {
    const envelope = JSON.parse(raw);
  const north_home = north_home_bang(process.env);
  const north_bin = $$bc$str(north_home, "/bin/north");
  const action = command_output(north_bin, ["config", "dispatch", "--guard-action"]);
  const admission = command_output(north_bin, ["config", "dispatch", "--managed-admission"]);
  if (((_truthy) => _truthy !== false && _truthy != null)(((!((_truthy) => _truthy !== false && _truthy != null)(["deny", "allow"].includes(action))) || (!((_truthy) => _truthy !== false && _truthy != null)(["deny", "allow"].includes(admission)))))) {
    return 0;
  } else {
    const decision = decide_bang(envelope, action, admission, north_home);
    if (((_truthy) => _truthy !== false && _truthy != null)(decision)) {
      process.stdout.write($$bc$str(JSON.stringify(decision), "\n"));
    }
    return 0;
  }
  } catch (_catch_3) {
    switch ($$bd$catch_dispatch(_catch_3, [Error])) {
      case 0: {
        const __ = _catch_3;
        return 0;
        break;
      }
    }
  } })());
}

if (((_truthy) => _truthy !== false && _truthy != null)(import.meta.main)) {
  main_bang([]).then((code) => (process.exitCode = code));
}
