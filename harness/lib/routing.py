#!/usr/bin/env python3
import json
import math
import os
import re
import subprocess
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
LATE = DEFAULTS["late_ratio"]


def ask(op, **request):
    result = subprocess.run([os.environ.get("NORTH_PACE_BIN", "north"), "pace"], input=json.dumps({"op": op, **request}, default=str), capture_output=True, text=True, timeout=10)
    if result.returncode:
        raise ValueError(result.stderr.strip())
    return json.loads(result.stdout)


CODEX_CLASS = {
    "tooling": {"route": DEFAULTS["codex_route_tooling"], "tiers": DEFAULTS["codex_tier_tooling"]},
    "native-check": {"route": DEFAULTS["codex_route_native-check"], "tiers": DEFAULTS["codex_tier_native-check"]},
    "feature": {"route": DEFAULTS["codex_route_feature"], "tiers": DEFAULTS["codex_tier_feature"]},
    "docs-policy": {"route": DEFAULTS["codex_route_docs-policy"], "tiers": DEFAULTS["codex_tier_docs-policy"]},
    "bug-known-cause": {"route": DEFAULTS["codex_route_bug-known-cause"], "tiers": DEFAULTS["codex_tier_bug-known-cause"]},
    "debugging-unknown-cause": {"route": DEFAULTS["codex_route_debugging-unknown-cause"], "tiers": DEFAULTS["codex_tier_debugging-unknown-cause"]},
    "research": {"route": DEFAULTS["codex_route_research"], "tiers": DEFAULTS["codex_tier_research"]},
    "mechanical": {"route": DEFAULTS["codex_route_mechanical"], "tiers": DEFAULTS["codex_tier_mechanical"]},
    "netcode-determinism": {"route": DEFAULTS["codex_route_netcode-determinism"], "tiers": DEFAULTS["codex_tier_netcode-determinism"]},
    "performance": {"route": DEFAULTS["codex_route_performance"], "tiers": DEFAULTS["codex_tier_performance"]},
    "balance-tuning": {"route": DEFAULTS["codex_route_balance-tuning"], "tiers": DEFAULTS["codex_tier_balance-tuning"]},
}
CODEX_ETA_MAX = DEFAULTS["codex_eta_max_min"]
CODEX_FILES_MAX = DEFAULTS["codex_files_max"]
HEADER_FACETS = {"spec": ("exact", "measured", "judged"), "scope": ("one-file", "module", "cross-module", "cross-repo"),
                 "surface": ("shell", "docs", "nix", "ts", "assets", "lua", "workflow"),
                 "verify": ("none", "local-test", "native", "visual", "farm")}
MEASURED_SPEC_CATEGORIES = ("tooling", "native-check")
CODEX_SCOPES = ("one-file", "module")
CODEX_SURFACES = ("shell", "docs", "nix")
CODEX_VERIFIES = ("none", "local-test", "native")
CODEX_EXPLORE_SURFACES = CODEX_SURFACES + ("assets",)
CODEX_EXPLORE_VERIFIES = CODEX_VERIFIES + ("visual",)


def provider(tier):
    return tier["key"].split(":", 1)[0]


def account_of(tier):
    return tier.get("account", provider(tier))


def expiry(acct):
    week = acct.get("windows", {}).get("week")
    return week["reset_in"] if week else math.inf


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


def launch_open(ctx, name):
    """Whether a launch on account name is admitted: no five-hour window at 95% and no provider week at 100%."""
    for key, window in ((ctx["accounts"].get(name) or {}).get("windows", {})).items():
        used = 100 - window["headroom"]
        if (key == "5h" and used >= 95) or ((key == "week" or key.endswith(":week")) and used >= 100):
            return False
    return True


def stale(acct):
    age = acct.get("age_min")
    return age is None or age > STALE_MIN


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
    return CODEX_CLASS.get(category, {"route": "claude", "tiers": []})


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
    m = re.fullmatch(r"([a-z0-9-]+)(?:\s+\(([^()]*)\))?", fields.get(name, "").strip().lower())
    facets = HEADER_FACETS.get(name, tuple(CODEX_CLASS))
    if not m or any(w in facets for w in re.findall(r"[a-z0-9-]+", m[2] or "")):
        return None
    return m[1]


def header_shown(fields, name):
    return fields.get(name, "").strip().split("\n")[0] or "missing"


def brief_category(box):
    return header_word(brief_header(box.get("brief")), "category")


def header_minutes(value):
    m = re.fullmatch(r"(?:(\d+)\s*-\s*)?(\d+)\s*(?:m|mins?|minutes?)?(?:\s+\([^()]*\))?", value.strip().lower())
    return int(m[2]) if m and (m[1] is None or int(m[1]) <= int(m[2])) else None


def header_files(value):
    names = [re.sub(r"^[-*]\s+", "", n.strip()).strip().strip("`\"'") for n in re.split(r",|\n", value)]
    return [n for n in names if n]


def codex_qualifies(box, shape_only=False, explore=False):
    fields = brief_header(box.get("brief"))
    if not fields:
        return False, "Header missing"
    category, spec = header_word(fields, "category"), header_word(fields, "spec")
    if not (shape_only or explore) and codex_class(category)["route"] != "codex":
        return False, f"Category {header_shown(fields, 'category')}"
    if spec != "exact" and not (spec == "measured" and category in MEASURED_SPEC_CATEGORIES):
        return False, f"Spec {header_shown(fields, 'spec')}"
    surfaces, verifies = (CODEX_EXPLORE_SURFACES, CODEX_EXPLORE_VERIFIES) if explore else (CODEX_SURFACES, CODEX_VERIFIES)
    for name, allowed in (("scope", CODEX_SCOPES), ("surface", surfaces), ("verify", verifies)):
        word = header_word(fields, name)
        if word not in allowed:
            return False, f"{name.capitalize()} {header_shown(fields, name)}"
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


