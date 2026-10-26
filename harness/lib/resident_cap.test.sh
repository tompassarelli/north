#!/usr/bin/env bash
# Recorded scenario (argv and process shapes recorded from live laptop leads, 13 Oct 2026): on host nexus with
# resident_cap = 6, resident_cap.py gate calls for claude and codex launches against a fake /proc and org registry. Six registered
# sessions hold a seventh launch with the Hold line, five admit it, a `claude --cloud` launch at 6/6 is admitted,
# the launch's own org node is not its peer, CASE=URGENT passes, and a host without the key is never held.
set -uo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/resident-cap-test.XXXXXX")"
trap 'rm -rf "${scratch:?}"' EXIT
pass=0 fail=0
check() { if [ "$1" = ok ]; then pass=$((pass + 1)); printf 'PASS  %s\n' "$2"; else fail=$((fail + 1)); printf 'FAIL  %s\n      %s\n' "$2" "${3:-}"; fi; }

mkdir -p "$scratch/home" "$scratch/ops" "$scratch/bin"; printf '#!/usr/bin/env bash\n[[ "$*" == "fleet list --json" ]] || exit 2; jq '\''[.nodes[] | {name: .id, host_pid: .pid, status: (if .state == "finished" then "exited" else "working" end)}]'\'' "$RESIDENT_FLEET_FIXTURE"\n' >"$scratch/bin/north"; chmod +x "$scratch/bin/north"
printf '[hosts.nexus]\nresident_cap = 6\n' >"$scratch/ops/orchestration.toml"

python3 - "$scratch" <<'PY'
import json, os, sys
root = sys.argv[1]
claude = "/home/tom/.local/lib/claude/current/bin/claude"
codex = "/home/tom/.local/lib/codex/current/bin/codex"

def proc(base, pid, ppid, comm, argv):
    d = f"{base}/{pid}"
    os.makedirs(d)
    open(f"{d}/stat", "w").write(f"{pid} ({comm}) S {ppid} {pid} {pid} 0 -1\n")
    open(f"{d}/cmdline", "wb").write(b"".join(a.encode() + b"\0" for a in argv))

def world(name, registered, unregistered=0, launcher_node=False):
    base = f"{root}/{name}"
    os.makedirs(base)
    nodes = []
    proc(base, 1, 0, "systemd", ["/run/current-system/systemd/lib/systemd/systemd"])
    for i in range(registered):
        shell, agent = 1000 + 10 * i, 1001 + 10 * i
        proc(base, shell, 1, "bash", ["bash", "-c", 'north fleet launch --name test; claude'])
        if i % 2:
            proc(base, agent, shell, "codex", [codex, "-c", 'service_tier="default"', "resume", "--remote", "ws://x"])
        else:
            proc(base, agent, shell, "claude", [claude, "-p", "--input-format", "stream-json", "--session-id", f"s{i}"])
        proc(base, agent + 1, agent, "bash", ["/run/current-system/sw/bin/bash", "-c", "rg x"])
        proc(base, agent + 2, agent, "claude", ["ugrep", "-G", "x"])
        nodes.append({"id": f"lead-{i}", "state": "active", "pid": shell})
    nodes.append({"id": "gone", "state": "active", "pid": 99999})
    nodes.append({"id": "done", "state": "finished", "pid": 1})
    for i in range(unregistered):
        proc(base, 3000 + i, 1, "claude", [claude, "--resume"])
    proc(base, 4000, 1, "codex", [codex, "app-server", "--listen", "ws://x"])
    proc(base, 4001, 1, "claude", [claude, "-p", "one shot"])
    proc(base, 4002, 1, "claude", [claude, "--cloud", "worker"])
    if launcher_node:
        proc(base, 5000, 1, "bash", ["bash", "-c", 'north fleet launch --name test; claude'])
        proc(base, 5001, 5000, "bash", ["bash", "/home/tom/.local/bin/codex", "resume"])
        nodes.append({"id": "new-lead", "state": "active", "pid": 5000})
    json.dump({"nodes": nodes}, open(f"{root}/{name}.fleet.json", "w"))

