#!/usr/bin/env bash
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
scratch=$(mktemp -d "${TMPDIR:-/tmp}/machine-capacity-test.XXXXXX")
export XDG_STATE_HOME="$scratch/state" AGENT_CAPACITY_DB="$scratch/capacity.db" AGENT_CAPACITY_SUBSLICE="test$$" AGENT_CAPACITY_CORES=24
fixture_pids=()
collector_pids=()
receipt_fds=()
cleanup() {
  for pid in "${fixture_pids[@]}"; do kill -TERM "$pid" 2>/dev/null || true; done
  for pid in "${fixture_pids[@]}"; do wait "$pid" 2>/dev/null || true; done
  for fd in "${receipt_fds[@]}"; do exec {fd}<&-; done
  for pid in "${collector_pids[@]}"; do kill -TERM "$pid" 2>/dev/null || true; done
  for pid in "${collector_pids[@]}"; do wait "$pid" 2>/dev/null || true; done
  { systemctl --user stop "agent-capacity-$AGENT_CAPACITY_SUBSLICE.slice" "native-$AGENT_CAPACITY_SUBSLICE.slice"; systemctl --user revert "agent-capacity-$AGENT_CAPACITY_SUBSLICE.slice" "native-$AGENT_CAPACITY_SUBSLICE.slice"; } >/dev/null 2>&1 || true
  rm -rf "${scratch:?}"
}
trap cleanup EXIT
wait_for() { local value; IFS= read -r value <"$1.receipt"; [[ -s "$1" ]]; }
user_runtime_dir=${XDG_RUNTIME_DIR:-/run/user/$(id -u)}
# Live runs read recorded system CPU pressure, so other machine load cannot hold them.
export AGENT_CAPACITY_CPU_PRESSURE="$scratch/cpu.pressure" AGENT_CAPACITY_LOADAVG=/dev/null AGENT_CAPACITY_NVIDIA_MEMORY=/dev/null
calm_pressure() { printf 'some avg10=%s avg60=0.00 avg300=0.00 total=0\n' "$1" >"$AGENT_CAPACITY_CPU_PRESSURE"; }
calm_pressure 0.00
export AGENT_CAPACITY_GPU_BUSY="$scratch/gpu-busy" AGENT_CAPACITY_GPU_CLIENTS=0 AGENT_CAPACITY_USAGE_LOG="$scratch/usage.jsonl" CAPACITY_WATCHDOG_DIR="$scratch/no-watchdog"
echo 0 >"$AGENT_CAPACITY_GPU_BUSY"

"$here/build-machine-capacity" "$scratch/machine-capacity.mjs"
bun "$here/capacity-unseen-pressure.recorded.test.ts" "$scratch/machine-capacity.mjs"
bun "$here/capacity-clamp.recorded.test.ts" "$scratch/machine-capacity.mjs"
bun "$here/charged-resources.property.ts" "$here/machine-capacity-logic.ts"

# Arguments: profile class available-MiB protected-slice-PSI memory-full-PSI
# leased-CPU-ceilings leased-MiB [options]. Protected PSI is the session and
# native slices' own CPU wait, not the system-wide figure batch work inflates.
fixture() {
  bun "$scratch/machine-capacity.mjs" fixture \
    --profile "${1}" \
    --class "${2}" \
    --cores 24 \
    --memory-total-mib 96343 \
    --memory-available-mib "${3}" \
    --protected-cpu-some-avg10-basis-points "${4}" \
    --memory-full-avg10-basis-points "${5}" \
    --leased-cpus "${6}" \
    --leased-memory-mib "${7}" "${@:8}" | jq -c 'del(.leasedCpuCeilings)'
}
decision() { fixture "$@" | jq -r '.decision' || true; }

[[ $(fixture attended heavy 70000 500 0 6 8192) == '{"decision":"RUN","profile":"attended","class":"heavy","cpus":6,"memoryMiB":8192,"memoryFullAvg10":0,"aggregateCpuLimit":20}' ]]
[[ $(fixture unattended exclusive 70000 500 0 0 0) == '{"decision":"RUN","profile":"unattended","class":"exclusive","cpus":24,"memoryMiB":16384,"memoryFullAvg10":0,"aggregateCpuLimit":24}' ]]
# Present: batch yields when the desktop itself waits for CPU. Away: no CPU refusal.
[[ $(decision attended heavy 70000 1000 394 0 0) == DEFER_INTERACTIVE_PRESSURE ]]
[[ $(decision attended heavy 70000 999 394 0 0) == RUN ]]
[[ $(decision unattended heavy 70000 10000 394 0 0) == RUN ]]
# Batch ceilings past the aggregate limit queue new batch work in every profile.
[[ $(decision attended moderate 70109 0 0 26 32768 --peer-batch-runs 7) == DEFER_CPU_CAPACITY ]]
[[ $(decision attended moderate 70109 0 0 18 32768 --peer-batch-runs 7) == RUN ]]
# Memory floor: 20% of RAM present, the hard 8 GiB floor away.
[[ $(decision attended heavy 21000 0 394 0 0) == DEFER_MEMORY_HEADROOM ]]
[[ $(decision unattended heavy 21000 0 394 0 0) == RUN ]]
[[ $(decision unattended heavy 16000 0 394 0 0) == DEFER_MEMORY_HEADROOM ]]
[[ $(decision unattended moderate 10241 0 0 0 0) == RUN ]]
[[ $(decision unattended heavy 70000 0 394 0 70000) == DEFER_MEMORY_CAPACITY ]]
[[ $(decision attended agent 73412 5000 394 8 9728) == RUN ]]
[[ $(decision attended heavy 73412 0 10000 8 9728) == RUN ]]
# Native clients ignore pressure and exclusive batch work; they are bounded by
# cores outside the present reserve and by memory.
[[ $(fixture attended native 70000 5000 0 0 0) == '{"decision":"RUN","profile":"attended","class":"native","cpus":2,"memoryMiB":4096,"memoryFullAvg10":0,"aggregateCpuLimit":20}' ]]
[[ $(decision attended native 70000 0 0 18 0 --leased-native-cpus 18) == RUN ]]
[[ $(decision attended native 70000 0 0 20 0 --leased-native-cpus 20) == DEFER_NATIVE_CPUS ]]
[[ $(decision unattended native 70000 0 0 20 0 --leased-native-cpus 20) == RUN ]]
[[ $(decision attended native 70000 0 0 24 16384 --peer-exclusive-runs 1) == RUN ]]
[[ $(decision attended native 12000 0 0 0 0) == DEFER_MEMORY_HEADROOM ]]
# A smaller client request retains the existing floor and leased-memory cap.
[[ $(decision unattended native 40000 0 0 0 70000) == DEFER_MEMORY_CAPACITY ]]
[[ $(fixture unattended native 40000 0 0 0 70000 --memory-gib 1.5 | jq -r '.memoryMiB') -eq 1536 ]]
[[ $(decision unattended native 40000 0 0 0 70000 --memory-gib 1.5) == RUN ]]
[[ $(decision unattended native 9600 0 0 0 0 --memory-gib 1.5) == DEFER_MEMORY_HEADROOM ]]
[[ $(decision attended native 20000 0 0 0 0 --memory-gib 1.5) == DEFER_MEMORY_HEADROOM ]]
[[ $(decision unattended native 40000 0 0 0 71000 --memory-gib 1.5) == DEFER_MEMORY_CAPACITY ]]
for memory in 0 -1 NaN Infinity 1.0001 100000000000000; do
  if fixture unattended native 40000 0 0 0 0 --memory-gib "$memory" >/dev/null 2>&1; then
    printf 'invalid memory request accepted: %s\n' "$memory" >&2
    exit 1
  fi
