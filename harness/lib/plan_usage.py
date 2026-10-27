"""Account and project lines for `agents plan`: build the router's ctx from orchestration.toml, the catalog's
[accounts.*] billing and the live usage snapshot (or a scenario file), then let `north pace` decide."""

import math
import re
import sqlite3
from calibration import open_ledger
from datetime import datetime, timedelta, timezone

import routing
from defaults import DEFAULTS
from run_outcome import succeeded, unjudged

MODES = ("standard", "fast", "ultrafast", "overflow")
MIN_RUNS = DEFAULTS["min_runs"]
RUN_TIERS = {
    "claude:haiku": "haiku",
    "claude:sonnet-high": "sonnet",
    "claude:opus-medium": "medium",
    "claude:opus-high": "high",
    "claude:opus-xhigh": "xhigh",
    "codex:luna-low": "gpt-6-luna low",
    "codex:sol-medium": "gpt-6.1-sol medium",
    "codex:sol-high": "gpt-6.1-sol high",
    "codex:sol-xhigh": "gpt-6.1-sol xhigh",
    "codex:astra": "gpt-6-astra xhigh",
}


def resolve_mode(cfg, override=""):
    providers = list(dict.fromkeys(ref.split(":")[0] for ref in cfg.get("tiers", {})))
    mode = override or cfg.get("mode", "split")
    if mode == "split":
        wanted = providers
    elif mode.endswith("-only") and mode[:-5] in providers:
        wanted = [mode[:-5]]
    else:
        raise ValueError(f"unknown mode {mode!r}; use split, " + ", ".join(p + "-only" for p in providers))
    return mode, providers, wanted


def bands_of(tiers):
    out, floor = [], 0
    for i, r in enumerate(tiers):
        lo = 0 if i == 0 else max(floor, tiers[r]["start"])
        floor = lo
        out.append([lo, 100, r])
    for i in range(len(out) - 1):
        out[i][1] = out[i + 1][0]
    return out


def resolve_bands(full, avail):
    if not avail:
        return []
    out, pending = [], None
    for lo, hi, r in full:
        if r in avail:
            out.append([lo if pending is None else pending, hi, r])
            pending = None
        elif pending is None:
            pending = lo
    if pending is not None:
        out[-1][1] = 100
    return [x for x in out if x[1] > x[0]]


def ts(s):
    d = datetime.fromisoformat(s.replace("Z", "+00:00"))
    return d if d.tzinfo else d.replace(tzinfo=timezone.utc)


def window_key(r):
    if r["window_min"] == 300:
        return "5h"
    if r["name"] in ("week", "weekly_all") or (r["provider"] == "gemini" and r["window_min"] == 10080):
        return "week"
    return None if "/" in r["name"] else r["name"]


def beta(db, r, now):
    if db is None:
        return None
    since = max(ts(r["resets_at"]) - timedelta(minutes=r["window_min"]), now - timedelta(hours=3))
    old = db.execute(
        "SELECT ts, used_pct FROM usage WHERE provider = ? AND account = ? AND kind = 'window' AND name = ? AND resets_at = ?"
        " AND ts >= ? AND ts < ? AND used_pct IS NOT NULL ORDER BY ts LIMIT 1",
        (r["provider"], r["account"], r["name"], r["resets_at"], since.isoformat().replace("+00:00", "Z"), r["ts"])).fetchone()
    hours = (ts(r["ts"]) - ts(old[0])).total_seconds() / 3600 if old else 0
    return max(r["used_pct"] - old[1], 0.0) / hours if hours >= 1 / 6 else None


def accounts_from(rows, db, now):
    """One account per provider is named by the provider; several (claude a and b) are named provider:account."""
    ids = {}
    for r in rows:
        ids.setdefault(r["provider"], set()).add(r["account"])
    accounts = {}
    for r in sorted(rows, key=lambda r: r["ts"], reverse=True):
        key = r["provider"] if len(ids[r["provider"]]) == 1 else f"{r['provider']}:{r['account']}"
        a = accounts.setdefault(key, {"id": r["account"], "windows": {}, "grants": [], "hasCredits": False,
                                      "extra_usage": False, "spend": False, "age_min": None, "resets": {}})
        if r["kind"] != "grant":
            age = (now - ts(r["ts"])).total_seconds() / 60
            a["age_min"] = age if a["age_min"] is None else max(a["age_min"], age)
        if r["kind"] == "window" and r["used_pct"] is not None and r["resets_at"]:
            k = window_key(r)
            if k and (k not in a["windows"] or (r["provider"] == "gemini"
                                                   and 100 - r["used_pct"] < a["windows"][k]["headroom"])):
                a["windows"][k] = {"headroom": 100 - r["used_pct"], "beta": beta(db, r, now),
                                   "reset_in": max((ts(r["resets_at"]) - now).total_seconds() / 3600, 0.0),
                                   "resets_at": r["resets_at"], "window_h": r["window_min"] / 60, "as_of": r["ts"]}
                a["resets"][k] = r["resets_at"]
        elif r["kind"] == "billing" and r["name"] in ("hasCredits", "extra_usage", "spend"):
            a[r["name"]] = bool(r["amount"])
        elif r["kind"] == "billing" and r["name"] == "plan":
            a.setdefault("plan", r["source"])
    return accounts


