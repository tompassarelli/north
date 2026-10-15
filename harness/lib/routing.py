#!/usr/bin/env python3
import json
import math
import re
import sys
from datetime import date, datetime, time, timedelta, timezone
from zoneinfo import ZoneInfo
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from project_score import missed_target
from defaults import DEFAULTS

SUCCESS_THRESHOLD = DEFAULTS["success_threshold"]
STATE_PRICE = {"under pace": 0, "at or over pace": 1}
STALE_MIN = DEFAULTS["usage_stale_min"]
MONEY_CEILING = DEFAULTS["money_ceiling"]
FAST_FLOOR = DEFAULTS["fast_floor"]
LATE = DEFAULTS["late_ratio"]
CAPACITY_MIN = DEFAULTS["capacity_min"]
ASTRA = {"key": "codex:astra", "model": "gpt-6-astra", "effort": "medium"}
CODEX_FLOOR = DEFAULTS["codex_floor"]
ASTRA_COST = DEFAULTS["astra_cost"]
CODEX_CLASS = {
    "tooling": {"route": DEFAULTS["codex_route_tooling"], "tiers": DEFAULTS["codex_tier_tooling"], "leash": DEFAULTS["codex_leash_tooling"]},
    "native-check": {"route": DEFAULTS["codex_route_native-check"], "tiers": DEFAULTS["codex_tier_native-check"], "leash": DEFAULTS["codex_leash_native-check"]},
    "feature": {"route": DEFAULTS["codex_route_feature"], "tiers": DEFAULTS["codex_tier_feature"], "leash": DEFAULTS["codex_leash_feature"]},
    "docs-policy": {"route": DEFAULTS["codex_route_docs-policy"], "tiers": DEFAULTS["codex_tier_docs-policy"], "leash": DEFAULTS["codex_leash_docs-policy"]},
    "bug-known-cause": {"route": DEFAULTS["codex_route_bug-known-cause"], "tiers": DEFAULTS["codex_tier_bug-known-cause"], "leash": DEFAULTS["codex_leash_bug-known-cause"]},
    "debugging-unknown-cause": {"route": DEFAULTS["codex_route_debugging-unknown-cause"], "tiers": DEFAULTS["codex_tier_debugging-unknown-cause"], "leash": DEFAULTS["codex_leash_debugging-unknown-cause"]},
    "research": {"route": DEFAULTS["codex_route_research"], "tiers": DEFAULTS["codex_tier_research"], "leash": DEFAULTS["codex_leash_research"]},
    "mechanical": {"route": DEFAULTS["codex_route_mechanical"], "tiers": DEFAULTS["codex_tier_mechanical"], "leash": DEFAULTS["codex_leash_mechanical"]},
    "netcode-determinism": {"route": DEFAULTS["codex_route_netcode-determinism"], "tiers": DEFAULTS["codex_tier_netcode-determinism"], "leash": DEFAULTS["codex_leash_netcode-determinism"]},
    "performance": {"route": DEFAULTS["codex_route_performance"], "tiers": DEFAULTS["codex_tier_performance"], "leash": DEFAULTS["codex_leash_performance"]},
    "balance-tuning": {"route": DEFAULTS["codex_route_balance-tuning"], "tiers": DEFAULTS["codex_tier_balance-tuning"], "leash": DEFAULTS["codex_leash_balance-tuning"]},
}
COACH = {"takeover_fixes": DEFAULTS["coach_takeover_fixes"], "first_steps": DEFAULTS["coach_first_steps"],
         "first_minutes": DEFAULTS["coach_first_minutes"], "ladder_steps": DEFAULTS["coach_ladder_steps"],
         "ladder_minutes": DEFAULTS["coach_ladder_minutes"]}
CODEX_ETA_MAX = DEFAULTS["codex_eta_max_min"]
CODEX_FILES_MAX = DEFAULTS["codex_files_max"]
MEASURED_SPEC_CATEGORIES = ("tooling", "native-check")
CODEX_SCOPES = ("one-file", "module")
CODEX_SURFACES = ("shell", "docs", "nix")
CODEX_VERIFIES = ("none", "local-test", "native")


