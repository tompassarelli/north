#!/usr/bin/env python3
"""Walk-forward measurements behind socrates/defaults.toml: each kind sweeps one default over a grid
on a read-only copy of the threads ledger and prints n and the score per value."""

import argparse
import importlib.machinery
import importlib.util
import json
import math
import random
import sqlite3
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "socrates/scripts"))
from policy_stats import wilson


def load_threads():
    loader = importlib.machinery.SourceFileLoader("threads_mod", str(ROOT / "bin/threads"))
    spec = importlib.util.spec_from_loader("threads_mod", loader)
    mod = importlib.util.module_from_spec(spec)
    loader.exec_module(mod)
    return mod


T = load_threads()


def open_db(path):
    db = sqlite3.connect(f"file:{path}?mode=ro", uri=True)
    db.row_factory = sqlite3.Row
    return db


def first_runs(db):
    rows = [dict(r) for r in db.execute("SELECT * FROM runs WHERE role = 'worker' AND follows IS NULL AND outcome != 'split'"
                                        " ORDER BY ended")]
    followed = {}
    for f, ended in db.execute("SELECT follows, ended FROM runs WHERE role = 'worker' AND follows IS NOT NULL"):
        followed[f] = min(ended, followed.get(f, ended))
    return rows, followed, T.run_ok(db)


def tier_picks(db, since, patch):
    """Walk-forward recommend picks under patched constants: (run, pick, final ok, cost_usd) per decision."""
    for k, v in patch.items():
        setattr(T, k, v)
    rows, followed, final = first_runs(db)
    by_cat = {}
    for r in rows:
        by_cat.setdefault(T.cell_value(r, "category"), []).append(r)
    held, out = {}, []
    for x in (r for r in rows if r["ended"] >= since and T.cell_value(r, "category") != "unknown"):
        at = x["started"] or x["ended"]
        asked = {k: T.cell_value(x, k) for k in T.GROUPS if T.cell_value(x, k) != "-"}

        def ok_then(r):
            return (r["outcome"] == "done" and not (r["agent"] in followed and followed[r["agent"]] < at)
                    and not (r["expects_landing"] and r["landed"] == 0))

        hist = [r for r in by_cat[asked["category"]] if r["ended"] < at]
        cell, _, pick, _, _ = T.recommend(T.recommend_runs(hist, ok_then), asked, held)
        held[cell] = pick
        out.append((x, pick, final(x), x["cost_usd"]))
    return out


def score_picks(picks):
    hit = [(ok, usd) for x, pick, ok, usd in picks if T.tier_key(x["tier"]) == pick]
    n, wins = len(hit), sum(ok for ok, _ in hit)
    priced = [(ok, usd) for ok, usd in hit if usd is not None]
    pw = sum(ok for ok, _ in priced)
    lo, hi = wilson(wins / n, n) if n else (0, 0)
    return {"decisions": len(picks), "n": n, "ok": wins, "rate": wins / n if n else None, "lo": lo, "hi": hi,
            "usd_per_success": sum(u for _, u in priced) / pw if pw else None, "priced": len(priced)}


TIER_GRIDS = {"success_threshold": ("SUCCESS_THRESHOLD", [60, 70, 75, 80, 85, 90]),
              "min_runs": ("MIN_RUNS", [3, 5, 8, 10]),
              "incumbent_margin": ("MARGIN", [0, 5, 10, 15, 20, 30])}


def sweep_tier(db, since, name):
    attr, grid = TIER_GRIDS[name]
    base = {a: getattr(T, a) for a, _ in TIER_GRIDS.values()}
    base["USD_MIN"] = T.USD_MIN
    rows = []
    for v in grid:
        patch = {**base, attr: v}
        if attr == "MIN_RUNS":
            patch["USD_MIN"] = v
        rows.append({"value": v, **score_picks(tier_picks(db, since, patch))})
    for a, v in base.items():
        setattr(T, a, v)
    return rows


def eta_pairs(db, floor, since):
    T.DEFAULTS["eta_min_samples"] = floor
    return T.walk_forward_eta(T.calibration_runs(db), since, "9999")