done
if fixture unattended heavy 40000 0 0 0 0 --memory-gib 1.5 >/dev/null 2>&1; then
  printf 'batch memory override accepted\n' >&2
  exit 1
fi

# Headless renders share the GPU two at a time and yield to native clients;
# GPU time leased renders hold does not defer a native client.
[[ $(decision unattended gpu 80000 0 0 1 2048 --gpu-runs 1) == RUN ]]
[[ $(decision unattended gpu 80000 0 0 2 4096 --gpu-runs 2) == DEFER_GPU_SLOTS ]]
[[ $(decision unattended gpu 80000 0 0 0 0 --native-waiting 1) == DEFER_NATIVE_WAITING ]]
[[ $(decision unattended native 80000 0 0 0 0 --gpu-busy-percent 95) == DEFER_GPU_BUSY ]]
[[ $(decision unattended native 80000 0 0 2 4096 --gpu-busy-percent 95 --gpu-runs 2) == RUN ]]
[[ $(decision attended native 80000 0 0 0 0 --gpu-clients 4 --new-clients 1 --gpu-runs 1) == DEFER_GPU_CLIENTS ]]

# Peer ceilings hold batch work at the aggregate limit: 20 cores present, 24 away.
[[ $(decision attended heavy 80000 0 0 18 21504 --peer-batch-runs 3) == DEFER_CPU_CAPACITY ]]
[[ $(decision attended heavy 80000 0 0 14 21504 --peer-batch-runs 3) == RUN ]]
[[ $(decision unattended heavy 80000 0 0 18 21504 --peer-batch-runs 3) == RUN ]]
[[ $(decision unattended heavy 80000 0 0 20 21504 --peer-batch-runs 4) == DEFER_CPU_CAPACITY ]]
# System CPU pressure above 30% holds moderate and heavy work, present or away.
[[ $(decision unattended heavy 80000 0 0 0 0 --cpu-some-avg10-basis-points 3001) == DEFER_CPU_PRESSURE ]]
[[ $(decision unattended heavy 80000 0 0 0 0 --cpu-some-avg10-basis-points 3000) == RUN ]]
[[ $(decision attended moderate 80000 0 0 0 0 --cpu-some-avg10-basis-points 8400) == DEFER_CPU_PRESSURE ]]
[[ $(decision unattended exclusive 80000 0 0 0 0 --cpu-some-avg10-basis-points 8400) == RUN ]]
[[ $(decision unattended native 80000 0 0 0 0 --cpu-some-avg10-basis-points 8400) == RUN ]]
[[ $(decision unattended heavy 80000 0 0 0 0 --queued-ahead 1) == DEFER_QUEUED ]]
[[ $(decision unattended native 80000 0 0 0 0 --queued-ahead 1) == RUN ]]
[[ $(decision attended exclusive 80000 0 0 4 3072) == RUN ]]
[[ $(decision attended exclusive 80000 0 0 2 2048 --peer-batch-runs 1) == DEFER_EXCLUSIVE ]]
[[ $(decision unattended moderate 80000 0 0 18 16384 --peer-batch-runs 1 --peer-exclusive-runs 1) == DEFER_EXCLUSIVE ]]
[[ $(decision attended heavy 80000 0 0 6 8192 --peer-batch-runs 1 --unbounded-runs 1) == DEFER_UNBOUNDED_PEER ]]
# A waiting exclusive request holds later heavy work and waits only for
# running batch leases: not for earlier batch tickets or pressure. Moderate
# builds and dependency updates still start.
[[ $(decision unattended heavy 80000 0 0 0 0 --exclusive-waiting 1) == DEFER_EXCLUSIVE_QUEUED ]]
[[ $(decision attended moderate 80000 0 0 0 0 --exclusive-waiting 1) == RUN ]]
[[ $(decision unattended native 80000 0 0 0 0 --exclusive-waiting 1) == RUN ]]
[[ $(decision unattended exclusive 80000 0 0 0 0 --queued-ahead 3) == RUN ]]
[[ $(decision unattended exclusive 80000 0 0 0 0 --exclusive-waiting 1) == DEFER_QUEUED ]]
[[ $(decision attended exclusive 80000 5000 0 0 0 --cpu-some-avg10-basis-points 9000) == RUN ]]
[[ $(decision attended exclusive 80000 5000 0 6 8192 --peer-batch-runs 1) == DEFER_EXCLUSIVE ]]
# Critical work skips the queue, a waiting exclusive and the CPU waits, keeps
# the attended desktop reserve, and still respects memory floors, a running
# exclusive lease and the desktop's own CPU wait.
[[ $(fixture unattended critical 70000 0 0 0 0 | jq -c '[.decision, .cpus]') == '["RUN",20]' ]]
[[ $(decision attended critical 80000 0 0 20 16384 --peer-batch-runs 4 --queued-ahead 3 --exclusive-waiting 1 --cpu-some-avg10-basis-points 9000) == RUN ]]
[[ $(decision unattended critical 20000 0 0 0 0) == DEFER_MEMORY_HEADROOM ]]
[[ $(decision unattended critical 80000 0 0 24 16384 --peer-batch-runs 1 --peer-exclusive-runs 1) == DEFER_EXCLUSIVE ]]
[[ $(decision attended critical 80000 1000 0 0 0) == DEFER_INTERACTIVE_PRESSURE ]]
# Tom-facing builds skip the queue and CPU waits like critical work but lease
# only a moderate 2-CPU, 2 GiB scope.
[[ $(fixture attended tom 70000 0 0 0 0 | jq -c '[.decision, .cpus, .memoryMiB]') == '["RUN",2,2048]' ]]
[[ $(decision attended tom 80000 0 0 20 16384 --peer-batch-runs 4 --queued-ahead 3 --cpu-some-avg10-basis-points 9000) == RUN ]]
[[ $(decision attended moderate 80000 0 0 20 16384 --peer-batch-runs 4 --queued-ahead 3 --cpu-some-avg10-basis-points 9000) != RUN ]]
[[ $(decision attended tom 80000 1000 0 0 0) == DEFER_INTERACTIVE_PRESSURE ]]
[[ $(decision unattended heavy 80000 0 0 20 16384 --peer-batch-runs 1) == DEFER_CPU_CAPACITY ]]
[[ $(decision unattended exclusive 80000 0 0 20 16384 --peer-batch-runs 1) == DEFER_EXCLUSIVE ]]

