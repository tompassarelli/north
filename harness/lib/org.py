import fcntl
import hashlib
import json
import os
import shlex
import signal
import sqlite3
import subprocess
import sys
import time
import tomllib
import uuid
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from laptop_line import laptop_line
VERSION = 1
BUDGETS = {"proxy": 3, "lead": 2, "sub-lead": 1, "worker": 0}
STATES = ("active", "finished")
PROXY = {"id": "proxy", "role": "proxy", "domain": "all", "provider": "claude",
         "session": "Tom's top-level claude", "depth": 0, "budget": BUDGETS["proxy"], "state": "active"}
INHERITED_SESSION = ("CLAUDE_CODE_SESSION_ID", "CLAUDE_JOB_DIR", "CLAUDE_CODE_SESSION_ATTENDED", "CODEX_THREAD_ID")
CLAUDE_LEAD = ["--model", "claude-opus-5-5", "--effort", "high", "--permission-mode", "bypassPermissions",
               "--settings", '{"crossSessionInbound": "accept"}']
FIELDS = {"role", "domain", "parent", "provider", "account", "session", "brief", "status", "budget", "depth", "pid", "state"}
USAGE = """usage: agents org [show]
       agents org add NAME --role ROLE --domain DOMAIN [--parent NAME] [--provider claude|codex]
                      [--session ID] [--brief FILE] [--status FILE] [--budget N] [--depth N] [--pid N]
       agents org set NAME [--parent NAME] [--role ROLE] [--state active|finished] [--session ID] [--status FILE]
                      [--budget N] [--pid N]
       agents org set proxy [--provider claude|codex] [--session ID] [--status FILE] [--pid N]
       agents org start proxy --brief FILE [--account LABEL] [--cwd DIR] [--timeout SECONDS] [-- CLAUDE ARGS]
       agents org finish NAME     mark the workstream finished, close its session and deregister it
       agents org exit NAME [--pid N]  launcher exit hook: remove a finished node, flag an active one as died
       agents org remove NAME     remove the node and its subtree
       agents org priority         print Tom's project priority order from orchestration.toml
       agents lead start --provider claude|codex --domain DOMAIN --brief FILE [--name NAME] [--account LABEL]
                      [--role lead|sub-lead] [--parent NAME] [--budget N] [--cwd DIR] [-- PROVIDER ARGS]
       agents lead restart NAME [--packet FILE]   resume a dead lead, or recycle a settled lead from its packet
       agents attach NAME         attach to a headless lead; closing the terminal detaches
The chain of command lives in $AGENTS_ORG_FILE (default ~/.local/state/agents/org.json,
schema north:harness/org.schema.json); the proxy,
Tom's top-level session, is always the root. A delegation budget limits how many levels of
delegation may follow below a node, not how many workers it runs; the spawn gate and ready work
bound the count. Default budgets: proxy 3, lead 2, sub-lead 1, worker 0. lead start registers the node, opens the session without taking focus with
AGENT_ROLE, AGENT_DEPTH, AGENT_DELEGATION_BUDGET and AGENT_ORG_NAME set, and runs `org exit` when
the session ends."""


def operator_dir():
    configured = os.environ.get("AGENTS_OPERATOR_DIR")
    if configured:
        return Path(configured)
    return Path.home() / "code/south/main/agents"


def die(msg, code=2):
    print(f"agents org: {msg}", file=sys.stderr)
    sys.exit(code)


def org_file():
    return os.environ.get("AGENTS_ORG_FILE") or os.path.join(
        os.environ.get("XDG_STATE_HOME") or os.path.expanduser("~/.local/state"), "agents", "org.json")


def priority():
    path = os.environ.get("AGENTS_ORCHESTRATION") or operator_dir() / "orchestration.toml"
    try:
        with open(path, "rb") as f:
            projects = tomllib.load(f).get("projects", {})
        order = sorted(projects, key=lambda name: projects[name].get("priority", float("inf")))
    except (OSError, ValueError, TypeError, AttributeError) as e:
        die(f"cannot read project priority from {path}: {e}")
    return "priority: " + (" > ".join(order) or "unset ([projects] in orchestration.toml)")


