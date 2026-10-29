#!/usr/bin/env bash
# shellcheck disable=SC2016,SC2034
set -uo pipefail

repo=$(cd "$(dirname "$0")/.." && pwd)
scratch=$(mktemp -d "${TMPDIR:-/tmp}/laptop-test.XXXXXX")
trap 'rm -rf "${scratch:?}"' EXIT
mkdir -p "$scratch/bin" "$scratch/run" "$scratch/state"
export THREADS_DB=$scratch/threads.db LAPTOP_STATE=$scratch/placement/laptop.json
export LAPTOP_WINDOW_FILE=$scratch/window.json XDG_RUNTIME_DIR=$scratch/run XDG_STATE_HOME=$scratch/state
export AGENTS_ORCHESTRATION=$scratch/orchestration.toml PATH=$scratch/bin:$repo/bin:$PATH NORTH_BIN=$(cd "$repo" && realpath "${CARGO_TARGET_DIR:-target}")/debug/north
: >"$AGENTS_ORCHESTRATION"
printf '{"tz": "Etc/GMT-8", "open": "23:00", "close": "10:00"}\n' >"$LAPTOP_WINDOW_FILE"
printf '#!/bin/sh\nexit 1\n' >"$scratch/bin/bun"
printf '#!/bin/sh\nexit 1\n' >"$scratch/bin/agents"
chmod +x "$scratch/bin/bun" "$scratch/bin/agents"

pass=0 fail=0 out=""
check() {
  if eval "$2"; then pass=$((pass + 1)); printf 'PASS  %s\n' "$1"
  else fail=$((fail + 1)); printf 'FAIL  %s\n%s\n' "$1" "$out"; fi
}
at() { LAPTOP_NOW=$1 laptop "${@:2}" 2>&1; }
field() { jq -r ".$2" <<<"$(at "$1" status --json)"; }
qstate() { sqlite3 "$THREADS_DB" "select state from job_queue where id = $1"; }

a=$(threads queue add laptop - -- echo a)
b=$(threads queue add laptop - -- echo b)
threads queue take laptop >/dev/null
out=$(threads queue msg "$a" requeued "rc=143" 2>&1)
took=$(threads queue take laptop | jq -r .id)
check '[spec] msg requeued puts a taken job back ahead of later jobs as requeued, and take claims it first' \
  '[ "$took" = "$a" ] && [ "$(qstate "$b")" = queued ] && threads queue msg "$a" done ok >/dev/null'

out=$(at 2026-10-13T12:00:00Z drain; at 2026-10-13T12:00:00Z status --json)
d=$(field 2026-10-13T12:00:00Z deadline)
check '[spec] drain is draining for 15 minutes from the word, recorded with reason and a ledger note' \
  '[ "$(field 2026-10-13T12:00:00Z state)" = draining ] && [ "$d" = "$(date -d 2026-10-13T12:15:00Z +%s)" ] && sqlite3 "$THREADS_DB" "select data from events where kind = '"'note'"'" | grep -q "laptop: draining since 20:00"'

out=$(at 2026-10-13T12:01:00Z close lid; at 2026-10-13T12:01:00Z status --json)
check '[spec] close drains with a 2-minute grace' \
  '[ "$(field 2026-10-13T12:01:00Z deadline)" = "$(date -d 2026-10-13T12:03:00Z +%s)" ] && [ "$(field 2026-10-13T12:01:00Z reason)" = lid ]'

c=$(threads queue add laptop - -- echo c)
held_id=$(threads queue take laptop | jq -r .id)
out=$(at 2026-10-13T12:05:00Z tick)
held=$(field 2026-10-13T12:05:00Z state)
threads queue msg "$held_id" requeued drained >/dev/null
out=$(at 2026-10-13T12:06:00Z tick)
check '[spec] tick keeps draining while a laptop job runs and settles unavailable once none is left' \
  '[ "$held" = draining ] && [ "$(field 2026-10-13T12:06:00Z state)" = unavailable ]'

out=$(at 2026-10-13T14:59:00Z tick; at 2026-10-13T15:00:00Z tick)
opened=$(field 2026-10-13T15:00:00Z state)
out=$(at 2026-10-13T17:00:00Z close until 03:00 back later; at 2026-10-13T18:59:00Z tick)
heldword=$(field 2026-10-13T18:59:00Z state)
out=$(at 2026-10-13T19:00:00Z tick)
reopened=$(field 2026-10-13T19:00:00Z state)
out=$(at 2026-10-14T02:00:00Z tick)
check '[spec] the window opens at 23:00 and drains at 10:00 (UTC+8 here), settling unavailable with nothing running; a word holds until its until, then the window state returns' \
  '[ "$opened" = available ] && [ "$heldword" = unavailable ] && [ "$reopened" = available ] && sqlite3 "$THREADS_DB" "select data from events where kind = '"'note'"'" | grep -q "laptop: draining since 10:00, window by window" && [ "$(field 2026-10-14T02:00:00Z state)" = unavailable ]'

out=$(at 2026-10-14T02:00:00Z status)
check '[spec] status prints one laptop: line with since, reason and running and queued laptop jobs' \
  '[[ "$out" == "laptop: unavailable since 10:00, drained (window) by laptop tick (0 running, 2 queued)" ]]'

cat >"$scratch/bin/laptop" <<'SH'
#!/bin/sh
cat "$LAPTOP_STUB"
SH
chmod +x "$scratch/bin/laptop"
export LAPTOP_STUB=$scratch/stub.json LAPTOP_RUNNER_POLL=1
printf '{"state":"draining","deadline":%s}\n' "$(( $(date +%s) + 3600 ))" >"$LAPTOP_STUB"
out=$(laptop-runner 2>&1)
gated=$(qstate "$b")
printf '{"state":"available"}\n' >"$LAPTOP_STUB"
out=$(laptop-runner 2>&1)
check '[spec] laptop-runner takes no laptop job unless the state is available, then runs the queue' \
  '[ "$gated" = requeued ] && [ "$(qstate "$b")" = done ] && [ "$(qstate "$c")" = done ]'

printf '{"state":"available"}\n' >"$LAPTOP_STUB"
u=$(threads queue add laptop - -- echo u)
mv "$LAPTOP_STUB" "$scratch/stub.saved"
out=$(laptop-runner 2>&1)
check '[spec] with the ledger host unreachable and the cached state available, the runner keeps taking jobs' \
  '[ "$(qstate "$u")" = done ] && jq -e ".state == \"available\"" "$XDG_STATE_HOME/laptop-runner/laptop-state.json" >/dev/null'
mv "$scratch/stub.saved" "$LAPTOP_STUB"

long=$(threads queue add laptop - -- sh -c 'echo $$ >"$0"; sleep 120' "$scratch/long.pid")
after=$(threads queue add laptop - -- echo after)
( until [ -s "$scratch/long.pid" ]; do sleep 0.1; done; printf '{"state":"draining","deadline":%s}\n' "$(date +%s)" >"$LAPTOP_STUB" ) & drain=$!
out=$(laptop-runner 2>&1)
kill "$drain" 2>/dev/null
check '[spec] past the drain deadline the runner SIGTERMs the job-run process group and records the job requeued, taking nothing more' \
  '[ "$(qstate "$long")" = requeued ] && [ "$(qstate "$after")" = queued ] && ! kill -0 "$(cat "$scratch/long.pid")" 2>/dev/null && sqlite3 "$THREADS_DB" "select result from job_queue where id = $long" | grep -q "^rc=143"'

printf 'laptop: %d passed, %d failed\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
