#!/usr/bin/env python3
import json
import math
import sys
from datetime import datetime, timedelta
from zoneinfo import ZoneInfo
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from project_score import missed_target

SUCCESS_THRESHOLD = 80
STATE_PRICE = {"under pace": 0, "at or over pace": 1}
STALE_MIN = 30
MONEY_CEILING = 95
FAST_FLOOR = 10
LATE = 1.5


def provider(tier):
    return tier["key"].split(":", 1)[0]


def account_of(tier):
    return tier.get("account", provider(tier))


def expiry(acct):
    week = acct.get("windows", {}).get("week")
    return week["reset_in"] if week else math.inf


def route_account(ctx, p):
    names = [n for n in ctx["accounts"] if n.split(":", 1)[0] == p and price(ctx, n) != math.inf
             and account_state(ctx, n) == "under pace"]
    return min(names, key=lambda n: (expiry(ctx["accounts"][n]), n)) if names else None


def pick_accounts(ctx):
    def account(p):
        routed = route_account(ctx, p)
        if routed:
            return routed
        names = [n for n in ctx["accounts"] if n.split(":", 1)[0] == p]
        return min(names, key=lambda n: (price(ctx, n), expiry(ctx["accounts"][n]), n)) if names else p
    return dict(ctx, tiers=[dict(t, account=t.get("account") or account(provider(t))) for t in ctx["tiers"]])


def stale(acct):
    age = acct.get("age_min")
    return age is None or age > STALE_MIN


def stale_warning(name, acct):
    age = acct.get("age_min")
    return f"account {name} STALE: usage {age if age is not None else 'unknown'} minutes old; run agents usage --refresh"


def pace_reading(ctx, p):
    """The account a new provider-p spawn routes to, with its pace state; a stale or unknown readout carries only
    its warning, so it never holds a spawn."""
    names = sorted(n for n in ctx["accounts"] if n.split(":", 1)[0] == p)
    for n in names:
        if stale(ctx["accounts"][n]):
            return {"account": n, "stale": stale_warning(n, ctx["accounts"][n])}
    if not names:
        return None
    name = route_account(ctx, p) or min(names, key=lambda n: (expiry(ctx["accounts"][n]), n))
    m = account_metrics(ctx["accounts"][name], ctx["now"])
    if m["used_pct"] is None:
        return {"account": name, "stale": stale_warning(name, ctx["accounts"][name])}
    return {"account": name, "state": m["state"], "used_pct": m["used_pct"], "pace_pct": m["pace_pct"], "stale": None}


def used(acct):
    return max((100 - w["headroom"] for w in acct["windows"].values()), default=100)


def billing(ctx, name, mode):
    acct = ctx["accounts"].get(name)
    value = ctx["billing"].get(name, ctx["billing"].get(name.split(":", 1)[0], {})).get(mode)
    if acct is None or stale(acct):
        return "unknown"
    if value not in ("plan", "money", "blocked"):
        return "unknown"
    return value


def overflow_money(ctx, name):
    acct = ctx["accounts"][name]
    live = acct.get("hasCredits") or acct.get("extra_usage") or acct.get("spend")
    return bool(live) or billing(ctx, name, "overflow") not in ("plan", "blocked")


def account_metrics(acct, now):
    week = acct.get("windows", {}).get("week")
    if week is None:
        return {"used_pct": None, "pace_pct": None, "available_usd": 0.0, "resets_at": None,
                "pace_at": None, "projected_left": None, "state": "at or over pace", "drain_behind": False}
    reset = datetime.fromisoformat(week["resets_at"].replace("Z", "+00:00"))
    start = reset - timedelta(hours=week["window_h"])
    cutoff = reset
    target = acct["target_left"]
    if acct.get("provider") == "codex":
        local = start.astimezone(ZoneInfo("America/New_York"))
        cutoff = (local.replace(hour=23, minute=59, second=0, microsecond=0)
                  + timedelta(days=(4 - local.weekday()) % 7))
        if cutoff <= local:
            cutoff += timedelta(days=7)
    duration = (cutoff - start).total_seconds()
    elapsed = max(0.0, min((now - start).total_seconds() / duration, 1.0)) if duration > 0 else 1.0
    pace_pct = (100 - target) * elapsed
    used_pct = 100 - week["headroom"]
    rate = week.get("beta")
    observed = datetime.fromisoformat(week["as_of"].replace("Z", "+00:00"))
    remaining_h = max((cutoff - observed).total_seconds() / 3600, 0.0)
    projected = max(0.0, min(100.0, week["headroom"] - rate * remaining_h)) if rate is not None else None
    thursday = (cutoff - timedelta(days=1)).replace(hour=0, minute=0)
    band = acct.get("band", [10, 20])
    return {"used_pct": used_pct, "pace_pct": pace_pct,
            "available_usd": max(0.0, week["headroom"]) / 100 * acct["usd_per_week"],
            "resets_at": week["resets_at"], "pace_at": cutoff.isoformat(), "projected_left": projected,
            "state": "under pace" if used_pct < pace_pct else "at or over pace",
            "drain_behind": bool(acct.get("thursday") and now >= thursday
                                 and projected is not None and projected > band[1])}