class Org:
    def __enter__(self):
        path = org_file()
        os.makedirs(os.path.dirname(path), exist_ok=True)
        self.lock = open(path + ".lock", "w")
        fcntl.flock(self.lock, fcntl.LOCK_EX)
        try:
            with open(path, encoding="utf-8") as f:
                data = json.load(f)
        except FileNotFoundError:
            data = {"version": VERSION}
        except (OSError, ValueError) as e:
            die(f"cannot read {path}: {e}")
        if data.get("version") != VERSION:
            die(f"{path} has schema version {data.get('version')!r}; this tool reads version {VERSION}")
        self.data = data
        self.nodes = data.get("nodes", [])
        return self

    def save(self):
        path = org_file()
        tmp = f"{path}.{os.getpid()}.tmp"
        self.data["nodes"] = self.nodes
        with open(tmp, "w", encoding="utf-8") as f:
            json.dump(self.data, f, indent=1)
            f.write("\n")
        os.replace(tmp, path)

    def __exit__(self, *_):
        self.lock.close()

    def find(self, name):
        if name == "proxy":
            return dict(PROXY, **self.data.get("proxy", {}))
        return next((n for n in self.nodes if n["id"] == name), None)

    def need(self, name):
        node = self.find(name)
        if node is None or name == "proxy":
            die(f"no registered node {name!r}; see agents org show")
        return node


def short(path):
    home = os.path.expanduser("~")
    return "~" + path[len(home):] if path and path.startswith(home + "/") else path


def parse_opts(args, keys):
    opts, rest = {}, []
    while args:
        a = args.pop(0)
        if a == "--":
            rest.extend(args)
            break
        if a.startswith("--") and a[2:] in keys:
            if not args:
                die(f"{a} needs a value")
            opts[a[2:]] = args.pop(0)
        elif a.startswith("--"):
            die(f"unknown option {a}\n{USAGE}")
        else:
            rest.append(a)
    return opts, rest


def as_int(value, what):
    try:
        n = int(value)
    except (TypeError, ValueError):
        die(f"{what} must be an integer, got {value!r}")
    if n < 0:
        die(f"{what} must be at least 0")
    return n


def path_of(value):
    return os.path.abspath(os.path.expanduser(value)) if value else ""


def apply(node, opts):
    for key, value in opts.items():
        if key in ("budget", "depth", "pid"):
            node[key] = as_int(value, f"--{key}")
        elif key in ("brief", "status"):
            node["brief" if key == "brief" else "status_file"] = path_of(value)
        elif key == "state":
            if value not in STATES:
                die(f"--state must be one of {', '.join(STATES)}")
            node[key] = value
            node.pop("died", None)
        else:
            node[key] = value


def check_budget(node, up):
    if node["budget"] >= up["budget"]:
        die(f"{up['id']} has delegation budget {up['budget']}, so {node['id']} can hold at most "
            f"{up['budget'] - 1}" if up["budget"] else f"{up['id']} has delegation budget 0 and cannot delegate")


def update(org, node, opts):
    apply(node, {k: v for k, v in opts.items() if k != "parent"})
    if "parent" in opts:
        up = org.find(opts["parent"])
        if up is None:
            die(f"unknown parent {opts['parent']!r}; see agents org show")
        below, frontier = set(), {node["id"]}
        while frontier:
            below |= frontier
            frontier = {n["id"] for n in org.nodes if n.get("parent") in frontier} - below
        if up["id"] in below:
            die(f"{up['id']} is {node['id']} or below it")
        node["parent"] = up["id"]
    up = org.find(node["parent"])
    if up is not None:
        check_budget(node, up)
        shift = up["depth"] + 1 - node["depth"]
        if shift:
            frontier = [node]
            while frontier:
                for n in frontier:
                    n["depth"] += shift
                ids = {n["id"] for n in frontier}
                frontier = [n for n in org.nodes if n.get("parent") in ids]


def add(org, name, opts, save=True):
    role = opts.get("role")
    if role not in BUDGETS or role == "proxy":
        die(f"--role must be one of lead, sub-lead, worker, got {role!r}")
    if name == "proxy" or not name or any(c.isspace() for c in name):
        die(f"invalid node name {name!r}")
    up = org.find(opts.get("parent", "proxy"))
    if up is None:
        die(f"unknown parent {opts.get('parent')!r}; see agents org show")
    node = {"id": name, "role": role, "domain": "", "parent": up["id"], "provider": "claude",
            "session": "", "brief": "", "status_file": "", "depth": up["depth"] + 1, "budget": BUDGETS[role],
            "state": "active", "started": time.strftime("%Y-%m-%dT%H:%M:%S%z")}
    apply(node, {k: v for k, v in opts.items() if k != "parent"})
    check_budget(node, up)
    org.nodes = [n for n in org.nodes if n["id"] != name] + [node]
    if save:
        org.save()
    return node


