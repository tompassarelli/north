import json
import os
import pwd
import subprocess
from pathlib import Path

def binary():
    return os.environ.get("NORTH_PACE_BIN") or str(Path(pwd.getpwuid(os.getuid()).pw_dir) / ".local/state/north/current/north")


def ask(op, **request):
    result = subprocess.run([binary(), "pace"], input=json.dumps({"op": op, **request}), capture_output=True, text=True, timeout=10)
    if result.returncode:
        raise ValueError(result.stderr.strip())
    return json.loads(result.stdout)


def target_for(account, at):
    return ask("target_for", account=account, at=at)


def advance(previous, windows, workers, at, initial=2, margin=10, target=None):
    return ask("advance", previous=previous, windows=windows, workers=workers, at=at, initial=initial, margin=margin, target=target)


def fresh(control, at):
    return ask("fresh", control=control, at=at)


def interval(previous, elapsed, throttled=False, retry_after=None):
    return ask("interval", previous=previous, elapsed=elapsed, throttled=throttled, retry_after=retry_after or 0)


def week_ceiling(window, target):
    return ask("week_ceiling", window=window, target=target)


def refusal(account, control, now):
    return ask("refusal", account=account, control=control, now=now)
