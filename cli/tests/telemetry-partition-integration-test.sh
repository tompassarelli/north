#!/usr/bin/env bash
# Stage-A telemetry partition: independent fenced writers, composed live reads,
# legacy telemetry preservation, failure isolation, and flag-only rollback.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
fram="${FRAM_PATH:-${FRAM_HOME:-$root/../fram/main}}"
for required in "$fram/bin/fram-daemon" "$fram/out/fram/rt.clj"; do
  [[ -e "$required" ]] || {
    echo "telemetry partition integration: missing Fram runtime: $required" >&2
    exit 2
  }
done

tmp="$(mktemp -d -t 'north telemetry partition.XXXXXX')"
coord_pid=
telemetry_pid=
cleanup() {
  for pid in "$telemetry_pid" "$coord_pid"; do
    if [[ -n "$pid" ]]; then
      kill "$pid" 2>/dev/null || true
      wait "$pid" 2>/dev/null || true
    fi
  done
  rm -rf "${tmp:?}"
}
trap cleanup EXIT

mkdir -p "$tmp/logs"
coord_log="$tmp/logs/coordination.log"
telemetry_log="$tmp/logs/telemetry.log"
: >"$coord_log"
printf '%s\n' \
  '{:tx 1 :op "assert" :l "@run:legacy" :p "kind" :r "run"}' \
  '{:tx 2 :op "assert" :l "@session:outage-presence" :p "agent" :r "outage-presence"}' \
  '{:tx 3 :op "assert" :l "@session:outage-presence" :p "session_id" :r "legacy-session"}' \
  '{:tx 4 :op "assert" :l "@session:outage-presence" :p "current_thread" :r "legacy-decoy"}' \
  >"$telemetry_log"

if ! ports="$(
  bb -e '(with-open [a (java.net.ServerSocket. 0)
                     b (java.net.ServerSocket. 0)]
           (println (.getLocalPort a) (.getLocalPort b)))' \
    2>"$tmp/port.err"
)"; then
  echo "telemetry partition integration: SKIP — loopback bind unavailable"
  exit 0
fi
read -r coord_port telemetry_port <<<"$ports"

start_writer() {
  local port="$1" log="$2" output="$3"
  env -u FRAM_TELEMETRY_LOG -u NORTH_TELEMETRY_PARTITION -u NORTH_TELEMETRY_PORT \
    FRAM_REQUIRE_LOG_FENCE=1 FRAM_PORT="$port" FRAM_LOG="$log" \
    "$fram/bin/fram-daemon" "$port" "$log" \
    >"$output.out" 2>"$output.err" &
}

await_writer() {
  local port="$1" log="$2" pid="$3"
  local result=
  for _ in $(seq 1 120); do
    result="$(
      FRAM_LOG="$log" bb "$root/cli/coord.clj" strict-probe "$port" "$log" \
        2>/dev/null || true
    )"
    if grep -q ':ready true' <<<"$result"; then
      return 0
    fi
    kill -0 "$pid" 2>/dev/null || break
    sleep 0.25
  done
  return 1
}

start_writer "$coord_port" "$coord_log" "$tmp/coord"
coord_pid=$!
start_writer "$telemetry_port" "$telemetry_log" "$tmp/telemetry"
telemetry_pid=$!
if ! await_writer "$coord_port" "$coord_log" "$coord_pid"; then
  cat "$tmp/coord.err" >&2
  echo "telemetry partition integration: coordination writer did not start" >&2
  exit 1
fi
if ! await_writer "$telemetry_port" "$telemetry_log" "$telemetry_pid"; then
  cat "$tmp/telemetry.err" >&2
  echo "telemetry partition integration: telemetry writer did not start" >&2
  exit 1
fi