fixture_runtime="$scratch/runtime"
mkdir -p "$fixture_runtime"
ln -s "$user_runtime_dir/systemd" "$fixture_runtime/systemd"
reservation=$(XDG_RUNTIME_DIR="$fixture_runtime" bun "$scratch/machine-capacity.mjs" reserve \
  --class agent --owner fixture:/capacity --timeout-seconds 5)
lease=$(jq -r '.lease' <<<"$reservation")
[[ $lease =~ ^[0-9a-f-]{36}$ ]]
renewed=$(XDG_RUNTIME_DIR="$fixture_runtime" bun "$scratch/machine-capacity.mjs" renew \
  --lease "$lease" --owner fixture:/capacity --timeout-seconds 5)
[[ $(jq -r '.decision' <<<"$renewed") == RENEWED ]]
released=$(XDG_RUNTIME_DIR="$fixture_runtime" bun "$scratch/machine-capacity.mjs" release \
  --lease "$lease" --owner fixture:/capacity)
[[ $(jq -r '.decision' <<<"$released") == RELEASED ]]

rejected_runtime="$scratch/rejected-runtime"
set +e
rejected=$(XDG_RUNTIME_DIR="$rejected_runtime" bun "$scratch/machine-capacity.mjs" reserve \
  --class heavy --owner fixture:/capacity --timeout-seconds 5 2>&1)
rejected_status=$?
set -e
[[ $rejected_status -eq 2 ]]
[[ ! -e "$rejected_runtime/agent-capacity-v1" ]]

expiring=$(XDG_RUNTIME_DIR="$fixture_runtime" bun "$scratch/machine-capacity.mjs" reserve \
  --class agent --owner fixture:/capacity --timeout-seconds 1)
[[ $(jq -r '.decision' <<<"$expiring") == RESERVED ]]
sleep 1.1
reclaimed=$(XDG_RUNTIME_DIR="$fixture_runtime" bun "$scratch/machine-capacity.mjs" probe --class agent)
[[ $(jq -r '.reclaimed' <<<"$reclaimed") == 1 ]]

sleeper_pid_file="$scratch/sleeper.pid"
set +e
XDG_RUNTIME_DIR="$fixture_runtime" bun "$scratch/machine-capacity.mjs" run \
  --class heavy \
  --owner fixture:/capacity \
  --timeout-seconds 1 \
  -- bash -c 'sleep 60 & child=$!; printf "%s\n" "$child" >"$1"; wait "$child"' \
  fixture-scope "$sleeper_pid_file"
scope_status=$?
set -e
[[ $scope_status -ne 0 ]]
read -r sleeper_pid <"$sleeper_pid_file"
[[ $sleeper_pid =~ ^[1-9][0-9]*$ ]]
! kill -0 "$sleeper_pid" 2>/dev/null

# Four heavy jobs request 24 CPU ceilings on this 24-core host. The attended
# fixture admits three (18 of 20 cores) and queues the fourth; the kernel
# ancestor enforces the shared cap, which keeps four cores for the desktop.
original_capacity_db=$AGENT_CAPACITY_DB original_usage_log=$AGENT_CAPACITY_USAGE_LOG
export AGENT_CAPACITY_DB="$scratch/aggregate.db" AGENT_CAPACITY_USAGE_LOG="$scratch/aggregate-usage.jsonl"
for sample in 1 2 3; do printf '{"at":"2026-10-10T00:00:0%sZ","owner":"fixture:/capacity","shape":"read","class":"heavy","meanCores":4.8,"p90Cores":6,"peakCores":6,"peakMemoryMiB":6826,"wallSeconds":120}\n' "$sample"; done >>"$AGENT_CAPACITY_USAGE_LOG"
for number in 1 2 3 4; do
  mkfifo "$scratch/group-$number.receipt"
  [[ $number -ne 4 ]] || mkfifo "$scratch/err-4-events"
  if [[ $number -eq 4 ]]; then
    exec {sink}> >(tee "$scratch/err-4" >"$scratch/err-4-events")
    collector_pids+=("$!")
  else
    exec {sink}>"$scratch/err-$number"
  fi
  XDG_RUNTIME_DIR="$fixture_runtime" bun "$scratch/machine-capacity.mjs" run \
    --class heavy --owner fixture:/capacity --timeout-seconds 30 \
    -- bash -c 'IFS=: read -r _ _ group < /proc/self/cgroup; printf "%s\n" "$group" >"$1"; printf "ready\n" >"$1.receipt"; sleep 60 & wait' \
    fixture-scope "$scratch/group-$number" >"$scratch/out-$number" 2>&"$sink" &
  fixture_pids+=("$!")
  exec {sink}>&-
  if [[ $number -eq 4 ]]; then
    exec {aggregate_events}<"$scratch/err-4-events"
    receipt_fds+=("$aggregate_events")
    IFS= read -r event <&"$aggregate_events"
  else
    wait_for "$scratch/group-$number"
  fi
