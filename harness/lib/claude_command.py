import json
import os
from pathlib import Path
import re
import sqlite3
import sys
import tomllib
from datetime import datetime, timezone

sys.path.insert(0, str(Path(__file__).resolve().parent))
from calibration import open_ledger
from claude_profiles import config_dir, observed_plan, profiles, select
from usage import account_settings
import resident_cap


def parse(args):
    label, remaining = None, []
    index = 0
    while index < len(args):
        word = args[index]
        if word == "--":
            remaining.extend(args[index:])
            break
        if word == "--account" or word.startswith("--account="):
            if label is not None:
                raise ValueError("--account may be specified once")
            if word == "--account":
                index += 1
                if index == len(args):
                    raise ValueError("--account needs a label")
                label = args[index]
            else:
                label = word.split("=", 1)[1]
        else:
            remaining.append(word)
        index += 1
    return label, remaining


def signed_profiles():
    found = {}
    for label in profiles():
        try:
            directory = select(label)
            if not ((directory or Path.home() / ".claude") / ".credentials.json").is_file():
                continue
            found[label] = directory
        except (OSError, ValueError):
            continue
    return found


def meters(label, directory):
    readings = {}
    try:
        cache = json.loads(((directory or Path.home()) / ".claude.json").read_text()).get("cachedUsageUtilization") or {}
        stamp = datetime.fromtimestamp(cache.get("fetchedAtMs", 0) / 1000, timezone.utc).isoformat()
        for row in (cache.get("utilization") or {}).get("limits") or []:
            name = row.get("kind")
            if name in ("weekly_all", "session") and row.get("percent") is not None:
                readings[name] = (stamp, row["percent"], row.get("resets_at"))
    except (OSError, ValueError):
        pass
    state = Path(os.environ.get("XDG_STATE_HOME") or Path.home() / ".local/state")
    path = Path(os.environ["THREADS_DB"]) if os.environ.get("THREADS_DB") else state / "threads/snapshot.db"
    if not path.exists() and not os.environ.get("THREADS_DB"):
        path = state / "threads/threads.db"
    try:
        with open_ledger(path, timeout=.05) as db:
            for name in ("weekly_all", "session"):
                row = db.execute("SELECT ts, used_pct, resets_at FROM usage WHERE provider='claude' AND account=? "
                                 "AND kind='window' AND name=? AND used_pct IS NOT NULL ORDER BY ts DESC, rowid DESC LIMIT 1",
                                 (label, name)).fetchone()
                if row and (name not in readings or datetime.fromisoformat(row[0].replace("Z", "+00:00"))
                            > datetime.fromisoformat(readings[name][0].replace("Z", "+00:00"))):
                    readings[name] = row
    except (OSError, ValueError, sqlite3.Error):
        pass
    return readings


def pick(available):
    settings = account_settings("claude")
    catalog = tomllib.loads((Path(__file__).resolve().parent.parent / "model-catalog.toml").read_text())
    plans = catalog["accounts"]["claude"]["plans"]
    choices, resets = [], []
    for label, directory in available.items():
        readings = meters(label, directory)
        weekly, session = readings.get("weekly_all"), readings.get("session")
        resets.append(f"{label}: weekly resets {weekly[2] if weekly else 'unknown'}, session resets {session[2] if session else 'unknown'}")
        plan = observed_plan(directory)
        required = settings.get("auto_route_requires", {}).get(label)
        if required and plan != required:
            continue
        plan = plans.get(plan) or plans.get(settings.get("plans", {}).get(label))
        if not weekly or not session or not plan or weekly[1] >= 95 or session[1] >= 95:
            continue
        headroom = (100 - weekly[1]) * plan["usage_multiplier"]
        choices.append((-headroom, label, weekly[1], plan["name"], directory))
    if not choices:
        raise RuntimeError("no Claude account has available headroom\n" + "\n".join(resets))
    _, label, used, plan, directory = min(choices)
    print(f"claude: using account {label} (weekly {used:g}% of {plan})", file=sys.stderr)
    return directory


def resume_profile(args, available):
    target = None
    for index, word in enumerate(args):
        if word.startswith("--resume="):
            target = word.split("=", 1)[1]
        elif word in ("--resume", "-r") and index + 1 < len(args) and not args[index + 1].startswith("-"):
            target = args[index + 1]
    project = re.sub(r"[^a-zA-Z0-9]", "-", str(Path.cwd()))
    matches = []
    for label, directory in available.items():
        root = (directory or Path.home() / ".claude") / "projects" / project
        candidates = [root / (target + ".jsonl")] if target else root.glob("*.jsonl")
        for path in candidates:
            if path.is_file():
                matches.append((path.stat().st_mtime_ns, label, directory))
    if not matches:
        raise RuntimeError("no Claude session found for this directory in the signed-in profiles")
    if target and len(matches) > 1:
        raise RuntimeError("session exists in multiple Claude profiles; choose --account explicitly")
    _, label, directory = max(matches, key=lambda row: (row[0], row[1]))
    print(f"claude: resuming account {label}", file=sys.stderr)
    return directory


def main(runtime, args):
    held = resident_cap.gate("claude", args)
    if held:
        print(held, file=sys.stderr)
        sys.exit(3)
    label, args = parse(args)
    if label is None and args[:1] and args[0] in set(account_settings("claude").get("labels", {}).values()):
        label, args = args[0], args[1:]
    if label is None and (os.environ.get("AGENT_ROLE") or os.environ.get("AGENT_ORG_NAME") or os.environ.get("CLAUDE_CONFIG_DIR")
                          or any(word in ("-p", "--print", "--version", "-v", "--help", "-h") for word in args)
                          or args[:1] == ["auth"]):
        os.execv(runtime, [runtime, *args])
    if label is not None:
        valid = sorted(set(account_settings("claude").get("labels", {}).values()))
        if label not in valid:
            raise ValueError("unknown account " + label + "; valid labels: " + ", ".join(valid))
        directory = select(label)
    else:
        available = signed_profiles()
        if any(word in ("--resume", "-r", "-c", "--continue") or word.startswith("--resume=") for word in args):
            directory = resume_profile(args, available)
        else:
            directory = pick(available)
    env = dict(os.environ)
    env.pop("CLAUDE_CONFIG_DIR", None)
    if directory is not None:
        env["CLAUDE_CONFIG_DIR"] = str(directory)
    os.execve(runtime, [runtime, *args], env)


if __name__ == "__main__":
    try:
        main(sys.argv[1], sys.argv[2:])
    except ValueError as error:
        print(f"claude: {error}", file=sys.stderr)
        sys.exit(2)
    except (OSError, RuntimeError) as error:
        print(f"claude: {error}", file=sys.stderr)
        sys.exit(1)