def state(acct, now):
    return account_metrics(acct, now)["state"]


def price(ctx, name):
    acct = ctx["accounts"].get(name)
    if billing(ctx, name, "standard") != "plan":
        return math.inf
    if used(acct) >= MONEY_CEILING and overflow_money(ctx, name):
        return math.inf
    return STATE_PRICE[state(acct, ctx["now"])]


def account_state(ctx, name):
    acct = ctx["accounts"].get(name)
    return None if acct is None else state(acct, ctx["now"])


def eligible(tier, box):
    return provider(tier) != "gemini" or box["sensitivity"] == "public"


def bands(ctx, project):
    starts, floor = [], -math.inf
    for t in sorted(ctx["tiers"], key=lambda t: t["rank"]):
        base = t["start"]
        if base >= 100:
            continue
        floor = max(base, floor)
        starts.append((t["key"], floor))
    return {k: (-math.inf if i == 0 else lo, starts[i + 1][1] if i + 1 < len(starts) else math.inf)
            for i, (k, lo) in enumerate(starts)}


def admit(ctx, tier, box, project, band=None):
    if not eligible(tier, box):
        return False
    rate = ctx.get("success", {}).get(f"{tier['key']}|{box['category']}")
    if rate is not None:
        return rate >= SUCCESS_THRESHOLD
    lo, hi = (bands(ctx, project) if band is None else band).get(tier["key"], (math.inf, math.inf))
    return lo <= box["difficulty"] < hi


def successor_soon(tier):
    s = tier.get("successor")
    return bool(s) and s["eta_days"] <= 7 and s["p"] >= 0.6


def explorable(ctx, tier, box, project):
    return (tier.get("runs", math.inf) < 10 and not successor_soon(tier)
            and eligible(tier, box) and account_state(ctx, account_of(tier)) == "under pace")


def choose(ctx, box, explore_slot=False):
    project = ctx["projects"][box["project"]]
    if box.get("local") and not ctx.get("lease"):
        return {"tier": None, "wait": "capacity lease"}
    prices = {n: price(ctx, n) for n in {account_of(t) for t in ctx["tiers"]}}
    priced = [(prices[account_of(t)], t["rank"], t) for t in ctx["tiers"]]
    if explore_slot:
        pool = [(p, r, t) for p, r, t in priced if p != math.inf and explorable(ctx, t, box, project)]
        if pool:
            return {"tier": min(pool, key=lambda x: x[1])[2], "explore": True}
    band = bands(ctx, project)
    admitted = [(p, r, t) for p, r, t in priced if admit(ctx, t, box, project, band)]
    open_ = [x for x in admitted if x[0] != math.inf and account_state(ctx, account_of(x[2])) == "under pace"]
    if not open_:
        if any(p != math.inf for p, _, _ in admitted):
            if project.get("target"):
                return {"tier": None, "needs": missed_target(project.get("name", box["project"]), project["target"], ctx.get("services"))}
            return {"tier": None, "wait": "account pace"}
        return {"tier": None, "needs": f"Blocked: decision for Tom: no under-pace plan-billed tier admits {box['id']} ({box['project']}, {box['category']} d{box['difficulty']}); bill usage credits for it?"}
    tier = min(open_, key=lambda x: (x[0], x[1]))[2]
    out = {"tier": tier}
    name = account_of(tier)
    if (len(admitted) == 1 and project["u"] >= LATE
            and billing(ctx, name, "fast") != "plan"):
        out["needs"] = missed_target(project.get("name", box["project"]), project.get("target", "its target"), ctx.get("services"))
    return out


def fast(ctx, name, u):
    acct = ctx["accounts"].get(name)
    if acct is None or billing(ctx, name, "fast") != "plan" or price(ctx, name) == math.inf:
        return False
    if state(acct, ctx["now"]) != "under pace":
        return False
    return all(w["headroom"] > FAST_FLOOR for w in acct["windows"].values())


