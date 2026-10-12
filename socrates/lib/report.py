from datetime import datetime, timedelta, timezone
import json
import os
from pathlib import Path
import sqlite3
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
import calibration
from stall_recovery import recovery

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from policy_stats import wilson

WINDOW = timedelta(days=7)
GATES = ("capacity", "flow", "pace")
ARMS = ("escalate", "cheap-fix", "split")
ARM_MIN = 15
RUN_COLUMNS = ("agent", "arm", "follows", "outcome", "expects_landing", "landed")


def age(delta):
    minutes = int(delta.total_seconds() // 60)
    return f"{minutes}m" if minutes < 120 else f"{minutes // 60}h" if minutes < 2880 else f"{minutes // 1440}d"


def money(value):
    return "?" if value is None else f"${value:.2f}"


def calibration_lines(rows, now):
    latest = calibration.latest(rows)
    if not latest:
        return ["calibration: no calibration yet"]
    lines = ["calibration:"]
    for kind, batch in latest.items():
        thin = [r for r in batch if calibration.fallback(r, now)]
        head = f"  {kind} {age(now - calibration.stamp(batch[0]['at']))} ago"
        if len(batch) > 4:
            lines.append(f"{head}: {len(batch)} cells, {len(batch) - len(thin)} calibrated, "
                         f"n={sum(r['n'] or 0 for r in batch)}")
            continue
        cells = [f"{r['cell']} n={r['n']} value={r['value']:.2f}" if r["value"] is not None else f"{r['cell']} n={r['n']}"
                 for r in batch]
        lines.append(f"{head}: {', '.join(cells)}")
        lines += [f"    {r['cell']} (uncalibrated: {calibration.fallback(r, now)})" for r in thin]
    return lines


def arm_runs(db_path=None):
    try:
        with sqlite3.connect((db_path or calibration.snapshot_path()).resolve().as_uri() + "?mode=ro", uri=True) as db:
            return [dict(zip(RUN_COLUMNS, r)) for r in db.execute(
                f"SELECT {', '.join(RUN_COLUMNS)} FROM runs WHERE role = 'worker'")]
    except sqlite3.Error:
        return []


def arm_lines(runs):
    splits = {r["agent"]: r for r in runs if r["outcome"] == "split"}
    followed = {r["follows"] for r in runs if r["follows"]}
    tally = {a: [0, 0] for a in ARMS}
    for r in runs:
        if r["agent"] in splits:
            continue
        arm = (r["arm"] or (splits[r["follows"]]["arm"] if r["follows"] in splits else "") or "").lower()
        if arm in tally:
            tally[arm][1] += 1
            tally[arm][0] += (r["outcome"] == "done" and r["agent"] not in followed
                              and not (r["expects_landing"] and r["landed"] == 0))
    cells = []
    for arm, (ok, n) in tally.items():
        lo, hi = wilson(ok / n if n else 0.0, n)
        cells.append(f"{arm} {ok}/{n} [{lo:.0%}-{hi:.0%}]")
    thin = min(n for _, n in tally.values()) < ARM_MIN
    return [f"escalation arms: {', '.join(cells)}" + (f" (needs {ARM_MIN} runs per arm)" if thin else "")]


def denial_lines(denials, now):
    since = (now - WINDOW).strftime("%Y-%m-%dT%H:%M:%SZ")
    dated = [d for d in denials if d.get("at")]
    recent = [d for d in dated if d["at"] >= since]
    counts = {g: sum(d.get("gate") == g for d in recent) for g in GATES}
    undated = len(denials) - len(dated)
    tail = f", undated {undated} (logged before timestamps)" if undated else ""
    return [f"spawn-gate denials 7d: {len(recent)} ({', '.join(f'{g} {n}' for g, n in counts.items())}){tail}"]


def goal_lines(scores):
    latest = {}
    for s in scores:
        latest[s["project"]] = s
    if not latest:
        return ["goal scores: none scored yet"]
    lines = ["goal scores:"]
    for project, s in sorted(latest.items()):
        lines.append(f"  {project} target {s['target'][:10]}: on-date {s['on-date']}, in-budget {s['in-budget']} "
                     f"(spent {money(s.get('spent_usd'))} vs budget {money(s.get('budget_usd'))}), spec {s['spec-pass']}")
    return lines


def render(cal_rows, stall_lines, denials, scores, now, runs=()):
    return (calibration_lines(cal_rows, now) + arm_lines(runs) + stall_lines + denial_lines(denials, now)
            + goal_lines(scores))


def jsonl(path):
    try:
        return [json.loads(l) for l in path.read_text().splitlines() if l.strip()]
    except OSError:
        return []


def main():
    now = datetime.now(timezone.utc)
    state = Path(os.environ.get("XDG_STATE_HOME") or Path.home() / ".local/state") / "agents"
    stalls = recovery(state / "worker-sweep", (now - WINDOW).date().isoformat())
    print("\n".join(render(calibration.rows(), stalls, jsonl(state / "spawn-gate-denials.jsonl"),
                           jsonl(state / "goal-scores.jsonl"), now, arm_runs())))


if __name__ == "__main__":
    main()
