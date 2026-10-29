import argparse
from datetime import datetime, timedelta, timezone
import math
import os
from pathlib import Path
import re
import sqlite3
import sys
import tomllib

sys.path.insert(0, str(Path(__file__).resolve().parent))
from calibration import open_ledger
import goal_eta
import ledger_context


def snapshot_path():
    state = Path(os.environ.get("XDG_STATE_HOME") or Path.home() / ".local/state")
    return Path(os.environ.get("THREADS_DB") or state / "threads/snapshot.db")


def snapshot_runs():
    path = snapshot_path()
    if path.name == "threads.db":
        raise ValueError("quotes require the mirrored snapshot.db, not threads.db")
    with open_ledger(path) as db:
        db.row_factory = sqlite3.Row
        return [dict(row) for row in db.execute("SELECT * FROM runs")]


def quote(project, boxes, metrics, now, width, landing, eta=None, eta_n=0, eta_source=None):
    """Date by the critical path of `boxes` independent boxes at the spawn gate's width, each box the category cell's
    calibrated ETA (or the uncalibrated project median) plus the landing overhead; $ is the median $ per done box."""
    minutes = eta if eta is not None else metrics["median_brief_to_landed_min"]
    cost = metrics["median_cost_per_done_usd"]
    total = goal_eta.schedule({str(i): {"eta": minutes, "deps": set()} for i in range(boxes)}, width, landing) \
        if minutes is not None else None
    date = goal_eta.iso(now + timedelta(minutes=total)) if total is not None else "n/a"
    dollars = f"{cost * boxes:.2f}" if cost is not None else "n/a"
    source = eta_source or "uncalibrated: project median brief-to-landed"
    lines = [f"project {project}: {boxes} boxes, date {date}, ${dollars}; "
             f"minutes/box {minutes if minutes is not None else 'n/a'} (n={eta_n}, {source}), width {width}, "
             f"landing {landing:g}m/box, median minutes/box {metrics['median_brief_to_landed_min']}, "
             f"concurrency {metrics['concurrency']}, done boxes {metrics['done_boxes']}"]
    return lines, date, dollars


def main():
    parser = argparse.ArgumentParser(prog="agents plan --quote")
    parser.add_argument("--quote", required=True, action="store_true")
    parser.add_argument("--project", required=True)
    parser.add_argument("--boxes", required=True, type=int)
    parser.add_argument("--category", help="the boxes' Category, for the calibrated cell ETA")
    args = parser.parse_args()
    if args.boxes < 1:
        parser.error("--boxes must be positive")
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.-]*", args.project):
        parser.error("--project must be a project name")
    operator = Path(os.environ.get("AGENTS_OPERATOR_DIR") or Path.home() / "code/south/main/agents")
    config = Path(os.environ.get("AGENTS_ORCHESTRATION") or operator / "orchestration.toml")
    try:
        with config.open("rb") as source:
            projects = tomllib.load(source).get("projects", {})
        if args.project not in projects:
            parser.error(f"unknown project {args.project!r}")
        try:
            rows = snapshot_runs()
        except (sqlite3.OperationalError, OSError):
            rows = []
        metrics = ledger_context.delivery_metrics(rows, args.project)
        if any(value is not None and not math.isfinite(value) for value in metrics.values()):
            raise ValueError("worker ledger returned a non-finite measurement")
        eta, eta_n, source = None, 0, None
        if args.category:
            measured = goal_eta.box_durations([f"Category: {args.category}"], db_path=snapshot_path())
            eta, eta_n, source = goal_eta.box_eta(measured["boxes"][0], (measured["scale"], measured["label"]), None)
        width, _ = goal_eta.gate_width(args.project, ledger_context.delivery_metrics(rows)["concurrency"])
        lines, date, dollars = quote(args.project, args.boxes, metrics, datetime.now(timezone.utc), width,
                                     0.0 if eta is not None else goal_eta.landing_minutes(args.project), eta, eta_n, source)
        if "target" in projects[args.project]:
            lines.append(f"Blocked: decision for Tom: {args.project} date {date} ${dollars} [approve|later date|smaller scope]")
        print("\n".join(lines))
    except (OSError, ValueError, sqlite3.Error) as error:
        parser.exit(1, f"agents plan --quote: {error}\n")


if __name__ == "__main__":
    main()
