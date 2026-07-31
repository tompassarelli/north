#!/usr/bin/env bash
# Live human clock authority stays available when the telemetry writer is down.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
fram="${FRAM_PATH:-${FRAM_HOME:-$HOME/code/fram/main}}"
hook="$root/profiles/tom/hooks/north-clock-guard.sh"

tmp="$(mktemp -d -t 'north client clock authority.XXXXXX')"
coord_pid=
telemetry_pid=
cleanup() {
  for pid in "$telemetry_pid" "$coord_pid"; do
    if [[ -n "$pid" ]]; then
      kill "$pid" 2>/dev/null || true
      wait "$pid" 2>/dev/null || true
    fi
  done
  if [[ "${KEEP_TMP:-0}" = 1 ]]; then
    echo "client clock authority: kept scratch at $tmp" >&2
  else
    rm -rf "${tmp:?}"
  fi
}
trap cleanup EXIT

coord_log="$tmp/coordination.log"
telemetry_log="$tmp/telemetry.log"
client_home="$tmp/home"
client_repo="$client_home/code/client/msa/work"
mkdir -p "$client_repo"
git -C "$client_repo" init -q -b msa-999-clock-authority
git -C "$client_repo" -c user.name=test -c user.email=test@example.invalid \
  commit --allow-empty --no-verify -qm init

fact() {
  printf '{:tx %s :op "assert" :l "%s" :p "%s" :r "%s" :by "fixture"}\n' \
    "$1" "$2" "$3" "$4"
}

legacy_start="$(date -d '2 minutes ago' '+%Y-%m-%dT%H:%M:%S')"
{
  fact 1 '@msa-thread' title 'MSA-999 clock authority'
  fact 2 '@msa-thread' owner msa
  fact 3 '@msa-thread' linear MSA-999
  fact 4 '@msa-thread' rate 100
  fact 5 '@msa-thread' invoice_id INV-SENT
  fact 6 '@msa-thread' invoice_state invoice-sent
  fact 7 '@client-rate:msa' owner msa
  fact 8 '@client-rate:msa' rate 175
  fact 9 '@client-rate:msa' kind client_rate_config
} >"$coord_log"
{
  fact 1 '@legacy-closed' session_of '@msa-thread'
  fact 2 '@legacy-closed' start_time '2026-07-30T09:00:00'
  fact 3 '@legacy-closed' end_time '2026-07-30T10:00:00'
  fact 4 '@legacy-open' session_of '@msa-thread'
  fact 5 '@legacy-open' start_time "$legacy_start"
  fact 6 '@legacy-open' owner msa
  fact 7 '@legacy-open' clocked_by user
  fact 8 '@legacy-open' rate 100
  fact 9 '@legacy-open' kind client_session
} >"$telemetry_log"

ports="$(
  bb -e '(with-open [a (java.net.ServerSocket. 0)
                     b (java.net.ServerSocket. 0)]
           (println (.getLocalPort a) (.getLocalPort b)))'
)"
read -r coord_port telemetry_port <<<"$ports"

start_writer() {
  local port="$1" log="$2" stem="$3"
  env -u FRAM_TELEMETRY_LOG -u NORTH_TELEMETRY_PARTITION \
    FRAM_REQUIRE_LOG_FENCE=1 \
    FRAM_PORT="$port" \
    FRAM_LOG="$log" \
    FRAM_SINGLE_VALUED='owner rate kind clocked_by start_time end_time invoice_id invoice_state linear title session_of' \
    "$fram/bin/fram-daemon" "$port" "$log" \
    >"$tmp/$stem.out" 2>"$tmp/$stem.err" &
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
    sleep 0.1
  done
  return 1
}

start_writer "$coord_port" "$coord_log" coord
coord_pid=$!
start_writer "$telemetry_port" "$telemetry_log" telemetry
telemetry_pid=$!
if ! await_writer "$coord_port" "$coord_log" "$coord_pid"; then
  cat "$tmp/coord.err" >&2
  echo "client clock authority: coordination writer did not start" >&2
  exit 1
fi
if ! await_writer "$telemetry_port" "$telemetry_log" "$telemetry_pid"; then
  cat "$tmp/telemetry.err" >&2
  echo "client clock authority: telemetry writer did not start" >&2
  exit 1
fi

partition_env=(
  NORTH_TELEMETRY_PARTITION=1
  NORTH_PORT="$coord_port"
  NORTH_TELEMETRY_PORT="$telemetry_port"
  FRAM_PORT="$coord_port"
  FRAM_LOG="$coord_log"
  FRAM_TELEMETRY_LOG="$telemetry_log"
  FRAM_HOME="$fram"
  NORTH_AGENT_ID=
  AGENT_ID=
  AGENT_TOPOLOGY=
  NORTH_COORD_CONNECT_TIMEOUT_MS=250
  NORTH_COORD_READ_TIMEOUT_MS=750
  FRAM_COORD_CONNECT_TIMEOUT_MS=250
  FRAM_COORD_READ_TIMEOUT_MS=750
)

clock() {
  env "${partition_env[@]}" "$root/bin/north" clock "$@"
}

authoring_budget_ms=3000
timed_clock() {
  local started finished output
  started="$(date +%s%3N)"
  output="$(clock "$@")"
  finished="$(date +%s%3N)"
  if (( finished - started > authoring_budget_ms )); then
    echo "client clock authority: clock $* exceeded ${authoring_budget_ms}ms" >&2
    return 1
  fi
  printf '%s\n' "$output"
}

