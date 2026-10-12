#!/usr/bin/env python3
"""Codex turns that failed because OpenAI had no capacity for the model, read from Codex rollouts.

usage: provider_capacity.py events [--minutes N]   capacity errors in the last N minutes (default 15), as JSON
       provider_capacity.py status [--minutes N]   {"codex": {"since", "model"}} while a model's last turn in the window failed for capacity
"""
import json
import os
import sqlite3
import sys
import time
from datetime import datetime
from pathlib import Path

ERROR_INFO = "server_overloaded"
MESSAGE = "Selected model is at capacity"


def stamp(s):
    return datetime.fromisoformat(s.replace("Z", "+00:00")).timestamp()


def at_capacity(payload):
    error = payload.get("error") or {}
    return error.get("codex_error_info") == ERROR_INFO or MESSAGE in str(error.get("message", ""))


def turns(records):
    """Each completed turn as (timestamp, model, effort, outcome) with outcome capacity, error or ok."""
    model = effort = None
    for r in records:
        p, kind = r.get("payload", {}), r.get("type")
        if kind == "turn_context":
            model, effort = p.get("model") or model, p.get("effort") or effort
        elif kind == "event_msg" and p.get("type") == "task_complete":
            outcome = "capacity" if at_capacity(p) else "error" if p.get("error") else "ok"
            yield r.get("timestamp"), model, effort, outcome


def records(path):
    try:
        with open(path) as f:
            for line in f:
                try:
                    yield json.loads(line)
                except json.JSONDecodeError:
                    continue
    except OSError:
        return


def rollouts(root, since):
    state = root / "sqlite/state_5.sqlite"
    if state.exists():
        db = sqlite3.connect(f"file:{state}?mode=ro", uri=True)
        try:
            return [p for (p,) in db.execute("SELECT rollout_path FROM threads WHERE updated_at >= ?", (int(since),))]
        finally:
            db.close()
    return [str(p) for p in (root / "sessions").rglob("rollout-*.jsonl") if p.stat().st_mtime >= since]


def outcomes(paths, since):
    out = []
    for path in paths:
        for at, model, effort, outcome in turns(records(path)):
            if at and stamp(at) >= since:
                out.append({"at": at, "model": model, "effort": effort, "outcome": outcome, "rollout": path})
    return sorted(out, key=lambda o: stamp(o["at"]))


def status(rows):
    """A model is at capacity while its latest turn failed for capacity; since is the first failure after its last answer."""
    open_ = {}
    for o in rows:
        if o["outcome"] == "capacity":
            open_.setdefault(o["model"], o["at"])
        elif o["outcome"] == "ok":
            open_.pop(o["model"], None)
    if not open_:
        return {}
    model, since = min(open_.items(), key=lambda kv: stamp(kv[1]))
    return {"codex": {"since": since, "model": model}}


def root():
    return Path(os.environ.get("CODEX_CONFIG_DIR", os.path.expanduser("~/code/north-data/codex-pooled")))


def main(argv):
    if len(argv) < 2 or argv[1] not in ("events", "status"):
        print(__doc__.strip(), file=sys.stderr)
        return 2
    minutes = float(argv[argv.index("--minutes") + 1]) if "--minutes" in argv else 15
    since = time.time() - minutes * 60
    rows = outcomes(rollouts(root(), since), since)
    out = [o for o in rows if o["outcome"] == "capacity"] if argv[1] == "events" else status(rows)
    print(json.dumps(out))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