partition_env=(
  NORTH_TELEMETRY_PARTITION=1
  NORTH_PORT="$coord_port"
  NORTH_TELEMETRY_PORT="$telemetry_port"
  FRAM_PORT="$coord_port"
  FRAM_LOG="$coord_log"
  FRAM_TELEMETRY_LOG="$telemetry_log"
  NORTH_TEST_ROOT="$root"
  NORTH_TEST_COORD_PORT="$coord_port"
)

composed="$(
  env "${partition_env[@]}" bb -e '
    (load-file (str (System/getenv "NORTH_TEST_ROOT") "/cli/coord.clj"))
    (let [port (Integer/parseInt (System/getenv "NORTH_TEST_COORD_PORT"))]
      (north.coord/append! port "@thread:current" "note" "coordination-origin")
      (north.coord/append! port "@run:current" "kind" "run")
      (let [view (north.coord/live-facts-view port)
            page
            (north.coord/query-page-in-domain
             port :telemetry
             {:find "telemetry_run"
              :rules [{:head {:rel "telemetry_run" :args [{:var "e"}]}
                       :body [{:rel "triple"
                               :args [{:var "e"} "kind" "run"]}]}]}
             16 nil)
            page-subjects (set (map first (:ok page)))]
        (when-not (= #{"@run:current" "@run:legacy"} page-subjects)
          (throw (ex-info "telemetry query page missed an origin fact"
                          {:subjects page-subjects})))
        (prn {:view view :run-page-ok true})))'
)"
grep -Fq '["@thread:current" "note" "coordination-origin"]' <<<"$composed"
grep -Fq '["@run:current" "kind" "run"]' <<<"$composed"
grep -Fq '["@run:legacy" "kind" "run"]' <<<"$composed"
grep -Fq ':complete true' <<<"$composed"
grep -Fq ':run-page-ok true' <<<"$composed"
grep -Fq '@thread:current' "$coord_log"
if grep -Fq '@run:current' "$coord_log"; then
  echo "telemetry partition integration: telemetry write crossed into coordination origin" >&2
  exit 1
fi
grep -Fq '@run:current' "$telemetry_log"

stage_a_show="$(
  env "${partition_env[@]}" FRAM_HOME="$fram" \
    "$root/bin/north" show run:current
)"
grep -Fq '  kind  run' <<<"$stage_a_show"

stage_a_history="$(
  env "${partition_env[@]}" FRAM_HOME="$fram" \
    "$root/bin/north" history run:current
)"
grep -Fq 'history of @run:current' <<<"$stage_a_history"
grep -Fq 'kind = run' <<<"$stage_a_history"

kill "$telemetry_pid"
wait "$telemetry_pid" 2>/dev/null || true
telemetry_pid=
# Hash after the writer has fully exited: a dirty writer appends its own
# shutdown checkpoint on SIGTERM, which is not a cross-origin write. The
# invariant is that nothing mutates the log once its sole writer is dead.
telemetry_before="$(sha256sum "$telemetry_log" | cut -d' ' -f1)"

# Presence is coordination authority, not telemetry. A legacy telemetry session
# descriptor remains readable history, but cannot block registration, provide
# liveness, override current focus, or produce a second live row.
presence_env=(
  "${partition_env[@]}"
  NORTH_COORD_CONNECT_TIMEOUT_MS=250
  NORTH_COORD_READ_TIMEOUT_MS=1000
)
env "${presence_env[@]}" \
  bb "$root/cli/presence-cli.clj" "$coord_port" \
  register outage-presence "$root" outage-session \
  >"$tmp/presence-register.out"
env "${presence_env[@]}" \
  bb "$root/cli/presence-cli.clj" "$coord_port" \
  focus outage-presence coordination-focus coordination-workflow \
  >"$tmp/presence-focus.out"
env "${presence_env[@]}" \
  bb "$root/cli/presence-cli.clj" "$coord_port" \
  task outage-presence coordination-task \
  >"$tmp/presence-task.out"
