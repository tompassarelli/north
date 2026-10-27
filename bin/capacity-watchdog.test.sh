#!/usr/bin/env bash
# Recorded scenario through the decision core: an unleased 5-core tree held
# for 2 minutes is one incident with cgroup, cwd and parent chain; the same
# load in a capacity scope and a 3.9-core tree are not. Git repack outside the
# nightly timer is terminated, inside it is not. Then the real sampler SIGTERMs
# a live fake `git repack` process. A recorded Codex capacity error flags provider-capacity codex in its window.
set -uo pipefail
target="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/capacity-watchdog"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/capacity-watchdog-test.XXXXXX")"
fake=""
fixture_scope=""
trap '[ -z "$fake" ] || kill "$fake" 2>/dev/null; [ -z "$fixture_scope" ] || systemctl --user stop "$fixture_scope" 2>/dev/null; rm -rf "${scratch:?}"' EXIT
pass=0 fail=0
check() { if [ "$1" = ok ]; then pass=$((pass + 1)); printf 'PASS  %s\n' "$2"; else fail=$((fail + 1)); printf 'FAIL  %s\n      %s\n' "$2" "${3:-}"; fi; }

python3 - "$scratch/scenario.jsonl" <<'EOF'
import json, sys
U = "/user.slice/user-1000.slice/user@1000.service/"
app, scope = U + "app.slice/app-niri-ghostty-1.scope", U + "agent.slice/agent-capacity.slice/agent-capacity-" + "a" * 32 + ".scope"
nightly = U + "app.slice/git-maintenance-nightly.service"
def proc(pid, ppid, sid, args, cgroup, rate, t, cwd="/srv/repo/main"):
    return {"pid": pid, "ppid": ppid, "sid": sid, "comm": args[0].split("/")[-1][:15], "args": args, "cgroup": cgroup,
            "ticks": int(rate * 100 * t), "start": str(pid * 10), "rssKiB": 1024, "cwd": cwd}
with open(sys.argv[1], "w") as out:
    for i in range(6):
        t = i * 30
        procs = [proc(400, 1, 400, ["claude"], app, 0.2, t)]
        procs.append(proc(500, 400, 500, ["git", "maintenance", "run", "--auto", "--detach"], app, 0, t))
        procs += [proc(501 + k, 500, 500, ["git", "pack-objects", "--all"], app, 1.0, t) for k in range(5)]
        procs += [proc(601 + k, 1, 600, ["cargo", "build"], scope, 1.0, t) for k in range(6)]
        procs += [proc(701 + k, 1, 700, ["bun", "test"], app, 0.975, t) for k in range(4)]
        procs.append(proc(800, 1, 800, ["git", "-C", "/x", "repack", "-d"], nightly, 0.5, t))
        procs.append(proc(900, 400, 400, ["git", "fetch", "origin"], app, 0.1, t))
        out.write(json.dumps({"at": 1_700_000_000_000 + t * 1000, "load": [1, 1, 1], "cpuPsi": 50, "cpuPsi60": 40, "memPsi": 0,
            "memFullPsi": 0, "memAvailableMiB": 60000, "memTotalMiB": 96000, "cgroups": {}, "procs": procs, "leases": []}) + "\n")
EOF
CAPACITY_WATCHDOG_DIR="$scratch/state" "$target" replay "$scratch/scenario.jsonl" >"$scratch/out.jsonl"

trees="$(jq -s '[.[] | select(.kind == "unleased-tree")]' "$scratch/out.jsonl")"
[ "$(jq length <<<"$trees")" = 1 ] && check ok 'one unleased-tree incident across the scenario' || check bad 'one unleased-tree incident' "$trees"
[ "$(jq -r '.[0].cgroup' <<<"$trees")" = 'app.slice/app-niri-ghostty-1.scope' ] && [ "$(jq -r '.[0].seconds' <<<"$trees")" = 120 ] \
  && check ok 'incident names the unleased cgroup after 120 s' || check bad 'incident cgroup and duration' "$trees"
[ "$(jq -r '.[0].cwd' <<<"$trees")" = /srv/repo/main ] && jq -e '.[0].chain | length == 3 and (.[1] | startswith("500 git maintenance")) and (.[2] | startswith("400 claude"))' <<<"$trees" >/dev/null \
  && check ok 'incident records cwd and the parent chain up to the agent' || check bad 'incident cwd and chain' "$trees"