def sweep_eta_min_samples(db, since):
    keep = T.DEFAULTS["eta_min_samples"]
    out = []
    for floor in (1, 2, 3, 5, 8):
        pairs = [(r, eta) for r, eta in eta_pairs(db, floor, since) if r["minutes"] > 0]
        errs = [abs(math.log(r["minutes"] / eta)) for r, eta in pairs]
        out.append({"value": floor, "n": len(pairs), "median_abs_log_err": T.quantile(errs, .5),
                    "p75_abs_log_err": T.quantile(errs, .75), "covered": sum(r["minutes"] <= eta for r, eta in pairs) / len(pairs)})
    T.DEFAULTS["eta_min_samples"] = keep
    return out


def sweep_eta_coverage(db, since, overdue):
    pairs = sorted(eta_pairs(db, T.DEFAULTS["eta_min_samples"], ""), key=lambda p: p[0]["ended"])
    out = []
    for q in (0.5, 0.6, 0.7, 0.75, 0.8, 0.9):
        false_alarm = unflagged = n = over_quote = 0
        ratios = []
        for r, eta in pairs:
            if r["ended"] >= since and len(ratios) >= T.CAL_FLOOR:
                quote = eta * T.quantile(ratios, q)
                flagged = r["minutes"] > overdue * quote
                done = r["outcome"] == "done"
                n += 1
                false_alarm += flagged and done
                unflagged += (not done) and not flagged
                over_quote += r["minutes"] > quote
            ratios.append(r["minutes"] / eta)
        out.append({"value": q, "n": n, "overdue_false_alarms": false_alarm, "unfinished_unflagged": unflagged,
                    "cost": false_alarm + unflagged, "over_quote": over_quote})
    return out


def usd_ratios(db, since):
    runs = T.calibration_runs(db)
    out = []
    for r in runs:
        if r["cost_usd"] is not None and r["ended"] >= since:
            usd, _ = T.recommend_usd([e for e in runs if e["ended"] < r["ended"]], r["category"], r["tier"])
            if usd:
                out.append(r["cost_usd"] / usd)
    return out


def sweep_thin_n(db, since, reps=400):
    pools = {"eta_pred": [r["minutes"] / eta for r, eta in eta_pairs(db, T.DEFAULTS["eta_min_samples"], since)],
             "usd": usd_ratios(db, since)}
    rnd = random.Random(35)
    out = []
    for n in (3, 5, 10, 20, 30, 50, 75, 100, 150):
        row = {"value": n}
        for kind, ratios in pools.items():
            full = T.quantile(ratios, .75)
            ps = sorted(T.quantile(rnd.choices(ratios, k=n), .75) for _ in range(reps))
            row[f"{kind}_pool"] = len(ratios)
            row[f"{kind}_half_width_80"] = (T.quantile(ps, .9) - T.quantile(ps, .1)) / 2 / full
        out.append(row)
    return out


def box_item_gaps(db):
    """Minutes from the latest end of an item's earlier runs to each later run's staffing, over every worker run."""
    by_item = {}
    for r in T.box_rows(db):
        if r["outcome"] != "split" and r["ended"]:
            by_item.setdefault(r["item"], []).append((T.moment(r["staffed"] or r["started"] or r["ended"]), T.moment(r["ended"])))
    gaps = []
    for rs in by_item.values():
        rs.sort()
        end = None
        for begin, ended in rs:
            if end is not None:
                gaps.append((begin - end).total_seconds() / 60)
            end = ended if end is None else max(end, ended)
    return gaps


def sweep_box_gap(db, since):
    gaps = box_item_gaps(db)
    rows = T.box_rows(db)
    out = []
    for value in (10, 20, 30, 45, 60, 90, 120):
        T.BOX_GAP = value
        chains = T.box_chains(rows)
        out.append({"value": value, "gaps": len(gaps), "share_within": sum(g <= value for g in gaps) / len(gaps),
                    "gaps_in_bin": sum(value - 10 < g <= value for g in gaps), "chains": len(chains),
                    "box_p75": T.quantile([c["minutes"] for c in chains], .75)})
    return out


def sweep_box_n(db, since, reps=400):
    chains = T.box_chains(T.box_rows(db))
    ratios = []
    for c in chains:
        if c["end"] >= T.moment(since):
            pred, n, _ = T.box_duration([e for e in chains if e["end"] < c["start"]], c["category"])
            if n and pred:
                ratios.append(c["minutes"] / pred)
    rnd = random.Random(35)
    full = T.quantile(ratios, .75)
    out = []
    for n in (5, 20, 50, 100, 150, 200, 300):
        ps = sorted(T.quantile(rnd.choices(ratios, k=n), .75) for _ in range(reps))
        out.append({"value": n, "pool": len(ratios), "p75": full, "half_width_80": (T.quantile(ps, .9) - T.quantile(ps, .1)) / 2 / full})
    return out