done
for number in 1 2 3; do
  [[ -s "$scratch/group-$number" ]]
  read -r group <"$scratch/group-$number"
  parent=${group%/*}
  [[ $parent == */agent.slice/agent-capacity.slice/agent-capacity-$AGENT_CAPACITY_SUBSLICE.slice ]]
  read -r quota period <"/sys/fs/cgroup$parent/cpu.max"
  [[ $quota != max ]]
  cores=$AGENT_CAPACITY_CORES
  [[ $quota -eq $((period * (cores - 4))) ]]
  read -r quota period <"/sys/fs/cgroup$group/cpu.max"
  [[ $((quota / period)) -eq 6 ]]
  [[ $(cat "/sys/fs/cgroup$group/memory.high") -eq $((8192 * 1024 * 1024)) ]]
done
[[ ! -s "$scratch/group-4" ]]
[[ $(head -n 1 "$scratch/err-4" | jq -r '.decision + " " + .reason') == 'QUEUED DEFER_CPU_CAPACITY' ]]
probe=$(XDG_RUNTIME_DIR="$fixture_runtime" bun "$scratch/machine-capacity.mjs" probe --class exclusive || true)
[[ $(jq -r '.reason' <<<"$probe") == DEFER_EXCLUSIVE ]]
[[ $(jq -r '.leasedCpuCeilings' <<<"$probe") -eq 18 ]]
[[ $(jq -r '.queuedAhead' <<<"$probe") -eq 1 ]]
[[ $(jq -r '.aggregateCpuLimit' <<<"$probe") -eq 20 ]]
[[ $(jq -r '.profile' <<<"$probe") == attended ]]
# Releasing one job starts the queued one.
kill -TERM "${fixture_pids[0]}"
wait_for "$scratch/group-4"
[[ -s "$scratch/group-4" ]]
for pid in "${fixture_pids[@]}"; do kill -TERM "$pid" 2>/dev/null || true; done
for pid in "${fixture_pids[@]}"; do wait "$pid" || true; done
fixture_pids=()
for pid in "${collector_pids[@]}"; do wait "$pid"; done
collector_pids=()
for number in 1 2 3 4; do
  [[ $(tail -n 1 "$scratch/err-$number" | jq -r '.decision') == RELEASED ]]
  read -r group <"$scratch/group-$number"
  [[ ! -e "/sys/fs/cgroup$group" ]]
done
[[ $(XDG_RUNTIME_DIR="$fixture_runtime" bun "$scratch/machine-capacity.mjs" probe --class exclusive | jq -r '.decision') == RUN ]]
[[ "$(AGENT_CAPACITY_GPU_CLIENTS=4 XDG_RUNTIME_DIR="$fixture_runtime" bun "$scratch/machine-capacity.mjs" probe --class native --memory-gib 1 | jq -r .reason) $(AGENT_CAPACITY_GPU_CLIENTS=4 XDG_RUNTIME_DIR="$fixture_runtime" bun "$scratch/machine-capacity.mjs" probe --class native --new-clients 0 --memory-gib 1 | jq -r .reason)" == 'DEFER_GPU_CLIENTS RUN' ]]
export AGENT_CAPACITY_DB=$original_capacity_db AGENT_CAPACITY_USAGE_LOG=$original_usage_log

# An agent allowance does not block its exclusive command, which then blocks peers.
reservation=$(XDG_RUNTIME_DIR="$fixture_runtime" bun "$scratch/machine-capacity.mjs" reserve \
  --class agent --owner fixture:/capacity --timeout-seconds 30)
lease=$(jq -r '.lease' <<<"$reservation")
mkfifo "$scratch/exclusive-ready.receipt"
XDG_RUNTIME_DIR="$fixture_runtime" bun "$scratch/machine-capacity.mjs" run \
  --class exclusive --owner fixture:/capacity --timeout-seconds 30 \
  -- bash -c 'printf "ready\n" >"$1"; printf "ready\n" >"$1.receipt"; sleep 60 & wait' fixture-exclusive "$scratch/exclusive-ready" \
  >"$scratch/exclusive-out" 2>"$scratch/exclusive-err" &
fixture_pids+=("$!")
wait_for "$scratch/exclusive-ready"
[[ -s "$scratch/exclusive-ready" ]]
probe=$(XDG_RUNTIME_DIR="$fixture_runtime" bun "$scratch/machine-capacity.mjs" probe --class moderate || true)
[[ $(jq -r '.reason' <<<"$probe") == DEFER_EXCLUSIVE ]]
kill -TERM "${fixture_pids[0]}"
wait "${fixture_pids[0]}" || true
fixture_pids=()
for pid in "${collector_pids[@]}"; do wait "$pid"; done
collector_pids=()
[[ $(tail -n 1 "$scratch/exclusive-err" | jq -r '.decision') == RELEASED ]]
XDG_RUNTIME_DIR="$fixture_runtime" bun "$scratch/machine-capacity.mjs" release \
  --lease "$lease" --owner fixture:/capacity

