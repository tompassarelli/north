"""agents usage: run every usage_<provider>.py adapter beside this file in parallel (30 s each, 0 model turns),
record their rows in threads.db's usage table, and print the latest row per account window."""

import json
import os
import socket
import sqlite3
import subprocess
import sys
from datetime import datetime, timedelta, timezone
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from defaults import DEFAULTS
from calibration import open_ledger
from ssh_transport import ssh_argv
REMOTE = not os.environ.get("THREADS_DB") and (Path.home() / ".config/threads/remote").exists()
DB = os.environ.get("THREADS_DB") or str(Path.home() / ".local/state/threads" / ("snapshot.db" if REMOTE else "threads.db"))
STALE_MIN = DEFAULTS["usage_stale_min"]
COLUMNS = ("ts", "provider", "account", "kind", "name", "used_pct", "window_min", "resets_at", "amount", "source")


def operator_dir():
    configured = os.environ.get("AGENTS_OPERATOR_DIR")
    if configured:
        return Path(configured)
    return Path.home() / "code/south/main/agents"


def machines():
    """Provider hosts to query for signed-in account readings."""
    import tomllib
    try:
        catalog = tomllib.load(open(HERE.parent / "model-catalog.toml", "rb"))
    except (OSError, ValueError):
        return {}
    return {p: a["machines"] for p, a in catalog.get("accounts", {}).items() if isinstance(a.get("machines"), list)}


def account_settings(provider):
    import tomllib
    path = Path(os.environ.get("AGENTS_ORCHESTRATION") or operator_dir() / "orchestration.toml")
    try:
        cfg = tomllib.loads(path.read_text())
    except (OSError, ValueError):
        return {}
    return cfg.get("accounts", {}).get(provider, {})


def account_label(provider, account):
    return account_settings(provider).get("labels", {}).get(account)


def local_account(provider):
    if provider != "claude":
        return None
    import hashlib
    path = Path(os.environ.get("CLAUDE_CONFIG_DIR") or Path.home()) / ".claude.json"
    try:
        cache = json.loads(path.read_text())
    except (OSError, ValueError):
        return None
    uuid = (cache.get("oauthAccount") or {}).get("accountUuid") or (cache.get("cachedUsageUtilization") or {}).get("accountUuid")
    return account_label(provider, hashlib.sha256(uuid.encode()).hexdigest()[:8]) if uuid else None


def reservation_stop(used: float, cap: float, expected: float, in_flight: int, in_flight_expected=None) -> bool:
    import math
    projected = used + expected + (expected if in_flight_expected is None else in_flight_expected) * in_flight
    return used >= cap or (projected > cap and not math.isclose(projected, cap, rel_tol=0.0, abs_tol=1e-9))


