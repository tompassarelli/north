from datetime import datetime, timezone
import json
import math
import os
from pathlib import Path
import sqlite3
import subprocess
import sys

from defaults import DEFAULTS

THIN_N = DEFAULTS["calibration_thin_n"]
RATE_N = DEFAULTS["min_runs"]
STALE_DAYS = DEFAULTS["calibration_stale_days"]
BOX_N = DEFAULTS["box_calibration_n"]
GOAL_KINDS = ("goal_eta", "goal_actual")
COLUMNS = ("at", "kind", "cell", "n", "p25", "median", "p75", "value", "source")


def threads_call(verb, request=None, db=None, timeout=600):
    """The JSON a `north threads` calibration verb prints for a JSON request on stdin, over the ledger at `db`; a verb that
    reads only its request runs against no ledger, so it never reaches for the remote one."""
    env = dict(os.environ, THREADS_SCHEMA_READY="1", THREADS_DB=str(os.devnull if db is None else db))
    result = subprocess.run([os.environ.get("NORTH_BIN") or "north", "threads", verb], input="" if request is None else json.dumps(request),
                            capture_output=True, text=True, env=env, timeout=timeout)
    if result.returncode:
        raise OSError(result.stderr.strip() or f"north threads {verb} exited {result.returncode}")
    return json.loads(result.stdout)


def quantile(values, fraction):
    values = sorted(v for v in values if v is not None)
    return values[max(0, math.ceil(len(values) * fraction) - 1)] if values else None


def owns_ledger():
    return not (Path.home() / ".config/threads/remote").exists()


def ledger_path():
    if os.environ.get("THREADS_DB"):
        return Path(os.environ["THREADS_DB"])
    state = Path(os.environ.get("XDG_STATE_HOME") or Path.home() / ".local/state") / "threads"
    return state / ("threads.db" if owns_ledger() else "snapshot.db")


def open_ledger(path=None, timeout=10):
    path = Path(path or ledger_path()).resolve()
    remote = Path.home() / ".config/threads/remote"
    snapshot = path.name == "snapshot.db"
    for attempt in range(2):
        db = None
        try:
            db = sqlite3.connect(path.as_uri() + "?mode=ro" + ("&immutable=1" if snapshot else ""),
                                 uri=True, timeout=timeout)
            if snapshot and db.execute("PRAGMA quick_check").fetchall() != [("ok",)]:
                raise sqlite3.DatabaseError("snapshot integrity check failed")
            return db
        except sqlite3.DatabaseError as error:
            if db is not None:
                db.close()
            if not snapshot or not remote.is_file() or os.environ.get("THREADS_DB"):
                raise
            if attempt:
                message = f"NorthThreads: snapshot still unreadable after one refresh: {error}"
                print(message, file=sys.stderr)
                raise sqlite3.DatabaseError(message) from error
            import runpy
            threads = runpy.run_path(str(Path(__file__).resolve().parents[2] / "bin/threads"))
            threads["refresh_snapshot"](remote.read_text().strip(), force=True, snapshot=path)


def stamp(value):
    result = datetime.fromisoformat(str(value).replace("Z", "+00:00"))
    return result if result.tzinfo else result.replace(tzinfo=timezone.utc)


def rows(db_path=None):
    try:
        with open_ledger(db_path) as db:
            return [dict(zip(COLUMNS, r)) for r in db.execute(f"SELECT {', '.join(COLUMNS)} FROM calibration")]
    except sqlite3.Error:
        return []


def latest(all_rows):
    newest = {}
    for r in all_rows:
        if r["kind"] in GOAL_KINDS:
            continue
        if r["kind"] not in newest or r["at"] > newest[r["kind"]]:
            newest[r["kind"]] = r["at"]
    return {kind: sorted((r for r in all_rows if r["kind"] == kind and r["at"] == at), key=lambda r: str(r["cell"]))
            for kind, at in sorted(newest.items())}


def fallback(row, now):
    floor = RATE_N if row["kind"] == "tier" else BOX_N if row["kind"] == "box" else THIN_N
    if (row["n"] or 0) < floor:
        return f"n={row['n'] or 0} < {floor}"
    if (now - stamp(row["at"])).days >= STALE_DAYS:
        return f"older than {STALE_DAYS} days"
    return None
