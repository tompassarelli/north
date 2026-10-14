#!/usr/bin/env python3
"""Worker placement: where a box's worker runs once its brief is written.

  placement.py place BRIEF...   one line per brief: `<item>: target <target> (<why>)`
  placement.py credit           one line: `cloud credit: unread|$X left|spent|expired, expires in N days`
"""

import json
import os
import re
import subprocess
import sys
import tomllib
from datetime import datetime, timezone
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import resident_cap
from ssh_transport import ssh_argv

CLOUD = ("anthropic-cloud", "codex-cloud")
READOUT_DAYS = 7


def now():
    at = os.environ.get("PLACEMENT_NOW")
    return datetime.fromisoformat(at.replace("Z", "+00:00")) if at else datetime.now(timezone.utc)


def config():
    return tomllib.loads(resident_cap.orchestration_path().read_text())


def operator_dir():
    return Path(os.environ.get("AGENTS_OPERATOR_DIR") or resident_cap.orchestration_path().parent)


def state_dir():
    return Path(os.environ.get("XDG_STATE_HOME") or Path.home() / ".local/state")


def credit(cfg, at):
    name = cfg.get("placement", {}).get("cloud_credit")
    try:
        services = tomllib.loads((operator_dir() / "costs.toml").read_text()).get("service", [])
        service = next(s for s in services if s.get("id") == name)
        expires = datetime.fromisoformat(str(service["expires"]).replace("Z", "+00:00"))
    except (OSError, ValueError, StopIteration, KeyError):
        return False, f"cloud credit: not configured ({name or 'no [placement] cloud_credit'})"
    days = (expires - at).total_seconds() / 86400
    if days <= 0:
        return False, f"cloud credit: expired {expires:%Y-%m-%dT%H:%MZ}"
    path = state_dir() / "agents/costs" / f"{name}.json"
    try:
        readout = json.loads(path.read_text())
        spent = float(readout["usd_spent_period"])
        read = (datetime.fromisoformat(readout["read_at"].replace("Z", "+00:00")) if readout.get("read_at")
                else datetime.fromtimestamp(path.stat().st_mtime, timezone.utc))
    except (OSError, ValueError, KeyError, TypeError):
        spent, read = None, None
    left = f"expires in {int(days)} days"
    if spent is not None and spent >= float(service.get("initial", 0)):
        return False, f"cloud credit: spent (read {read:%Y-%m-%d}), {left}"
    if spent is None or (at - read).total_seconds() > READOUT_DAYS * 86400:
        return True, f"cloud credit: unread, {left}"
    return True, f"cloud credit: ${float(service['initial']) - spent:.2f} left (read {read:%Y-%m-%d}), {left}"


def laptop_state():
    path = os.environ.get("LAPTOP_STATE")
    try:
        if path:
            return json.loads(Path(path).read_text()).get("state") or "unknown"
        done = subprocess.run(["laptop", "status", "--json"], capture_output=True, text=True, timeout=20)
        return json.loads(done.stdout).get("state") or "unknown" if done.returncode == 0 else "unknown"
    except (OSError, ValueError, AttributeError, subprocess.TimeoutExpired):
        return "unknown"


def host_load(name):
    fixture = os.environ.get("PLACEMENT_HOSTS")
    if fixture:
        try:
            row = json.loads(Path(fixture).read_text()).get(name)
        except (OSError, ValueError, AttributeError):
            row = None
        if isinstance(row, dict):
            return bool(row.get("online")), int(row.get("live", 0))
    if name == resident_cap.host():
        return True, resident_cap.count(launcher=os.getpid())
    try:
        done = subprocess.run(ssh_argv("-o", "BatchMode=yes", "-o", "ConnectTimeout=5", name, "python3", "-I",
                                      "code/north/main/harness/lib/resident_cap.py", "status"),
                              capture_output=True, text=True, timeout=20)
    except (OSError, subprocess.TimeoutExpired):
        return False, 0
    match = re.match(r"\S+: (\d+)", done.stdout)
    return (True, int(match.group(1))) if done.returncode == 0 and match else (False, 0)


def facets(text):
    def field(key):
        found = re.search(rf"(?mi)^\s*{key}:\s*(.+)$", text)
        return found.group(1).strip() if found else ""
    needs = {w for w in re.split(r"[\s,|]+", field("Needs").lower()) if w}
    item = field("Item")
    native = field("Verify").lower().split()[:1] == ["native"] or bool(needs & {"native", "sign-ins"})
    return item, native, needs


def place(text, cfg, at, probe=None):
    probe = probe or {}
    rules = cfg.get("placement", {})
    item, native, needs = facets(text)
    repo = item.split("#", 1)[0].split()[0].lower() if item else ""
    private = bool(needs & {"private", "local-state"}) or repo in rules.get("private_repos", [])
    code_only = not native and not private
    hosts = cfg.get("hosts", {})
    why, laptop_why = [], None
    for target in rules.get("order", []):
        if target in CLOUD and not code_only:
            continue
        if native and target != "laptop":
            continue
        if target == "anthropic-cloud":
            if "credit" not in probe:
                probe["credit"] = credit(cfg, at)
            usable, line = probe["credit"]
            if usable:
                return target, line
            why.append(f"anthropic-cloud {line.removeprefix('cloud credit: ')}")
        elif target == "codex-cloud":
            if rules.get("codex_cloud") is True:
                return target, "codex_cloud on"
            why.append("codex-cloud off until C1")
        elif target == "laptop":
            state = probe.setdefault("laptop", laptop_state())
            if state == "available":
                return target, "laptop available"
            laptop_why = f"laptop {state}"
            why.append(laptop_why)
        elif isinstance(hosts.get(target, {}).get("resident_cap"), int):
            cap = hosts[target]["resident_cap"]
            online, live = probe.setdefault(target, host_load(target))
            if online and live < cap:
                return target, f"{target} {live}/{cap}"
            why.append(f"{target} {live}/{cap}" if online else f"{target} offline")
    return f"held: {laptop_why or (why[0] if why else 'no target in [placement] order')}", "; ".join(why)


def main(argv):
    if argv[:1] == ["credit"] and len(argv) == 1:
        print(credit(config(), now())[1])
        return 0
    if argv[:1] == ["place"] and len(argv) > 1:
        cfg, at, probe = config(), now(), {}
        for path in argv[1:]:
            text = sys.stdin.read() if path == "-" else Path(path).read_text()
            target, why = place(text, cfg, at, probe)
            item = facets(text)[0] or path
            print(f"{item}: target {target} ({why})")
        return 0
    print("\n".join(__doc__.strip().splitlines()[2:4]), file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