domain_hash() {
  grep -v '"@snapshot:' "$1" | sha256sum | cut -d' ' -f1
}

telemetry_before="$(domain_hash "$telemetry_log")"
telemetry_bytes_before="$(sha256sum "$telemetry_log" | cut -d' ' -f1)"
telemetry_client_session_count_before="$(
  grep -Ec ':p "kind",? :r "client_session"' "$telemetry_log"
)"
first_current="$(clock current)"
grep -Fq 'clocked in for client msa' <<<"$first_current"
grep -Fq '"@legacy-open"' "$coord_log"
grep -Eq ':p "kind",? :r "client_session"' "$coord_log"
grep -Eq ':p "owner",? :r "msa"' "$coord_log"
grep -Eq ':p "clocked_by",? :r "user"' "$coord_log"
grep -Eq ':p "session_of",? :r "@msa-thread"' "$coord_log"
grep -Eq ':p "rate",? :r "100"' "$coord_log"
grep -Eq ":p \"start_time\",? :r \"$legacy_start\"" "$coord_log"
[[ "$telemetry_before" == "$(domain_hash "$telemetry_log")" ]]
[[ "$telemetry_bytes_before" == "$(sha256sum "$telemetry_log" | cut -d' ' -f1)" ]]

kind_count_before="$(
  grep -F '"@legacy-open"' "$coord_log" |
    grep -Ec ':p "kind",? :r "client_session"'
)"
second_current="$(clock current)"
kind_count_after="$(
  grep -F '"@legacy-open"' "$coord_log" |
    grep -Ec ':p "kind",? :r "client_session"'
)"
grep -Fq 'clocked in for client msa' <<<"$second_current"
[[ "$kind_count_before" == 1 && "$kind_count_after" == 1 ]]
[[ "$telemetry_bytes_before" == "$(sha256sum "$telemetry_log" | cut -d' ' -f1)" ]]

kill "$telemetry_pid"
wait "$telemetry_pid" 2>/dev/null || true
telemetry_pid=
telemetry_stopped_hash="$(sha256sum "$telemetry_log" | cut -d' ' -f1)"
mv "$telemetry_log" "$telemetry_log.offline"

offline_current="$(timed_clock current)"
grep -Fq 'clocked in for client msa' <<<"$offline_current"

payload="$(
  printf '{"tool_name":"Edit","tool_input":{"file_path":"%s/api.py"},"cwd":"%s"}' \
    "$client_repo" "$client_repo"
)"
guard_started="$(date +%s%3N)"
guard_out="$(
  printf '%s' "$payload" |
    env -u AGENT_NO_AUTHORING_HOOKS -u CLAUDE_NO_AUTHORING_HOOKS \
      HOME="$client_home" NORTH_CLOCK_GUARD_ATTEST=1 \
      FRAM_LOG="$coord_log" FRAM_TELEMETRY_LOG="$telemetry_log" \
      AUTHORING_KILLSWITCH_STATE="$tmp/killswitch.state" \
      "$hook"
)"
guard_finished="$(date +%s%3N)"
(( guard_finished - guard_started <= authoring_budget_ms ))
[[ "$guard_out" == '{ "northClockGuard": "allow" }' ]]

bridge_out="$(timed_clock out)"
grep -Fq 'clocked out of client msa' <<<"$bridge_out"
grep -F '"@legacy-open"' "$coord_log" | grep -Eq ':p "end_time",?'

mv "$telemetry_log.offline" "$telemetry_log"
[[ "$telemetry_stopped_hash" == "$(sha256sum "$telemetry_log" | cut -d' ' -f1)" ]]

sent_csv="$(
  env FRAM_HOME="$fram" FRAM_LOG="$coord_log" FRAM_TELEMETRY_LOG="$telemetry_log" \
    "$root/bin/north-timelog" msa INV-SENT 2>"$tmp/timelog.err"
)"
grep -Fq 'MSA-999' <<<"$sent_csv"
grep -Eq ',1[.]00,100,100[.]00,INV-SENT,invoice-sent$' <<<"$sent_csv"
[[ "$(grep -c '^2026-' <<<"$sent_csv")" == 1 ]]

mv "$telemetry_log" "$telemetry_log.offline"
new_in="$(timed_clock in msa)"
grep -Fq 'clocked in for client msa' <<<"$new_in"
new_current="$(timed_clock current)"
grep -Fq 'clocked in for client msa' <<<"$new_current"
new_out="$(timed_clock out)"
grep -Fq 'clocked out of client msa' <<<"$new_out"
mv "$telemetry_log.offline" "$telemetry_log"
[[ "$telemetry_stopped_hash" == "$(sha256sum "$telemetry_log" | cut -d' ' -f1)" ]]

client_session_count="$(
  grep -E ':p "kind",? :r "client_session"' "$coord_log" | wc -l
)"
[[ "$client_session_count" == 2 ]]
telemetry_client_session_count_after="$(
  grep -Ec ':p "kind",? :r "client_session"' "$telemetry_log"
)"
if [[ "$telemetry_client_session_count_after" != "$telemetry_client_session_count_before" ]]; then
  echo "client clock authority: client_session marker count changed in telemetry" >&2
  exit 1
fi

echo "client clock authority: PASS"
