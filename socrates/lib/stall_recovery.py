import json
from pathlib import Path
import sys


def recovery(base, since):
    base = Path(base)
    try:
        log = [json.loads(l) for l in (base / "stalls.jsonl").read_text().splitlines() if l.strip()]
    except OSError:
        log = []
    log = [r for r in log if r["at"] >= since]
    stages = {s: sum(r["stage"] == s for r in log) for s in ("nudge", "handoff", "escalation")}
    opened = sum(len(json.loads(p.read_text())) for p in base.glob("recovery-*.json"))
    share = f"{(stages['nudge'] + stages['handoff']) * 100 // len(log)}%" if log else "-"
    lines = [f"stall_recovery: closed={len(log)} nudge={stages['nudge']} handoff={stages['handoff']} "
            f"escalation={stages['escalation']} lapsed={sum(r['outcome'] == 'lapsed' for r in log)} open={opened} "
            f"without_escalation={share} (goal 70% over 7 days)"]
    try:
        events = [json.loads(l) for l in (base / "capacity.jsonl").read_text().splitlines() if l.strip()]
    except OSError:
        events = []
    counts = {}
    for e in (e for e in events if e["at"] >= since):
        key = f"{e['step']} {e.get('model') or 'none'} {e.get('effort') or '-'}"
        counts[key] = counts.get(key, 0) + 1
    if counts:
        lines.append("capacity_recovery: " + ", ".join(f"{k}={n}" for k, n in sorted(counts.items())))
    return lines


if __name__ == "__main__":
    print("\n".join(recovery(sys.argv[1], sys.argv[2])))