env "${presence_env[@]}" \
  bb "$root/cli/presence-cli.clj" "$coord_port" \
  renew outage-presence \
  >"$tmp/presence-renew.out"
env "${presence_env[@]}" \
  bb "$root/cli/presence-cli.clj" "$coord_port" presence-online-json \
  >"$tmp/presence-online.json"
env "${presence_env[@]}" \
  bb "$root/cli/presence-cli.clj" "$coord_port" presence-online \
  >"$tmp/presence-online.out"
python3 - "$tmp/presence-online.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)
rows = [
    row for row in payload["sessions"]
    if row["control_id"] == "outage-presence"
]
assert len(rows) == 1
assert rows[0]["online"] is True
PY
grep -Fq 'outage-presence' "$tmp/presence-online.out"
grep -Fq 'coordination-workflow' "$tmp/presence-online.out"
if grep -Fq 'legacy-decoy' "$tmp/presence-online.out"; then
  echo "telemetry partition integration: legacy telemetry focus became live authority" >&2
  exit 1
fi

# Roster enrichment must stay on the same origin as the liveness lease. This
# exact-subject slice succeeds while the telemetry socket is absent and cannot
# import the legacy @session descriptor into the live row.
env "${presence_env[@]}" \
  NORTH_AGENTS_LIB=1 \
  bb -e '
    (System/setProperty "north.agents.lib" "1")
    (load-file (str (System/getenv "NORTH_TEST_ROOT") "/cli/agents-cli.clj"))
    (let [projection (roster-facts ["outage-presence"])
          facts (get-in projection [:agents "outage-presence"])]
      (when-not
       (and (= "outage-session" (get facts "session_id"))
            (= "coordination-focus" (get facts "current_thread"))
            (= "coordination-workflow" (get facts "active_workflow"))
            (= {} (:sessions projection)))
        (throw (ex-info "live roster did not use its coordination-only slice"
                        {:projection projection}))))'

# The real native hook's focused fixture exercises this publisher end to end.
# Keep the cross-origin test joined to that exact coordination-owned subject
# rather than copying the publisher implementation into this harness.
grep -Fq 'NORTH_NATIVE_SUBJECT="@agent:$ID"' "$root/bin/north-on-spawn"

grep -Fq '@agent:outage-presence' "$coord_log"
grep -Fq '@lease:session:outage-presence' "$coord_log"
grep -Fq '"session_id", :r "outage-session"' "$coord_log"
grep -Fq '"current_thread", :r "coordination-focus"' "$coord_log"
grep -Fq '"active_workflow", :r "coordination-workflow"' "$coord_log"
grep -Fq '"task", :r "coordination-task"' "$coord_log"
if grep -Fq '@session:outage-presence' "$coord_log"; then
  echo "telemetry partition integration: live presence descriptor used telemetry namespace" >&2
  exit 1
fi
grep -Fq 'legacy-decoy' "$telemetry_log"
[[ "$telemetry_before" == "$(sha256sum "$telemetry_log" | cut -d' ' -f1)" ]]

env "${presence_env[@]}" \
  bb "$root/cli/presence-cli.clj" "$coord_port" \
  forget outage-presence \
  >"$tmp/presence-forget.out"
env "${presence_env[@]}" \
  bb "$root/cli/presence-cli.clj" "$coord_port" presence-online-json \
  >"$tmp/presence-after-forget.json"
python3 - "$tmp/presence-after-forget.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)
assert all(
    row["control_id"] != "outage-presence"
    for row in payload["sessions"]
)
PY
[[ "$telemetry_before" == "$(sha256sum "$telemetry_log" | cut -d' ' -f1)" ]]

env "${partition_env[@]}" bb -e '
  (load-file (str (System/getenv "NORTH_TEST_ROOT") "/cli/coord.clj"))
  (prn (north.coord/append!
        (Integer/parseInt (System/getenv "NORTH_TEST_COORD_PORT"))
        "@thread:after-kill" "note" "coordination-survives"))'