def remove(org, name):
    gone, frontier = set(), {name}
    while frontier:
        gone |= frontier
        frontier = {n["id"] for n in org.nodes if n.get("parent") in frontier} - gone
    before = len(org.nodes)
    org.nodes = [n for n in org.nodes if n["id"] not in gone]
    org.save()
    return before - len(org.nodes)


def alive(pid):
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except OSError:
        return True
    return True


def owned_pid(pid, name):
    try:
        environ = (Path("/proc") / str(pid) / "environ").read_bytes().split(b"\0")
    except OSError:
        return False
    return f"AGENT_ORG_NAME={name}".encode() in environ


def descendants(pid):
    children = {}
    for stat in Path("/proc").glob("[0-9]*/stat"):
        try:
            fields = stat.read_text().rsplit(")", 1)[1].split()
        except (OSError, IndexError):
            continue
        children.setdefault(int(fields[1]), []).append(int(stat.parent.name))
    found, frontier = [], [pid]
    while frontier:
        frontier = [c for p in frontier for c in children.get(p, [])]
        found += frontier
    return found


def close(name, wait=8, recycle=False, expected_session=None):
    with Org() as org:
        node = org.find(name)
        if node is None or (name == "proxy" and (not expected_session or node.get("session") != expected_session)) or (node.get("state") != "finished" and not recycle):
            return False
        pid = node.get("pid")
    host_name = f"proxy-{expected_session}" if name == "proxy" else name
    tmux = ["tmux", "-L", f"socrates-{host_name}"]
    if subprocess.run([*tmux, "has-session", "-t", host_name], stderr=subprocess.DEVNULL).returncode == 0:
        subprocess.run([*tmux, "kill-session", "-t", host_name], stderr=subprocess.DEVNULL)
    elif pid and alive(pid):
        if not owned_pid(pid, name):
            print(f"agents org: pid {pid} is not {name}'s session; left it running", file=sys.stderr)
            return False
        mine = {os.getpid(), os.getppid()}
        for target in [*reversed(descendants(pid)), pid]:
            if target not in mine:
                try:
                    os.kill(target, signal.SIGTERM)
                except ProcessLookupError:
                    pass
    elif not pid:
        return False
    deadline = time.monotonic() + wait
    while pid and alive(pid) and time.monotonic() < deadline:
        time.sleep(0.2)
    if pid and alive(pid):
        print(f"agents org: {name}'s session (pid {pid}) is still running", file=sys.stderr)
        return False
    with Org() as org:
        node = org.find(name)
        if not recycle and node is not None and node is not PROXY and node.get("state") == "finished":
            remove(org, name)
    return True


def health(n):
    if n.get("state") in ("launching", "finished", "failed"):
        return n["state"]
    if n.get("died") or (n.get("pid") and not alive(n["pid"])):
        return f"DIED while active, restart: agents lead restart {n['id']}"
    return "active"


def address(session, directory=None):
    sockets = Path(directory or f"/run/user/{os.getuid()}/cc-socks")
    for path in sorted(sockets.glob("*.sock")) if sockets.is_dir() else []:
        try:
            if session in (Path("/proc") / path.stem / "cmdline").read_bytes().split(b"\0")[1:]:
                return f"uds:{path}"
        except OSError:
            pass
    return None


def claude_ancestor():
    sockets = Path(f"/run/user/{os.getuid()}/cc-socks")
    pid = os.getppid()
    while pid > 1:
        if (sockets / f"{pid}.sock").exists():
            return pid
        try:
            pid = int((Path("/proc") / str(pid) / "stat").read_text().rsplit(")", 1)[1].split()[1])
        except (OSError, IndexError, ValueError):
            break
    die("no Claude session socket among the caller's ancestors; pass --pid")


def line(n):
    where = f"{n['provider']}:{n['session']}" if n.get("session") else n["provider"]
    out = (f"{n['id']}  {n['role']} · {n.get('domain') or '-'} · {where} · {health(n)}"
           f" · depth {n['depth']} · budget {n['budget']}")
    reach = address(n["session"].encode()) if n.get("provider") == "claude" and n.get("session") else None
    if reach:
        out += f" · message {reach}"
    for key, label in (("brief", "brief"), ("status_file", "status")):
        if n.get(key):
            out += f" · {label} {short(n[key])}"
    return out