def ledger_reservation(provider: str, model: str | None, effort: str | None, threads=None, label=None, details=False) -> tuple[float | None, float | None] | dict | None:
    from plan_usage import RUN_TIERS, accounts_from, costs_by_id, open_db
    import importlib.machinery
    import importlib.util
    import statistics
    import time
    import tomllib
    if threads is None:
        loader = importlib.machinery.SourceFileLoader("reservation_threads", str(HERE.parents[1] / "bin/threads"))
        spec = importlib.util.spec_from_loader(loader.name, loader)
        threads = importlib.util.module_from_spec(spec)
        loader.exec_module(threads)
    T = threads
    path = Path(T.DB if os.environ.get("THREADS_DB") else T.SNAPSHOT).resolve()
    if str(path) == str(Path(T.SNAPSHOT).resolve()):
        remote = Path(T.REMOTE)
        if remote.is_file():
            T.refresh_snapshot(remote.read_text().strip())
    if str(path) == str(Path(T.SNAPSHOT).resolve()) and (not path.exists() or time.time() - path.stat().st_mtime >= T.SNAPSHOT_SECONDS):
        raise RuntimeError("harness-tuner: cap reached: seven-day reservation unavailable: NorthThreads snapshot missing or stale")
    db = open_db(path)
    if db is None:
        return None if details else (None, None)
    try:
        db.row_factory = sqlite3.Row
        prices = costs_by_id(tomllib.loads((operator_dir() / "costs.toml").read_text()))
        accounts = [name for name in prices if name == provider or name.startswith(provider + "-")]
        label = local_account(provider) if label is None else label
        if provider == "codex" and not label:
            now = datetime.now(timezone.utc)
            recent = [r for r in latest(db) if datetime.fromisoformat(r["ts"].replace("Z", "+00:00")) >= now - timedelta(days=7)]
            matched = [a for name, a in accounts_from(recent, db, now).items()
                       if name.split(":", 1)[0] == "codex"]
            if len(matched) > 1:
                raise RuntimeError("harness-tuner: cap reached: ambiguous codex account "
                                   + ", ".join(sorted(str(a["id"]) for a in matched)))
            label = matched[0]["id"] if matched else label
        account = provider + "-" + label if provider == "claude" and label else accounts[0] if len(accounts) == 1 else provider
        catalog = tomllib.loads((HERE.parent / "model-catalog.toml").read_text())
        tiers = {RUN_TIERS.get(key, f"{model} {effort}") for key, value in catalog["models"].items()
                 if key.startswith(provider + ":") and (effort is None or value.get("effort") == effort)
                 and (model is None or model in (value.get("model"), key.split(":", 1)[1].split("-", 1)[0]))}
        provider_tiers = {RUN_TIERS.get(key, f"{value['model']} {value['effort']}")
                          for key, value in catalog["models"].items() if key.startswith(provider + ":")}
        calibrated = [r for r in T.calibration_runs(db) if r["tier"] in provider_tiers]
        runs = [r for r in calibrated if r["tier"] in tiers]
        points = [r["cost_usd"] * 100 / prices[account] for r in runs
                  if r["tier"] in tiers and r["cost_usd"] is not None and r["cost_usd"] > 0 and prices.get(account, 0) > 0]
        expected = statistics.mean(points) if points else None
        by_tier = {}
        for r in calibrated:
            if r["cost_usd"] is not None and r["cost_usd"] > 0 and prices.get(account, 0) > 0:
                by_tier.setdefault(r["tier"], []).append(r["cost_usd"] * 100 / prices[account])
        existing = max((statistics.mean(values) for values in by_tier.values()), default=None)
        if model is None and effort is None and points:
            expected = existing
        reading = None
        if db.execute("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'usage'").fetchone():
            meter = db.execute("SELECT used_pct FROM usage WHERE provider = ? AND kind = 'window'"
                               " AND name IN ('week', 'weekly_all') AND COALESCE(account, '') = ?"
                               " AND used_pct IS NOT NULL ORDER BY ts DESC LIMIT 1",
                               (provider, label or "")).fetchone()
            reading = meter[0] if meter else None
        if details:
            walls = [r["minutes"] * 60 for r in runs if r["minutes"] is not None and r["minutes"] > 0]
            return {"expected": expected, "existing": existing, "reading": reading,
                    "p75_points": T.quantile(points, .75) if points else None,
                    "p75_s": T.quantile(walls, .75) if walls else None,
                    "source": "T.calibration_runs: tier cost_usd / weekly plan price and minutes"}
        return expected, reading
    except (OSError, ValueError, sqlite3.Error):
        return None if details else (None, None)
    finally:
        db.close()



def remaining_p75(measurement, elapsed):
    wall, points = measurement.get("p75_s"), measurement.get("p75_points")
    if wall is None or points is None or elapsed >= wall:
        return None
    return points * max(0.0, 1.0 - elapsed / wall)


def hard_stop_due(deadline, now, reading, cap):
    return now >= deadline or reading >= cap + 1


