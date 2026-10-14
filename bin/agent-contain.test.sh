#!/usr/bin/env bash
set -uo pipefail

here=$(cd "$(dirname "$0")" && pwd)
tool=$here/agent-contain
tmp=$(mktemp -d)
trap 'rm -rf -- "$tmp"' EXIT
pass=0
fail=0

user=/user.slice/user-1000.slice/user@1000.service
proc=$tmp/proc
cg=$tmp/cgroup
inbox=$tmp/inbox.md

add() {
  local pid=$1 comm=$2 ppid=$3 group=$4 cmd=$5
  mkdir -p "$proc/$pid"
  printf '%s (%s) S %s 1 1 0\n' "$pid" "$comm" "$ppid" > "$proc/$pid/stat"
  printf '0::%s\n' "$group" > "$proc/$pid/cgroup"
  printf '%s\0' $cmd > "$proc/$pid/cmdline"
}

check() {
  local name=$1 want=$2 got=$3
  if [ "$got" = "$want" ]; then
    pass=$((pass + 1)); echo "PASS  $name"
  else
    fail=$((fail + 1)); echo "FAIL  $name"; echo "  want: $want"; echo "  got:  $got"
  fi
}

busctl=$tmp/busctl
printf '#!/usr/bin/env bash\nprintf "%%s\\n" "$*" >> %s\n' "$tmp/busctl.log" > "$busctl"
chmod +x "$busctl"

run() {
  AGENT_CONTAIN_STATE=$tmp/reported.json AGENT_CONTAIN_BUSCTL=$busctl AGENT_CONTAIN_PROC=$proc AGENT_CONTAIN_CGROUP=$cg AGENT_CONTAIN_INBOX=$inbox AGENT_CONTAIN_UID=1000 \
    AGENT_CONTAIN_HOST=testhost python3 "$tool" sweep "$@"
}

lead=$user/app.slice/app-spawn-quiet-lead-1.scope
pod=$user/user.slice/libpod-abc.scope
farm=$user/agent.slice/agent-capacity.slice/agent-capacity-farm.slice/libpod-def.scope
add 1 systemd 0 /init.scope "systemd"
add 100 ghostty 1 $user/app.slice/app-ghostty-1.scope "ghostty"
add 101 bash 100 $user/app.slice/app-ghostty-1.scope "bash"
add 200 claude 1 "$lead" "claude"
add 201 bash 200 "$lead" "bash -c bun test"
add 202 bun 201 "$lead" "bun test"
add 300 codex 1 $user/agent.slice/run-p300.scope "codex"
add 301 bash 300 $user/agent.slice/run-p300.scope "bash -c make"
add 400 conmon 1 "$pod" "conmon"
add 401 lua 400 "$pod/container" "lua perf.lua"
add 500 conmon 1 "$farm" "conmon"
add 501 lua 500 "$farm/container" "lua perf.lua"
mkdir -p "$cg$pod" "$cg$farm"
echo 0 > "$cg$pod/cgroup.freeze"
echo 0 > "$cg$farm/cgroup.freeze"

plan=$(run --dry-run | awk '{print $1, $2}' | sort | tr '\n' ' ')
check "a claude child outside agent.slice is moved, a codex child inside it and the CLIs stay, Tom's own shell stays" \
  "freeze 400 move 201 move 202 " "$plan"

out=$(run)
check "a container scope outside agent.slice is frozen" "1" "$(cat "$cg$pod/cgroup.freeze")"
check "the claude children are moved together into a transient scope in agent-capacity.slice" \
  "1" "$(rg -c 'StartTransientUnit ssa\(sv\)a\(sa\(sv\)\) agent-contain-201-[0-9]+\.scope fail 2 PIDs au 2 201 202 Slice s agent-capacity.slice 0' "$tmp/busctl.log")"
check "the moves out of one cgroup get one INCIDENT line with the first pid, its command and the count" \
  "1" "$(rg -c "action=moved pid=201 cmd='bash -c bun test' cgroup=$lead processes=2$" "$inbox")"
check "a container inside agent-capacity-farm.slice is left running" "0" "$(cat "$cg$farm/cgroup.freeze")"
check "the frozen container gets one INCIDENT line naming pid, command and cgroup" \
  "1" "$(rg -c "^INCIDENT .* agent-contain host=testhost action=frozen pid=400 cmd='conmon' cgroup=$pod$" "$inbox")"

run >/dev/null
check "an already frozen container and an already reported cgroup are not reported again within the hour" "2" "$(rg -c '^INCIDENT' "$inbox")"
check "repeat moves still happen" "2" "$(rg -c 'StartTransientUnit' "$tmp/busctl.log")"

echo "agent-contain: $pass passed, $fail failed"
[ "$fail" -eq 0 ]