def show(org):
    print(priority())
    print(laptop_line())
    names = {n["id"] for n in org.nodes}
    kids = {}
    for n in org.nodes:
        parent = n.get("parent") if n.get("parent") in names else "proxy"
        kids.setdefault(parent, []).append(n)

    def walk(node, prefix, last, top):
        print(line(node) if top else prefix + ("└─ " if last else "├─ ") + line(node))
        below = kids.get(node["id"], [])
        for i, child in enumerate(below):
            walk(child, "" if top else prefix + ("   " if last else "│  "), i == len(below) - 1, False)

    walk(org.find("proxy"), "", True, True)


def session_host():
    host = os.environ.get("AGENTS_SESSION_HOST")
    if not host:
        path = os.environ.get("AGENTS_ORCHESTRATION") or operator_dir() / "orchestration.toml"
        try:
            with open(path, "rb") as f:
                host = tomllib.load(f).get("sessions", {}).get("host")
        except FileNotFoundError:
            pass
    host = host or ("ghostty" if os.environ.get("WAYLAND_DISPLAY") else "tmux")
    if host not in ("ghostty", "tmux"):
        die("session host must be ghostty or tmux")
    return host


def transcript_exists(node):
    session = node.get("session")
    if not session:
        return False
    if node["provider"] == "claude":
        directory = selected_profile(node)
        root = (directory or Path.home() / ".claude") / "projects"
        return any(root.glob(f"*/{session}.jsonl"))
    root = Path(os.environ.get("NORTH_CODEX_POOLED_HOME") or Path.home() / ".local/state/north/codex-pooled")
    db = root / "sqlite/state_5.sqlite"
    if not db.is_file():
        return False
    with sqlite3.connect(f"{db.as_uri()}?mode=ro", uri=True) as connection:
        row = connection.execute("select rollout_path from threads where id=?", (session,)).fetchone()
    return bool(row and Path(row[0]).is_file())


def attach(args):
    if len(args) != 1:
        die(USAGE)
    with Org() as org:
        name = org.need(args[0])["id"]
    command = ["tmux", "-L", f"socrates-{name}", "attach", "-t", name]
    if not sys.stdin.isatty():
        command = ["ghostty", f"--title={name}", "-e", *command]
    os.execvp(command[0], command)


def claude_config(path):
    try:
        return json.loads(path.read_text())
    except FileNotFoundError:
        return {}
    except (OSError, ValueError) as error:
        die(f"cannot read Claude startup settings {path}: {error}")


def trust(cwd, directory=None, onboarding=False):
    path = (directory or Path.home()) / ".claude.json"
    lock = path.with_name(path.name + ".lock")
    deadline = time.monotonic() + 3
    while True:
        try:
            lock.mkdir()
            break
        except FileExistsError:
            if time.monotonic() >= deadline:
                die("Claude startup config lock is held; session was not opened")
            time.sleep(0.05)
    temporary = path.with_name(f".claude.json.{os.getpid()}")
    try:
        while True:
            before = claude_config(path)
            data = json.loads(json.dumps(before))
            projects = data.setdefault("projects", {})
            consent = projects
            if directory is not None:
                consent = claude_config(Path.home() / ".claude.json").get("projects", {}) | projects
            if not any(consent.get(str(parent), {}).get("hasTrustDialogAccepted") for parent in (Path(cwd), *Path(cwd).parents)):
                die(f"Claude Code has not trusted {cwd} or any parent; a headless lead there would wait on the trust prompt")
            projects.setdefault(cwd, {})["hasTrustDialogAccepted"] = True
            if onboarding:
                data["hasCompletedOnboarding"] = True
                data.setdefault("theme", "dark")
            if data == before:
                return
            temporary.write_text(json.dumps(data, indent=2))
            temporary.chmod(path.stat().st_mode & 0o777 if path.exists() else 0o600)
            if claude_config(path) != before:
                if time.monotonic() >= deadline:
                    die("Claude startup config kept changing; session was not opened")
                continue
            os.replace(temporary, path)
            return
    finally:
        temporary.unlink(missing_ok=True)
        lock.rmdir()


