from datetime import datetime, timedelta, timezone
import json
import os
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
import calibration
from stall_recovery import recovery

WINDOW = timedelta(days=7)
GATES = ("capacity", "flow", "pace")


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


def render(cal_rows, stall_lines, denials, scores, now):
    return calibration_lines(cal_rows, now) + stall_lines + denial_lines(denials, now) + goal_lines(scores)


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
                           jsonl(state / "goal-scores.jsonl"), now)))


if __name__ == "__main__":
    main()