def costs_by_id(costs):
    if costs is None:
        return {}
    if "service" in costs:
        return {s["id"]: s["usd_per_week"] for s in costs["service"] if s.get("kind") == "ai-plan"}
    return costs


def configure_accounts(accounts, cfg, costs):
    prices = costs_by_id(costs)
    for name, acct in accounts.items():
        provider = name.split(":", 1)[0]
        settings = cfg.get("accounts", {}).get(provider, {})
        label = acct.get("id")
        cost_id = f"claude-{label}" if provider == "claude" and label else provider
        required_plan = settings.get("auto_route_requires", {}).get(label)
        acct["auto_route"] = required_plan is None or acct.get("plan") == required_plan
        acct.update(provider=provider, usd_per_week=prices.get(cost_id, 0.0),
                    target_left=settings.get("target_left", DEFAULTS["pace_target_left_claude"]),
                    band=settings.get("band", DEFAULTS["pace_band"]), thursday=settings.get("thursday", False))


def run_tier(key, t):
    return RUN_TIERS.get(key) or f"{t['model']} {t['effort']}"


def query(db, sql, args=()):
    if db is None:
        return []
    try:
        return db.execute(sql, args).fetchall()
    except sqlite3.Error:
        return []


def measured(db, tiers):
    """Per tier: worker runs, and success per category with at least MIN_RUNS runs, where a success is NorthThreads' run_ok:
    done, not followed by a later run, and landed when it was expected to land; unjudged runs count in neither."""
    keys = {run_tier(k, t): k for k, t in tiers.items()}
    escalated = {f for (f,) in query(db, "SELECT follows FROM runs WHERE role = 'worker' AND follows IS NOT NULL")}
    runs, cells = {k: 0 for k in tiers}, {}
    columns = ("agent", "tier", "category", "outcome", "expects_landing", "landed", "item")
    for r in query(db, f"SELECT {', '.join(columns)} FROM runs WHERE role = 'worker' AND follows IS NULL AND outcome != 'split'"):
        r = dict(zip(columns, r))
        key = keys.get(r["tier"])
        if key is None or unjudged(r):
            continue
        runs[key] += 1
        cells.setdefault(f"{key}|{r['category']}", []).append(succeeded(r, r["agent"] in escalated))
    success = {c: sum(oks) * 100 // len(oks) for c, oks in cells.items() if len(oks) >= MIN_RUNS}
    return success, runs


def lineage(model):
    parts = re.split(r"[-.]", model.split(":", 1)[-1])
    return tuple(p for p in parts if not p.isdigit()), tuple(int(p) for p in parts if p.isdigit())


def successors(db, now, tiers):
    latest = {}
    for model, at, p in query(db, "SELECT model, at, p FROM model_events WHERE kind IN ('expected', 'market')"
                                  " AND at IS NOT NULL ORDER BY ts"):
        latest[model] = (at, p)
    out = {}
    for key, t in tiers.items():
        family, version = lineage(t["model"])
        due = [((ts(at) - now).total_seconds() / 86400, 1.0 if p is None else p, m) for m, (at, p) in latest.items()
               if ts(at) >= now and lineage(m)[0] == family and lineage(m)[1] > version]
        if due:
            eta, p, model = min(due)
            out[key] = {"eta_days": eta, "p": p, "model": model}
    return out


def live_ctx(cfg, catalog, rows, db, now, tiers, costs=None):
    billing = {n: {m: v for m, v in a.items() if m in MODES} for n, a in catalog.get("accounts", {}).items()}
    projects = {}
    for name, p in cfg.get("projects", {}).items():
        projects[name] = {**p, "u": 1.0, "unmeasured": "target" in p}
    success, runs = measured(db, tiers)
    due = successors(db, now, tiers)
    ts_tiers = [{"key": k, "model": t["model"], "effort": t["effort"], "rank": i, "start": t["start"], "runs": runs[k],
                 **({"successor": due[k]} if k in due else {})}
                for i, (k, t) in enumerate(tiers.items())]
    accounts = accounts_from(rows, db, now)
    configure_accounts(accounts, cfg, costs)
    return {"now": now, "tiers": ts_tiers, "billing": billing, "lease": False, "success": success,
            "accounts": accounts, "projects": projects, "boxes": [], "pace": cfg.get("pace", {}),
            "services": costs.get("service", []) if costs else []}


def provider_of_tier(tier):
    return "gemini" if tier.startswith("gemini") else "codex" if tier.startswith("gpt-") else "claude"


def promo_lines(catalog, db, now, days=7):
    """A Tom decision line for each promotion ending within `days`: does the account earn its full price, i.e. is its
    share of the last 30 days' runs at least its share of the plans' monthly spend at full price."""
    accounts = catalog.get("accounts", {})
    full = {p: a.get("monthly_total", a["monthly_price"] * a.get("account_count", 1)) for p, a in accounts.items() if "monthly_price" in a}
    out = []
    for p, a in accounts.items():
        end = a.get("promo_end")
        left = end and (datetime.combine(end, datetime.min.time(), timezone.utc) - now).days
        if left is None or not 0 <= left <= days:
            continue
        counts = {}
        for (tier,) in (db.execute("SELECT tier FROM runs WHERE started >= ?",
                                   ((now - timedelta(days=30)).strftime("%Y-%m-%dT%H:%M:%SZ"),)) if db else []):
            counts[provider_of_tier(tier or "")] = counts.get(provider_of_tier(tier or ""), 0) + 1
        runs, total = counts.get(p, 0), sum(counts.values())
        run_share, cost_share = (100 * runs / total if total else 0.0), 100 * full.get(p, 0) / (sum(full.values()) or 1)
        verdict = "earned its full price" if run_share >= cost_share else "did not earn its full price"
        out.append(f"Blocked: decision for Tom: {p} promotion (USD {a['promo_price']}/month) ends {end} ({left} days, "
                   f"{'confirmed' if a.get('promo_end_confirmed') else 'unconfirmed'}); at USD {a['monthly_price']}/month it is "
                   f"{cost_share:.1f}% of plan spend and ran {run_share:.1f}% of runs in 30 days ({runs} of {total}): {verdict}; keep it at full price?")
    return out


def project_line(name, p):
    target = p.get("target")
    if target is None:
        urgency = "no target"
    elif "u" in p and not p.get("unmeasured"):
        urgency = f"target {target}, urgency {p['u']:.2f}"
    else:
        urgency = f"target {target}, urgency unmeasured"
    share = f", share {p['share']}%" if "share" in p else ""
    return f"project {name}: priority {p.get('priority', '-')}{share}, {urgency}"


def measured_lines(ctx):
    out = []
    for t in ctx["tiers"]:
        rates = sorted((c.split("|", 1)[1], r) for c, r in ctx.get("success", {}).items() if c.split("|", 1)[0] == t["key"])
        if "runs" not in t and not rates and not t.get("successor"):
            continue
        line = f"measured {t['key']}: {t.get('runs', 'unknown')} runs"
        line += "; success " + ", ".join(f"{c} {r}%" for c, r in rates) if rates else f"; no category with {MIN_RUNS} runs"
        s = t.get("successor")
        if s:
            line += f"; successor {s.get('model', 'unnamed')} in {s['eta_days']:.1f} days at p {s['p']:.2f}"
        out.append(line)
    return out


def lines(ctx, result=None):
    shown = routing.ask("plan_lines", ctx=ctx, result=result)
    projects = sorted(ctx["projects"].items(), key=lambda kv: kv[1].get("priority", math.inf))
    return shown["head"] + [project_line(n, p) for n, p in projects] + measured_lines(ctx) + shown["tail"]


def replay(ctx):
    result = json_plan(ctx)
    return lines(ctx, result), routing.ask("divergence", result=result, expect=ctx.get("expect", {}))


def json_plan(ctx):
    return routing.ask("plan", ctx=ctx)


def open_db(path):
    try:
        return open_ledger(path)
    except sqlite3.Error:
        return None
