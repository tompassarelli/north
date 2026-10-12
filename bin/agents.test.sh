#!/usr/bin/env bash
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
agents="$repo/bin/agents"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/agents-test.XXXXXX")"
trap 'rm -rf "${scratch:?}"' EXIT

if AGENTS_NORTH_BIN="$scratch/bin/missing" "$agents" status \
  >"$scratch/missing.out" 2>"$scratch/missing.err"; then
  printf 'agents accepted a missing North CLI\n' >&2
  exit 1
fi

printf 'ok: agents is a thin North CLI client\n'

export AGENTS_ORCHESTRATION="$scratch/orchestration.toml" THREADS_DB="$scratch/snapshot.db"
cat >"$AGENTS_ORCHESTRATION" <<'TOML'
[projects.north]
target = "2026-10-13T09:30:00Z"
[projects.wisp]
TOML
python3 - "$THREADS_DB" <<'PYTHON'
import sqlite3
import sys
with sqlite3.connect(sys.argv[1]) as db:
    db.execute("CREATE TABLE runs(agent TEXT, item TEXT, outcome TEXT, role TEXT, staffed TEXT, started TEXT, ended TEXT, landed_at TEXT, cost_usd REAL, expects_landing INTEGER, landed INTEGER)")
    db.executemany("INSERT INTO runs VALUES (?, ?, 'done', 'worker', ?, ?, ?, ?, ?, 1, 1)", [
        (f"{project}-{number}", f"{project}#{number}", "2026-10-12T06:00:00Z", "2026-10-12T06:00:00Z", end, end, cost)
        for project in ("north", "wisp") for number, end, cost in ((1, "2026-10-12T06:20:00Z", 1), (2, "2026-10-12T06:40:00Z", 3))])
PYTHON
"$agents" plan --quote --project north --boxes 4 >"$scratch/dated.out"
"$agents" plan --quote --project wisp --boxes 4 >"$scratch/undated.out"
python3 - "$scratch" <<'PYTHON'
from datetime import datetime, timezone
from pathlib import Path
import re
import sys
scratch = Path(sys.argv[1])
dated = (scratch / "dated.out").read_text().splitlines()
undated = (scratch / "undated.out").read_text().splitlines()
decisions = [line for line in dated if line.startswith("Blocked: decision for Tom:")]
assert len(decisions) == 1, dated
assert not any(line.startswith("Blocked: decision for Tom:") for line in undated), undated
match = re.fullmatch(r"Blocked: decision for Tom: north date (.+) \$8.00 \[approve\|later date\|smaller scope\]", decisions[0])
assert match, decisions
minutes = (datetime.fromisoformat(match[1].replace("Z", "+00:00")) - datetime.now(timezone.utc)).total_seconds() / 60
assert 59 <= minutes <= 60, minutes
assert "median minutes/box 30" in dated[0] and "concurrency 2" in dated[0], dated
PYTHON
printf 'ok: project quote uses snapshot measurements and one dated decision\n'
