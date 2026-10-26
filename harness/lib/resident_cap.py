#!/usr/bin/env python3
"""Resident-CLI cap: hold a new resident agent CLI on a host already running its cap of them.

  resident_cap.py gate claude|codex [ARG...]   exit 3 with the Hold line when this launch is refused
  resident_cap.py status                        print this host's live count and cap

A resident CLI is a live claude or codex process a live Fleet session runs, in any mode, or an unregistered
top-level interactive one (no claude or codex ancestor). Otherwise `-p`/`--print`,
`--cloud`, codex exec/review/app-server/mcp-server/cloud and maintenance subcommands are not resident, and an
org node above the launch with no agent CLI in between is the launch itself, not a peer. The cap is
`[hosts.<hostname>] resident_cap = N` in orchestration.toml; a host without it is never held. `CASE=URGENT` with a
FACT of at least 15 characters (in the environment, an argument or the lead brief) passes and is logged beside the
spawn gate's overrides.

Inputs for tests: RESIDENT_CAP_PROC (default /proc), RESIDENT_CAP_HOST (default the hostname), RESIDENT_CAP_PID
(default this process), NORTH_BIN, AGENTS_ORCHESTRATION, AGENTS_OPERATOR_DIR, CODEX_BEHAVIOR_STATE.
"""

import json
import os
import re
import socket, subprocess
import sys
import time
import tomllib
from pathlib import Path

AGENTS = ("claude", "codex")
NON_RESIDENT_ARGS = {"-p", "--print", "--cloud", "exec", "app-server", "mcp-server", "cloud"}
CLAUDE_ONE_SHOT = {"-p", "--print", "--cloud", "-v", "--version", "-h", "--help"}
CLAUDE_SUBCOMMANDS = {"auth", "mcp", "plugin", "doctor", "update", "install", "config", "setup-token",
                      "migrate-installer", "agents"}
CODEX_VALUE_OPTIONS = {"-c", "--config", "--enable", "--disable", "--remote-auth-token-env", "-i", "--image", "-m",
                       "--model", "--local-provider", "-p", "--profile", "-s", "--sandbox", "-C", "--cd", "--add-dir",
                       "-a", "--ask-for-approval"}
CODEX_SUBCOMMANDS = {"exec", "e", "review", "login", "logout", "mcp", "plugin", "app-server", "remote-control",
                     "completion", "update", "doctor", "sandbox", "debug", "apply", "a", "queue", "archive", "delete",
                     "migrate-rollouts", "unarchive", "cloud", "exec-server", "features", "help", "agents",
                     "mcp-server"}
URGENT = re.compile(r"(?m)^\W*CASE=URGENT\s+FACT=(?:[\"“”]([^\"“”]*)[\"“”]|['‘’]([^'‘’]*)['‘’])", re.IGNORECASE)


def host():
    return os.environ.get("RESIDENT_CAP_HOST") or socket.gethostname()


def orchestration_path():
    configured = os.environ.get("AGENTS_ORCHESTRATION")
    if configured:
        return Path(configured)
    operator = os.environ.get("AGENTS_OPERATOR_DIR")
    return (Path(operator) if operator else Path.home() / "code/south/main/agents") / "orchestration.toml"


def cap(name):
    try:
        value = tomllib.loads(orchestration_path().read_text()).get("hosts", {}).get(name, {}).get("resident_cap")
    except (OSError, ValueError, AttributeError):
        return None
    return value if isinstance(value, int) and not isinstance(value, bool) else None


def process_table():
    table = {}
    root = Path(os.environ.get("RESIDENT_CAP_PROC", "/proc"))
    try:
        entries = list(root.iterdir())
    except OSError:
        return table
    for entry in entries:
        if not entry.name.isdigit():
            continue
        try:
            stat = (entry / "stat").read_text()
            argv = (entry / "cmdline").read_bytes().split(b"\0")[:-1]
        except OSError:
            continue
        head, _, tail = stat.rpartition(")")
        table[int(entry.name)] = {"comm": head.partition("(")[2], "ppid": int(tail.split()[1]),
                                  "argv": [a.decode("utf-8", "replace") for a in argv]}
    return table


def agent_kind(info):
    if not info["argv"]:
        return None
    name = os.path.basename(info["argv"][0])
    if name in AGENTS and info["comm"] == name and not NON_RESIDENT_ARGS.intersection(info["argv"][1:]):
        return name
    return None


