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
import ledger_context


def snapshot_runs():
    state = Path(os.environ.get("XDG_STATE_HOME") or Path.home() / ".local/state")
    path = Path(os.environ.get("THREADS_DB") or state / "threads/snapshot.db")
    if path.name == "threads.db":
        raise ValueError("quotes require the mirrored snapshot.db, not threads.db")
    with sqlite3.connect(path.resolve().as_uri() + "?mode=ro", uri=True) as db:
        db.row_factory = sqlite3.Row
        return [dict(row) for row in db.execute("SELECT * FROM runs")]


def quote(project, boxes, metrics, now):
    minutes = metrics["median_brief_to_landed_min"]
    cost = metrics["median_cost_per_done_usd"]
    date = ((now + timedelta(minutes=minutes * boxes / metrics["concurrency"]))
            .astimezone(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ") if minutes is not None else "n/a")
    dollars = f"{cost * boxes:.2f}" if cost is not None else "n/a"
    lines = [f"project {project}: {boxes} boxes, date {date}, ${dollars}; "
             f"median minutes/box {minutes if minutes is not None else 'n/a'}, "
             f"concurrency {metrics['concurrency']}, done boxes {metrics['done_boxes']}"]
    return lines, date, dollars


def main():
    parser = argparse.ArgumentParser(prog="agents plan --quote")
    parser.add_argument("--quote", required=True, action="store_true")
    parser.add_argument("--project", required=True)
    parser.add_argument("--boxes", required=True, type=int)
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
        lines, date, dollars = quote(args.project, args.boxes, metrics, datetime.now(timezone.utc))
        if "target" in projects[args.project]:
            lines.append(f"Blocked: decision for Tom: {args.project} date {date} ${dollars} [approve|later date|smaller scope]")
        print("\n".join(lines))
    except (OSError, ValueError, sqlite3.Error) as error:
        parser.exit(1, f"agents plan --quote: {error}\n")


if __name__ == "__main__":
    main()