def proxy_gates(cwd, directory, extra):
    config = claude_config((directory or Path.home()) / ".claude.json")
    profile = directory or Path.home() / ".claude"
    settings = claude_config(profile / "settings.json")
    local = claude_config(Path(cwd) / ".claude/settings.local.json")
    credentials = claude_config(profile / ".credentials.json")
    key = os.environ.get("ANTHROPIC_API_KEY", "").strip()
    if key and key[-20:] not in config.get("customApiKeyResponses", {}).get("approved", []):
        die("Claude API key confirmation gate is unmet; session was not opened")
    if not (key or os.environ.get("CLAUDE_CODE_OAUTH_TOKEN") or os.environ.get("ANTHROPIC_AUTH_TOKEN")
            or credentials.get("claudeAiOauth", {}).get("accessToken")):
        die("Claude authentication gate is unmet: profile needs sign-in; session was not opened")
    mode = "bypassPermissions"
    for index, arg in enumerate(extra):
        if arg == "--permission-mode" and index + 1 < len(extra):
            mode = extra[index + 1]
        elif arg.startswith("--permission-mode="):
            mode = arg.split("=", 1)[1]
    if (mode == "bypassPermissions" or any(arg in extra for arg in (
            "--dangerously-skip-permissions", "--allow-dangerously-skip-permissions"))) and not (
            settings.get("skipDangerousModePermissionPrompt") or local.get("skipDangerousModePermissionPrompt")
            or config.get("bypassPermissionsModeAccepted")):
        die("Claude bypassPermissions acceptance gate is unmet; session was not opened")
    if mode == "auto" and not settings.get("skipAutoPermissionPrompt"):
        die("Claude auto permission-mode acceptance gate is unmet; session was not opened")


def selected_profile(node):
    if not node.get("account"):
        return Path(os.environ["CLAUDE_CONFIG_DIR"]) if os.environ.get("CLAUDE_CONFIG_DIR") else None
    from claude_profiles import select
    try:
        return select(node["account"])
    except (OSError, ValueError) as error:
        die(f"Claude authentication gate: {error}")


