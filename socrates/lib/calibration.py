from datetime import datetime, timezone
import os
from pathlib import Path
import sqlite3

from defaults import DEFAULTS

THIN_N = DEFAULTS["calibration_thin_n"]
RATE_N = DEFAULTS["min_runs"]
STALE_DAYS = DEFAULTS["calibration_stale_days"]
COLUMNS = ("at", "kind", "cell", "n", "p25", "median", "p75", "value", "source")


def snapshot_path():
    return Path(os.environ.get("THREADS_DB") or Path(os.environ.get("XDG_STATE_HOME") or Path.home() / ".local/state") / "threads/snapshot.db")


def stamp(value):
    result = datetime.fromisoformat(str(value).replace("Z", "+00:00"))
    return result if result.tzinfo else result.replace(tzinfo=timezone.utc)


def rows(db_path=None):
    try:
        with sqlite3.connect((db_path or snapshot_path()).resolve().as_uri() + "?mode=ro", uri=True) as db:
            return [dict(zip(COLUMNS, r)) for r in db.execute(f"SELECT {', '.join(COLUMNS)} FROM calibration")]
    except sqlite3.Error:
        return []


def latest(all_rows):
    newest = {}
    for r in all_rows:
        if r["kind"] not in newest or r["at"] > newest[r["kind"]]:
            newest[r["kind"]] = r["at"]
    return {kind: sorted((r for r in all_rows if r["kind"] == kind and r["at"] == at), key=lambda r: str(r["cell"]))
            for kind, at in sorted(newest.items())}


def newest_at(all_rows):
    return max((stamp(r["at"]) for r in all_rows), default=None)


def fallback(row, now):
    floor = RATE_N if row["kind"] == "tier" else THIN_N
    if (row["n"] or 0) < floor:
        return f"n={row['n'] or 0} < {floor}"
    if (now - stamp(row["at"])).days >= STALE_DAYS:
        return f"older than {STALE_DAYS} days"
    return None