def plan(ctx):
    ctx = dict(ctx, now=datetime.fromisoformat(ctx["now"]) if isinstance(ctx["now"], str) else ctx["now"])
    for name, acct in ctx["accounts"].items():
        if stale(acct):
            raise ValueError(stale_warning(name, acct))
    ctx = pick_accounts(ctx)
    accounts = {n: {**account_metrics(a, ctx["now"]), "price": price(ctx, n)}
                for n, a in ctx["accounts"].items()}
    boxes, needs, slot = {}, [], 0
    for box in ctx["boxes"]:
        project = ctx["projects"][box["project"]]
        explore_slot = slot % 5 == 0
        slot += 1
        r = choose(ctx, box, explore_slot)
        t = r["tier"]
        boxes[box["id"]] = {"tier": t and t["key"], "account": t and account_of(t), "explore": r.get("explore", False), "wait": r.get("wait"),
                            "fast": bool(t) and fast(ctx, account_of(t), project["u"])}
        if r.get("needs"):
            if r["needs"] not in needs:
                needs.append(r["needs"])
    explore = [t["key"] for t in ctx["tiers"] if t.get("runs", math.inf) < 10 and not successor_soon(t)]
    held = [t["key"] for t in ctx["tiers"] if successor_soon(t)]
    routes = {t["key"]: route_account(ctx, provider(t)) for t in ctx["tiers"]}
    return {"accounts": accounts, "boxes": boxes, "explore": explore, "held": held, "needs": needs, "routes": routes}


def projection_lines(ctx):
    out = []
    for name, acct in sorted(ctx["accounts"].items()):
        if acct.get("provider") != "codex":
            continue
        metrics = account_metrics(acct, ctx["now"])
        projected = metrics["projected_left"]
        band = acct["band"]
        left = f"{projected:.1f}%" if projected is not None else "unknown (need two readings in last 3h)"
        out.append(f"{name}: projected {left} left at Fri 23:59 ET (band {band[0]}-{band[1]})")
        if metrics["drain_behind"]:
            out.append("FLAG codex drain behind")
    return out



def render(ctx, result):
    lines = []
    for n, a in result["accounts"].items():
        p = "inf" if a["price"] == math.inf else str(a["price"])
        lines.append(f"account {n}: state {a['state']}, used {a['used_pct']}%, pace {a['pace_pct']}%, available ${a['available_usd']:.2f}, resets_at {a['resets_at']}, price {p}")
    lines += projection_lines(dict(ctx, now=datetime.fromisoformat(ctx["now"]) if isinstance(ctx["now"], str) else ctx["now"]))
    for box in ctx["boxes"]:
        b = result["boxes"][box["id"]]
        what = b["tier"] and b["tier"] + (f" on {b['account']}" if ":" in b["account"] else "")
        what = what or (f"wait for {b['wait']}" if b["wait"] else "none")
        tag = " (explore)" if b["explore"] else ""
        lines.append(f"box {box['id']} ({box['project']} {box['category']} d{box['difficulty']}): {what}{tag}, fast {'on' if b['fast'] else 'off'}")
    if result["explore"]:
        lines.append("explore: " + ", ".join(result["explore"]))
    for k in result["held"]:
        lines.append(f"no explore and no new lead: {k} (successor due)")
    lines += [f"route: {tier}: {account or 'none under pace'}" for tier, account in result["routes"].items()]
    lines.extend(result["needs"])
    return "\n".join(lines)


def divergence(result, expect):
    for n, s in expect.get("states", {}).items():
        if result["accounts"][n]["state"] != s:
            return f"account {n}: state {result['accounts'][n]['state']}, expected {s}"
    for b, t in expect.get("tiers", {}).items():
        if result["boxes"][b]["tier"] != t:
            return f"box {b}: tier {result['boxes'][b]['tier']}, expected {t}"
    for b, a in expect.get("accounts", {}).items():
        if result["boxes"][b]["account"] != a:
            return f"box {b}: account {result['boxes'][b]['account']}, expected {a}"
    for b, f in expect.get("fast", {}).items():
        if result["boxes"][b]["fast"] != f:
            return f"box {b}: fast {result['boxes'][b]['fast']}, expected {f}"
    for k in ("explore", "held", "needs", "routes"):
        if k in expect and result[k] != expect[k]:
            return f"{k}: {result[k]}, expected {expect[k]}"
    return None


def main(argv):
    if len(argv) != 3 or argv[1] != "scenario":
        print("usage: routing.py scenario FILE.json", file=sys.stderr)
        return 2
    with open(argv[2]) as f:
        ctx = json.load(f)
    result = plan(ctx)
    print(render(ctx, result))
    bad = "expect" in ctx and divergence(result, ctx["expect"])
    if bad:
        print(f"MISMATCH {bad}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