def idle_gaps(root, days, floor):
    """Gaps of floor+ minutes between consecutive events in Claude subagent transcripts changed within days."""
    from datetime import datetime, timezone
    now = datetime.now(timezone.utc).timestamp()
    resumed, files = [], 0
    for path in Path(root).expanduser().glob("*/*/subagents/*.jsonl"):
        if now - path.stat().st_mtime > days * 86400:
            continue
        stamps = []
        for line in path.open(errors="replace"):
            ts = line.find('"timestamp":"')
            if ts >= 0:
                stamps.append(datetime.fromisoformat(line[ts + 13:line.index('"', ts + 13)].replace("Z", "+00:00")).timestamp())
        if not stamps:
            continue
        files += 1
        stamps.sort()
        resumed += [(b - a) / 60 for a, b in zip(stamps, stamps[1:]) if b - a >= floor * 60]
    return files, resumed


def sweep_stale_idle(a):
    files, resumed = idle_gaps(a.transcripts, a.days, T.DEFAULTS["stall_idle_min"])
    out = [{"transcripts": files, "resumed_gaps": len(resumed), "p50": T.quantile(resumed, .5), "p90": T.quantile(resumed, .9),
            "p99": T.quantile(resumed, .99), "max": max(resumed, default=None)}]
    for x in (60, 120, 180, 300, 600):
        out.append({"value": x, "resumed_after": sum(g > x for g in resumed), "share_resumed_after": sum(g > x for g in resumed) / len(resumed)})
    return out


def capacity_episodes(a):
    """Codex capacity episodes: minutes from a model's first capacity failure to its next ok turn on any rollout."""
    sys.path.insert(0, str(ROOT / "socrates/lib"))
    import provider_capacity as pc
    import time
    since = time.time() - a.days * 86400
    rows = pc.outcomes(pc.rollouts(pc.root(), since), since)
    open_, spans = {}, []
    for o in rows:
        if o["outcome"] == "capacity":
            open_.setdefault(o["model"], o["at"])
        elif o["outcome"] == "ok" and o["model"] in open_:
            spans.append((pc.stamp(o["at"]) - pc.stamp(open_.pop(o["model"]))) / 60)
    out = [{"capacity_turns": sum(o["outcome"] == "capacity" for o in rows), "turns": len(rows), "episodes": len(spans),
            "unclosed": len(open_), "p50": T.quantile(spans, .5), "p90": T.quantile(spans, .9), "max": max(spans, default=None)}]
    for x in (5, 10, 15, 30, 60):
        out.append({"value": x, "episodes_longer": sum(s > x for s in spans)})
    return out


def sweep_money_ceiling(db, horizon):
    """Rise in a window's used_pct within horizon minutes of each readout (same account, window and reset)."""
    rows = db.execute("SELECT provider, account, name, ts, used_pct, resets_at FROM usage WHERE kind = 'window'"
                      " AND used_pct IS NOT NULL ORDER BY provider, account, name, ts").fetchall()
    from datetime import datetime
    stamp = lambda s: datetime.fromisoformat(s.replace("Z", "+00:00")).timestamp()
    by = {}
    for r in rows:
        by.setdefault((r["provider"], r["account"], r["name"]), []).append((stamp(r["ts"]), r["used_pct"], r["resets_at"]))
    rises = {}
    for (prov, _, name), xs in by.items():
        for i, (t, u, reset) in enumerate(xs):
            later = [v for s, v, rs in xs[i + 1:] if s - t <= horizon * 60 and rs == reset]
            if later:
                rises.setdefault(f"{prov} {name.split('/')[-1]}", []).append(max(0, max(later) - u))
    allr = [x for xs in rises.values() for x in xs]
    out = [{"cell": c, "n": len(xs), "p99": T.quantile(xs, .99), "max": max(xs)} for c, xs in sorted(rises.items())]
    out.append({"cell": "all", "n": len(allr), "p99": T.quantile(allr, .99), "p999": T.quantile(allr, .999), "max": max(allr),
                "ceiling_p99": 100 - T.quantile(allr, .99)})
    for c in (85, 88, 90, 92, 95):
        out.append({"value": c, "overshoot": sum(x > 100 - c for x in allr), "share": sum(x > 100 - c for x in allr) / len(allr)})
    return out