def provider(tier):
    return tier["key"].split(":", 1)[0]


def account_of(tier):
    return tier.get("account", provider(tier))


def expiry(acct):
    week = acct.get("windows", {}).get("week")
    return week["reset_in"] if week else math.inf


def route_account(ctx, p):
    names = [n for n in ctx["accounts"] if n.split(":", 1)[0] == p and ctx["accounts"][n].get("auto_route", True) and price(ctx, n) != math.inf
             and account_state(ctx, n) == "under pace"]
    return min(names, key=lambda n: (expiry(ctx["accounts"][n]), n)) if names else None


def provider_account(ctx, p):
    routed = route_account(ctx, p)
    if routed:
        return routed
    names = [n for n in ctx["accounts"] if n.split(":", 1)[0] == p and ctx["accounts"][n].get("auto_route", True)]
    return min(names, key=lambda n: (price(ctx, n), expiry(ctx["accounts"][n]), n)) if names else p


def moment(value):
    if isinstance(value, datetime):
        return value if value.tzinfo else value.replace(tzinfo=timezone.utc)
    if isinstance(value, date):
        return datetime.combine(value, time.min, timezone.utc)
    try:
        return moment(datetime.fromisoformat(str(value).replace("Z", "+00:00")))
    except ValueError:
        return None


def enforce_from(ctx, name):
    value = (ctx.get("pace") or {}).get("enforce_from", {}).get(name)
    return None if value is None else moment(value)


def enforced(ctx, name):
    start = enforce_from(ctx, name)
    return start is not None and ctx["now"] >= start


def target_open(target, now):
    if not target:
        return False
    if isinstance(target, date) and not isinstance(target, datetime):
        return target >= now.date()
    when = moment(target)
    return when is None or when >= now


def drain_open(project, name, now):
    """A live target with Tom's drain grant for this account (or its provider) in orchestration.toml."""
    grants = project.get("drain") if isinstance(project.get("drain"), dict) else {}
    granted = any(isinstance(grants.get(k), str) and grants[k].strip() for k in (name, name.split(":", 1)[0]))
    return granted and target_open(project.get("target"), now)


def exhausted(acct):
    """At or past its drain floor: the week window has no more headroom than the account's target_left."""
    week = (acct or {}).get("windows", {}).get("week")
    return bool(week) and week["headroom"] <= acct["target_left"]


def pace_open(ctx, name, project):
    """Whether pace admits a spawn of project on account name: under pace, before its enforce_from while above its
    drain floor, or drained under Tom's grant."""
    return (account_state(ctx, name) == "under pace"
            or (not enforced(ctx, name) and not exhausted(ctx["accounts"].get(name)))
            or drain_open(project, name, ctx["now"]))


def route(ctx, p):
    """The account a new provider-p spawn routes to: the under-pace one, else one whose pace is not yet enforced."""
    routed = route_account(ctx, p)
    if routed:
        return routed
    names = [n for n in ctx["accounts"] if n.split(":", 1)[0] == p and ctx["accounts"][n].get("auto_route", True) and price(ctx, n) != math.inf and not enforced(ctx, n)
             and not exhausted(ctx["accounts"][n])]
    return min(names, key=lambda n: (expiry(ctx["accounts"][n]), n)) if names else None


def route_label(ctx, p):
    name = route(ctx, p)
    if name is None:
        return "none under pace"
    if account_state(ctx, name) == "under pace":
        return name
    start = enforce_from(ctx, name)
    return f"{name} (at or over pace, enforced from {start.isoformat() if start else 'unset'})"


def pick_accounts(ctx):
    return dict(ctx, tiers=[dict(t, account=t.get("account") or route(ctx, provider(t)) or provider_account(ctx, provider(t)))
                            for t in ctx["tiers"]])


def stale(acct):
    age = acct.get("age_min")
    return age is None or age > STALE_MIN