terms="$(jq -s -c '[.[] | select(.term) | .term] | unique' "$scratch/out.jsonl")"
[ "$terms" = '[500,501,502,503,504,505]' ] && check ok 'git maintenance and its pack-objects outside the timer are terminated; nightly repack and fetch are not' \
  || check bad 'terminated pids' "$terms"

mkdir -p "$scratch/fake"
mkfifo "$scratch/fake/blocker" "$scratch/fake/ready"
fixture_scope="watchdog-test-$$.scope"
systemd-run --user --scope --quiet --slice=app.slice --unit="$fixture_scope" \
  bash -c 'cd "$1"; exec -a git bash -c '\''printf "%s\n" "$$" >ready; read -r _ <blocker; :'\'' repack' fixture "$scratch/fake" >"$scratch/process.out" 2>&1 &
launcher=$!
read -r fake <"$scratch/fake/ready"
CAPACITY_WATCHDOG_DIR="$scratch/state" CAPACITY_WATCHDOG_NOTIFY= "$target" sample --once >/dev/null 2>&1
python3 - "$scratch/state/samples.jsonl" <<'PY' && check ok 'live sampler reads kernel pressure and memory' || check bad 'live kernel sample'
import json, math, sys
from pathlib import Path
text = Path(sys.argv[1]).read_text()
sample = json.loads(text.splitlines()[-1])
assert text.endswith('\n') and all(isinstance(sample[key], (int, float)) and math.isfinite(sample[key])
           for key in ('cpuPsi', 'cpuPsi60', 'memPsi', 'memFullPsi'))
assert 0 < sample['memAvailableMiB'] <= sample['memTotalMiB']
PY
systemctl --user stop "$fixture_scope" 2>/dev/null || true
wait "$launcher" 2>/dev/null || true
fake=""
fixture_scope=""
grep -q "git-maintenance-terminated: SIGTERM git repack" "$scratch/state/incidents.md" 2>/dev/null \
  && check ok 'termination is logged to incidents.md' || check bad 'termination logged' "$(cat "$scratch/state/incidents.md" 2>/dev/null)"

THREADS_DB="$scratch/threads.db" CAPACITY_WATCHDOG_DIR="$scratch/empty" python3 - "$target" <<'PY' && check ok 'sanitized ledger scenario flags only silent Claude workers in the window; --since exposes the pre-fix run' || check bad 'handoff ledger scenario'
import json,os,subprocess,sys
from datetime import datetime,timedelta,timezone
from pathlib import Path
target=Path(sys.argv[1])
now=datetime.now(timezone.utc)
def row(agent,tier='high',role='worker',peak=364576,fires=0,ignored=0,ended=None):
    at=(ended or now-timedelta(minutes=1)).isoformat()
    return dict(id=agent,tier=tier,role=role,peak_ctx=peak,handoff_fires=fires,handoff_ignored=ignored,started=at,ended=at,
                actual_min=0,outcome='done',brief='Category: tooling\n[seeded sanitized ledger replay]')
rows=[row('silent'),row('effective',fires=1),row('marker-only',fires=1,ignored=None),
      row('ignored',fires=1,ignored=1,peak=20000),row('threshold',peak=350000),
      row('codex',tier='gpt-6.1-sol high'),row('gemini',tier='gemini-3.1-pro high'),
      row('parent',role='parent'),row('unknown',fires=None),
      row('older-window',ended=now-timedelta(minutes=10)),
      row('pre-fix',ended=datetime(2026,10,10,10,24,8,tzinfo=timezone.utc))]
subprocess.run([str(target.with_name('threads')),'ingest'],input=''.join(json.dumps(r)+'\n' for r in rows),text=True,check=True,capture_output=True)
def flags(*args):
    result=subprocess.run([str(target),'report','--minutes','5',*args],text=True,check=True,capture_output=True)
    return [f for f in json.loads(result.stdout)['flags'] if f.startswith(('handoff hook silent:','handoff ignored:'))]
observed=flags()
assert observed==['handoff hook silent: silent peak 365k, 0 fires','handoff ignored: ignored'],observed
observed=flags('--since','2026-10-10')
assert set(observed)=={'handoff hook silent: silent peak 365k, 0 fires','handoff ignored: ignored',
                       'handoff hook silent: older-window peak 365k, 0 fires',
                       'handoff hook silent: pre-fix peak 365k, 0 fires'},observed
PY