def capacity_series(db):
    by = {}
    for r in db.execute("SELECT family, repo, package, class, at, peakMemoryMiB, p90Cores FROM capacity_runs"
                        " WHERE signature_exact = 1 AND peakMemoryMiB > 0 ORDER BY at"):
        by.setdefault((r["family"], r["repo"], r["package"], r["class"]), []).append((r["peakMemoryMiB"], r["p90Cores"]))
    return by


def charge(xs):
    """machine-capacity's B0: the smallest value at or above 90% of the samples."""
    return min(x for x in xs if sum(y <= x for y in xs) * 10 >= len(xs) * 9)


def sweep_capacity_sizing(db):
    """Walk-forward over exact signatures: each run charged from its newest N prior runs (at least 3)."""
    margin = T.DEFAULTS["capacity_memory_margin"]
    by = capacity_series(db)
    out = []
    for n in (5, 10, 20, 40, 1000):
        under = slack = k = 0
        for xs in by.values():
            for i in range(3, len(xs)):
                prior = [m for m, _ in xs[max(0, i - n):i]]
                charged = charge(prior) * margin
                k += 1
                under += xs[i][0] > charged
                slack += charged / xs[i][0]
        out.append({"value": n, "n": k, "under_charged": under, "mean_charged_over_peak": slack / k if k else None})
    return out


def sweep_capacity_confidence(db):
    """Walk-forward under-charge rate and charge/peak spread by how many prior exact runs the estimate had."""
    margin = T.DEFAULTS["capacity_memory_margin"]
    window = T.DEFAULTS["capacity_sizing_runs"]
    bands = [(3, 5), (5, 10), (10, 20), (20, 10 ** 9)]
    acc = {b: [] for b in bands}
    for xs in capacity_series(db).values():
        for i in range(3, len(xs)):
            prior = [m for m, _ in xs[max(0, i - window):i]]
            band = next(b for b in bands if b[0] <= i < b[1])
            acc[band].append(xs[i][0] / (charge(prior) * margin))
    out = []
    for (lo, hi), rs in acc.items():
        rs.sort()
        out.append({"prior_runs": f"{lo}-{hi - 1}" if hi < 10 ** 9 else f"{lo}+", "n": len(rs), "over_charge": sum(r > 1 for r in rs),
                    "p50_peak_over_charge": T.quantile(rs, .5), "p90": T.quantile(rs, .9), "max": rs[-1] if rs else None})
    return out


def state_dir(a):
    import os
    return Path(a.state or Path(os.environ.get("XDG_STATE_HOME") or Path.home() / ".local/state") / "agents")


def jsonl_rows(path):
    try:
        lines = path.read_text(errors="replace").splitlines()
    except OSError:
        return []
    rows = []
    for line in lines:
        try:
            rows.append(json.loads(line))
        except ValueError:
            pass
    return [r for r in rows if isinstance(r, dict)]


def count_capacity_episodes(db, a):
    head, *_ = capacity_episodes(argparse.Namespace(**{**vars(a), "days": 60}))
    return head["episodes"], math.ceil(head["p90"]) if head["episodes"] else None


def count_astra_runs(db, a):
    from datetime import datetime
    stamp = lambda s: datetime.fromisoformat(s.replace("Z", "+00:00")).timestamp()
    week = [(stamp(r["ts"]), r["used_pct"], r["resets_at"]) for r in db.execute(
        "SELECT ts, used_pct, resets_at FROM usage WHERE provider = 'codex' AND kind = 'window' AND name = 'week'"
        " AND used_pct IS NOT NULL ORDER BY ts")]
    rises = []
    for r in db.execute("SELECT started, ended FROM runs WHERE role = 'worker' AND tier = 'gpt-6-astra medium'"
                        " AND started IS NOT NULL"):
        start, end = stamp(r["started"]), stamp(r["ended"])
        before = [(t, u, rs) for t, u, rs in week if t <= start]
        after = [(t, u, rs) for t, u, rs in week if t >= end]
        if before and after and before[-1][2] == after[0][2]:
            rises.append(max(0, after[0][1] - before[-1][1]))
    return len(rises), T.quantile(rises, .5)


