#!/usr/bin/env python3
"""Worker placement: where a box's worker runs once its brief is written.

  placement.py place BRIEF...   one line per brief: `<item>: target <target> (<why>)`
  placement.py credit           one line: `cloud credit: unread|$X left|spent|expired, expires in N days`
"""

import json
import os
import re
import sqlite3
import subprocess
import sys
import tomllib
from datetime import datetime, timedelta, timezone
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import resident_cap
import pace_control
import plan_usage
import routing
import pace_sampler
import usage
from defaults import DEFAULTS
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


def claude_session():
    path = Path(os.environ["THREADS_DB"]) if os.environ.get("THREADS_DB") else state_dir() / "threads/snapshot.db"
    if not path.is_file():
        return [], None
    try:
        db = sqlite3.connect(path.resolve().as_uri() + "?mode=ro", uri=True, timeout=1)
        try:
            db.row_factory = sqlite3.Row
            rows = [dict(r) for r in db.execute(
                "SELECT ts, used_pct, resets_at, source FROM usage WHERE provider='claude' AND account='a' "
                "AND name='session' ORDER BY ts")]
        finally:
            db.close()
    except sqlite3.Error:
        return [], None
    source = str(rows[-1].get("source") or "") if rows else ""
    throttled = rows[-1]["ts"] if re.search(r"429|throttled=true|rate limit", source, re.I) else None
    return [{k: r[k] for k in ("ts", "used_pct", "resets_at")} for r in rows], throttled


def claude_out(at, rules, probe):
    if "claude_rows" not in probe or "claude_throttled" not in probe:
        probe["claude_rows"], probe["claude_throttled"] = claude_session()
    if "claude_paces" not in probe:
        probe["claude_paces"] = (pace_sampler.load(pace_sampler.state_dir() / "usage-gate.json", {}).get("claude") or {}).get("paces")
    return routing.claude_unavailable(probe["claude_paces"], probe["claude_rows"], at, rules.get("claude_session_target", 90),
                                      rules.get("claude_session_ceiling", 95), probe["claude_throttled"])


def place(text, cfg, at, probe=None, *, overflow=True):
    probe = probe or {}
    rules = cfg.get("placement", {})
    item, native, needs = facets(text)
    repo = item.split("#", 1)[0].split()[0].lower() if item else ""
    private = bool(needs & {"private", "local-state"}) or repo in rules.get("private_repos", [])
    code_only = not native and not private
    hosts = cfg.get("hosts", {})
    box = {"brief": text}
    if overflow and code_only:
        over, reason = claude_out(at, rules, probe)
        if over:
            return "codex", reason if routing.codex_qualifies(box)[0] else f"{reason}; codex:sol-high on the tight leash"
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
            if "laptop" not in probe:
                probe["laptop"] = laptop_state()
            state = probe["laptop"]
            if state == "available":
                return target, "laptop available"
            laptop_why = f"laptop {state}"
            why.append(laptop_why)
        elif isinstance(hosts.get(target, {}).get("resident_cap"), int):
            cap = hosts[target]["resident_cap"]
            if target not in probe:
                probe[target] = host_load(target)
            online, live = probe[target]
            if online and live < cap:
                return target, f"{target} {live}/{cap}"
            why.append(f"{target} {live}/{cap}" if online else f"{target} offline")
    return f"held: {laptop_why or (why[0] if why else 'no target in [placement] order')}", "; ".join(why)


def release(reason, control, at):
    if reason == "short-window":
        return [routing.moment(w["resets_at"]) for w in control.get("windows", []) if w["window"] != "week"]
    if reason in (None, "floor", "blocked"):
        return []
    if reason == "workers":
        dwell = datetime.fromtimestamp(control.get("state_since", at.timestamp()), timezone.utc)
        return [max(at + timedelta(minutes=1), dwell + timedelta(minutes=DEFAULTS["pace_state_dwell_min"]))]
    return [at + timedelta(minutes=1)]