def read_all(remote=True):
    """Local adapters, plus each labelled account signed in on another machine, read there over ssh."""
    procs = {p.stem.removeprefix("usage_"): subprocess.Popen([sys.executable, "-I", str(p)], stdout=subprocess.PIPE,
                                                            stderr=subprocess.DEVNULL, text=True)
             for p in sorted([*HERE.glob("usage_*.py"), *HERE.glob("billing_*.py")])}
    hosts = machines()
    here = socket.gethostname()
    remotes = {(p, h): subprocess.Popen(ssh_argv("-o", "BatchMode=yes", "-o", "ConnectTimeout=5", h, "agents", "usage",
                                               "--refresh", "--local", "--json"), stdout=subprocess.PIPE,
                                        stderr=subprocess.DEVNULL, text=True)
               for p, m in hosts.items() for h in m if h != here and remote}
    rows = []
    for (p, h), proc in remotes.items():
        try:
            out, _ = proc.communicate(timeout=45)
            remote_rows = json.loads(out or "[]")
        except (subprocess.TimeoutExpired, ValueError):
            proc.kill()
            continue
        rows += [{c: r.get(c) for c in COLUMNS} for r in remote_rows if r.get("provider") == p]
    for name, proc in procs.items():
        try:
            out, _ = proc.communicate(timeout=30)
        except subprocess.TimeoutExpired:
            proc.kill()
            print(f"agents usage: {name} adapter timed out after 30 s", file=sys.stderr)
            continue
        for line in out.splitlines():
            try:
                row = json.loads(line)
            except ValueError:
                continue
            rows.append(row)
    return rows


def record(db, rows):
    db.execute(f"CREATE TABLE IF NOT EXISTS usage({', '.join(c + (' REAL' if c in ('used_pct', 'amount') else ' INTEGER' if c == 'window_min' else ' TEXT') for c in COLUMNS)}) STRICT")
    db.execute("CREATE INDEX IF NOT EXISTS usage_latest ON usage(provider, account, kind, name, ts)")
    db.executemany(f"INSERT INTO usage({', '.join(COLUMNS)}) VALUES ({', '.join('?' * len(COLUMNS))})",
                   [tuple(r.get(c) for c in COLUMNS) for r in rows])
    db.commit()


def open_readout_db():
    if not REMOTE:
        return sqlite3.connect(DB, timeout=10)
    db = sqlite3.connect(":memory:")
    path = Path(DB)
    if path.exists():
        with open_ledger(path) as source:
            source.backup(db)
    return db


def publish(db, rows):
    if REMOTE and rows:
        payload = "".join(json.dumps(dict(r, type="usage")) + "\n" for r in rows)
        command = HERE.parent.parent / "bin/threads"
        subprocess.run([str(command), "ingest"], input=payload, text=True, check=True)
    record(db, rows)


def hosted_claude():
    return set(account_settings("claude").get("profiles", {}))


def latest(db):
    if not db.execute("SELECT 1 FROM sqlite_master WHERE name = 'usage'").fetchone():
        return []
    db.row_factory = sqlite3.Row
    hosted = hosted_claude()
    return [dict(r) for r in db.execute(
        "SELECT * FROM usage u WHERE rowid = (SELECT rowid FROM usage WHERE provider = u.provider AND account = u.account"
        " AND kind = u.kind AND name = u.name ORDER BY ts DESC, rowid DESC LIMIT 1)"
        " ORDER BY provider, account, kind DESC, window_min, name")
            if r["provider"] != "claude" or not hosted or r["account"] in hosted]


def age_min(ts, now):
    return (now - datetime.fromisoformat(ts.replace("Z", "+00:00"))).total_seconds() / 60 if ts else None