def count_goal_targets(db, a):
    rows = jsonl_rows(state_dir(a) / "goal-scores.jsonl")
    return len({(r.get("project"), r.get("target")) for r in rows if r.get("on-date") in ("pass", "fail")}), None


def count_explored_tier_runs(db, a):
    runs = {}
    for (tier,) in db.execute("SELECT tier FROM runs WHERE role = 'worker'"):
        runs[T.tier_key(tier)] = runs.get(T.tier_key(tier), 0) + 1
    explored = [n for n in runs.values() if n >= T.DEFAULTS["explore_runs"]]
    return min(explored, default=0), None


def count_successor_outcomes(db, a):
    from datetime import datetime, timezone
    now = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    due = {r["model"]: r["at"] for r in db.execute(
        "SELECT model, max(at) AS at FROM model_events WHERE kind IN ('expected', 'market') AND at IS NOT NULL GROUP BY model")
        if r["at"] < now}
    return len(due), None


def count_capacity_denials(db, a):
    rows = jsonl_rows(state_dir(a) / "spawn-gate-denials.jsonl")
    return sum(r.get("gate") == "capacity" and bool(r.get("at")) for r in rows), None


def count_meter_test_haiku_runs(db, a):
    files = [[r for r in jsonl_rows(path) if "haiku" in str(r.get("model", "")) and r.get("scenario") is not None
              and r.get("five_hour_pct") is not None and r.get("seven_day_pct") is not None]
             for path in sorted((state_dir(a) / "replay").rglob("*.jsonl"), key=lambda p: p.stat().st_mtime)]
    files = [rows for rows in files if rows]
    pcts = [r["five_hour_pct"] for r in files[-1]] if files else []
    return sum(map(len, files)), max(pcts) - min(pcts) if pcts else None


def priors(db, a):
    import tomllib
    registry = tomllib.loads((ROOT / "socrates/defaults.toml").read_text())
    counted = {}
    for name, entry in registry.items():
        if not str(entry.get("source", "")).startswith("prior: "):
            continue
        settles = entry["settles_when"]
        fn = settles["count"]
        if fn not in counted:
            counted[fn] = globals()[fn](db, a)
        n, value = counted[fn]
        if n >= settles["need"]:
            yield f"SETTLED {name} n={n}" + (f" value={value:g}" if value is not None else "")
        else:
            yield f"WAIT {name} n={n}/{settles['need']}"


KINDS = {**{k: lambda db, a, k=k: sweep_tier(db, a.since, k) for k in TIER_GRIDS},
         "eta_min_samples": lambda db, a: sweep_eta_min_samples(db, a.since),
         "eta_coverage": lambda db, a: sweep_eta_coverage(db, a.since, T.DEFAULTS["overdue_ratio"]),
         "calibration_thin_n": lambda db, a: sweep_thin_n(db, a.since),
         "box_chain_gap_min": lambda db, a: sweep_box_gap(db, a.since),
         "box_calibration_n": lambda db, a: sweep_box_n(db, a.since),
         "stale_idle_min": lambda db, a: sweep_stale_idle(a),
         "capacity_min": lambda db, a: capacity_episodes(a),
         "capacity_sizing_runs": lambda db, a: sweep_capacity_sizing(db),
         "capacity_high_confidence_runs": lambda db, a: sweep_capacity_confidence(db),
         "money_ceiling": lambda db, a: sweep_money_ceiling(db, T.DEFAULTS["usage_stale_min"])}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("db", nargs="?", help="a copy of the threads ledger, opened read-only")
    p.add_argument("kind", nargs="?", choices=sorted(KINDS))
    p.add_argument("--priors", metavar="DB", help="count each prior's settling data in this ledger copy instead of a sweep")
    p.add_argument("--state", help="agents state directory (default $XDG_STATE_HOME/agents)")
    p.add_argument("--since", default="2026-10-05")
    p.add_argument("--transcripts", default="~/.claude/projects")
    p.add_argument("--days", type=float, default=7)
    a = p.parse_args()
    if a.priors:
        for line in priors(open_db(a.priors), a):
            print(line)
        return
    if not (a.db and a.kind):
        p.error("db and kind are required without --priors")
    for r in KINDS[a.kind](open_db(a.db), a):
        print(json.dumps({k: round(v, 4) if isinstance(v, float) else v for k, v in r.items()}))


if __name__ == "__main__":
    main()