def place_spawn(brief_text, spawner, gate, cfg, now):
    at = routing.moment(now)
    stamp = at.timestamp()
    def field(key):
        match = re.search(rf"(?mi)^\s*{key}:\s*(.+)$", brief_text)
        return match.group(1).strip() if match else ""

    def queued(reason, eta):
        return {"kind": "queued", "reason": reason, "eta": eta.isoformat().replace("+00:00", "Z")}

    item, _, needs = facets(brief_text)
    project = item.split("#", 1)[0].split()[0].lower() if item else ""
    p = spawner["provider"]
    own = spawner.get("account", p)
    if ":" not in own and own != p:
        own = f"{p}:{own}"
    tiers = cfg.get("tiers", {})
    requested = field("Tier") or spawner.get("tier") or spawner.get("subagent_type", "")
    requested = next((k for k, t in tiers.items() if requested in (k, k.split(":", 1)[1], t.get("agent"))), requested)
    if requested not in tiers:
        requested = DEFAULTS["default_tier"]
    floor = tiers.get(requested, {}).get("start", 0)
    difficulty = max(float(field("Difficulty") or floor), floor)
    category = field("Category") or "tooling"
    _, _, providers = plan_usage.resolve_mode(cfg)
    accounts, controls, reasons, etas = {}, {}, {}, []
    for provider in providers:
        verdict = gate.get(provider, {})
        policy = cfg.get("accounts", {}).get(provider, {})
        if not verdict.get("controls") and policy.get("auto_route", True):
            reasons[provider] = "stale"
            etas.append(at + timedelta(minutes=1))
        for name, control in verdict.get("controls", {}).items():
            if not control.get("auto_route", policy.get("auto_route", True)):
                continue
            windows = {w["window"]: dict(w, headroom=100 - w["used_pct"],
                       reset_in=(routing.moment(w["resets_at"]) - at).total_seconds() / 3600,
                       window_h=168 if w["window"] == "week" else 5,
                       beta=w.get("smoothed_pct_h")) for w in control.get("windows", [])}
            accounts[name] = {"windows": windows, "provider": provider, "age_min": 0,
                              "target_left": policy.get("target_left", 5), "usd_per_week": 0}
            controls[name] = control
            billing = cfg.get("billing", {}).get(name, cfg.get("billing", {}).get(provider, {}))
            reason = control.get("held_reason")
            if billing.get("standard", "plan") != "plan":
                reason = "blocked"
            elif not pace_control.fresh(control, stamp) or "held_reason" not in control:
                reason = "stale"
            reasons[name] = reason
            etas.extend(release(reason, control, at))
    open_accounts = [n for n, reason in reasons.items() if reason is None]
    if not open_accounts:
        if reasons and all(r == "floor" for r in reasons.values()):
            return {"kind": "refused", "reason": "every eligible account at its floor"}
        return queued("accounts temporarily unavailable", min(etas) if etas else at + timedelta(minutes=1))

    margin = DEFAULTS["placement_drain_margin"]
    def draining(name):
        control = controls[name]
        week = next((w for w in control["windows"] if w["window"] == "week"), {})
        goal, burn = control.get("target_burn_pct_h"), week.get("smoothed_pct_h")
        return goal is not None and burn is not None and goal - burn > margin

    has_drain = any(draining(name) for name in open_accounts)
    def order(name):
        reset = routing.expiry(accounts[name])
        return (not draining(name), reset, name != own, name) if has_drain else (name != own, reset, name)

    def spawn_tier(names):
        available = {k: t for k, t in tiers.items() if k.split(":", 1)[0] in
                     {n.split(":", 1)[0] for n in names}}
        bands = plan_usage.resolve_bands(plan_usage.bands_of(tiers), available)
        starts = {key: lo for lo, hi, key in bands if hi > floor}
        choices = [dict(t, key=k, rank=i, start=starts.get(k, 100),
                        account=next(n for n in names if n.split(":", 1)[0] == k.split(":", 1)[0]))
                   for i, (k, t) in enumerate(available.items()) if k in starts]
        ctx = {"now": at, "accounts": accounts, "tiers": choices, "success": cfg.get("success", {}),
               "projects": {project: dict(cfg.get("projects", {}).get(project, {}), u=1)}}
        box = {"id": item, "project": project, "category": category, "difficulty": difficulty, "brief": brief_text,
               "sensitivity": "private" if needs & {"private", "local-state"} else "public"}
        return routing.choose(ctx, box, account_prices={n: i for i, n in enumerate(names)})["tier"]

    open_accounts.sort(key=order)
    rules = cfg.get("placement", {})
    local = resident_cap.host()
    if "probe" in rules:
        probe = {n: (False, 0) for n in cfg.get("hosts", {})}
        probe.update(laptop="unavailable", credit=(False, "cloud credit: no snapshot"))
        probe.update(claude_rows=[], claude_throttled=None)
        probe.update(rules["probe"])
        hosts = cfg.get("hosts", {})
    else:
        probe = {}
        hosts = dict(cfg.get("hosts", {}))
        hosts[local] = dict(hosts.get(local, {}), resident_cap=hosts.get(local, {}).get("resident_cap", sys.maxsize))
        rules = dict(rules, order=[local, "laptop"])
    probe.setdefault("claude_paces", (gate.get("claude") or {}).get("paces"))
    if needs & {"private", "local-state"} or project in rules.get("private_repos", []):
        rules = dict(rules, order=[h for h in rules.get("order", []) if h == local])
    candidates, host_reasons = [], []
    laptop_queue = None
    _, native, _ = facets(brief_text)
    code_only = not native and not (needs & {"private", "local-state"}) and project not in rules.get("private_repos", [])
    overflowed = code_only and claude_out(at, rules, probe)[0]
    while open_accounts:
        first = open_accounts[0]
        if first.split(":", 1)[0] == p and requested in tiers and requested.split(":", 1)[0] == p:
            name, key = first, requested
        else:
            chosen = spawn_tier([first])
            if chosen is None and overflowed and first.split(":", 1)[0] == "codex" and "codex:sol-high" in tiers:
                chosen = {"account": first, "key": "codex:sol-high"}
            if chosen is None:
                open_accounts.remove(first)
                continue
            name, key = chosen["account"], chosen["key"]
        open_accounts.remove(name)
        provider = name.split(":", 1)[0]
        cloud = "anthropic-cloud" if provider == "claude" else f"{provider}-cloud"
        host_cfg = dict(cfg, hosts=hosts,
                        placement=dict(rules, order=[h for h in rules.get("order", []) if h not in CLOUD or h == cloud]))
        host, why = place(brief_text, host_cfg, at, probe, overflow=(name == "claude:a"))
        if host == "codex" and name == "claude:a":
            host_reasons.append(f"claude:a overflow: {why}")
            continue
        if host.startswith("held:"):
            host_reasons.append(why or host)
            continue
        if host not in (local, "laptop"):
            host_reasons.append(f"unsupported launch host {host}: {why}")
            continue
        if host == "laptop":
            laptop_queue = dict(queued("laptop job: threads queue add laptop", at + timedelta(minutes=1)),
                                host="laptop")
            continue
        tier = tiers[key]
        candidates.append({"kind": "native" if name == own and host == local else "launch",
                           "provider": provider, "account": name, "tier": key,
                           "model": tier["model"], "effort": tier["effort"], "host": host})
    if candidates:
        return dict(candidates[0], candidates=candidates, eligible=list(reasons))
    if laptop_queue:
        return laptop_queue
    return queued("; ".join(host_reasons) or "no open tier admits the requested band", at + timedelta(minutes=1))