def ancestors(table, pid):
    chain, seen = [], set()
    parent = table.get(pid, {}).get("ppid")
    while parent in table and parent not in seen:
        chain.append(parent)
        seen.add(parent)
        parent = table[parent]["ppid"]
    return chain


def registered_pids(table):
    try:
        sessions = json.loads(subprocess.run(
            [os.environ.get("NORTH_BIN", "north"), "fleet", "list", "--json"],
            capture_output=True, text=True, timeout=10, check=True).stdout)
    except (OSError, ValueError, subprocess.SubprocessError):
        return set()
    return {pid for session in sessions if isinstance(session, dict) and session.get("status") != "exited"
            for pid, info in table.items() if info["ppid"] == session.get("host_pid") and info["comm"] in AGENTS}


def count(table=None, launcher=None):
    """Live resident agent CLIs on this host, leaving out the Fleet session this launch runs under."""
    table = process_table() if table is None else table
    own = set()
    for pid in ancestors(table, launcher) if launcher is not None else []:
        if agent_kind(table[pid]):
            break
        own.add(pid)
    nodes = registered_pids(table)
    registered = nodes - own
    unregistered = 0
    for pid, info in table.items():
        if not agent_kind(info):
            continue
        chain = ancestors(table, pid)
        if pid in nodes or any(agent_kind(table[p]) for p in chain) or nodes.intersection(chain):
            continue
        unregistered += 1
    return len(registered) + unregistered


def resident_launch(provider, args):
    if provider == "claude":
        if CLAUDE_ONE_SHOT.intersection(args):
            return False
        return not (args[:1] and args[0] in CLAUDE_SUBCOMMANDS)
    skip = False
    for word in args:
        if skip:
            skip = False
        elif word in ("-h", "--help", "-V", "--version"):
            return False
        elif word in CODEX_VALUE_OPTIONS:
            skip = True
        elif word == "--":
            return True
        elif not word.startswith("-"):
            return word not in CODEX_SUBCOMMANDS
    return True


def urgent_fact(texts):
    if os.environ.get("CASE", "").upper() == "URGENT" and len(os.environ.get("FACT", "").strip()) >= 15:
        return os.environ["FACT"].strip()
    for match in filter(None, map(URGENT.search, texts)):
        fact = (match.group(1) if match.group(1) is not None else match.group(2)).strip()
        if len(fact) >= 15:
            return fact
    return None


def log_override(fact):
    try:
        root = Path(os.environ.get("CODEX_BEHAVIOR_STATE",
                                   Path(os.environ.get("XDG_RUNTIME_DIR", "/tmp")) / "codex-behavior"))
        root.mkdir(parents=True, exist_ok=True)
        with open(root / "verify-overrides.jsonl", "a") as log:
            log.write(json.dumps({"at": time.time(), "gate": "resident-cap", "case": "URGENT", "host": host(),
                                  "cwd": os.getcwd(), "fact": fact[:300]}) + "\n")
    except OSError:
        pass


def hold(texts=(), launcher=None):
    """The Hold line refusing a new resident CLI on this host, or None to admit it."""
    name = host()
    limit = cap(name)
    if limit is None:
        return None
    live = count(launcher=os.getpid() if launcher is None else launcher)
    if live < limit:
        return None
    fact = urgent_fact(list(texts))
    if fact:
        log_override(fact)
        return None
    return f"Hold: {name} has {live}/{limit} resident agent CLIs; place this worker on laptop|cloud"


def gate(provider, args):
    if not resident_launch(provider, args):
        return None
    launcher = int(os.environ["RESIDENT_CAP_PID"]) if os.environ.get("RESIDENT_CAP_PID", "").isdigit() else None
    return hold(args, launcher)


def status():
    name = host()
    limit = cap(name)
    live = count(launcher=os.getpid())
    return f"{name}: {live}/{limit} resident agent CLIs" if limit is not None else f"{name}: {live} resident agent CLIs, no cap"


def main(argv):
    if argv[:1] == ["status"]:
        print(status())
        return 0
    if len(argv) >= 2 and argv[0] == "gate" and argv[1] in AGENTS:
        line = gate(argv[1], argv[2:])
        if line:
            print(line, file=sys.stderr)
            return 3
        return 0
    print(__doc__.strip().splitlines()[2], file=sys.stderr)
    print(__doc__.strip().splitlines()[3], file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
