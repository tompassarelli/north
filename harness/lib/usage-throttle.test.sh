#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
north="${NORTH_BIN:-$root/target/debug/north}"
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT
mkdir -p "$scratch/profile/projects/-home-t/s1/subagents" "$scratch/op"
: >"$scratch/op/costs.toml"
stop_at="$(date -u -d '-3 min' +%Y-%m-%dT%H:%M:%S.123Z)"
printf '{"type":"user","message":{"content":"grep rate_limit 429"},"timestamp":"%s"}\n' "$stop_at" >"$scratch/profile/projects/-home-t/s1.jsonl"
printf '{"type":"assistant","isApiErrorMessage":true,"error":"rate_limit","apiErrorStatus":429,"timestamp":"%s","message":{"model":"<synthetic>","content":[{"type":"text","text":"You'"'"'ve hit your session limit · resets 7:20pm (Asia/Taipei)"}]}}\n' \
  "$stop_at" >"$scratch/profile/projects/-home-t/s1/subagents/agent-x.jsonl"
export THREADS_DB="$scratch/threads.db" AGENTS_OPERATOR_DIR="$scratch/op" XDG_STATE_HOME="$scratch/state"
scan() { "$north" usage-throttle --profile "a=$scratch/profile" --state "$scratch/seen.json" --threads "$root/bin/threads"; }
first="$(scan)"
second="$(scan)"
python3 -I - "$root/harness/lib" "${stop_at:0:19}Z" "$first" "$second" <<'PY'
import sqlite3, sys
from datetime import datetime, timedelta, timezone
sys.path.insert(0, sys.argv[1])
import placement, routing
stop, first, second = sys.argv[2:5]
db = sqlite3.connect(placement.os.environ["THREADS_DB"])
rows = db.execute("SELECT ts, source FROM usage WHERE provider='claude' AND account='a' AND name='session'").fetchall()
assert len(rows) == 1 and rows[0][0] == stop and "429" in rows[0][1], rows
assert first.count("\n") == 0 and '"kind":"throttle"' in first and second == "", (first, second)
later = (datetime.now(timezone.utc) - timedelta(seconds=30)).strftime("%Y-%m-%dT%H:%M:%SZ")
resets = (datetime.now(timezone.utc) + timedelta(hours=2)).strftime("%Y-%m-%dT%H:%M:%SZ")
db.execute("INSERT INTO usage(ts, provider, account, kind, name, used_pct, window_min, resets_at, source) VALUES (?, 'claude', 'a', 'window', 'session', 40, 300, ?, 'claude -p /usage')", (later, resets))
db.commit()
readings, throttled = placement.claude_session()
assert throttled == stop, (throttled, stop)
over, reason = routing.claude_session_overflow(readings, datetime.now(timezone.utc), throttled_at=throttled)
assert over and reason == "claude:a 5h 429 3 min ago", (over, reason)
print(f"PASS usage_throttle_429_row_drives_overflow: transcript 429 at {stop} -> one throttle row, rescan adds none, newer 40% reading keeps it, overflow '{reason}'")
PY