# Pressure holds a run in the queue until it eases; leaving the queue drops the ticket.
calm_pressure 90.00
mkfifo "$scratch/held-ready.receipt" "$scratch/held-events"
exec {sink}> >(tee "$scratch/held-err" >"$scratch/held-events")
collector_pids+=("$!")
XDG_RUNTIME_DIR="$fixture_runtime" bun "$scratch/machine-capacity.mjs" run \
  --class moderate --owner fixture:/capacity --timeout-seconds 30 \
  -- bash -c 'printf "ready\n" >"$1"; printf "ready\n" >"$1.receipt"; sleep 60 & wait' fixture-held "$scratch/held-ready" \
  >"$scratch/held-out" 2>&"$sink" &
fixture_pids+=("$!")
exec {sink}>&-
exec {held_events}<"$scratch/held-events"
receipt_fds+=("$held_events")
IFS= read -r event <&"$held_events"
[[ ! -s "$scratch/held-ready" ]]
[[ $(head -n 1 "$scratch/held-err" | jq -r '.decision') == QUEUED ]]
[[ $(XDG_RUNTIME_DIR="$fixture_runtime" bun "$scratch/machine-capacity.mjs" probe --class agent | jq -r '.queuedAhead') -eq 1 ]]
calm_pressure 0.00
wait_for "$scratch/held-ready"
[[ -s "$scratch/held-ready" ]]
kill -TERM "${fixture_pids[0]}"
wait "${fixture_pids[0]}" || true
fixture_pids=()
for pid in "${collector_pids[@]}"; do wait "$pid"; done
collector_pids=()
[[ $(tail -n 1 "$scratch/held-err" | jq -r '.decision') == RELEASED ]]
calm_pressure 90.00
mkfifo "$scratch/cancel-events"
exec {sink}> >(tee "$scratch/cancel-err" >"$scratch/cancel-events")
collector_pids+=("$!")
XDG_RUNTIME_DIR="$fixture_runtime" bun "$scratch/machine-capacity.mjs" run \
  --class heavy --owner fixture:/capacity --timeout-seconds 30 -- true 2>&"$sink" &
fixture_pids+=("$!")
exec {sink}>&-
exec {cancel_events}<"$scratch/cancel-events"
receipt_fds+=("$cancel_events")
IFS= read -r event <&"$cancel_events"
[[ $(ls "$fixture_runtime/agent-capacity-v1/queue" | wc -l) -eq 1 ]]
kill -TERM "${fixture_pids[0]}"
wait "${fixture_pids[0]}" || true
fixture_pids=()
for pid in "${collector_pids[@]}"; do wait "$pid"; done
collector_pids=()
[[ $(ls "$fixture_runtime/agent-capacity-v1/queue" | wc -l) -eq 0 ]]
calm_pressure 0.00

# A batch session takes its class's default deadline and keeps its allowance
# until its scope and descendants stop.
mkfifo "$scratch/session-group.receipt" "$scratch/session-child.receipt"
XDG_RUNTIME_DIR="$fixture_runtime" bun "$scratch/machine-capacity.mjs" session \
  --class moderate --owner fixture:/capacity \
  -- bash -c 'IFS=: read -r _ _ group < /proc/self/cgroup; printf "%s\n" "$group" >"$1"; printf "ready\n" >"$1.receipt"; sleep 60 & printf "%s\n" "$!" >"$2"; printf "ready\n" >"$2.receipt"; wait' \
  fixture-session "$scratch/session-group" "$scratch/session-child" \
  >"$scratch/session-out" 2>"$scratch/session-err" &
fixture_pids+=("$!")
wait_for "$scratch/session-group"
wait_for "$scratch/session-child"
read -r group <"$scratch/session-group"
read -r session_child <"$scratch/session-child"
[[ $(systemctl --user show "${group##*/}" --property=RuntimeMaxUSec --value) == 30min ]]
[[ $(head -n 1 "$scratch/session-err" | jq -r '.expiresAt') != null ]]
probe=$(XDG_RUNTIME_DIR="$fixture_runtime" bun "$scratch/machine-capacity.mjs" probe --class exclusive || true)
[[ $(jq -r '.reason' <<<"$probe") == DEFER_EXCLUSIVE ]]
[[ $(jq -r '.leasedMemoryMiB' <<<"$probe") -eq 2048 ]]
[[ $(jq -r '.reclaimed' <<<"$probe") -eq 0 ]]
kill -TERM "${fixture_pids[0]}"
wait "${fixture_pids[0]}" || true
fixture_pids=()
for pid in "${collector_pids[@]}"; do wait "$pid"; done
collector_pids=()
[[ $(tail -n 1 "$scratch/session-err" | jq -r '.decision') == RELEASED ]]
[[ ! -e "/sys/fs/cgroup$group" ]]
! kill -0 "$session_child" 2>/dev/null
set +e
XDG_RUNTIME_DIR="$fixture_runtime" bun "$scratch/machine-capacity.mjs" session \
  --class moderate --owner fixture:/capacity -- bash -c 'exit 7' 2>"$scratch/session-exit"
session_status=$?
set -e
[[ $session_status -eq 7 ]]
[[ $(tail -n 1 "$scratch/session-exit" | jq -r '.decision') == RELEASED ]]
[[ $(XDG_RUNTIME_DIR="$fixture_runtime" bun "$scratch/machine-capacity.mjs" probe --class agent | jq -r '.leasedMemoryMiB') -eq 0 ]]

# A native client runs unthrottled at high weight beside the batch slice.
mkfifo "$scratch/native-group.receipt"
XDG_RUNTIME_DIR="$fixture_runtime" bun "$scratch/machine-capacity.mjs" run \
  --class native --owner fixture:/capacity --timeout-seconds 30 \
  -- bash -c 'IFS=: read -r _ _ group < /proc/self/cgroup; printf "%s\n" "$group" >"$1"; printf "ready\n" >"$1.receipt"; sleep 60 & wait' \
  fixture-native "$scratch/native-group" >"$scratch/native-out" 2>"$scratch/native-err" &
