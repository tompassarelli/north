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
REMOTE = not os.environ.get("THREADS_DB") and (Path.home() / ".config/threads/remote").exists()
DB = os.environ.get("THREADS_DB") or str(Path.home() / ".local/state/threads" / ("snapshot.db" if REMOTE else "threads.db"))
STALE_MIN = 30
COLUMNS = ("ts", "provider", "account", "kind", "name", "used_pct", "window_min", "resets_at", "amount", "source")


def operator_dir():
    configured = os.environ.get("AGENTS_OPERATOR_DIR")
    if configured:
        return Path(configured)
    return Path.home() / "code/south/main/agents"


def machines():
    """provider -> {host: account label} from the catalog, e.g. claude a on whiterabbit and b on nexus."""
    import tomllib
    try:
        catalog = tomllib.load(open(HERE.parent / "model-catalog.toml", "rb"))
    except (OSError, ValueError):
        return {}
    return {p: a["machines"] for p, a in catalog.get("accounts", {}).items() if isinstance(a.get("machines"), dict)}


def local_account(provider):
    return machines().get(provider, {}).get(socket.gethostname())


def read_all(remote=True):
    """Local adapters, plus each labelled account signed in on another machine, read there over ssh."""
    procs = {p.stem.removeprefix("usage_"): subprocess.Popen([sys.executable, "-I", str(p)], stdout=subprocess.PIPE,
                                                            stderr=subprocess.DEVNULL, text=True)
             for p in sorted([*HERE.glob("usage_*.py"), *HERE.glob("billing_*.py")])}
    hosts = machines()
    here = socket.gethostname()
    remotes = {(p, h): subprocess.Popen(["ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=5", h, "agents", "usage",
                                         "--refresh", "--local", "--json"], stdout=subprocess.PIPE,
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
        rows += [{c: r.get(c) for c in COLUMNS} | {"account": hosts[p][h]} for r in remote_rows
                 if r.get("provider") == p and r.get("account") == hosts[p][h]]
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
            label = hosts.get(row.get("provider"), {}).get(here)
            rows.append(row | {"account": label} if label else row)
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
        with sqlite3.connect(path.as_uri() + "?mode=ro", uri=True, timeout=10) as source:
            source.backup(db)
    return db


def publish(db, rows):
    if REMOTE and rows:
        payload = "".join(json.dumps(dict(r, type="usage")) + "\n" for r in rows)
        command = HERE.parent.parent / "bin/threads"
        subprocess.run([str(command), "ingest"], input=payload, text=True, check=True)
    record(db, rows)


def latest(db):
    if not db.execute("SELECT 1 FROM sqlite_master WHERE name = 'usage'").fetchone():
        return []
    db.row_factory = sqlite3.Row
    return [dict(r) for r in db.execute(
        "SELECT * FROM usage u WHERE rowid = (SELECT rowid FROM usage WHERE provider = u.provider AND account = u.account"
        " AND kind = u.kind AND name = u.name ORDER BY ts DESC, rowid DESC LIMIT 1)"
        " ORDER BY provider, account, kind DESC, window_min, name")]


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
    routing.py decides; this only feeds it."""
    import math
    import plan_usage
    import routing
    rows = [r for r in latest(db) if r["provider"] == provider]
    ctx = plan_usage.live_ctx(cfg, catalog, rows, db, now, cfg.get("tiers", {}))
    if provider not in ctx["accounts"] and local_account(provider):
        provider = f"{provider}:{local_account(provider)}"
    acct = ctx["accounts"].get(provider)
    if acct is not None and routing.price(ctx, provider) != math.inf:
        return None
    if acct is None:
        why = f"no usage snapshot for {provider} could be read (`agents usage --refresh` shows why)"
    elif routing.stale(acct):
        why = f"{provider}'s usage snapshot is {acct['age_min']:.0f} minutes old even after a refresh"
    elif routing.billing(ctx, provider, "standard") != "plan":
        why = f"the catalog bills {provider}'s standard usage as {routing.billing(ctx, provider, 'standard')}"
    else:
        live = [f for f in ("hasCredits", "extra_usage", "spend") if acct.get(f)]
        why = (f"{provider} is at {routing.used(acct):.0f}% of a plan window (ceiling {routing.MONEY_CEILING}%) "
               f"and {', '.join(live) or 'its overflow'} would bill money past it")
    return (f"No: a new {provider} worker could spend money: {why}. Unknown or money-billed usage is never chosen "
            "automatically (firn#12). Use another provider's tier from `agents plan`, wait for the window to reset, or "
            "report `Blocked: decision for Tom: <question>` for the proxy to put to Tom.")


def write_gate(db, providers):
    """Record each provider's refusal (or null) and pace reading in usage-gate.json, which the spawn hook reads without a subprocess."""
    import tomllib
    sys.path.insert(0, str(HERE))
    root = HERE.parent
    cfg = tomllib.load(open(os.environ.get("AGENTS_ORCHESTRATION") or operator_dir() / "orchestration.toml", "rb"))
    catalog = tomllib.load(open(root / "model-catalog.toml", "rb"))
    now = datetime.now(timezone.utc)
    path = gate_path()
    try:
        verdicts = json.loads(path.read_text())
    except (OSError, ValueError):
        verdicts = {}
    import plan_usage
    import routing
    for provider in providers:
        ctx = plan_usage.live_ctx(cfg, catalog, [r for r in latest(db) if r["provider"] == provider], db, now,
                                  cfg.get("tiers", {}))
        verdicts[provider] = {"ts": now.strftime("%Y-%m-%dT%H:%M:%SZ"), "epoch": now.timestamp(), "stale_min": STALE_MIN,
                              "refusal": refusal(db, provider, cfg, catalog, now),
                              "pace": routing.pace_reading(ctx, provider)}
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


def gate(db, provider):
    """Exit 3 with the refusal when a spawn on this provider's account could bill money, 0 otherwise; refreshes the
    provider's usage first so the verdict is current."""
    publish(db, [r for r in read_all(remote=False) if r.get("provider") == provider])
    reason = write_gate(db, [provider])[provider]["refusal"]
    if reason:
        print(reason)
        return 3
    return 0


def fast(db, provider, domain, cached=False):
    """Exit 0 printing "on" when routing turns plan-billed fast on for a new lead on this provider while under pace;
    exit 1 printing "off" otherwise. Refreshes the provider's usage first unless
    cached, when a snapshot older than stale_min reads as off."""
    import tomllib
    sys.path.insert(0, str(HERE))
    import plan_usage
    import routing
    if not cached:
        publish(db, [r for r in read_all(remote=False) if r.get("provider") == provider])
    root = HERE.parent
    cfg = tomllib.load(open(os.environ.get("AGENTS_ORCHESTRATION") or operator_dir() / "orchestration.toml", "rb"))
    catalog = tomllib.load(open(root / "model-catalog.toml", "rb"))
    rows = [r for r in latest(db) if r["provider"] == provider]
    ctx = plan_usage.live_ctx(cfg, catalog, rows, db, datetime.now(timezone.utc), cfg.get("tiers", {}))
    name = provider if provider in ctx["accounts"] or not local_account(provider) else f"{provider}:{local_account(provider)}"
    on = routing.fast(ctx, name, ctx["projects"].get(domain or "", {}).get("u", 1.0))
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
                spent = json.loads((costs_dir() / f"{s['id']}.json").read_text())["usd_spent_period"]
            except (OSError, ValueError, KeyError):
                spent = 0.0
            expires = datetime.fromisoformat(str(s["expires"]).replace("Z", "+00:00"))
            left = 0.0 if now >= expires else max(s["initial"] - spent, 0.0)
            lines.append(f"{s['id']} ({s['kind']}, prepaid): ${left:.2f} left of ${s['initial']:.2f}, "
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
        days = max((end - start).total_seconds() / 86400, 1)
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
    db = open_readout_db()
    if "--gate" in argv:
        return gate(db, argv[argv.index("--gate") + 1])
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
            why = ("Chrome is signed into account a only"
                   if provider == "claude" else "codex app-server rateLimitResetCredits.availableCount unavailable"
                   if provider == "codex" else "provider adapter has no reset availability")
            resets = f"resets=unknown ({why})"
        billing = [f"{r['name']}={r['amount']:g}" for r in mine if r["kind"] in ("billing", "grant")
                   and r["amount"] is not None and r["name"] != reset_name and not r["name"].startswith("cedar_ember/")]
        print(f"{provider} {account} ({age:.0f} min ago{', STALE' if age > STALE_MIN else ''}): "
              + "; ".join(windows) + (f" | {', '.join(billing)}" if billing else "") + f" | {resets} | {burn(db, mine, now)}")
    for line in cost_lines(now):
        print(line)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