def launch(node, extra, cwd, resume=False, dry=False):
    """Open the node's session without focus; returns the session id or None."""
    # Claude fast mode bills usage credits even with plan usage left (code.claude.com/docs/en/fast-mode).
    env = dict(os.environ, AGENT_ROLE=node["role"], AGENT_DEPTH=str(node["depth"]),
               AGENT_DELEGATION_BUDGET=str(node["budget"]), AGENT_ORG_NAME=node["id"], AGENT_DOMAIN=node["domain"], CLAUDE_CODE_DISABLE_FAST_MODE="1")
    for key in INHERITED_SESSION:
        env.pop(key, None)
    directory = selected_profile(node) if node["provider"] == "claude" else None
    if node.get("account"):
        env.pop("CLAUDE_CONFIG_DIR", None)
        if directory is not None:
            env["CLAUDE_CONFIG_DIR"] = str(directory)
    host = session_host()
    resuming = resume and transcript_exists(node)
    prompt = f"Read {node['brief']} and follow it."
    if resume and node.get("status_file"):
        prompt += f" You were restarted after an unexpected exit; resume from {node['status_file']}."
    if node["provider"] == "codex":
        env.update(CODEX_LEAD_PROMPT=prompt, CODEX_LEAD_HOST=host,
                   CODEX_LEAD_RESUME=node["session"] if resuming else "")
        command = ["codex-lead", "start", node["brief"], "--org-name", node["id"], *extra]
        if dry:
            print(shlex.join(["env", *(f"{k}={env[k]}" for k in ("AGENT_ROLE", "AGENT_DEPTH", "AGENT_DELEGATION_BUDGET", "AGENT_ORG_NAME", "AGENT_DOMAIN")), *command]))
            return f"dry-{node['id']}"
        result = subprocess.run(command, env=env, cwd=cwd,
                                stdout=subprocess.PIPE, text=True)
        out = result.stdout.strip().splitlines()
        return out[-1] if result.returncode == 0 and out else None
    if not dry:
        if node["role"] == "proxy":
            proxy_gates(cwd, directory, extra)
        trust(cwd, directory, onboarding=node["role"] == "proxy")
    session = node["session"] if resuming else str(uuid.uuid4())
    settings = (["--model", node.get("model", CLAUDE_LEAD[1]), "--effort", node.get("effort", CLAUDE_LEAD[3]), "--permission-mode", "bypassPermissions"]
                if node["role"] == "worker" else CLAUDE_LEAD)
    inner = shlex.join(["claude", "--resume" if resuming else "--session-id", session, "--name", node["id"],
                        *settings, *extra, prompt])
    if dry:
        print(shlex.join(["env", *(f"{k}={env[k]}" for k in ("AGENT_ROLE", "AGENT_DEPTH", "AGENT_DELEGATION_BUDGET", "AGENT_ORG_NAME", "AGENT_DOMAIN", "CLAUDE_CONFIG_DIR") if k in env)]) + " " + inner)
        return session
    registration = f" --provider claude --session {session}" if node["role"] == "proxy" else ""
    script = ('if [ -n "${AGENT_SPAWN_CLAIM:-}" ]; then python3 "$AGENT_SPAWN_TOOL" started "$AGENT_SPAWN_CLAIM" || exit; unset AGENT_SPAWN_CLAIM AGENT_SPAWN_TOOL; fi; '
              f'agents org set "$AGENT_ORG_NAME" --pid $${registration} >/dev/null; '
              f'trap \'agents org exit "$AGENT_ORG_NAME" --pid $$ >/dev/null\' EXIT HUP TERM; {inner}; exit $?')
    host_name = f"proxy-{session}" if node["role"] == "proxy" else node["id"]
    if host == "tmux":
        tmux = ["tmux", "-L", f"socrates-{host_name}"]
        body = (shlex.join([*tmux, "-f", "/dev/null", "new-session", "-d", "-s", host_name,
                           "-x", "200", "-y", "50", shlex.join(["bash", "-c", script])]) + "; " +
                shlex.join([*tmux, "set-option", "-g", "set-clipboard", "off"]) + "; " +
                f"while {shlex.join([*tmux, 'has-session', '-t', host_name])} 2>/dev/null; do sleep 5; done")
        command = ["systemd-run", "--user", "--expand-environment=no", f"--unit=socrates-{host_name}", "--collect",
                   "-p", "KillMode=control-group", f"--working-directory={cwd}"]
        for key, value in env.items():
            if key.startswith("AGENT") or key in ("CLAUDE_CODE_DISABLE_FAST_MODE", "CLAUDE_CONFIG_DIR",
                    "PATH", "HOME", "XDG_RUNTIME_DIR", "WAYLAND_DISPLAY", "NIRI_SOCKET",
                    "DBUS_SESSION_BUS_ADDRESS", "SSH_AUTH_SOCK"):
                command.append(f"--setenv={key}={value}")
        command.extend(["bash", "-c", body])
    else:
        command = ["spawn-quiet", "ghostty", f"--title={node['id']}", f"--working-directory={cwd}",
                   "-e", "bash", "-c", script]
    result = subprocess.run(command, env=env, cwd=cwd, stdout=subprocess.DEVNULL)
    return session if result.returncode == 0 else None


def first_turn(session, directory, timeout):
    root = (directory or Path.home() / ".claude") / "projects"
    deadline = time.monotonic() + timeout
    while True:
        for path in root.glob(f"*/{session}.jsonl"):
            try:
                with path.open() as transcript:
                    for line in transcript:
                        try:
                            row = json.loads(line)
                        except ValueError:
                            continue
                        if row.get("type") == "system" and row.get("subtype") == "turn_duration":
                            return True
            except OSError:
                continue
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            return False
        time.sleep(min(0.2, remaining))


def proxy_start(args):
    opts, extra = parse_opts(args, {"brief", "account", "cwd", "timeout"})
    if "brief" not in opts:
        die("proxy start needs --brief")
    brief = path_of(opts["brief"])
    if not os.path.isfile(brief):
        die(f"brief not found: {brief}")
    try:
        timeout = float(opts.get("timeout", "120"))
        if not 0 < timeout < float("inf"):
            raise ValueError
    except ValueError:
        die("--timeout must be a positive number of seconds")
    cwd = path_of(opts.get("cwd", os.getcwd()))
    node = dict(PROXY, brief=brief, cwd=cwd)
    if "account" in opts:
        node["account"] = opts["account"]
    directory = selected_profile(node)
    session = launch(node, extra, cwd)
    if session is None:
        die("proxy failed to start", 1)
    if not first_turn(session, directory, timeout):
        close("proxy", recycle=True, expected_session=session)
        die("proxy failed to start: timed out waiting for its first completed turn", 1)
    with Org() as org:
        registered = org.data.get("proxy", {})
        pid = registered.get("pid")
        if registered.get("session") != session or not pid or not alive(pid) or not owned_pid(pid, "proxy"):
            if registered.get("session") == session:
                registered["died"] = time.strftime("%Y-%m-%dT%H:%M:%S%z")
                org.save()
            die("proxy failed to start: matching session exited after its first completed turn", 1)
        node.update(registered)
        node.update(provider="claude", session=session, brief=brief, cwd=cwd, state="active")
        node.pop("died", None)
        node.pop("account", None)
        if "account" in opts:
            node["account"] = opts["account"]
        org.data["proxy"] = node
        org.save()
    print(f"proxy started {session}: first turn completed")