fixture_pids+=("$!")
wait_for "$scratch/native-group"
read -r group <"$scratch/native-group"
[[ ${group%/*} == */native.slice/native-$AGENT_CAPACITY_SUBSLICE.slice ]]
[[ $(cat "/sys/fs/cgroup${group%/*}/cpu.weight") -eq 200 ]]
[[ ! -e "/sys/fs/cgroup$group/cpu.max" || $(cut -d' ' -f1 "/sys/fs/cgroup$group/cpu.max") == max ]]
[[ $(cat "/sys/fs/cgroup$group/memory.high") -eq $((4096 * 1024 * 1024)) ]]
probe=$(XDG_RUNTIME_DIR="$fixture_runtime" bun "$scratch/machine-capacity.mjs" probe --class heavy)
[[ $(jq -r '.decision' <<<"$probe") == RUN ]]
[[ $(jq -r '.leasedNativeCpus' <<<"$probe") -eq 2 ]]
kill -TERM "${fixture_pids[0]}"
wait "${fixture_pids[0]}" || true
fixture_pids=()
for pid in "${collector_pids[@]}"; do wait "$pid"; done
collector_pids=()
[[ $(tail -n 1 "$scratch/native-err" | jq -r '.decision') == RELEASED ]]

# Requests reach both lease accounting and the kernel for runs and sessions.
for lifetime in run session; do
  options=()
  [[ $lifetime != run ]] || options=(--timeout-seconds 30)
  mkfifo "$scratch/memory-group-$lifetime.receipt"
  XDG_RUNTIME_DIR="$fixture_runtime" bun "$scratch/machine-capacity.mjs" "$lifetime" \
    --class native --memory-gib 1.5 --owner fixture:/capacity "${options[@]}" \
    -- bash -c 'IFS=: read -r _ _ group < /proc/self/cgroup; printf "%s\n" "$group" >"$1"; printf "ready\n" >"$1.receipt"; sleep 60 & wait' \
    fixture-native-memory "$scratch/memory-group-$lifetime" \
    >"$scratch/memory-out-$lifetime" 2>"$scratch/memory-err-$lifetime" &
  fixture_pids+=("$!")
  wait_for "$scratch/memory-group-$lifetime"
  read -r group <"$scratch/memory-group-$lifetime"
  [[ $(cat "/sys/fs/cgroup$group/memory.high") -eq 1610612736 ]]
  # Native sessions alone have no deadline; probing must not reclaim it.
  [[ $lifetime == run || $(systemctl --user show "${group##*/}" --property=RuntimeMaxUSec --value) == infinity ]]
  [[ $(head -n 1 "$scratch/memory-err-$lifetime" | jq -r '.requestedMemoryMiB') -eq 1536 ]]
  lease=$(head -n 1 "$scratch/memory-err-$lifetime" | jq -r '.lease')
  [[ $(jq -r '.memoryMiB' "$fixture_runtime/agent-capacity-v1/leases/$lease.json") -eq 1536 ]]
  probe=$(XDG_RUNTIME_DIR="$fixture_runtime" bun "$scratch/machine-capacity.mjs" probe --class native --memory-gib 1.5)
  [[ $(jq -r '.leasedMemoryMiB' <<<"$probe") -eq 1536 ]]
  [[ $(jq -r '.requestedMemoryMiB' <<<"$probe") -eq 1536 ]]
  kill -TERM "${fixture_pids[0]}"
  wait "${fixture_pids[0]}" || true
  fixture_pids=()
  [[ $(tail -n 1 "$scratch/memory-err-$lifetime" | jq -r '.decision') == RELEASED ]]
done

# Deadlines are capped per class: an hour for batch work, 15 minutes exclusive.
for request in 'exclusive 901' 'heavy 3601'; do
  read -r class seconds <<<"$request"
  set +e
  refused=$(XDG_RUNTIME_DIR="$fixture_runtime" bun "$scratch/machine-capacity.mjs" run \
    --class "$class" --owner fixture:/capacity --timeout-seconds "$seconds" -- true 2>&1)
  refused_status=$?
  set -e
  [[ $refused_status -eq 2 ]]
done

held_run() {
  local class=$1 owner=$2 name=$3; shift 3
  mkfifo "$scratch/$name-ready.receipt" "$scratch/$name-events"
  exec {sink}> >(tee "$scratch/$name-err" >"$scratch/$name-events")
  collector_pids+=("$!")
  XDG_RUNTIME_DIR="$fixture_runtime" bun "$scratch/machine-capacity.mjs" "$@" \
    --class "$class" --owner "fixture:/$owner" \
    -- bash -c 'IFS=: read -r _ _ group < /proc/self/cgroup; printf "%s\n" "$group" >"$1"; printf "ready\n" >"$1.receipt"; sleep 60 & wait' \
    fixture-drain "$scratch/$name-ready" >"$scratch/$name-out" 2>&"$sink" &
  fixture_pids+=("$!")
  pid_of[$name]=$!
  exec {sink}>&-
  exec {events}<"$scratch/$name-events"
  receipt_fds+=("$events")
  event_of[$name]=$events
}
declare -A pid_of event_of
stop_run() {
  kill -TERM "${pid_of[$1]}"
  wait "${pid_of[$1]}" || true
  while IFS= read -r event <&"${event_of[$1]}"; do
    [[ $(jq -r .decision <<<"$event") != RELEASED ]] || break
  done
  [[ $(tail -n 1 "$scratch/$1-err" | jq -r .decision) == RELEASED ]]
}
status() { XDG_RUNTIME_DIR="$fixture_runtime" bun "$scratch/machine-capacity.mjs" status; }
# The held moderate fixtures are known 2-core builds, so two of them fill a 4-core host's 3 attended CPUs.
export AGENT_CAPACITY_DB="$scratch/drain.db" AGENT_CAPACITY_USAGE_LOG="$scratch/drain-usage.jsonl"
for sample in 1 2 3; do printf '{"at":"2026-10-10T00:00:0%sZ","owner":"fixture:/seed","shape":"read","class":"moderate","meanCores":2,"p90Cores":2,"peakCores":2,"peakMemoryMiB":512,"wallSeconds":120}\n' "$sample"; done >>"$AGENT_CAPACITY_USAGE_LOG"
# An exclusive request passes an earlier batch ticket held by pressure (the
# recorded file is also the protected slices' reading).
calm_pressure 90.00
held_run moderate early early run --timeout-seconds 30
IFS= read -r event <&"${event_of[early]}"
[[ $(head -n 1 "$scratch/early-err" | jq -r '.reason') == DEFER_INTERACTIVE_PRESSURE ]]
held_run exclusive drain drain session
wait_for "$scratch/drain-ready"
read -r group <"$scratch/drain-ready"
[[ $(systemctl --user show "${group##*/}" --property=RuntimeMaxUSec --value) == infinity && $(xargs -I{} cat /proc/{}/cmdline <"/sys/fs/cgroup$group/cgroup.procs" | tr '\0' ' ') == *'capacity deadline '*' 900 --'* ]]
calm_pressure 0.00
IFS= read -r event <&"${event_of[early]}"
[[ $(jq -r .reason <<<"$event") == DEFER_EXCLUSIVE ]]
[[ ! -s "$scratch/early-ready" ]]
[[ $(status | jq -c '[.holding[].owner, .queued[].owner, .queued[0].position]') == '["fixture:/drain","fixture:/early",1]' ]]
stop_run drain
wait_for "$scratch/early-ready"
# While a running lease holds the machine, a queued exclusive request goes first
# and later heavy work waits behind it; a moderate build still starts.
held_run exclusive drain second run --timeout-seconds 30
IFS= read -r event <&"${event_of[second]}"
held_run moderate build build run --timeout-seconds 30
wait_for "$scratch/build-ready"
AGENT_CAPACITY_CORES=4 SAFE_PUSH_LAND_HEAD=0123456 XDG_RUNTIME_DIR="$fixture_runtime" timeout 60 bun "$scratch/machine-capacity.mjs" run --class moderate --owner land-check --timeout-seconds 30 -- true 2>/dev/null
status=0; AGENT_CAPACITY_CORES=4 XDG_RUNTIME_DIR="$fixture_runtime" timeout 8 bun "$scratch/machine-capacity.mjs" run --class moderate --owner land-check --timeout-seconds 30 -- true 2>"$scratch/saturated-err" || status=$?
[[ $status == 124 && $(head -n 1 "$scratch/saturated-err" | jq -r .reason) == DEFER_CPU_CAPACITY ]]
held_run heavy late late run --timeout-seconds 30
IFS= read -r event <&"${event_of[late]}"
[[ $(head -n 1 "$scratch/second-err" | jq -r '.reason') == DEFER_EXCLUSIVE ]]
[[ $(head -n 1 "$scratch/late-err" | jq -r '.reason') == DEFER_EXCLUSIVE_QUEUED ]]
[[ $(status | jq -c '[.holding[] | .owner + " " + .class] + [.queued[] | .class]') == '["fixture:/early moderate","fixture:/build moderate","exclusive","heavy"]' ]]
[[ $(status | jq '.holding[0].remainingSeconds') -le 35 ]]
stop_run early
stop_run build
wait_for "$scratch/second-ready"
IFS= read -r event <&"${event_of[late]}"
[[ $(jq -r .reason <<<"$event") == DEFER_EXCLUSIVE ]]
[[ ! -s "$scratch/late-ready" ]]
stop_run second
wait_for "$scratch/late-ready"
stop_run late
fixture_pids=()
for pid in "${collector_pids[@]}"; do wait "$pid"; done
collector_pids=()
[[ $(status | jq -c '[.holding, .queued]') == '[[],[]]' ]]
export AGENT_CAPACITY_DB=$original_capacity_db AGENT_CAPACITY_USAGE_LOG=$original_usage_log

# Mode selects the profile; auto without an away signal is attended.
mode=$(XDG_RUNTIME_DIR="$fixture_runtime" bun "$scratch/machine-capacity.mjs" mode away)
[[ $(jq -r '.profile' <<<"$mode") == unattended ]]
[[ $(XDG_RUNTIME_DIR="$fixture_runtime" bun "$scratch/machine-capacity.mjs" probe --class agent | jq -r '.profile') == unattended ]]
XDG_RUNTIME_DIR="$fixture_runtime" bun "$scratch/machine-capacity.mjs" mode auto >/dev/null
[[ $(XDG_RUNTIME_DIR="$fixture_runtime" bun "$scratch/machine-capacity.mjs" probe --class agent | jq -r '.profile') == attended ]]
[[ $(XDG_RUNTIME_DIR="$fixture_runtime" bun "$scratch/machine-capacity.mjs" mode present | jq -r '.profile') == attended ]]

# A lease scope throttled by its own CPUQuota stalls in /proc/pressure/cpu and
# agent.slice; that wait must not hold the queue. Other slices' wait still does.
cgroups="$scratch/cgroup"
user_manager="$cgroups/user.slice/user-$(id -u).slice/user@$(id -u).service"
lease_scope="$user_manager/agent.slice/agent-capacity-lease.scope"
mkdir -p "$cgroups/system.slice" "$lease_scope" "$user_manager"/{app,background,session,native}.slice
psi() { printf 'some avg10=%s avg60=0.00 avg300=0.00 total=0\n' "$2" >"$1/cpu.pressure"; }
for slice in "$cgroups/system.slice" "$user_manager"/{app,background,session,native}.slice; do psi "$slice" 0.00; done
psi "$user_manager/agent.slice" 90.00
psi "$lease_scope" 90.00
throttled_probe() {
  env -u AGENT_CAPACITY_CPU_PRESSURE AGENT_CAPACITY_CGROUP_ROOT="$cgroups" XDG_RUNTIME_DIR="$fixture_runtime" \
    bun "$scratch/machine-capacity.mjs" probe --class moderate | jq -r '.reason // .decision' || true
}
[[ $(throttled_probe) != DEFER_CPU_PRESSURE ]]
psi "$user_manager/app.slice" 90.00
[[ $(throttled_probe) == DEFER_CPU_PRESSURE ]]

# A deferred native client holds new gpu leases until it is admitted.
gpu_probe() { XDG_RUNTIME_DIR="$fixture_runtime" bun "$scratch/machine-capacity.mjs" probe --class "$1" | jq -r '.reason' || true; }
echo 95 >"$AGENT_CAPACITY_GPU_BUSY"
[[ $(gpu_probe native) == DEFER_GPU_BUSY ]]
[[ $(gpu_probe gpu) == DEFER_NATIVE_WAITING ]]
echo 0 >"$AGENT_CAPACITY_GPU_BUSY"
[[ $(gpu_probe native) == RUN ]]
[[ $(gpu_probe gpu) == RUN ]]

# Release records what the scope used: two busy loops under a 2-CPU quota.
XDG_RUNTIME_DIR="$fixture_runtime" bun "$scratch/machine-capacity.mjs" run --class moderate --owner fixture:/measure \
  --timeout-seconds 30 -- bash -c 'cd / && timeout 3 sh -c "while :; do :; done" & timeout 3 sh -c "while :; do :; done"; wait' 2>/dev/null
measured=$(jq -c 'select(.owner == "fixture:/measure")' "$AGENT_CAPACITY_USAGE_LOG")
[[ $(jq -r '.shape' <<<"$measured") == sh ]]
[[ $(jq '.meanCores > 1.5 and .meanCores <= 2.1 and .peakCores <= 2.2 and .cpuSeconds > 4' <<<"$measured") == true ]]

export AGENT_CAPACITY_DB="$scratch/sizing.db" AGENT_CAPACITY_USAGE_LOG="$scratch/sizing-usage.jsonl"
for sample in 1 2 3; do
  printf '{"at":"2026-10-10T00:00:0%sZ","owner":"fixture:/sized","shape":"fixturebuild","class":"moderate","wallSeconds":10,"meanCores":3,"p90Cores":4,"peakCores":4,"peakMemoryMiB":3200,"gpuSeconds":0}\n' "$sample"
  printf '{"at":"2026-10-10T00:00:0%sZ","owner":"fixture:/sized","shape":"fixturerender","class":"moderate","wallSeconds":10,"meanCores":0.5,"p90Cores":0.6,"peakCores":0.6,"peakMemoryMiB":853,"gpuSeconds":6}\n' "$sample"
done >>"$AGENT_CAPACITY_USAGE_LOG"
for program in fixturebuild fixturerender fixturenew; do printf '#!/bin/sh\n' >"$scratch/$program"; chmod +x "$scratch/$program"; done
sized() {
  XDG_RUNTIME_DIR="$fixture_runtime" bun "$scratch/machine-capacity.mjs" run --owner fixture:/sized --timeout-seconds 30 \
    -- "$scratch/$1" 2>&1 >/dev/null | head -1 | jq -c '{class, requestedCpus, requestedMemoryMiB}'
}
[[ $(sized fixturebuild) == '{"class":"heavy","requestedCpus":4,"requestedMemoryMiB":3840}' ]]
[[ $(sized fixturerender) == '{"class":"gpu","requestedCpus":1,"requestedMemoryMiB":1024}' ]]
export AGENT_CAPACITY_DB="$scratch/unseen.db" AGENT_CAPACITY_USAGE_LOG="$scratch/unseen-usage.jsonl"
[[ $(sized fixturenew) == '{"class":"moderate","requestedCpus":2,"requestedMemoryMiB":2458}' ]]
export AGENT_CAPACITY_DB=$original_capacity_db AGENT_CAPACITY_USAGE_LOG=$original_usage_log

# Unleased load comes from the watchdog's latest sample: leaf cgroups over one
# core outside the desktop session and lease scopes are reported.
watchdog="$scratch/watchdog"
mkdir -p "$watchdog"
printf '%s\n' '{"at":"1970-01-01T00:20:00.000Z","intervalSeconds":30,"cgroups":[{"cgroup":"app.slice","cores":2.5},{"cgroup":"app.slice/app-build.scope","cores":2},{"cgroup":"session.slice/niri.scope","cores":2},{"cgroup":"agent.slice","cores":4.6},{"cgroup":"agent.slice/agent-capacity.slice","cores":2},{"cgroup":"agent.slice/agent-capacity.slice/agent-capacity-0123abcd.scope","cores":2},{"cgroup":"agent.slice/run-p2-i2.scope","cores":0.6}],"top":[{"pid":7,"cores":2,"cgroup":"app.slice/app-build.scope","command":"make -j"}]}' >"$watchdog/samples.jsonl"
unleased_sample() {
  env -u AGENT_CAPACITY_CPU_PRESSURE CAPACITY_WATCHDOG_DIR="$watchdog" AGENT_CAPACITY_NOW="$1" \
    XDG_RUNTIME_DIR="$fixture_runtime" bun "$scratch/machine-capacity.mjs" "${@:2}"
}
[[ $(unleased_sample 1300000 status | jq -c '.unleasedHeavy') == null ]]
[[ $(unleased_sample 1210000 status | jq -c '.unleasedHeavy | {cores, windowSeconds, heavy: [.heavy[] | {cgroup, cores}]}') == '{"cores":4.6,"windowSeconds":30,"heavy":[{"cgroup":"app.slice/app-build.scope","cores":2}]}' ]]
# The spawn gate's committed CPUs add unleased heavy load to live batch leases.
[[ $(unleased_sample 1210000 probe --class agent | jq -c '{leasedBatchCpus, committedBatchCpus}') == '{"leasedBatchCpus":0,"committedBatchCpus":4.6}' ]]

for pid in "${collector_pids[@]}"; do wait "$pid"; done
collector_pids=()
printf 'machine-capacity policy, aggregate enforcement, and session lifetime: PASS\n'
