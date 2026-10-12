#!/usr/bin/env python3
"""Compare policy-replay arms (model, effort, advisor) against an incumbent: pass rate, paired task-cluster difference, list and plan $ per success, wall time, advisor use and one verdict per challenger."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import random
import statistics
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "lib"))
from defaults import DEFAULTS
from policy_stats import paired_diff, wilson

SEED, B = 0, 4000


def load(paths: list[Path], arm_key: str | None) -> dict[str, list[dict]]:
    arms: dict[str, list[dict]] = {}
    for path in paths:
        for line in path.read_text().splitlines():
            if line.strip():
                row = json.loads(line)
                arms.setdefault(str(row[arm_key]) if arm_key else path.stem, []).append(row)
    return arms


def plan_costs(rows: list[dict], codex_plan_usd: float | None) -> list[float | None]:
    codex = sum(1 for r in rows if r.get("provider") == "codex")
    ratio = DEFAULTS["plan_per_list_claude"]
    return [(codex_plan_usd / codex if codex_plan_usd is not None else None) if r.get("provider") == "codex"
            else (r.get("usd") or 0.0) * ratio for r in rows]


def per(total: float | None, count: int) -> float | None:
    return total / count if total is not None and count else None


def plan_upper(rows: list[dict], plans: list[float | None], seed: int = SEED, b: int = B) -> float | None:
    if any(p is None for p in plans):
        return None
    clusters: dict[str, list[float]] = {}
    for r, p in zip(rows, plans):
        c = clusters.setdefault(r["scenario"], [0.0, 0.0])
        c[0] += p
        c[1] += 1 if r.get("pass") is True else 0
    cells = [clusters[s] for s in sorted(clusters)]
    if not cells:
        return None
    rng = random.Random(seed)
    boot = []
    for _ in range(b):
        pick = [rng.choice(cells) for _ in cells]
        wins = sum(c[1] for c in pick)
        boot.append(sum(c[0] for c in pick) / wins if wins else float("inf"))
    boot.sort()
    return boot[min(b - 1, int(0.975 * b))]


def summarize(rows: list[dict], codex_plan_usd: float | None = None) -> dict:
    scored = [r for r in rows if r.get("pass") is not None]
    wins = sum(1 for r in scored if r["pass"])
    rate = wins / len(scored) if scored else 0.0
    priced = [r["usd"] for r in rows if r.get("usd") is not None]
    plans = plan_costs(rows, codex_plan_usd)
    plan_total = None if any(p is None for p in plans) else sum(plans)
    claude_usd = sum(r.get("usd") or 0.0 for r in rows if r.get("provider") != "codex")
    walls = [r["wall_s"] for r in rows if r.get("wall_s") is not None]
    calls = [r.get("advisor_calls") or 0 for r in rows]
    return {
        "runs": len(rows), "n": len(scored), "wins": wins, "rate": rate, "wilson": wilson(rate, len(scored)),
        "list_run": per(sum(priced), len(priced)) if priced else None,
        "list_success": per(sum(priced), wins) if priced else None,
        "plan_run": per(plan_total, len(rows)), "plan_success": per(plan_total, wins),
        "plan_success_hi": plan_upper(rows, plans),
        "wall_median": statistics.median(walls) if walls else None,
        "advisor_mean": sum(calls) / len(calls) if calls else 0.0,
        "advisor_share_runs": sum(1 for c in calls if c > 0) / len(calls) if calls else 0.0,
        "advisor_usd_share": sum(r.get("advisor_usd") or 0.0 for r in rows) / claude_usd if claude_usd else None,
    }


def verdict(challenger: dict, incumbent: dict, diff: tuple[float, float, float], margin: float) -> str:
    _, lo, hi = diff
    if challenger["wilson"][0] > incumbent["wilson"][1]:
        return "BEATS"
    cheaper = (challenger["plan_success_hi"] is not None and incumbent["plan_success"] is not None
               and challenger["plan_success_hi"] < incumbent["plan_success"])
    if lo >= -margin and cheaper:
        return "MATCHES-FOR-LESS"
    if hi < margin:
        return "NO-GAIN"
    return "UNDECIDED"


def report(arms: dict[str, list[dict]], incumbent: str, codex_plan: dict[str, float]) -> tuple[dict, dict, dict]:
    stats = {arm: summarize(rows, codex_plan.get(arm)) for arm, rows in arms.items()}
    diffs = {arm: paired_diff(rows, arms[incumbent], seed=SEED, b=B) for arm, rows in arms.items() if arm != incumbent}
    margin = DEFAULTS["noninferiority_margin"] / 100
    verdicts = {arm: verdict(stats[arm], stats[incumbent], diffs[arm], margin) for arm in diffs}
    return stats, diffs, verdicts


def money(x: float | None) -> str:
    return "-" if x is None else "inf" if x == float("inf") else f"{x:.4f}"


def pct(x: float | None) -> str:
    return "-" if x is None else f"{100 * x:.1f}%"


def render(stats: dict, diffs: dict, verdicts: dict, incumbent: str) -> str:
    head = ("arm", "n", "pass", "wilson95", f"diff vs {incumbent}", "list$/run", "list$/win", "plan$/run", "plan$/win",
            "plan$/win hi", "wall_med", "adv mean", "adv runs", "adv $")
    table = [head]
    for arm, s in stats.items():
        d = diffs.get(arm)
        table.append((
            arm, str(s["n"]), pct(s["rate"]), f"[{pct(s['wilson'][0])}, {pct(s['wilson'][1])}]",
            "incumbent" if d is None else f"{100 * d[0]:+.1f} [{100 * d[1]:+.1f}, {100 * d[2]:+.1f}]",
            money(s["list_run"]), money(s["list_success"]), money(s["plan_run"]), money(s["plan_success"]),
            money(s["plan_success_hi"]), "-" if s["wall_median"] is None else f"{s['wall_median']:.1f}",
            f"{s['advisor_mean']:.2f}", pct(s["advisor_share_runs"]), pct(s["advisor_usd_share"]),
        ))
    widths = [max(len(row[i]) for row in table) for i in range(len(head))]
    lines = ["  ".join(c.ljust(w) for c, w in zip(row, widths)).rstrip() for row in table]
    lines += [f"{arm}: {v} vs {incumbent}" for arm, v in verdicts.items()]
    return "\n".join(lines)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("files", type=Path, nargs="+", help="policy-replay results jsonl")
    ap.add_argument("--arm-key", help="row field naming the arm (default: the file stem)")
    ap.add_argument("--incumbent", required=True, metavar="ARM")
    ap.add_argument("--codex-plan-usd", action="append", default=[], metavar="ARM=VALUE",
                    help="plan $ an arm's codex runs cost in total, spread per run; repeatable")
    args = ap.parse_args()
    codex_plan = {}
    for item in args.codex_plan_usd:
        arm, sep, value = item.partition("=")
        if not sep:
            ap.error(f"--codex-plan-usd {item!r} is not ARM=VALUE")
        codex_plan[arm] = float(value)
    arms = load(args.files, args.arm_key)
    if args.incumbent not in arms:
        ap.error(f"incumbent {args.incumbent!r} is not one of {sorted(arms)}")
    print(render(*report(arms, args.incumbent, codex_plan), args.incumbent))
    return 0


if __name__ == "__main__":
    sys.exit(main())