def resident_hold(brief):
    from resident_cap import hold
    try:
        text = Path(brief).read_text()
    except OSError:
        text = ""
    line = hold([text])
    if line:
        die(line, 3)


def lead_start(args):
    opts, extra = parse_opts(args, {"provider", "domain", "brief", "name", "parent", "budget", "cwd", "role", "account"})
    provider = opts.get("provider", "claude")
    if provider not in ("claude", "codex"):
        die("--provider must be claude or codex")
    if "account" in opts:
        if provider != "claude":
            die("--account is available for Claude sessions")
        selected_profile({"account": opts["account"]})
    if "domain" not in opts or "brief" not in opts:
        die(f"lead start needs --domain and --brief\n{USAGE}")
    brief = os.path.realpath(os.path.expanduser(opts["brief"]))
    if not os.path.isfile(brief):
        die(f"brief not found: {opts['brief']}")
    resident_hold(brief)
    name = opts.get("name") or f"{opts['domain']}-{'lead' if provider == 'claude' else 'codex-lead'}"
    cwd = path_of(opts.get("cwd", os.getcwd()))
    node_opts = {"role": opts.get("role", "lead"), "domain": opts["domain"], "provider": provider, "brief": brief,
                 "status": os.path.splitext(brief)[0].removesuffix("-brief") + "-status.md",
                 "parent": opts.get("parent", os.environ.get("AGENT_ORG_NAME") or "proxy")}
    if "budget" in opts:
        node_opts["budget"] = opts["budget"]
    if "account" in opts:
        node_opts["account"] = opts["account"]
    with Org() as org:
        if org.find(name) is not None:
            die(f"{name} is already in the org; agents org remove {name} first")
        node = add(org, name, node_opts)
        node["cwd"] = cwd
        node["args"] = extra
        org.save()
    started(name, launch(node, extra, cwd))


def lead_restart(args):
    opts, names = parse_opts(args, {"packet"})
    if len(names) != 1:
        die(USAGE)
    if "packet" in opts:
        return lead_recycle(names[0], opts["packet"])
    with Org() as org:
        node = org.need(names[0])
        if health(node) == "active":
            die(f"{node['id']} is alive and active; nothing to restart")
        if node.get("state") == "finished":
            die(f"{node['id']} finished its workstream; start a new lead for new work")
        if not os.path.isfile(node.get("brief") or ""):
            die(f"{node['id']} has no readable brief to restart from: {node.get('brief') or 'none'}")
        resident_hold(node["brief"])
        cwd = node.get("cwd") or os.getcwd()
        if node["provider"] == "claude":
            trust(cwd, selected_profile(node))
        node.pop("died", None)
        node.pop("pid", None)
        org.save()
    started(node["id"], launch(node, node.get("args", []), cwd, resume=True))


def lead_recycle(name, packet):
    packet = path_of(packet)
    try:
        contents = Path(packet).read_bytes()
    except OSError as error:
        die(f"recycle packet unreadable: {error}")
    if not contents.strip():
        die("recycle packet is empty")
    digest = hashlib.sha256(contents).hexdigest()
    with Org() as registry:
        node = dict(registry.need(name))
        prior = node.get("recycle", {})
        if prior.get("hash") == digest and node.get("session") != prior.get("previous_session") and health(node) == "active":
            print(line(node))
            return
        if node.get("state") != "active":
            die("cannot recycle a finished workstream")
        if any(child.get("parent") == name for child in registry.nodes):
            die("settle and deregister child nodes before recycling")
        resident_hold(packet)
    if node["provider"] == "codex" and node.get("session"):
        root = Path(os.environ.get("NORTH_CODEX_POOLED_HOME") or Path.home() / ".local/state/north/codex-pooled")
        try:
            with sqlite3.connect(f"{(root / 'sqlite/state_5.sqlite').as_uri()}?mode=ro", uri=True) as db:
                child = db.execute("WITH RECURSIVE children(id) AS (SELECT child_thread_id FROM thread_spawn_edges WHERE parent_thread_id=? UNION SELECT e.child_thread_id FROM thread_spawn_edges e JOIN children c ON e.parent_thread_id=c.id) SELECT t.id FROM children c JOIN threads t ON t.id=c.id WHERE t.archived=0 LIMIT 1", (node["session"],)).fetchone()
            if child:
                die("archive and settle native children before recycling")
        except sqlite3.Error as error:
            die(f"cannot verify settled native children: {error}")
    if not close(name, recycle=True):
        die("prior session did not stop; successor not started")
    with Org() as registry:
        current = registry.need(name)
        current.update(brief=packet, recycle={"hash": digest, "previous_session": node.get("session")})
        current.pop("died", None)
        current.pop("pid", None)
        registry.save()
        node = dict(current)
    started(name, launch(node, node.get("args", []), node.get("cwd") or os.getcwd(), resume=False))