claude_home="$scratch/claude" subagents="$scratch/claude/projects/-srv-repo/lead/subagents"; mkdir -p "$subagents" "$scratch/handoffs" "$scratch/bin"
printf '#!/usr/bin/env bash\n[ "$1 $2" = "fleet list" ] && printf "%%s\\n" '\''[{"name":"fleet-parent","conversation":"parent","parent":null,"provider":"claude","status":"working","cwd":"/srv/repo/main"},{"name":"done","conversation":"done","parent":"fleet-parent","provider":"claude","status":"working","cwd":"/srv/repo/main"},{"name":"not-done","conversation":"not-done","parent":"fleet-parent","provider":"claude","status":"working","cwd":"/srv/repo/main"},{"name":"ignored","conversation":"ignored","parent":"fleet-parent","provider":"claude","status":"working","cwd":"/srv/repo/main"}]'\''\n' >"$scratch/bin/north"; chmod +x "$scratch/bin/north"
python3 - "$subagents" <<'PY'
import json, sys
from datetime import datetime, timedelta, timezone
from pathlib import Path
d = Path(sys.argv[1])
at = lambda m: (datetime.now(timezone.utc) - timedelta(minutes=m)).isoformat().replace("+00:00", "Z")
usage = {"input_tokens": 9, "cache_read_input_tokens": 360000, "output_tokens": 40}
def assistant(n, content, m):
    return {"type": "assistant", "timestamp": at(m), "requestId": f"r{n}", "attributionAgent": "worker-high",
            "message": {"id": f"m{n}", "model": "claude-opus-5-5", "usage": usage, "content": content}}
def tool(n, m):
    return [assistant(n, [{"type": "tool_use", "id": f"t{n}", "name": "Bash", "input": {"command": "bash bin/x.test.sh"}}], m),
            {"type": "user", "timestamp": at(m), "message": {"content": [{"type": "tool_result", "tool_use_id": f"t{n}", "content": "ok"}]}}]
fire = {"type": "attachment", "timestamp": at(6), "attachment": {"type": "hook_additional_context",
        "content": ["Your context is 360k tokens, past the 350k handoff point. Write HANDOFF <path> or finish."]}}
for agent, final in (("done", "Done: scenario green on the fix, red on main."), ("not-done", "Not done: blocked on a missing fixture."),
                     ("ignored", "Checked the remaining cases; all of them look fine.")):
    rows = [{"type": "user", "timestamp": at(9), "sessionId": "parent", "message": {"content": "Item: wisp#9\nCategory: tooling"}},
            *tool(1, 8), fire, *tool(2, 5), *tool(3, 4), assistant(4, [{"type": "text", "text": final}], 3)]
    (d / f"agent-{agent}.jsonl").write_text("".join(json.dumps(r) + "\n" for r in rows))
    (d / f"agent-{agent}.meta.json").write_text(json.dumps({"agentType": "worker-high"}))
PY
ledger_env=(THREADS_DB="$scratch/fires.db" CLAUDE_CONFIG_DIR="$claude_home" CODEX_CONFIG_DIR="$scratch/none" AGY_CONFIG_DIR="$scratch/none"
  LEDGER_REPOS="$scratch/none" CAPACITY_WATCHDOG_DIR="$scratch/empty" PATH="$scratch/bin:$PATH" WORKER_LEDGER_REAL_NORTH="$(cd "$(dirname "$target")/.." && realpath "${CARGO_TARGET_DIR:-target}")/debug/north")
env "${ledger_env[@]}" "$WORKER_LEDGER_REAL_NORTH" worker-ledger --since "$(date -u -d '1 day ago' +%FT%TZ)" >/dev/null 2>"$scratch/ledger.err"
fires=$(env "${ledger_env[@]}" "$target" report --minutes 30 | jq -c '[.flags[] | select(startswith("handoff"))]')
[ "$fires" = '["handoff ignored: ignored"]' ] \
  && check ok 'a fire answered within three assistant turns by a Done: or Not done: report is not handoff ignored; one answered by neither is' \
  || check bad 'handoff fire answered by a final report' "$fires $(cat "$scratch/ledger.err")"

codex_home="$scratch/codex"
mkdir -p "$codex_home/sessions/2026/10/12"
python3 - "$codex_home/sessions/2026/10/12/rollout-capacity.jsonl" <<'PY'
import json, sys, time
from datetime import datetime, timezone
at = lambda m: datetime.fromtimestamp(time.time() - m * 60, timezone.utc).isoformat().replace("+00:00", "Z")
real = {"timestamp": "2026-10-12T06:43:25.408Z", "type": "event_msg", "payload": {"type": "task_complete", "turn_id": "01a12eda-03a2-7590-86bc-a83582c7b81a", "last_agent_message": None, "error": {"message": "Selected model is at capacity. Please try a different model.", "codex_error_info": "server_overloaded"}, "started_at": 1791787402, "completed_at": 1791787405, "duration_ms": 3258}}
rows = [{"timestamp": at(20), "type": "turn_context", "payload": {"model": "gpt-6.1-sol", "effort": "high"}},
        {"timestamp": at(20), "type": "event_msg", "payload": {"type": "task_complete", "last_agent_message": "ok"}},
        dict(real, timestamp=at(2))]