def stale_warning(name, acct):
    age = acct.get("age_min")
    return f"account {name} STALE: usage {age if age is not None else 'unknown'} minutes old; run agents usage --refresh"


def pace_reading(ctx, p, account=None):
    """The named account, else the account a new provider-p spawn routes to, with its pace state; a stale or unknown
    readout carries only its warning, so it never holds a spawn."""
    names = sorted(n for n in ctx["accounts"] if n.split(":", 1)[0] == p and account in (None, n))
    for n in names:
        if stale(ctx["accounts"][n]):
            return {"account": n, "stale": stale_warning(n, ctx["accounts"][n])}
    if not names:
        return None
    name = account or route_account(ctx, p) or min(names, key=lambda n: (expiry(ctx["accounts"][n]), n))
    m = account_metrics(ctx["accounts"][name], ctx["now"])
    if m["used_pct"] is None:
        return {"account": name, "stale": stale_warning(name, ctx["accounts"][name])}
    return {"account": name, "state": m["state"], "used_pct": m["used_pct"], "pace_pct": m["pace_pct"],
            "exhausted": exhausted(ctx["accounts"][name]), "stale": None}


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
    band = acct.get("band", DEFAULTS["pace_band"])
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


def codex_class(category):
    return CODEX_CLASS.get(category, {"route": "claude", "tiers": [], "leash": "tight"})


def brief_header(text):
    """Header fields of a brief: lines up to the first blank line, a field starting a line, indented or bulleted lines continuing it."""
    fields, last = {}, None
    for line in (text or "").splitlines():
        if not line.strip():
            break
        m = re.match(r"([A-Za-z][A-Za-z ]*?):[ \t]*(.*)$", line)
        if m and not line[0].isspace():
            last = m[1].lower()
            fields.setdefault(last, m[2])
        elif last and (line[0].isspace() or line.lstrip()[:2] in ("- ", "* ")):
            fields[last] += "\n" + line
    return fields


def header_word(fields, name):
    words = re.findall(r"[a-z0-9-]+", fields.get(name, "").lower())
    return words[0] if words else None


def header_minutes(value):
    m = re.search(r"(\d+(?:\.\d+)?)\s*(h(?:ours?|rs?)?\b)?", value)
    return None if not m else float(m[1]) * (60 if m[2] else 1)


def header_files(value):
    names = [re.sub(r"^[-*]\s+", "", n.strip()).strip().strip("`\"'") for n in re.split(r",|\n", value)]
    return [n for n in names if n]


def codex_qualifies(box):
    fields = brief_header(box.get("brief"))
    if not fields:
        return False, "Header missing"
    category, spec = header_word(fields, "category"), header_word(fields, "spec")
    if codex_class(category)["route"] != "codex":
        return False, f"Category {category or 'missing'}"
    if spec != "exact" and not (spec == "measured" and category in MEASURED_SPEC_CATEGORIES):
        return False, f"Spec {spec or 'missing'}"
    for name, allowed in (("scope", CODEX_SCOPES), ("surface", CODEX_SURFACES), ("verify", CODEX_VERIFIES)):
        word = header_word(fields, name)
        if word not in allowed:
            return False, f"{name.capitalize()} {word or 'missing'}"
    if not re.search(r"`[^`\n]+`", fields.get("done when", "")):
        return False, "Done when has no command"
    if fields.get("open questions", "").strip():
        return False, "Open questions"
    eta = header_minutes(fields.get("eta", ""))
    if eta is None or eta > CODEX_ETA_MAX:
        return False, f"ETA {fields.get('eta', '').strip() or 'missing'}"
    files = header_files(fields.get("files", ""))
    if not files or len(files) > CODEX_FILES_MAX or not all(re.fullmatch(r"[A-Za-z0-9_./-]+", f) for f in files):
        return False, "Files"
    if re.search(r"gpt-", fields.get("follows", ""), re.I):
        return False, "Follows a Codex run"
    return True, None


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
    if provider(tier) == "codex" and not (codex_qualifies(box)[0] and tier["key"] in codex_class(box["category"])["tiers"]):
        return False
    rate = ctx.get("success", {}).get(f"{tier['key']}|{box['category']}")
    if rate is not None:
        return rate >= SUCCESS_THRESHOLD
    lo, hi = (bands(ctx, project) if band is None else band).get(tier["key"], (math.inf, math.inf))
    return lo <= box["difficulty"] < hi