def started(name, session):
    with Org() as org:
        node = org.need(name)
        if session is None:
            node["died"] = time.strftime("%Y-%m-%dT%H:%M:%S%z")
            org.save()
            die(f"{name} failed to start", 1)
        node["session"] = session
        org.save()
        print(line(node))


def main(argv):
    starting = argv[:2] == ["lead", "start"] or argv[:3] == ["org", "start", "proxy"]
    help_args = argv[:argv.index("--")] if starting and "--" in argv else argv
    if any(arg in ("-h", "--help") for arg in help_args):
        print(USAGE)
        return
    if argv[:1] == ["attach"]:
        return attach(argv[1:])
    if argv[:1] == ["lead"]:
        sub = argv[1:2]
        if sub == ["start"]:
            return lead_start(argv[2:])
        if sub == ["restart"]:
            return lead_restart(argv[2:])
        die(USAGE)
    args = argv[1:] if argv[:1] == ["org"] else list(argv)
    cmd = args.pop(0) if args else "show"
    if cmd in ("-h", "--help", "help"):
        print(USAGE)
        return
    if cmd == "priority":
        if args:
            die(USAGE)
        print(priority())
        return
    if cmd == "start" and args[:1] == ["proxy"]:
        return proxy_start(args[1:])
    with Org() as org:
        if cmd == "show" and not args:
            show(org)
        elif cmd == "set" and args[:1] == ["proxy"]:
            opts, rest = parse_opts(args[1:], {"pid", "provider", "session", "status"})
            if rest:
                die(USAGE)
            node = dict(org.data.get("proxy", {}))
            apply(node, opts)
            provider = node.get("provider", PROXY["provider"])
            if provider not in ("claude", "codex"):
                die("--provider must be claude or codex")
            if provider == "codex":
                try:
                    uuid.UUID(node.get("session") or "")
                except (ValueError, AttributeError):
                    die("Codex proxy needs a UUID --session")
                if not node.get("pid"):
                    die("Codex proxy needs --pid")
            elif "pid" not in opts:
                node["pid"] = claude_ancestor()
            org.data["proxy"] = node
            org.save()
            print(f"proxy  {provider} pid {node['pid']}")
        elif cmd in ("add", "set"):
            opts, rest = parse_opts(args, FIELDS)
            if len(rest) != 1:
                die(USAGE)
            if cmd == "add":
                if "domain" not in opts:
                    die("add needs --role and --domain")
                node = add(org, rest[0], opts)
            else:
                node = org.need(rest[0])
                update(org, node, opts)
                org.save()
            print(line(node))
        elif cmd == "finish" and len(args) == 1:
            node = org.need(args[0])
            node["state"] = "finished"
            org.save()
            print(line(node))
        elif cmd == "exit" and args:
            opts, rest = parse_opts(args, {"pid"})
            if len(rest) != 1:
                die(USAGE)
            node = org.find(rest[0])
            if node is None or rest[0] == "proxy":
                return
            if "pid" in opts and node.get("pid") and node["pid"] != as_int(opts["pid"], "--pid"):
                return
            if node.get("state") == "finished":
                remove(org, node["id"])
            else:
                node["died"] = time.strftime("%Y-%m-%dT%H:%M:%S%z")
                org.save()
        elif cmd == "remove" and len(args) == 1:
            print(f"removed {remove(org, args[0])} node(s)")
        else:
            die(USAGE)
    if cmd == "finish":
        close(args[0])


if __name__ == "__main__":
    main(sys.argv[1:])