def admit(candidates, gate_path, *, eligible=None, session=None, task=None, now=None):
    path = Path(gate_path)
    at = routing.moment(now) if now is not None else datetime.now(timezone.utc)
    reasons, holds, etas = [], {}, []

    def held(name):
        provider = name.split(":", 1)[0]
        verdict = pace_sampler.load(path, {}).get(provider, {})
        control = (verdict.get("controls") or {}).get(name) or (verdict.get("controls") or {}).get(provider) or {}
        hold = control.get("held_reason", "stale") if pace_control.fresh(control, at.timestamp()) else "stale"
        holds[name] = hold
        return hold, control, verdict

    for candidate in candidates:
        provider, name = candidate["provider"], candidate["account"]
        hold, control, verdict = held(name)
        if hold:
            reasons.append(f"{name} controller held: {hold}")
            etas.extend(release(hold, control, at))
            continue
        account = (verdict.get("accounts") or {}).get(name) or (verdict.get("accounts") or {}).get(provider) or {}
        used, cap = account.get("used_pct"), account.get("cap_pct")
        expected = existing = DEFAULTS["reservation_fallback_points"]
        if used is not None and cap is not None:
            try:
                measured = usage.ledger_reservation(provider, candidate["model"], candidate["effort"],
                                                    label=name.partition(":")[2], details=True) or {}
            except (OSError, ValueError, RuntimeError, StopIteration):
                measured = {}
            expected = measured.get("expected") if measured.get("expected") is not None else expected
            existing = measured.get("existing") if measured.get("existing") is not None else existing
        reason = pace_sampler.reserve(path, provider, name, session=session, task=task, reservation=(expected, existing))
        if reason is None:
            return candidate
        reasons.append(reason)
        etas.append(at + timedelta(minutes=1))
    for name in eligible or []:
        if name not in holds:
            hold, control, _ = held(name)
            etas.extend(release(hold, control, at))
    if holds and all(reason == "floor" for reason in holds.values()):
        return {"kind": "refused", "reason": "every eligible account at its floor"}
    return {"kind": "queued", "reason": "; ".join(reasons) or "accounts temporarily unavailable",
            "eta": (min(etas) if etas else at + timedelta(minutes=1)).isoformat().replace("+00:00", "Z")}


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
