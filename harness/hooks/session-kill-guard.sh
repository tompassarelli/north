#!/usr/bin/env bash
# PreToolUse guard — refuses session-killing signal shapes and unmanaged
# background-child launch shapes: broadcast kill (`kill -1`), user-wide process
# sweeps (`pkill -u` with no pattern, `killall -u`), compositor kills
# (pkill/killall niri), login-session teardown (`loginctl terminate-*`/`kill-*`,
# `systemctl --user exit`, stop/kill/restart of `user@*` or the compositor
# unit), and detached agent child processes (`nohup`, `setsid`, `disown`, bare
# background `&`, or direct Bun/Node /tmp scripts outside a session scratchpad).
# ============================================================================
# kill(-1, SIG) signals EVERY process the user owns — the compositor, the
# user manager, the login shell, and every other agent — in one syscall. The
# scoped alternatives (a specific PID, a unique -f pattern, a named unit the
# agent itself started) stay allowed. A background child instead uses
# `run-bounded <duration> -- <command>`: its 24h maximum owns a transient
# cgroup plus PID namespace, reaps every descendant, and gives all background
# jobs a shared 48G hard ceiling. Quoted mentions in commit messages and
# heredoc bodies stay allowed: only a command-position invocation is denied.
#
# Kill-switch: persistent `north config agents off session-kill-guard` OR env
# AGENT_NO_AUTHORING_HOOKS (any value but
# 0/false; 0/false forces guards live). Shared impl: lib/authoring-killswitch.sh.
# ============================================================================
set -uo pipefail