grep -Fq '@thread:after-kill' "$coord_log"
kill -0 "$coord_pid"

set +e
env "${partition_env[@]}" \
  NORTH_COORD_CONNECT_TIMEOUT_MS=250 \
  bb -e '
    (load-file (str (System/getenv "NORTH_TEST_ROOT") "/cli/coord.clj"))
    (north.coord/append!
     (Integer/parseInt (System/getenv "NORTH_TEST_COORD_PORT"))
     "@run:after-kill" "kind" "run")' \
  >"$tmp/dead-telemetry.out" 2>"$tmp/dead-telemetry.err"
dead_telemetry_rc=$?
set -e
[[ "$dead_telemetry_rc" -ne 0 ]]
[[ "$telemetry_before" == "$(sha256sum "$telemetry_log" | cut -d' ' -f1)" ]]

for read_verb in show history; do
  set +e
  env "${partition_env[@]}" \
    FRAM_HOME="$fram" \
    NORTH_COORD_CONNECT_TIMEOUT_MS=250 \
    "$root/bin/north" "$read_verb" run:current \
    >"$tmp/dead-$read_verb.out" 2>"$tmp/dead-$read_verb.err"
  dead_read_rc=$?
  set -e
  [[ "$dead_read_rc" -eq 4 ]]
  grep -Fq "north: $read_verb REFUSED — telemetry writer unavailable for @run:current" \
    "$tmp/dead-$read_verb.err"
  if grep -Fq '  kind  run' "$tmp/dead-$read_verb.out"; then
    echo "telemetry partition integration: $read_verb fell back after telemetry writer death" >&2
    exit 1
  fi
done

# Rollback changes one flag and restarts the coordination writer in the
# pre-partition, two-origin configuration. No log is moved, merged, or deleted.
kill "$coord_pid"
wait "$coord_pid" 2>/dev/null || true
coord_pid=
FRAM_REQUIRE_LOG_FENCE=1 FRAM_PORT="$coord_port" FRAM_LOG="$coord_log" \
  FRAM_TELEMETRY_LOG="$telemetry_log" \
  "$fram/bin/fram-daemon" "$coord_port" "$coord_log" \
  >"$tmp/rollback.out" 2>"$tmp/rollback.err" &
coord_pid=$!
if ! await_writer "$coord_port" "$coord_log" "$coord_pid"; then
  cat "$tmp/rollback.err" >&2
  echo "telemetry partition integration: rollback writer did not start" >&2
  exit 1
fi

NORTH_TELEMETRY_PARTITION=0 \
FRAM_LOG="$coord_log" \
FRAM_TELEMETRY_LOG="$telemetry_log" \
NORTH_TEST_ROOT="$root" \
NORTH_TEST_COORD_PORT="$coord_port" \
  bb -e '
    (load-file (str (System/getenv "NORTH_TEST_ROOT") "/cli/coord.clj"))
    (let [port (Integer/parseInt (System/getenv "NORTH_TEST_COORD_PORT"))]
      (north.coord/append! port "@run:rollback" "kind" "run")
      (prn (north.coord/live-facts-view port)))' \
  >"$tmp/rollback-view"
grep -Fq '@run:legacy' "$tmp/rollback-view"
grep -Fq '@run:rollback' "$tmp/rollback-view"
grep -Fq '@run:rollback' "$telemetry_log"

rollback_show="$(
  NORTH_TELEMETRY_PARTITION=0 \
  NORTH_PORT="$coord_port" \
  FRAM_PORT="$coord_port" \
  FRAM_LOG="$coord_log" \
  FRAM_TELEMETRY_LOG="$telemetry_log" \
  FRAM_HOME="$fram" \
    "$root/bin/north" show run:rollback
)"
grep -Fq '  kind  run' <<<"$rollback_show"

echo "telemetry partition integration: PASS"
