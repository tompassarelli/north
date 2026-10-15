import argparse
import json
import math
import os
from pathlib import Path
import subprocess
import sys
import tomllib
from datetime import datetime, timedelta, timezone

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "harness/lib"))
import calibration
import plan_usage

PREFIX = "codex_route_"
MARGIN = 0.10
WINDOW = timedelta(days=14)
Z = 1.959963984540054


def stamp(value):
    parsed = datetime.fromisoformat(value.replace("Z", "+00:00"))
    return parsed.replace(tzinfo=parsed.tzinfo or timezone.utc).astimezone(timezone.utc)


def iso(moment):
    return moment.astimezone(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def worker_runs(db):
    escalated = {f for (f,) in db.execute("SELECT follows FROM runs WHERE role = 'worker' AND follows IS NOT NULL")}
    columns = ("agent", "tier", "category", "outcome", "expects_landing", "landed", "item", "ended")
    out = []
    for r in db.execute(f"SELECT {', '.join(columns)} FROM runs WHERE role = 'worker' AND follows IS NULL"
                        " AND outcome != 'split' AND category IS NOT NULL AND tier IS NOT NULL"):
        r = dict(zip(columns, r))
        if plan_usage.unjudged(r) or not r["ended"]:
            continue
        out.append({"category": r["category"], "ended": stamp(r["ended"]),
                    "engine": "codex" if r["tier"].startswith("gpt-") else "claude",
                    "ok": plan_usage.succeeded(r, r["agent"] in escalated)})
    return out


def adopted_at(repo, key):
    done = subprocess.run(["git", "log", "-1", "--format=%cI", "--no-patch",
                           "-L", f"/^\\[{key}\\]$/,/^value = /:harness/defaults.toml"],
                          cwd=repo, text=True, capture_output=True)
    if done.returncode or not done.stdout.strip():
        raise RuntimeError(f"no commit sets {key}: {done.stderr.strip()}")
    return stamp(done.stdout.strip().splitlines()[0])


def wilson(k, n):
    p = k / n
    centre = (p + Z * Z / (2 * n)) / (1 + Z * Z / n)
    half = Z * math.sqrt(p * (1 - p) / n + Z * Z / (4 * n * n)) / (1 + Z * Z / n)
    return centre - half, centre + half


def newcombe(k1, n1, k2, n2):
    p1, p2 = k1 / n1, k2 / n2
    (l1, u1), (l2, u2) = wilson(k1, n1), wilson(k2, n2)
    d = p1 - p2
    return d - math.sqrt((p1 - l1) ** 2 + (u2 - p2) ** 2), d + math.sqrt((u1 - p1) ** 2 + (p2 - l2) ** 2)


def proposal(category, current, since, runs, now):
    cell = [r for r in runs if r["category"] == category and r["ended"] >= since and r["ended"] >= now - WINDOW]
    counts = {e: (sum(r["ok"] for r in cell if r["engine"] == e), sum(r["engine"] == e for r in cell))
              for e in ("codex", "claude")}
    proposed = "claude" if current == "codex" else "codex"
    row = {"dimension": f"codex-route-{category}", "default": PREFIX + category, "current": current,
           "proposed": proposed, "n": min(n for _, n in counts.values()), "eligible": True,
           "since": iso(since), "train_until": iso(since), "produced_at": iso(now)}
    rates = ", ".join(f"{e} {k}/{n}" + (f" {100 * k / n:.0f}%" if n else "") for e, (k, n) in counts.items())
    if row["n"] == 0:
        return dict(row, reason=f"{rates} since {iso(since)}: an engine has no judged runs")
    (k1, n1), (k2, n2) = counts[proposed], counts[current]
    shift = MARGIN if proposed == "codex" else -MARGIN
    low, high = newcombe(k1, n1, k2, n2)
    d = k1 / n1 - k2 / n2 + shift
    return dict(row, score_current=0, score_proposed=round(d, 4), ci_low=round(low + shift, 4),
                ci_high=round(high + shift, 4),
                reason=f"{rates} since {iso(since)}; {proposed} minus {current} {'+' if shift > 0 else '-'}"
                       f"{MARGIN * 100:.0f}-point margin")


def main():
    parser = argparse.ArgumentParser(description="Propose codex_route_<category> flips from held-out worker outcomes.")
    parser.add_argument("--repo", type=Path, default=ROOT)
    parser.add_argument("--state", type=Path, default=Path(os.environ.get("XDG_STATE_HOME", Path.home() / ".local/state")) / "agents/learn")
    args = parser.parse_args()
    now = datetime.now(timezone.utc)
    registry = tomllib.loads((args.repo / "harness/defaults.toml").read_text())
    db = calibration.open_ledger(calibration.ledger_path())
    try:
        runs = worker_runs(db)
    finally:
        db.close()
    out = args.state / "proposals"
    out.mkdir(parents=True, exist_ok=True)
    code = 0
    for key, entry in registry.items():
        if not key.startswith(PREFIX) or entry.get("value") not in ("codex", "claude"):
            continue
        category = key[len(PREFIX):]
        try:
            row = proposal(category, entry["value"], adopted_at(args.repo, key), runs, now)
        except RuntimeError as error:
            print(f"codex-route-propose: {category}: {error}", file=sys.stderr)
            code = 1
            continue
        path = out / f"codex-route-{category}.jsonl"
        tmp = path.with_suffix(".jsonl.tmp")
        tmp.write_text(json.dumps(row) + "\n")
        tmp.replace(path)
        print(f"{row['dimension']}: n={row['n']} {row['reason']}")
    return code


if __name__ == "__main__":
    sys.exit(main())