# Drain before every decision, including the kill-switch. Keep active-path input
# memory-bounded; an oversized envelope follows the existing malformed fail-open.
capture_hook_stdin() {
  local chunk status keep
  local LC_ALL=C
  payload=""
  payload_oversized=0
  while :; do
    chunk=""
    IFS= read -r -N 65536 chunk
    status=$?
    if [ -n "$chunk" ]; then
      keep=$((1048576 - ${#payload}))
      [ "$keep" -le 0 ] || payload+="${chunk:0:$keep}"
      [ "${#chunk}" -le "$keep" ] || payload_oversized=1
    fi
    [ "$status" -eq 0 ] || break
  done
}
capture_hook_stdin

[ "$payload_oversized" -eq 0 ] || exit 0

case "$payload" in
  *kill*|*loginctl*|*systemctl*|*nohup*|*setsid*|*disown*|*run-bounded*|\
  *bun*|*node*|*/tmp/*|*'&'*|*.claude*|*cc-socks*|*RUNTIME_DIR*|*CONFIG_DIR*|\
  *ross-session*|*rom-mode*|*SendMessage*|*Grep*|*Glob*|*sessions*|\
  *rg*|*grep*|*find*|*tar*|*cp*|*rsync*|*zip*|*du*|*ln*|*"${CLAUDE_CONFIG_DIR:-$HOME/.claude}"*) ;;
  *) exit 0 ;;
esac

authoring_killswitch="$(dirname "$0")/lib/authoring-killswitch.sh"
[ -r "$authoring_killswitch" ] \
  || authoring_killswitch="$(dirname "$0")/../lib/authoring-killswitch.sh"
# shellcheck disable=SC1090,SC1091
. "$authoring_killswitch" 2>/dev/null || true
type hook_decide >/dev/null 2>&1 || exit 0
type authoring_guards_off >/dev/null 2>&1 && authoring_guards_off && exit 0
north_agent_unit_active_json hook session-kill-guard || exit 0

read -r -d '' PY <<'PYEOF' || true
import sys
sys.path.insert(0, sys.argv[1])
from shellcmd import strip_heredocs, strip_quotes, command as lifecycle_command_index
import sys, json, re, shlex, os, ast, html, unicodedata
from pathlib import Path
from shellcmd import words, segments, command_index
import shellcmd

def allow():
    sys.exit(0)

try:
    data = json.load(sys.stdin)
except Exception:
    sys.exit(65)

if not isinstance(data, dict):
    allow()
if data.get("hook_event_name", "PreToolUse") != "PreToolUse":
    allow()

tool = data.get("tool_name", "")
inputs = data.get("tool_input") or {}
if isinstance(inputs, str):
    try:
        inputs = json.loads(inputs)
    except ValueError:
        allow()
if not isinstance(inputs, dict):
    allow()

KEY_REFUSAL = "Worker session-key reads are refused; ask your lead to inspect session metadata or use agents org show for peer addresses."
ENVELOPE_REFUSAL = "Cross-session envelope markup is refused in message bodies; send plain text with SendMessage without cross-session-message tags or from-mode attributes."
SOCKET_REFUSAL = "Worker direct peer-socket writes are refused; use SendMessage for plain-text messages or report to your lead through your status file."


def refuse(reason):
    print(json.dumps({"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "deny", "permissionDecisionReason": reason}}))
    allow()


if tool == "SendMessage":
    body = inputs.get("message")
    if isinstance(body, str):
        body = "".join(c for c in unicodedata.normalize("NFKC", html.unescape(body)) if unicodedata.category(c) != "Cf").lower()
    if isinstance(body, str) and re.search(r"<\s*/?\s*cross-session-message|from-mode\s*=", body):
        refuse(ENVELOPE_REFUSAL)
    allow()


def worker():
    kind = data.get("agent_type")
    if isinstance(kind, str) and kind.startswith("worker"):
        return True
    if data.get("agent_id"):
        return True
    try:
        org = Path(os.environ.get("AGENTS_ORG_FILE") or Path.home() / ".local/state/agents/org.json")
        nodes = json.loads(org.read_text()).get("nodes", [])
        matches = [n for n in nodes if n.get("session") == data.get("session_id") and data.get("session_id")]
        if len(matches) == 1:
            return matches[0].get("role") not in {"lead", "sub-lead", "proxy"}
    except (OSError, ValueError, TypeError, AttributeError):
        pass
    role = os.environ.get("AGENT_ROLE")
    if role in {"lead", "sub-lead", "proxy"}:
        return False
    if role == "worker":
        return True
    return os.environ.get("CLAUDE_CODE_ENTRYPOINT") != "cli"


home = str(Path.home())
cwd = inputs.get("workdir") or data.get("cwd") or os.getcwd()
variables = {"HOME": home, "CLAUDE_CONFIG_DIR": os.environ.get("CLAUDE_CONFIG_DIR", home + "/.claude"), "XDG_RUNTIME_DIR": os.environ.get("XDG_RUNTIME_DIR", "/run/user/" + str(os.getuid()))}
roots = {os.path.realpath(home + "/.claude/sessions"), os.path.realpath(variables["CLAUDE_CONFIG_DIR"] + "/sessions")}


def located(raw, here, env):
    if not isinstance(raw, str):
        return None
    for _ in range(6):
        expanded = re.sub(r"\$\{([A-Za-z_][A-Za-z0-9_]*)\}|\$([A-Za-z_][A-Za-z0-9_]*)", lambda m: env.get(m.group(1) or m.group(2), m.group(0)), raw)
        if expanded == raw:
            break
        raw = expanded
    if re.search(r"[$`*?{}]", raw):
        return None
    raw = home + raw[1:] if raw == "~" or raw.startswith("~/") else raw
    return os.path.realpath(raw if os.path.isabs(raw) else os.path.join(here, raw))


def protected_tail(raw, names):
    return bool(re.match(r"^(?:\$\(|`|\$\{)", raw) and any(re.search(r"/" + re.escape(name) + r"(?:/|$)", raw) for name in names))


def key_path(raw, here, env, ancestor=False):
    if not isinstance(raw, str):
        return False
    for _ in range(6):
        expanded = re.sub(r"\$\{([A-Za-z_][A-Za-z0-9_]*)\}|\$([A-Za-z_][A-Za-z0-9_]*)", lambda m: env.get(m.group(1) or m.group(2), m.group(0)), raw)
        if expanded == raw:
            break
        raw = expanded
    if protected_tail(raw, {".claude/sessions", os.path.basename(variables["CLAUDE_CONFIG_DIR"]) + "/sessions"}):
        return True
    glob = re.search(r"[*?\[{]", raw)
    path = located(raw[:glob.start()] if glob else raw, here, env)
    return path is not None and any(path == root or path.startswith(root + "/") or (glob and path.startswith(root)) or (ancestor and (root.startswith(path + "/") or (glob and root.startswith(path)))) for root in roots)


def socket_path(raw, here, env):
    if isinstance(raw, str) and protected_tail(raw, {"cc-socks"}):
        return True
    path = located(raw, here, env)
    runtime = os.path.realpath(variables["XDG_RUNTIME_DIR"] + "/cc-socks")
    return path is not None and (bool(re.match(r"^/run/user/[0-9]+/cc-socks(?:/|$)", path)) or path == runtime or path.startswith(runtime + "/"))


def expression(node, values):
    if isinstance(node, ast.Constant) and isinstance(node.value, str):
        return node.value
    if isinstance(node, ast.Name):
        return values.get(node.id)
    if isinstance(node, ast.Subscript) and ast.unparse(node.value) == "os.environ":
        return variables.get(expression(node.slice, values))
    if isinstance(node, ast.BinOp) and isinstance(node.op, (ast.Add, ast.Div)):
        left, right = expression(node.left, values), expression(node.right, values)
        if left is not None and right is not None:
            return left + right if isinstance(node.op, ast.Add) else os.path.join(left, right)
    if isinstance(node, ast.Call):
        name = ast.unparse(node.func)
        args = [expression(a, values) for a in node.args]
        if name in {"os.getenv", "os.environ.get"} and args:
            return variables.get(args[0], args[1] if len(args) > 1 else None)
        if name in {"Path.home", "pathlib.Path.home", "os.path.expanduser"} and (not args or args == ["~"]):
            return home
        if name in {"Path", "pathlib.Path", "os.path.join"} and args and all(a is not None for a in args):
            return os.path.join(*args)
        if name == "os.path.expanduser" and len(args) == 1 and args[0] is not None:
            return home + args[0][1:] if args[0].startswith("~/") else args[0]
    return None


def python_hit(script, here, env):
    try:
        tree = ast.parse(script)
    except (SyntaxError, ValueError):
        return None
    values = {}
    calls = []
    for node in ast.walk(tree):
        if isinstance(node, ast.Assign):
            value = expression(node.value, values)
            for target in node.targets:
                if isinstance(target, ast.Name):
                    values[target.id] = value
        if isinstance(node, ast.Call):
            calls.append(node)
            name = ast.unparse(node.func)
            target = expression(node.args[0], values) if node.args else None
            if name in {"open", "io.open"}:
                reading = len(node.args) < 2 or expression(node.args[1], values) not in {"w", "wb", "a", "ab", "x", "xb"}
                if reading and key_path(target, here, env):
                    return KEY_REFUSAL
            if isinstance(node.func, ast.Attribute) and node.func.attr in {"read_text", "read_bytes", "open"} and key_path(expression(node.func.value, values), here, env):
                return KEY_REFUSAL
    if any(isinstance(n.func, ast.Attribute) and n.func.attr in {"send", "sendall", "write"} for n in calls):
        for node in calls:
            if isinstance(node.func, ast.Attribute) and node.func.attr in {"connect", "connect_ex"} and node.args and socket_path(expression(node.args[0], values), here, env):
                return SOCKET_REFUSAL
    return None


def shell_hit(text, here, env, depth=0):
    if depth > 6:
        return None
    live = re.sub(r"'(?:[^']*)'|\"(?:\\.|[^\"\\])*\"", lambda m: "" if m.group(0).startswith("'") else m.group(0), strip_heredocs(text))
    for match in re.finditer(r"\$\((.*?)\)|`([^`]*)`", live, re.S):
        hit = shell_hit(match.group(1) if match.group(1) is not None else match.group(2), here, dict(env), depth + 1)
        if hit:
            return hit
    tokens = words(text)
    if tokens is None:
        return None
    for segment in segments(tokens):
        for word in segment:
            if re.match(r"^[A-Za-z_][A-Za-z0-9_]*=", word):
                name, value = word.split("=", 1)
                env[name] = value
        index = command_index(segment)
        if index >= len(segment):
            continue
        executable = os.path.basename(segment[index])
        args = segment[index + 1:]
        targets = [re.sub(r"^\d*(?:<<<|<<|<>|>>|<|>)+", "", word) for word in segment]
        targets += [word.split("=", 1)[1] for word in segment if "=" in word]
        recursive = executable in {"rg", "find", "tar", "rsync", "zip", "ln"} or (executable in {"grep", "cp", "du"} and any(a == "--recursive" or (a.startswith("-") and not a.startswith("--") and any(c in a[1:] for c in ({"r", "R"} if executable == "grep" else {"r", "R", "a", "s"} if executable == "cp" else {"a"}))) for a in args))
        reader = executable in {"cat", "head", "tail", "sed", "awk", "xxd", "od", "perl", "jq", "bat", "vim", "less", "more", "wc", "read", "grep"}
        if any(key_path(word, here, env, recursive or (reader and bool(re.search(r"[*?\[{]", word)))) for word in targets):
            return KEY_REFUSAL
        if executable == "cd" and args:
            here = located(args[0], here, env) or here
        elif executable in {"bash", "sh", "zsh", "dash"} and any(a in {"-c", "-lc", "-ec"} for a in args):
            flag = next(i for i, a in enumerate(args) if a in {"-c", "-lc", "-ec"})
            if flag + 1 < len(args):
                hit = shell_hit(args[flag + 1], here, dict(env), depth + 1)
                if hit:
                    return hit
        elif executable in {"python", "python3"}:
            script = args[args.index("-c") + 1] if "-c" in args and args.index("-c") + 1 < len(args) else None
            if script is None and "-" in args:
                match = shellcmd._HEREDOC_BODY.search(text)
                script = match.group(3) if match else None
            if script:
                hit = python_hit(script, here, env)
                if hit:
                    return hit
        elif executable in {"node", "nodejs", "bun"}:
            flag = next((i for i, a in enumerate(args) if a in {"-e", "--eval", "-p", "--print"}), None)
            script = args[flag + 1] if flag is not None and flag + 1 < len(args) else ""
            reads = re.findall(r"\b(?:readFileSync|readFile|createReadStream|copyFileSync|copyFile)\s*\(\s*([\"'])(.*?)\1", script)
            if any(key_path(a, here, env) for _, a in reads):
                return KEY_REFUSAL
            connections = re.findall(r"\b(?:connect|createConnection)\s*\(\s*(?:\{\s*path\s*:\s*)?([\"'])(.*?)\1", script)
            if re.search(r"\b(?:write|end)\s*\(", script) and any(socket_path(a, here, env) for _, a in connections):
                return SOCKET_REFUSAL
        elif executable in {"socat", "nc", "ncat", "netcat"}:
            if "-z" not in args and any(socket_path(re.sub(r"^UNIX(?:-CONNECT|-CLIENT)?:", "", a), here, env) for a in args):
                return SOCKET_REFUSAL
    return None


if tool in {"Read", "Grep", "Glob"}:
    target = inputs.get("file_path") or inputs.get("filePath") if tool == "Read" else inputs.get("path")
    if worker() and (key_path(target, cwd, variables, tool != "Read") or (tool == "Glob" and key_path(inputs.get("pattern"), target or cwd, variables, True))):
        refuse(KEY_REFUSAL)
    allow()

if tool not in {"Bash", "exec_command", "functions.exec_command"}:
    allow()
cmd = inputs.get("command") or inputs.get("cmd") or ""
if not isinstance(cmd, str) or not cmd:
    allow()
if worker():
    refusal = shell_hit(cmd, cwd, dict(variables))
    if refusal:
        refuse(refusal)

# --- Strip heredoc bodies, then quoted segments, so prose that merely
# mentions a trigger phrase is never treated as an invocation. Blanking (not
# deleting) preserves newline structure for the command-position anchors. ---

cleaned = strip_quotes(strip_heredocs(cmd))

SEP = r"(?:^|[\n;&|({`])"
WRAP = r"(?:(?:sudo|doas|command|exec|nohup|setsid)\s+)*"
ARG_SPAN = r"[^\n;&|)}`]*"

CMD_RE = re.compile(
    SEP + r"\s*" + WRAP + r"(kill|pkill|killall|loginctl|systemctl)\s+("
    + ARG_SPAN + r")")

# The compositor and its session units: killing these IS killing the session.
SESSION_PROCS = {"niri", "niri-session"}
SESSION_UNITS = {"niri", "niri.service", "graphical-session.target"}

SIG_FLAG = re.compile(r"^-(?:[0-9]+|[A-Za-z]+)$")

def kill_hit(tokens):
    # Separate signal-spec tokens from PID operands; the broadcast shape is
    # `-1` in OPERAND position (`kill -9 -1`, `kill -- -1`). `kill -1 <pid>`
    # is SIGHUP to a pid and stays allowed.
    operands = []
    i, n = 0, len(tokens)
    seen_ddash = False
    while i < n:
        t = tokens[i]
        if not seen_ddash:
            if t == "--":
                seen_ddash = True
                i += 1
                continue
            if t in ("-l", "-L"):
                return None
            if t in ("-s", "-n", "--signal") and i + 1 < n:
                i += 2
                continue
            if i == 0 and SIG_FLAG.match(t) and (t != "-1" or n > 1):
                i += 1
                continue
        operands.append(t)
        i += 1
    if "-1" in operands:
        return "broadcast kill -1"
    return None

VALUE_FLAGS = {"-u", "-U", "--uid", "--euid", "-g", "-G", "--group",
               "-s", "--signal", "-P", "--parent", "-t", "--terminal",
               "-F", "--pidfile", "-d", "--delay", "-o", "-n"}

def pkill_hit(tokens):
    has_user = False
    operands = []
    skip = False
    for t in tokens:
        if skip:
            skip = False
            continue
        if t in ("-u", "-U") or t.startswith(("--uid", "--euid")):
            has_user = True
            if t in VALUE_FLAGS:
                skip = True
            continue
        if t in VALUE_FLAGS:
            skip = True
            continue
        if t.startswith("-"):
            continue
        operands.append(t)
    if has_user and not operands:
        return "user-wide pkill sweep"
    if any(o in SESSION_PROCS for o in operands):
        return "compositor kill"
    return None

def killall_hit(tokens):
    if any(t in ("-u", "--user") for t in tokens):
        return "user-wide killall sweep"
    if any(t in SESSION_PROCS for t in tokens):
        return "compositor kill"
    return None

LOGINCTL_VERBS = {"terminate-user", "kill-user", "terminate-session",
                  "kill-session", "terminate-seat"}

def loginctl_hit(tokens):
    for t in tokens:
        if t.startswith("-"):
            continue
        if t in LOGINCTL_VERBS:
            return "login-session teardown"
        return None
    return None

def systemctl_hit(tokens):
    user_mode = "--user" in tokens
    verb = None
    args = []
    for t in tokens:
        if t.startswith("-"):
            continue
        if verb is None:
            verb = t
            continue
        args.append(t)
    if verb is None:
        return None
    if user_mode and verb == "exit":
        return "user-manager exit"
    if verb == "isolate" and any(a.startswith("exit") for a in args):
        return "user-manager exit"
    if verb in ("stop", "kill", "restart"):
        if any(a.startswith("user@") for a in args):
            return "user-manager teardown"
        if user_mode and any(a in SESSION_UNITS for a in args):
            return "compositor teardown"
    return None

# The same command-position boundary covers process detachment. This is a
# deliberately small shell pass: it recognizes command lists, not arbitrary
# runtime-built shell source. Heredoc bodies are blanked; quoted prose remains
# an argument to its leading command and cannot become an invocation.
CONTROL = {";", "&&", "||", "|", "|&", "&", "\n", "(", ")"}
ASSIGNMENT = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*=.*$", re.S)
REDIRECTION = re.compile(r"^\d*(?:<>|>>|<<|>|<).*$", re.S)
WRAPPERS = {"command", "exec", "env", "sudo", "doas"}

def command_base(token):
    return token.rstrip("/").rsplit("/", 1)[-1]

def command_word(segment):
    index = 0
    while index < len(segment) and (ASSIGNMENT.fullmatch(segment[index]) or REDIRECTION.fullmatch(segment[index])):
        index += 1
    index += lifecycle_command_index(segment[index:], WRAPPERS)
    if index < len(segment):
        return command_base(segment[index]), segment[index + 1:]
    return None, []

# A session scratchpad script run in the foreground is owned by the agent's
# shell like any project script; backgrounding or detaching it is still caught.
SCRATCHPAD = re.compile(r"/tmp/claude-\d+/[^/\s]+/[^/\s]+/scratchpad/[^\s]+")

def temporary_script(args):
    return any(re.fullmatch(r"/tmp/[^\s]*\.(?:js|mjs|cjs|ts)", arg)
               and not (SCRATCHPAD.fullmatch(arg) and "/../" not in arg)
               for arg in args)

def background_shape(segment, background=False):
    word, args = command_word(segment)
    if word in {"nohup", "setsid", "disown"}:
        return word
    if word in {"bun", "node"} and temporary_script(args):
        return "direct %s /tmp script" % word
    if background and word != "run-bounded":
        return "unmanaged background job"
    return None

def unmanaged_background_hit(command):
    try:
        lexer = shlex.shlex(strip_heredocs(command), posix=True, punctuation_chars=";&|()\n")
        lexer.whitespace_split = True
        lexer.commenters = ""
        lexer.whitespace = " \t\r"
        tokens = list(lexer)
    except (TypeError, ValueError):
        return None
    segment = []
    for index, token in enumerate(tokens):
        # shlex splits fd redirects such as 2>&1 and &>file; those ampersands
        # are redirections, not background operators.
        redirect_amp = token == "&" and ((segment and segment[-1].endswith(">"))
            or (index + 1 < len(tokens) and tokens[index + 1].startswith(">")))
        if token == "&" and not redirect_amp:
            hit = background_shape(segment, background=True)
            if hit:
                return hit
            segment = []
        elif token in CONTROL:
            hit = background_shape(segment)
            if hit:
                return hit
            segment = []
        else:
            segment.append(token)
    return background_shape(segment)

hit = None
for m in CMD_RE.finditer(cleaned):
    name = m.group(1)
    tokens = m.group(2).split()
    if name == "kill":
        hit = kill_hit(tokens)
    elif name == "pkill":
        hit = pkill_hit(tokens)
    elif name == "killall":
        hit = killall_hit(tokens)
    elif name == "loginctl":
        hit = loginctl_hit(tokens)
    elif name == "systemctl":
        hit = systemctl_hit(tokens)
    if hit:
        break

background_hit = unmanaged_background_hit(cmd)

if hit is None and background_hit is None:
    allow()

if hit is not None:
    reason = (
        "BLOCKED: session-killing shape refused (%s) — this signals the whole "
        "desktop session (compositor, user manager, every agent), not just your "
        "target. Signal only processes YOU started, by specific PID (`kill <pid>`)"
        " or a unique pattern (`pkill -f '<unique-pattern>'`); manage your own "
        "units by exact name. Session/compositor teardown is the operator's call. "
        "Prose that mentions these phrases is unaffected — only a command-position "
        "invocation is denied. Rare explicit override: "
        "`north config agents off session-kill-guard` "
        "(persistent, live), or a session LAUNCHED with AGENT_NO_AUTHORING_HOOKS=1."
    ) % hit
else:
    reason = (
        "BLOCKED: %s would detach an agent child from its owner. Use "
        "`run-bounded <duration> -- <command>`; the wrapper has a 24h maximum, "
        "owns a transient cgroup plus PID namespace, reaps every descendant when "
        "the owner ends, and gives all background jobs a shared 48G hard ceiling."
    ) % background_hit

print(json.dumps({
    "hookSpecificOutput": {
        "hookEventName": "PreToolUse",
        "permissionDecision": "deny",
        "permissionDecisionReason": reason,
    }
}))
sys.exit(0)
PYEOF

hook_decide "${NORTH_AGENT_PYTHON:-python3}" -c "$PY" "$(dirname "$(readlink -f "$0")")/lib"