def successor_soon(tier):
    s = tier.get("successor")
    return bool(s) and s["eta_days"] <= DEFAULTS["successor_days"] and s["p"] >= DEFAULTS["successor_p"]


def explorable(ctx, tier, box, project):
    return (tier.get("runs", math.inf) < DEFAULTS["explore_runs"] and not successor_soon(tier)
            and eligible(tier, box) and account_state(ctx, account_of(tier)) == "under pace")


def choose(ctx, box, explore_slot=False, *, account_prices=None):
    project = ctx["projects"][box["project"]]
    if box.get("local") and not ctx.get("lease"):
        return {"tier": None, "wait": "capacity lease"}
    prices = ({n: price(ctx, n) for n in {account_of(t) for t in ctx["tiers"]}}
              if account_prices is None else account_prices)
    priced = [(prices[account_of(t)], t["rank"], t) for t in ctx["tiers"]]
    if explore_slot:
        pool = [(p, r, t) for p, r, t in priced if p != math.inf and explorable(ctx, t, box, project)]
        if pool:
            return {"tier": min(pool, key=lambda x: x[1])[2], "explore": True}
    band = bands(ctx, project)
    admitted = [(p, r, t) for p, r, t in priced if admit(ctx, t, box, project, band)]
    open_ = [x for x in admitted if x[0] != math.inf and (account_prices is not None or pace_open(ctx, account_of(x[2]), project))]
    if not open_:
        if any(p != math.inf for p, _, _ in admitted):
            if project.get("target"):
                return {"tier": None, "needs": missed_target(project.get("name", box["project"]), project["target"], ctx.get("services"))}
            return {"tier": None, "wait": "account pace"}
        return {"tier": None, "needs": f"Blocked: decision for Tom: no under-pace plan-billed tier admits {box['id']} ({box['project']}, {box['category']} d{box['difficulty']}); bill usage credits for it?"}
    tier = min(open_, key=lambda x: (x[0], x[1]))[2]
    out = {"tier": tier}
    name = account_of(tier)
    if account_prices is None and account_state(ctx, name) != "under pace":
        start = enforce_from(ctx, name)
        why = (f"{project.get('name', box['project'])} drains it under Tom's grant until {project['target']}"
               if enforced(ctx, name) else f"pace enforced from {start.isoformat() if start else 'unset'}")
        out["warn"] = f"account {name} at or over pace; admitted, {why}"
    if (account_prices is None and len(admitted) == 1 and project["u"] >= LATE
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


def capacity(ctx):
    """Providers whose model hit a capacity error in the last 15 minutes. Codex falls back to Astra medium only while
    its Friday projection (else its current headroom) stays ASTRA_COST above the floor; otherwise it is unavailable."""
    out = {}
    for p, c in ctx.get("capacity", {}).items():
        since = datetime.fromisoformat(c["since"].replace("Z", "+00:00"))
        if (ctx["now"] - since).total_seconds() > CAPACITY_MIN * 60:
            continue
        name = provider_account(ctx, p)
        m = account_metrics(ctx["accounts"][name], ctx["now"]) if name in ctx["accounts"] else None
        left = m and (m["projected_left"] if m["projected_left"] is not None else
                      None if m["used_pct"] is None else 100 - m["used_pct"])
        astra = p == "codex" and c.get("model") != ASTRA["model"] and left is not None and left - ASTRA_COST >= CODEX_FLOOR
        out[p] = {"since": c["since"], "model": c.get("model"), "left": left, "fallback": ASTRA["key"] if astra else None}
    return out


def capacity_lines(ctx, cap):
    out = []
    for p, c in sorted(cap.items()):
        at = datetime.fromisoformat(c["since"].replace("Z", "+00:00")).astimezone().strftime("%H:%M")
        if c["fallback"]:
            out.append(f"account {p}: {c['model']} at capacity since {at}; its tiers fall back to "
                       f"{ASTRA['model']} {ASTRA['effort']} ({c['left']:.1f}% left at Fri 23:59 ET, floor {CODEX_FLOOR}%)")
        else:
            out.append(f"account {p}: unavailable (provider capacity, since {at})")
    return out


def plan(ctx):
    ctx = dict(ctx, now=datetime.fromisoformat(ctx["now"]) if isinstance(ctx["now"], str) else ctx["now"])
    for name, acct in ctx["accounts"].items():
        if stale(acct):
            raise ValueError(stale_warning(name, acct))
    cap = capacity(ctx)
    hit = lambda t: provider(t) in cap and cap[provider(t)]["model"] in (None, t.get("model"))
    ctx = dict(ctx, tiers=[dict(t, model=ASTRA["model"], effort=ASTRA["effort"], fallback=True) if hit(t) else t
                           for t in ctx["tiers"] if not hit(t) or cap[provider(t)]["fallback"]])
    ctx = pick_accounts(ctx)
    accounts = {n: {**account_metrics(a, ctx["now"]), "price": price(ctx, n)}
                for n, a in ctx["accounts"].items()}
    boxes, needs, warnings, slot = {}, [], [], 0
    for box in ctx["boxes"]:
        project = ctx["projects"][box["project"]]
        explore_slot = slot % DEFAULTS["explore_every"] == 0
        slot += 1
        r = choose(ctx, box, explore_slot)
        t = r["tier"]
        boxes[box["id"]] = {"tier": t and t["key"], "account": t and account_of(t), "explore": r.get("explore", False), "wait": r.get("wait"),
                            "fast": bool(t) and fast(ctx, account_of(t), project["u"]),
                            "fallback": bool(t) and t.get("fallback", False)}
        if r.get("needs"):
            if r["needs"] not in needs:
                needs.append(r["needs"])
        if r.get("warn") and r["warn"] not in warnings:
            warnings.append(r["warn"])
    explore = [t["key"] for t in ctx["tiers"] if t.get("runs", math.inf) < DEFAULTS["explore_runs"] and not successor_soon(t)]
    held = [t["key"] for t in ctx["tiers"] if successor_soon(t)]
    routes = {t["key"]: route(ctx, provider(t)) for t in ctx["tiers"]}
    return {"accounts": accounts, "boxes": boxes, "explore": explore, "held": held, "needs": needs, "routes": routes,
            "warnings": warnings,
            "capacity": cap}


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
    lines += capacity_lines(ctx, result["capacity"])
    for box in ctx["boxes"]:
        b = result["boxes"][box["id"]]
        what = b["tier"] and b["tier"] + (f" on {b['account']}" if ":" in b["account"] else "")
        what = what or (f"wait for {b['wait']}" if b["wait"] else "none")
        tag = (" (explore)" if b["explore"] else "") + (f" on {ASTRA['model']} {ASTRA['effort']} (capacity fallback)" if b["fallback"] else "")
        lines.append(f"box {box['id']} ({box['project']} {box['category']} d{box['difficulty']}): {what}{tag}, fast {'on' if b['fast'] else 'off'}")
    if result["explore"]:
        lines.append("explore: " + ", ".join(result["explore"]))
    for k in result["held"]:
        lines.append(f"no explore and no new lead: {k} (successor due)")
    lines += [f"route: {tier}: {account or 'none under pace'}" for tier, account in result["routes"].items()]
    lines.extend(result["warnings"])
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
    for p, f in expect.get("capacity", {}).items():
        if result["capacity"].get(p, {}).get("fallback", "available") != f:
            return f"capacity {p}: {result['capacity'].get(p)}, expected fallback {f}"
    for k in ("explore", "held", "needs", "routes", "warnings"):
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