def claude_unavailable(paces, rows, now, target=90, ceiling=95, throttled_at=None):
    over, reason = claude_session_overflow(rows, now, target, ceiling, throttled_at)
    held = sorted(n for n, p in (paces or {}).items() if p.get("state") == "at or over pace" or p.get("stale"))
    if over or not paces or len(held) < len(paces):
        return over, reason
    return True, "no Claude account admits: " + ", ".join(f"{n} at or over pace" for n in held)


def claude_session_overflow(rows, now, target=90, ceiling=95, throttled_at=None):
    now = moment(now)
    throttled = moment(throttled_at) if throttled_at else None
    if throttled and timedelta(0) <= now - throttled <= timedelta(minutes=15):
        return True, f"claude:a 5h 429 {(now - throttled).total_seconds() / 60:.0f} min ago"
    readings = [(moment(r["ts"]), float(r["used_pct"]), moment(r["resets_at"])) for r in rows
                if r.get("used_pct") is not None and moment(r.get("ts")) and moment(r.get("resets_at"))]
    readings = [r for r in readings if r[0] <= now]
    if not readings:
        return False, "claude:a 5h unread"
    resets = max(r[2] for r in readings)
    if resets <= now:
        return False, "claude:a 5h unread"
    window = sorted(r for r in readings if resets - r[2] <= timedelta(minutes=5))
    latest = window[-1][1]
    recent = [r for r in window if window[-1][0] - r[0] <= timedelta(minutes=30)]
    span = (recent[-1][0] - recent[0][0]).total_seconds() / 60 if recent else 0
    burn = max(0.0, (recent[-1][1] - recent[0][1]) / span) if span > 0 else 0.0
    projected = latest + burn * (resets - window[-1][0]).total_seconds() / 60
    if latest >= ceiling:
        return True, f"claude:a 5h {latest:.0f}% >= {ceiling:g}% ceiling"
    if latest >= target:
        return True, f"claude:a 5h {latest:.0f}% >= {target:g}%"
    if projected > target:
        return True, f"claude:a 5h projected {projected:.0f}% > {target:g}%"
    return False, f"claude:a 5h {latest:.0f}% projected {projected:.0f}%"


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
    if provider(tier) == "codex" and not (codex_qualifies(box)[0] and tier["key"] in codex_class(brief_category(box))["tiers"]):
        return False
    rate = ctx.get("success", {}).get(f"{tier['key']}|{box['category']}")
    if rate is not None:
        return rate >= SUCCESS_THRESHOLD
    lo, hi = (bands(ctx, project) if band is None else band).get(tier["key"], (math.inf, math.inf))
    return lo <= box["difficulty"] < hi


def successor_soon(tier):
    s = tier.get("successor")
    return bool(s) and s["eta_days"] <= DEFAULTS["successor_days"] and s["p"] >= DEFAULTS["successor_p"]


def codex_explorable(tier, box):
    category = brief_category(box)
    if not codex_qualifies(box, explore=True)[0]:
        return False
    if codex_class(category)["route"] == "codex":
        return tier.get("runs", math.inf) < DEFAULTS["explore_runs"]
    return (tier["key"] in codex_class(category)["tiers"]
            and tier.get("class_runs", {}).get(category, 0) < DEFAULTS["codex_explore_min_n"])


def explorable(ctx, tier, box, project):
    return (not successor_soon(tier) and eligible(tier, box) and account_state(ctx, account_of(tier)) == "under pace"
            and (codex_explorable(tier, box) if provider(tier) == "codex" else tier.get("runs", math.inf) < DEFAULTS["explore_runs"]))


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
            picked = min(pool, key=lambda x: x[1])[2]
            return {"tier": picked, "explore": True}
    band = bands(ctx, project)
    admitted = [(p, r, t) for p, r, t in priced if admit(ctx, t, box, project, band)]
    open_ = [x for x in admitted if x[0] != math.inf and (account_prices is not None or launch_open(ctx, account_of(x[2])))]
    if not open_:
        if any(p != math.inf for p, _, _ in admitted):
            if project.get("target"):
                return {"tier": None, "needs": missed_target(project.get("name", box["project"]), project["target"], ctx.get("services"))}
            return {"tier": None, "wait": "account exhausted"}
        return {"tier": None, "needs": f"Blocked: decision for Tom: no under-pace plan-billed tier admits {box['id']} ({box['project']}, {box['category']} d{box['difficulty']}); bill usage credits for it?"}
    tier = min(open_, key=lambda x: (x[0], x[1]))[2]
    out = {"tier": tier}
    name = account_of(tier)
    if account_prices is None and account_state(ctx, name) != "under pace":
        start = enforce_from(ctx, name)
        why = (f"{project.get('name', box['project'])} drains it under Tom's grant until {project['target']}"
               if enforced(ctx, name) and drain_open(project, name, ctx["now"]) else f"pace enforced from {start.isoformat() if start else 'unset'}")
        out["warn"] = f"account {name} at or over pace; admitted, {why}"
    if (account_prices is None and len(admitted) == 1 and project["u"] >= LATE
            and billing(ctx, name, "fast") != "plan"):
        out["needs"] = missed_target(project.get("name", box["project"]), project.get("target", "its target"), ctx.get("services"))
    return out