open(sys.argv[1], "w").writelines(json.dumps(r) + "\n" for r in rows)
PY
windows=$(CODEX_CONFIG_DIR="$codex_home" CAPACITY_WATCHDOG_DIR="$scratch/empty" THREADS_DB="$scratch/none.db" "$target" report --minutes 1 --minutes 5)
jq -s -e '(.[0].flags | map(select(startswith("provider-capacity"))) == []) and (.[1].flags | map(select(startswith("provider-capacity codex: 1 turn(s) of gpt-6.1-sol at capacity, last "))) | length == 1)' <<<"$windows" >/dev/null \
  && check ok 'a recorded Codex capacity error 2 minutes ago flags provider-capacity codex in the 5-minute window, not the 1-minute one' || check bad 'provider-capacity flag' "$windows"

python3 - "$scratch" <<'PY'
import json, sys
d = sys.argv[1]
json.dump({"system": ["getty@*.service"], "user": ["niri.service", "worker-ledger.timer"],
           "sockets": ["tcp 127.0.0.1:7070 kanata-main.service", "udp *:* -"], "cores": 0.02, "budgets": ["niri.service 0.06"]}, open(f"{d}/rest.json", "w"))
units = [{"scope": "system", "name": "getty@tty1.service", "cores": 0, "csPerSec": 0},
         {"scope": "user", "name": "niri.service", "cores": 0.05, "csPerSec": 9}, {"scope": "user", "name": "worker-ledger.timer"},
         {"scope": "user", "name": "worker-ledger.service", "cores": 0.01, "csPerSec": 2},
         {"scope": "user", "name": "app-niri-ghostty-1.scope", "cores": 0.3, "csPerSec": 150, "agent": "claude"}]
extra = [{"scope": "user", "name": "smashcraft-green-builds.timer"}, {"scope": "user", "name": "smashcraft-vm-retention.timer"},
         {"scope": "user", "name": "agent-capacity-presence.service", "cores": 0.01, "csPerSec": 29},
         {"scope": "system", "name": "getty@tty1.service", "cores": 0.03, "csPerSec": 5}]
sockets = ["tcp 127.0.0.1:7070 kanata-main.service", "udp 0.0.0.0:51820 -"]
json.dump({"seconds": 30, "units": units[1:] + extra, "sockets": sockets + ["tcp 0.0.0.0:22 sshd.service"]}, open(f"{d}/busy.json", "w"))
json.dump({"seconds": 30, "units": units, "sockets": sockets}, open(f"{d}/rest-observed.json", "w"))
PY
"$target" rest-check --expected "$scratch/rest.json" --observed "$scratch/busy.json" >"$scratch/busy.out"; code=$?
[ "$code" = 1 ] && [ "$(grep -c '^extra user unit: \(smashcraft-green-builds.timer\|smashcraft-vm-retention.timer\|agent-capacity-presence.service\)' "$scratch/busy.out")" = 3 ] \
  && grep -q '^extra socket: tcp 0.0.0.0:22 sshd.service$' "$scratch/busy.out" && grep -q '^over budget: system getty@tty1.service 0.03 cores > 0.02' "$scratch/busy.out" \
  && grep -q '^agent: user app-niri-ghostty-1.scope' "$scratch/busy.out" && ! grep -q 'extra.*\(ghostty\|niri\|worker-ledger\|51820\)' "$scratch/busy.out" \
  && check ok 'rest-check exits 1 naming extra units, sockets and over-budget units, and lists agents apart' || check bad 'rest-check extras' "exit $code: $(cat "$scratch/busy.out")"
"$target" rest-check --expected "$scratch/rest.json" --observed "$scratch/rest-observed.json" >"$scratch/rest.out"; code=$?
[ "$code" = 0 ] && grep -q '^rest-check: at rest; .* 1 agents; non-agent 0.06 cores, 11 cs/s$' "$scratch/rest.out" \
  && check ok 'rest-check exits 0 on only the expected set plus agents' || check bad 'rest-check at rest' "exit $code: $(cat "$scratch/rest.out")"

printf 'capacity-watchdog: %d passed, %d failed\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
