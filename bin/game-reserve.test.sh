#!/usr/bin/env bash
set -uo pipefail

here=$(cd "$(dirname "$0")" && pwd)
north=$(cd "$here/.." && realpath "${CARGO_TARGET_DIR:-target}")/debug/north
tmp=$(mktemp -d)
trap 'rm -rf -- "$tmp"' EXIT
pass=0 fail=0

user=/user.slice/user-1000.slice/user@1000.service
proc=$tmp/proc
log=$tmp/calls.log

add() {
  local pid=$1 comm=$2 ppid=$3 group=$4 cmd=$5 start=${6:-0}
  mkdir -p "$proc/$pid"
  printf '%s (%s) S %s 1 1 0 -1 0 0 0 0 0 0 0 0 0 20 0 1 0 %s 0\n' "$pid" "$comm" "$ppid" "$start" > "$proc/$pid/stat"
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
  GAME_RESERVE_AFFINITY_LOG=$tmp/pins.log GAME_RESERVE_PROC=$proc GAME_RESERVE_STATE=$tmp/state.json GAME_RESERVE_SYSTEMCTL=$tmp/systemctl GAME_RESERVE_BUSCTL=$tmp/busctl \
    "$north" capacity game-reserve "$@"
}

rofi=$user/app.slice/app-niri-rofi-1.scope
mkdir -p "$proc" && echo "1000.00 9000.00" > "$proc/uptime"
add 1 systemd 0 /init.scope "systemd"
add 10 steam 1 "$rofi" "steam"
add 11 reaper 10 "$rofi" "reaper SteamLaunch"
add 12 wine64-preloader 11 "$rofi" "C:\\Program Files (x86)\\Battle.net\\Battle.net.exe"
add 13 "Warcraft III.ex" 12 "$rofi" "C:\\Program Files (x86)\\Warcraft III\\_retail_\\x86_64\\Warcraft III.exe -launch" 88000
add 20 "Warcraft III.ex" 1 "$user/native.slice/agent-capacity-1.scope" "C:\\Program Files (x86)\\Warcraft III\\_retail_\\x86_64\\Warcraft III.exe -launch"
add 30 bash 1 "$user/app.slice/app-ghostty-1.scope" "bash"
add 50 bun 1 "$user/agent.slice/agent-capacity.slice/run-1.scope" "bun test"

run status >/dev/null
check "status exits non-zero before the reserve is on" "1" "$?"

out=$(run on)
check "on confines agents to CCX1 with the memory cap" \
  "1" "$(rg -c '^systemctl --user set-property --runtime agent.slice AllowedCPUs=4-11,16-23 MemoryMax=80G$' "$log")"
check "on moves only Warcraft III, never Steam, Battle.net, a pool client or Tom's terminal, into game-reserve.slice" \
  "1" "$(rg -c 'StartTransientUnit ssa\(sv\)a\(sa\(sv\)\) game-reserve-[0-9]+-13\.scope fail 2 PIDs au 1 13 Slice s game-reserve.slice 0$' "$log")"
check "on pins Warcraft III to CCX0, agent and pool client threads to CCX1, never Steam, Battle.net or Tom's terminal" \
  "pin 13 0-15:8 pin 20 4-23:16 pin 50 4-23:16 " "$(sort -n -k2 "$tmp/pins.log" | tr '\n' ' ')"
check "on reserves CCX0, weights and the memory floor for the game" \
  "1" "$(rg -c '^systemctl --user set-property --runtime game-reserve.slice AllowedCPUs=0-3,12-15 CPUWeight=1000 IOWeight=1000 MemoryMin=4G MemoryLow=6G$' "$log")"

printf '0::%s/game-reserve.slice/game-reserve-1-13.scope\n' "$user" > "$proc/13/cgroup"
add 40 "Warcraft III.ex" 12 "$rofi" "C:\\Program Files (x86)\\Warcraft III\\_retail_\\x86_64\\Warcraft III.exe -launch" 90000
run on >/dev/null
check "a second on moves only a Warcraft III started since" \
  "1" "$(rg -c 'PIDs au 1 40 Slice' "$log")" && printf '0::%s/game-reserve.slice/game-reserve-2-40.scope\n' "$user" > "$proc/40/cgroup"
check "status exits 0 when on and reports the game inside game-reserve.slice" \
  "0 game-reserve on: game CPUs 0-3,12-15, agents CPUs 4-11,16-23; Warcraft III 2/2 in game-reserve.slice" \
  "$(out=$(run status); echo "$? $out")"

run off >/dev/null
check "off gives agent.slice every CPU and lifts the cap" \
  "1" "$(rg -c '^systemctl --user set-property --runtime agent.slice AllowedCPUs= MemoryMax=infinity$' "$log")"
run status >/dev/null
check "status exits non-zero after off" "1" "$?"

hook() {
  FARM_RUNNER_GAME=$1 FARM_RUNNER_GAME_RESERVE_IDLE=$2 FARM_RUNNER_GAME_RESERVE_STATE=$tmp FARM_RUNNER_NORTH=$north GAME_RESERVE_PROC=$proc \
    GAME_RESERVE_STATE=$tmp/state.json GAME_RESERVE_SYSTEMCTL=$tmp/systemctl GAME_RESERVE_BUSCTL=$tmp/busctl \
    GAME_RESERVE_AFFINITY_LOG=$tmp/pins.log bash -c 'eval "$(sed -n "/^state_cache=/,/^units()/p" "$0" | head -n -1)"; game_reserve_hook' "$here/farm-runner"
}

hook '[b]ash' 600
check "Tom's game starting switches the reserve on automatically" "true true" "$(jq -r '"\(.on) \(.auto)"' "$tmp/state.json")"
hook 'no-such-game-zz9' 600
check "an auto reserve stays on while the game was seen inside the idle period" "true" "$(jq -r .on "$tmp/state.json")"
hook 'no-such-game-zz9' 0
check "an auto reserve switches off once the game has been gone for the idle period" "false" "$(jq -r .on "$tmp/state.json")"
run on >/dev/null
hook 'no-such-game-zz9' 0
check "a reserve Tom switched on by hand is never switched off by the hook" "true false" "$(jq -r '"\(.on) \(.auto)"' "$tmp/state.json")"

echo "game-reserve: $pass passed, $fail failed"
[ "$fail" -eq 0 ]
