#!/usr/bin/env bash
set -uo pipefail

here=$(cd "$(dirname "$0")" && pwd)
tool=$here/playtest
tmp=$(mktemp -d)
trap 'rm -rf -- "$tmp"' EXIT
pass=0
fail=0

user=/user.slice/user-1000.slice/user@1000.service
proc=$tmp/proc
log=$tmp/calls.log

add() {
  local pid=$1 comm=$2 ppid=$3 group=$4 cmd=$5
  mkdir -p "$proc/$pid"
  printf '%s (%s) S %s 1 1 0\n' "$pid" "$comm" "$ppid" > "$proc/$pid/stat"
  printf '0::%s\n' "$group" > "$proc/$pid/cgroup"
  printf '%s\0' "$cmd" > "$proc/$pid/cmdline"
}

check() {
  local name=$1 want=$2 got=$3
  if [ "$got" = "$want" ]; then
    pass=$((pass + 1)); echo "PASS  $name"
  else
    fail=$((fail + 1)); echo "FAIL  $name"; echo "  want: $want"; echo "  got:  $got"
  fi
}

for tool_name in systemctl busctl; do
  printf '#!/usr/bin/env bash\nprintf "%s %%s\\n" "$*" >> %s\n' "$tool_name" "$log" > "$tmp/$tool_name"
  chmod +x "$tmp/$tool_name"
done

run() {
  PLAYTEST_AFFINITY_LOG=$tmp/pins.log PLAYTEST_PROC=$proc PLAYTEST_STATE=$tmp/state.json PLAYTEST_SYSTEMCTL=$tmp/systemctl PLAYTEST_BUSCTL=$tmp/busctl \
    python3 "$tool" "$@"
}

rofi=$user/app.slice/app-niri-rofi-1.scope
add 1 systemd 0 /init.scope "systemd"
add 10 steam 1 "$rofi" "steam"
add 11 reaper 10 "$rofi" "reaper SteamLaunch"
add 12 wine64-preloader 11 "$rofi" "C:\\Program Files (x86)\\Battle.net\\Battle.net.exe"
add 13 "Warcraft III.ex" 12 "$rofi" "C:\\Program Files (x86)\\Warcraft III\\_retail_\\x86_64\\Warcraft III.exe -launch"
add 20 "Warcraft III.ex" 1 "$user/native.slice/agent-capacity-1.scope" "C:\\Program Files (x86)\\Warcraft III\\_retail_\\x86_64\\Warcraft III.exe -launch"
add 30 bash 1 "$user/app.slice/app-ghostty-1.scope" "bash"
add 50 bun 1 "$user/agent.slice/agent-capacity.slice/run-1.scope" "bun test"

run status >/dev/null
check "status exits non-zero before playtest is on" "1" "$?"

out=$(run on)
check "on confines agents to CCX1 with the memory cap" \
  "1" "$(rg -c '^systemctl --user set-property --runtime agent.slice AllowedCPUs=4-11,16-23 MemoryMax=80G$' "$log")"
check "on moves Steam and every descendant, never a pool client or Tom's terminal, into playtest.slice" \
  "1" "$(rg -c 'StartTransientUnit ssa\(sv\)a\(sa\(sv\)\) playtest-[0-9]+-10\.scope fail 2 PIDs au 4 10 11 12 13 Slice s playtest.slice 0$' "$log")"
check "on pins the game tree to CCX0, agent and pool client threads to CCX1, never Tom's terminal" \
  "pin 10 0-15:8 pin 11 0-15:8 pin 12 0-15:8 pin 13 0-15:8 pin 20 4-23:16 pin 50 4-23:16 " "$(sort -n -k2 "$tmp/pins.log" | tr '\n' ' ')"
check "on reserves CCX0, weights and the memory floor for the game" \
  "1" "$(rg -c '^systemctl --user set-property --runtime playtest.slice AllowedCPUs=0-3,12-15 CPUWeight=1000 IOWeight=1000 MemoryMin=4G MemoryLow=6G$' "$log")"

for pid in 10 11 12 13; do printf '0::%s/playtest.slice/playtest-1-10.scope\n' "$user" > "$proc/$pid/cgroup"; done
add 40 wine64-preloader 1 "$user/app.slice/app-ghostty-2.scope" "C:\\Program Files (x86)\\Battle.net\\Battle.net.exe"
run on >/dev/null
check "a second on moves only a stray Battle.net started outside Steam" \
  "1" "$(rg -c 'PIDs au 1 40 Slice' "$log")"
check "status exits 0 when on and reports the game inside playtest.slice" \
  "0 playtest on: game CPUs 0-3,12-15, agents CPUs 4-11,16-23; Warcraft III 1/1 in playtest.slice" \
  "$(out=$(run status); echo "$? $out")"

run off >/dev/null
check "off gives agent.slice every CPU and lifts the cap" \
  "1" "$(rg -c '^systemctl --user set-property --runtime agent.slice AllowedCPUs= MemoryMax=infinity$' "$log")"
run status >/dev/null
check "status exits non-zero after off" "1" "$?"

hook() {
  FARM_RUNNER_GAME=$1 FARM_RUNNER_PLAYTEST_IDLE=$2 FARM_RUNNER_PLAYTEST_STATE=$tmp PLAYTEST_PROC=$proc \
    PLAYTEST_STATE=$tmp/state.json PLAYTEST_SYSTEMCTL=$tmp/systemctl PLAYTEST_BUSCTL=$tmp/busctl \
    PLAYTEST_AFFINITY_LOG=$tmp/pins.log bash -c 'eval "$(sed -n "/^state_cache=/,/^units()/p" "$0" | head -n -1)"; playtest_hook' "$here/farm-runner"
}

hook '[b]ash' 600
check "Tom's game starting switches playtest on automatically" "true true" "$(jq -r '"\(.on) \(.auto)"' "$tmp/state.json")"
hook 'no-such-game-zz9' 600
check "an auto playtest stays on while the game was seen inside the idle period" "true" "$(jq -r .on "$tmp/state.json")"
hook 'no-such-game-zz9' 0
check "an auto playtest switches off once the game has been gone for the idle period" "false" "$(jq -r .on "$tmp/state.json")"
run on >/dev/null
hook 'no-such-game-zz9' 0
check "a playtest Tom switched on by hand is never switched off by the hook" "true false" "$(jq -r '"\(.on) \(.auto)"' "$tmp/state.json")"

echo "playtest: $pass passed, $fail failed"
[ "$fail" -eq 0 ]