def until(ts, now):
    if not ts:
        return "-"
    minutes = int((datetime.fromisoformat(ts.replace("Z", "+00:00")) - now).total_seconds() // 60)
    return f"{minutes // 1440}d{minutes % 1440 // 60}h" if minutes >= 1440 else f"{minutes // 60}h{minutes % 60:02d}m"


def burn(db, mine, now):
    """Weekly window's %/h from plan_usage.beta and when it reaches 100%, or why that is unknown."""
    import plan_usage
    r = next((r for r in mine if r["kind"] == "window" and r["name"] in ("week", "weekly_all")
              and r["used_pct"] is not None and r["resets_at"] and r["window_min"]), None)
    if r is None:
        return "burn unknown (no weekly window)"
    n = db.execute("SELECT count(*) FROM usage WHERE provider = ? AND account = ? AND name = ? AND kind = 'window'"
                   " AND abs(julianday(resets_at) - julianday(?)) * 86400 < 120",
                   (r["provider"], r["account"], r["name"], r["resets_at"])).fetchone()[0]
    if n < 2:
        return f"burn unknown ({n} reading{'s' if n != 1 else ''})"
    rate = plan_usage.beta(db, r, now)
    if rate is None:
        return "burn unknown (need two readings in last 3h)"
    reset = plan_usage.ts(r["resets_at"])
    if rate <= 0:
        return f"burn 0.0%/h, lasts to reset ({until(r['resets_at'], now)})"
    empty = now + timedelta(hours=max(100 - r["used_pct"], 0) / rate)
    if empty >= reset:
        return f"burn {rate:.1f}%/h, lasts to reset ({until(r['resets_at'], now)})"
    stamp = empty.strftime("%Y-%m-%dT%H:%M:%SZ")
    return f"burn {rate:.1f}%/h, empty in {until(stamp, now)} ({empty.day} {empty:%b %H:%M}Z)"


def gate_path():
    return Path(os.environ.get("XDG_STATE_HOME") or Path.home() / ".local/state") / "agents/usage-gate.json"


def refusal(db, provider, cfg, catalog, now):
    """Why a new worker on this provider's account could bill money (or its usage is unknown), else None.
    `north pace` decides; this only feeds it."""
    import routing
    import plan_usage
    rows = [r for r in latest(db) if r["provider"] == provider]
    ctx = plan_usage.live_ctx(cfg, catalog, rows, db, now, cfg.get("tiers", {}))
    if provider not in ctx["accounts"] and local_account(provider):
        provider = f"{provider}:{local_account(provider)}"
    why = routing.ask("money_refusal", ctx=ctx, account=provider)
    return why and (f"No: a new {provider} worker could spend money: {why}. Unknown or money-billed usage is never chosen "
            "automatically (firn#12). Use another provider's tier from `agents plan`, wait for the window to reset, or "
            "report `Blocked: decision for Tom: <question>` for the proxy to put to Tom.")


def write_gate(db, providers, pace_locked=False):
    """Record each provider's refusal (or null) and pace reading in usage-gate.json, which the spawn hook reads without a subprocess."""
    import tomllib
    sys.path.insert(0, str(HERE))
    root = HERE.parent
    cfg = tomllib.load(open(os.environ.get("AGENTS_ORCHESTRATION") or operator_dir() / "orchestration.toml", "rb"))
    catalog = tomllib.load(open(root / "model-catalog.toml", "rb"))
    now = datetime.now(timezone.utc)
    path = gate_path()
    if not pace_locked:
        import fcntl
        path.parent.mkdir(parents=True, exist_ok=True)
        with (path.parent / "pace-control.lock").open("a") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            return write_gate(db, providers, pace_locked=True)
    try:
        verdicts = json.loads(path.read_text())
    except (OSError, ValueError):
        verdicts = {}
    import routing
    import plan_usage
    for provider in providers:
        ctx = plan_usage.live_ctx(cfg, catalog, [r for r in latest(db) if r["provider"] == provider], db, now,
                                  cfg.get("tiers", {}))
        verdicts[provider] = {"ts": now.strftime("%Y-%m-%dT%H:%M:%SZ"), "epoch": now.timestamp(), "stale_min": STALE_MIN,
                              "pace_control": True,
                              "controls": verdicts.get(provider, {}).get("controls", {}),
                              "refusal": refusal(db, provider, cfg, catalog, now),
                              "projections": {(f"claude:{a['id']}" if provider == "claude" else n): [{"window": kind, "used_pct": 100 - w["headroom"],
                                                     "quota_ceiling": min(DEFAULTS["plan_stop"], 100 - a["target_left"]),
                                                     "burn_pct_h": w["beta"], "reset_h": w["reset_in"],
                                                     "age_s": (now - plan_usage.ts(w["as_of"])).total_seconds(),
                                                     "as_of": w["as_of"], "resets_at": w["resets_at"],
                                                     "projected_pct": (100 - w["headroom"] + w["beta"] * w["reset_in"]
                                                                       if w["beta"] is not None else None)}
                                                    for kind, w in a["windows"].items()]
                                              for n, a in ctx["accounts"].items() if n.split(":", 1)[0] == provider},
                              **routing.ask("gate", ctx=ctx, provider=provider)}
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_suffix(".tmp")
    tmp.write_text(json.dumps(verdicts))
    tmp.replace(path)
    return verdicts


def notify_promos(db):
    """One desktop notification per promotion, from 7 days before it ends (plan_usage.promo_lines decides)."""
    import tomllib
    import plan_usage
    catalog = tomllib.load(open(HERE.parent / "model-catalog.toml", "rb"))
    sent = Path(os.environ.get("XDG_STATE_HOME") or Path.home() / ".local/state") / "agents/promo-notified"
    for line in plan_usage.promo_lines(catalog, db, datetime.now(timezone.utc)):
        stamp = sent / (line.split(" promotion", 1)[0].removeprefix("Blocked: decision for Tom: ") + "-" + line.split(" ends ", 1)[1].split()[0])
        if stamp.exists():
            continue
        stamp.parent.mkdir(parents=True, exist_ok=True)
        subprocess.run(["notify-send", "Plan promotion ending", line], check=False)
        stamp.write_text(line + "\n")


def burn_policy(provider, now=None):
    import math
    import re
    import tomllib
    try:
        with open(os.environ.get("AGENTS_ORCHESTRATION") or operator_dir() / "orchestration.toml", "rb") as source:
            entry = tomllib.load(source).get("burn", {}).get(provider)
        if not isinstance(entry, dict):
            return None
        until = datetime.fromisoformat(entry["until"].replace("Z", "+00:00"))
        if not (now or datetime.now(timezone.utc)) < until:
            return None
        if provider != "codex" or set(entry) != {"until", "rate"}:
            return None
        match = re.fullmatch(r"(\d+(?:\.\d+)?)%/h", entry["rate"])
        rate = float(match[1]) if match else 0
        return dict(entry, target=rate) if math.isfinite(rate) and rate > 0 else None
    except (OSError, ValueError, TypeError, KeyError, AttributeError):
        return None


BURN_WINDOW_SECONDS = 600


def burn_state_path():
    return Path(os.environ.get("XDG_STATE_HOME") or Path.home() / ".local/state") / "agents/burn.json"


def burn_state():
    try:
        state = json.loads(burn_state_path().read_text())
        return state if isinstance(state, dict) else {}
    except (OSError, ValueError):
        return {}


def burn_level(entry, state):
    allowance = state.get("parallel", 2)
    return allowance if state.get("policy") == entry and type(allowance) is int and allowance >= 2 else 2


def burn_override(provider):
    entry = burn_policy(provider)
    if entry:
        return entry | {"parallel": burn_level(entry, burn_state().get(provider, {})),
                        "model": "gpt-6.1-sol", "effort": "high", "fast": False}
    return None


def burn_decide(allowance, measured, target):
    if measured is None:
        return allowance
    if measured > target * 1.2:
        return max(allowance - 1, 2)
    if measured < target:
        return allowance + 1
    return allowance


def burn_measure(db, provider, now):
    import math
    try:
        rows = [r for r in latest(db) if r["provider"] == provider and r["kind"] == "window"
                and r["name"] in ("week", "weekly_all") and r["used_pct"] is not None
                and r["resets_at"] and r["window_min"]]
        account = local_account(provider)
        rows = [r for r in rows if r["account"] == account] if account else rows
        if len(rows) != 1:
            return None
        row = rows[0]
        end = datetime.fromisoformat(row["ts"].replace("Z", "+00:00"))
        reset = datetime.fromisoformat(row["resets_at"].replace("Z", "+00:00"))
        if not 0 <= (now - end).total_seconds() <= 240 or reset <= now:
            return None
        candidates = []
        for old in db.execute("SELECT ts, used_pct, resets_at FROM usage WHERE provider = ? AND account = ?"
                              " AND kind = 'window' AND name = ? AND ts < ? AND used_pct IS NOT NULL",
                              (provider, row["account"], row["name"], row["ts"])):
            old_at = datetime.fromisoformat(old["ts"].replace("Z", "+00:00"))
            old_reset = datetime.fromisoformat(old["resets_at"].replace("Z", "+00:00"))
            seconds = (end - old_at).total_seconds()
            if 480 <= seconds <= 720 and abs((reset - old_reset).total_seconds()) <= 60:
                candidates.append((abs(seconds - BURN_WINDOW_SECONDS), seconds, old_at, old["used_pct"]))
        if not candidates:
            return None
        _, seconds, begin, before = min(candidates)
        measured = (row["used_pct"] - before) * 3600 / seconds
        if not math.isfinite(measured) or measured < 0:
            return None
        return {"measured": measured, "sample": [row["account"], row["ts"]], "seconds": seconds,
                "window": f"{row['name']} {seconds / 60:.2f}min snapshot-to-snapshot ending {row['ts']}"}
    except (ValueError, TypeError, KeyError, OverflowError):
        return None


def burn_step(db, provider, now=None):
    import fcntl
    now = now or datetime.now(timezone.utc)
    entry = burn_policy(provider, now)
    path = burn_state_path()
    if not entry and not path.exists():
        return []
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.with_suffix(".lock").open("w") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        states = burn_state()
        old = states.get(provider, {})
        if not entry:
            if provider not in states:
                return []
            del states[provider]
            lines = [f"burn {provider}: advisory active -> inactive; measured unknown target inactive; window inactive"]
        else:
            allowance = burn_level(entry, old)
            reading = burn_measure(db, provider, now)
            same_policy = old.get("policy") == entry
            if reading is None or (same_policy and old.get("sample") == reading["sample"]):
                return []
            new = burn_decide(allowance, reading["measured"], entry["target"])
            if new > allowance and same_policy and now.timestamp() - old.get("at", 0) < BURN_WINDOW_SECONDS:
                return []
            states[provider] = {"policy": entry, "parallel": new, "sample": reading["sample"],
                                "at": now.timestamp() if new != allowance else old.get("at", 0)}
            lines = []
            if new != allowance:
                lines.append(f"burn {provider}: parallel {allowance} -> {new}; measured {reading['measured']:.2f}%/h "
                             f"target {entry['target']:g}%/h; window {reading['window']}; "
                             "advisory workers per sub-lead; sole worker ceiling: pace-enforcement per-account allowed_workers")
        temporary = path.with_suffix(".tmp")
        temporary.write_text(json.dumps(states) + "\n")
        temporary.replace(path)
        return lines


def burn_report(db, provider, now=None):
    now = now or datetime.now(timezone.utc)
    entry = burn_policy(provider, now)
    reading = burn_measure(db, provider, now) if db is not None else None
    measured = f"{reading['measured']:.2f}%/h" if reading else "unknown (missing/stale short-window snapshots)"
    target = f"{entry['target']:g}%/h" if entry else "inactive"
    window = reading["window"] if reading else "unavailable"
    allowance = burn_level(entry, burn_state().get(provider, {})) if entry else 2
    return f"burn {provider}: parallel {allowance} -> {allowance}; measured {measured} target {target}; window {window}; read-only"


def fast(db, provider, domain, cached=False):
    """Exit 0 printing "on" when routing turns plan-billed fast on for a new lead on this provider while under pace;
    exit 1 printing "off" otherwise. Refreshes the provider's usage first unless
    cached, when a snapshot older than stale_min reads as off."""
    override = burn_override(provider)
    if override and (override["fast"] or "target" in override):
        print("on" if override["fast"] else "off")
        return 0 if override["fast"] else 1
    import tomllib
    sys.path.insert(0, str(HERE))
    import routing
    import plan_usage
    if not cached:
        publish(db, [r for r in read_all(remote=False) if r.get("provider") == provider])
    root = HERE.parent
    cfg = tomllib.load(open(os.environ.get("AGENTS_ORCHESTRATION") or operator_dir() / "orchestration.toml", "rb"))
    catalog = tomllib.load(open(root / "model-catalog.toml", "rb"))
    rows = [r for r in latest(db) if r["provider"] == provider]
    ctx = plan_usage.live_ctx(cfg, catalog, rows, db, datetime.now(timezone.utc), cfg.get("tiers", {}))
    name = provider if provider in ctx["accounts"] or not local_account(provider) else f"{provider}:{local_account(provider)}"
    on = routing.ask("fast", ctx=ctx, account=name)
    print("on" if on else "off")
    return 0 if on else 1


def costs_dir():
    return Path(os.environ.get("XDG_STATE_HOME") or Path.home() / ".local/state") / "agents/costs"


def cost_lines(now):
    """One line per non-AI service of the operator's costs.toml from its costs/<id>.json readout, then the weekly total."""
    import tomllib
    try:
        services = tomllib.load(open(operator_dir() / "costs.toml", "rb")).get("service", [])
    except (OSError, ValueError):
        return []
    lines, metered = [], 0.0
    for s in services:
        if s.get("billing") == "prepaid" and "initial" in s:
            try:
                spent = float(json.loads((costs_dir() / f"{s['id']}.json").read_text())["usd_spent_period"])
            except (OSError, ValueError, KeyError, TypeError):
                spent = None
            expires = datetime.fromisoformat(str(s["expires"]).replace("Z", "+00:00"))
            left = ("$0.00" if now >= expires else "no readout, unknown" if spent is None
                    else f"${max(s['initial'] - spent, 0.0):.2f}")
            lines.append(f"{s['id']} ({s['kind']}, prepaid): {left} left of ${s['initial']:.2f}, "
                         f"expires {expires:%Y-%m-%dT%H:%MZ} (readout: {s.get('readout', '?')})")
            continue
        if s.get("kind") == "ai-plan":
            continue
        try:
            r = json.loads((costs_dir() / f"{s['id']}.json").read_text())
        except (OSError, ValueError):
            lines.append(f"{s['id']} ({s['kind']}, {s['billing']}): no readout yet")
            continue
        start = datetime.fromisoformat(r["period_start"].replace("Z", "+00:00"))
        end = datetime.fromisoformat(r["period_end"].replace("Z", "+00:00"))
        read = datetime.fromisoformat(r["read_at"].replace("Z", "+00:00")) if r.get("read_at") else now
        days = max((min(read, end) - start).total_seconds() / 86400, 1)
        if s["billing"] != "fixed":
            metered += r["usd_spent_period"] * 7 / days
        line = f"{s['id']} ({s['kind']}, {s['billing']}): ${r['usd_spent_period']:.2f} spent {start:%Y-%m-%d}..{end:%Y-%m-%d}"
        if r.get("usd_balance") is not None:
            line += f", balance ${r['usd_balance']:.2f}"
        if r.get("credits_left") is not None:
            line += f", credits left {r['credits_left']:g}"
        age = age_min(r.get("read_at"), now)
        lines.append(line + f" ({r.get('source', '?')}" + (f", {age:.0f} min ago)" if age is not None else ")"))
    fixed = sum(s.get("usd_per_week", 0.0) for s in services)
    lines.append(f"weekly operating ${fixed + metered:.2f} = fixed ${fixed:.2f} + metered ${metered:.2f}")
    return lines


def main(argv):
    if "--reservation-json" in argv:
        print(json.dumps(ledger_reservation(**json.load(sys.stdin), details=True)))
        return 0
    if "--burn" in argv:
        entry = burn_override(argv[argv.index("--burn") + 1])
        if entry:
            print(entry["model"], entry["effort"])
            return 0
        return 1
    if "--burn-report" in argv:
        try:
            with sqlite3.connect(Path(DB).resolve().as_uri() + "?mode=ro", uri=True, timeout=10) as db:
                print(burn_report(db, argv[argv.index("--burn-report") + 1]))
        except sqlite3.Error:
            print(burn_report(None, argv[argv.index("--burn-report") + 1]))
        return 0
    try:
        db = open_readout_db()
    except sqlite3.DatabaseError as error:
        if "snapshot still unreadable after one refresh" not in str(error):
            print(f"agents usage: {error}", file=sys.stderr)
        return 1
    if "--write-pace-gate" in argv:
        print(json.dumps(write_gate(db, ("claude", "codex", "gemini"), pace_locked=True)))
        return 0
    if "--burn-step" in argv or "--burn-tick" in argv:
        option = "--burn-step" if "--burn-step" in argv else "--burn-tick"
        provider = argv[argv.index(option) + 1]
        lines = burn_step(db, provider)
        if lines:
            print("\n".join(lines))
        return 0

    if "--gate" in argv:
        return subprocess.run([os.environ.get("NORTH_PACE_BIN", "north"), "pace", "gate", argv[argv.index("--gate") + 1]]).returncode
    if "--fast" in argv:
        return fast(db, argv[argv.index("--fast") + 1], argv[argv.index("--domain") + 1] if "--domain" in argv else None,
                    "--cached" in argv)
    if "--refresh" in argv:
        publish(db, read_all(remote="--local" not in argv))
        write_gate(db, sorted({r["provider"] for r in latest(db)}))
        notify_promos(db)
    now = datetime.now(timezone.utc)
    rows = latest(db)
    for r in rows:
        r["stale"] = (age_min(r["ts"], now) or STALE_MIN + 1) > STALE_MIN
    if "--json" in argv:
        print(json.dumps(rows))
        return 0
    if not rows:
        print("agents usage: no snapshot yet; run `agents usage --refresh`")
        print("\n".join(cost_lines(now)))
        return 1
    sys.path.insert(0, str(HERE))
    for (provider, account), group in sorted({(r["provider"], r["account"]): None for r in rows}.items()):
        mine = [r for r in rows if (r["provider"], r["account"]) == (provider, account)]
        age = min(age_min(r["ts"], now) for r in mine)
        windows = [f"{r['name']} {r['used_pct']:.0f}% used, resets in {until(r['resets_at'], now)}"
                   for r in mine if r["kind"] == "window" and r["used_pct"] is not None]
        reset_name = {"codex": "rateLimitResetCredits", "claude": "cedar_ember"}.get(provider)
        reset_row = next((r for r in mine if r["name"] == reset_name and r["kind"] in ("billing", "grant")), None)
        reset_kinds = []
        if provider == "claude" and (reset_row is None or reset_row["amount"] is None):
            sys.path.insert(0, str(HERE))
            import usage_claude
            reset_kinds = usage_claude.web_rows(account, now.isoformat(), labelled=True)
        if reset_row is not None and reset_row["amount"] is not None:
            source = ("codex app-server rateLimitResetCredits.availableCount" if provider == "codex"
                      else reset_row["source"])
            resets = f"resets={reset_row['amount']:g} ({source})"
        elif reset_kinds:
            kinds = [f"{'full' if r['name'].endswith('/full') else '5h'}={r['amount']:g}"
                     + (f" (expires {r['resets_at']})" if r["resets_at"] else "")
                     for r in sorted(reset_kinds, key=lambda r: r["name"] != "cedar_ember/full")]
            resets = "resets " + " ".join(kinds) + f" ({reset_kinds[0]['source']})"
        else:
            why = ("no web reset reading for this account"
                   if provider == "claude" else "codex app-server rateLimitResetCredits.availableCount unavailable"
                   if provider == "codex" else "provider adapter has no reset availability")
            resets = f"resets=unknown ({why})"
        billing = [f"{r['name']}={r['amount']:g}" for r in mine if r["kind"] in ("billing", "grant")
                   and r["amount"] is not None and r["name"] not in (reset_name, "plan") and not r["name"].startswith("cedar_ember/")]
        observed_plan = next((r["source"] for r in sorted(mine, key=lambda r: r["ts"], reverse=True)
                              if r["kind"] == "billing" and r["name"] == "plan"), None)
        print(f"{provider} {account} ({age:.0f} min ago{', STALE' if age > STALE_MIN else ''}): "
              + "; ".join(windows) + (f" | {', '.join(billing)}" if billing else "") + f" | {resets} | {burn(db, mine, now)}"
              + plan_text(provider, account, observed_plan))
    for line in cost_lines(now):
        print(line)
    return 0


def plan_text(provider, account, observed=None):
    import tomllib
    key = observed or account_settings(provider).get("plans", {}).get(account)
    if not key:
        return ""
    catalog = tomllib.loads((HERE.parent / "model-catalog.toml").read_text())
    plan = catalog.get("accounts", {}).get(provider, {}).get("plans", {}).get(key, {})
    return f" | plan {plan['name']} ${plan['monthly_price']}/month" if plan else f" | plan {key}"


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