world("six", 6)
world("five", 5)
world("five-plus-tom", 5, unregistered=1)
world("five-plus-self", 5, launcher_node=True)
world("five-plus-self-and-tom", 5, unregistered=1, launcher_node=True)
PY

launch() {
  local world=$1 host=$2 tool=$3; shift 3
  env -u NORTH_SLICE_ENTERED HOME="$scratch/home" AGENT_ROLE=worker NORTH_NO_SLICE=1 RESIDENT_CAP_PROC="$scratch/$world" \
    RESIDENT_CAP_HOST="$host" NORTH_BIN="$scratch/bin/north" RESIDENT_FLEET_FIXTURE="$scratch/$world.fleet.json" AGENTS_OPERATOR_DIR="$scratch/ops" \
    CODEX_BEHAVIOR_STATE="$scratch/behavior" CODEX_HOME="$scratch/codex-home" python3 -I "$root/harness/lib/resident_cap.py" gate "$tool" "$@" 2>&1
}
hold='Hold: nexus has 6/6 resident agent CLIs; place this worker on laptop|cloud'

out=$(launch six nexus claude --name worker-7 "Read brief.md"); rc=$?
[ "$rc" -ne 0 ] && [ "$out" = "$hold" ] && check ok "6 registered sessions on nexus: a seventh claude launch exits $rc with the Hold line" || check bad 'six claude' "rc=$rc $out"
out=$(launch six nexus codex resume -m gpt-6.1-sol "read brief.md"); rc=$?
[ "$rc" -ne 0 ] && [ "$out" = "$hold" ] && check ok "6 registered sessions on nexus: a seventh codex launch exits $rc with the Hold line" || check bad 'six codex' "rc=$rc $out"
out=$(launch five nexus claude --name worker-6 "Read brief.md"); rc=$?
[ "$rc" -eq 0 ] && [ -z "$out" ] && check ok '5 registered sessions on nexus: the launch is admitted' || check bad 'five' "rc=$rc $out"
out=$(launch six nexus claude --cloud "Read brief.md"); rc=$?
[ "$rc" -eq 0 ] && [ -z "$out" ] && check ok '6/6 on nexus: a claude --cloud launch is admitted' || check bad 'cloud' "rc=$rc $out"
out=$(launch five-plus-tom nexus claude); rc=$?
[ "$rc" -ne 0 ] && [ "$out" = "$hold" ] && check ok '5 registered plus an unregistered top-level claude hold at 6/6' || check bad 'unregistered' "rc=$rc $out"
out=$(RESIDENT_CAP_PID=5001 launch five-plus-self nexus codex resume); rc=$?
[ "$rc" -eq 0 ] && [ -z "$out" ] && check ok "the launch's own org node is not counted as a peer" || check bad 'self' "rc=$rc $out"
out=$(RESIDENT_CAP_PID=5001 launch five-plus-self-and-tom nexus codex resume); rc=$?
[ "$rc" -ne 0 ] && [ "$out" = "$hold" ] && check ok "under its own org node, a launch still counts an unregistered top-level claude" || check bad 'self and tom' "rc=$rc $out"
out=$(CASE=URGENT FACT="lead restart after crash, cannot wait" launch six nexus claude); rc=$?
[ "$rc" -eq 0 ] && [ -z "$out" ] && rg -q '"gate": "resident-cap"' "$scratch/behavior/verify-overrides.jsonl" \
  && check ok 'CASE=URGENT with a FACT passes at 6/6 and is logged' || check bad 'urgent' "rc=$rc $out"
out=$(launch six whiterabbit claude); rc=$?
[ "$rc" -eq 0 ] && [ -z "$out" ] && check ok 'a host without resident_cap is never held' || check bad 'laptop' "rc=$rc $out"

printf '%d passed, %d failed\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
