#!/usr/bin/env bash
# Checks are eval strings, expanded when they run.
# shellcheck disable=SC2016,SC2034
# Fixture session for worker-sweep: which workers count as running, the
# STALLED and HANDOFF flags, session selection and --wait's exit, plus one
# fixture per WAIT flag: landing queue, Actions queue, serial debugging, GPU, one stall-recovery scenario,
# and Codex capacity recovery from a recorded capacity error: Astra fallback and switch back, and Sol retries past the floor.
set -uo pipefail

repo=$(cd "$(dirname "$0")/.." && pwd)
sweep=$repo/bin/worker-sweep
scratch=$(mktemp -d "${TMPDIR:-/tmp}/worker-sweep-test.XXXXXX")
trap 'rm -rf "${scratch:?}"' EXIT
export CLAUDE_CONFIG_DIR=$scratch/claude THREADS_DB=$scratch/threads.db CODEX_CONFIG_DIR=$scratch/codex
export AGENTS_ORG_FILE=$scratch/org.json LEDGER_HANDOFFS=$scratch/handoffs AGENTS_ORCHESTRATION=$scratch/orchestration.toml
printf '[tiers]\n' >"$AGENTS_ORCHESTRATION"
export XDG_STATE_HOME=$scratch/state AGENT_CAPACITY_GPU_BUSY=$scratch/gpu WORKER_SWEEP_PS=$scratch/ps PATH=$scratch/bin:$PATH
mkdir -p "$scratch/bin"
cat >"$scratch/bin/gh" <<'GH'
#!/usr/bin/env bash
case "$2" in
  repos/tompassarelli/smashcraft/actions/*)
    printf '{"total_count":5,"workflow_runs":[{"created_at":"%s"}]}\n' "$(date -u -d '-3 minutes' +%FT%TZ)" ;;
  repos/tompassarelli/wisp/actions/*)
    printf '{"total_count":1,"workflow_runs":[{"created_at":"%s"}]}\n' "$(date -u -d '-25 minutes' +%FT%TZ)" ;;
  repos/tompassarelli/smashcraft/issues*) printf 'smashcraft#9\n' ;;
  *) : ;;
esac
GH
chmod +x "$scratch/bin/gh"
echo 95 >"$scratch/gpu"
: >"$scratch/ps"
mkdir -p "$XDG_STATE_HOME/agents/lane-gc"
printf '{"at":"%s","repos":{}}\n' "$(date -u +%FT%TZ)" >"$XDG_STATE_HOME/agents/lane-gc/stale.json"

python3 - "$CLAUDE_CONFIG_DIR/projects" <<'PY'
import json, os, sys, time
from datetime import datetime, timezone

root = sys.argv[1]
now = time.time()
def ts(minutes_ago):
    return datetime.fromtimestamp(now - minutes_ago * 60, timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.000Z")
def usage(tokens):
    return {"input_tokens": 10, "cache_read_input_tokens": tokens - 10, "cache_creation_input_tokens": 0}
def brief(m, words="brief"): return {"type": "user", "timestamp": ts(m), "message": {"role": "user", "content": words}}
def tool_use(m, tokens, tid="t1", bg=False):
    return {"type": "assistant", "timestamp": ts(m), "message": {"role": "assistant", "usage": usage(tokens),
            "content": [{"type": "tool_use", "id": tid, "name": "Bash", "input": {"command": "x", "run_in_background": bg}}]}}
def result(m, tid="t1"):
    return {"type": "user", "timestamp": ts(m), "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": tid, "content": "ok"}]}}
def text(m, tokens, words):
    return {"type": "assistant", "timestamp": ts(m), "message": {"role": "assistant", "usage": usage(tokens), "content": [{"type": "text", "text": words}]}}
def reminder(m): return {"type": "attachment", "timestamp": ts(m), "attachment": {"type": "total_tokens_reminder"}}
def notice(m, tid):
    return {"type": "attachment", "timestamp": ts(m), "attachment": {"type": "queued_command",
            "prompt": f"<task-notification>\n<tool-use-id>{tid}</tool-use-id>\n<status>completed</status>\n</task-notification>"}}

def session(project, sid, agents):
    d = os.path.join(root, project, sid, "subagents")
    os.makedirs(d)
    for aid, kind, lines in agents:
        with open(os.path.join(d, f"agent-{aid}.jsonl"), "w") as f:
            f.writelines(json.dumps(l) + "\n" for l in lines)
        with open(os.path.join(d, f"agent-{aid}.meta.json"), "w") as f:
            json.dump({"agentType": kind}, f)
    return d

old = session("-home-tom", "old-session", [
    ("oldtool", "worker", [brief(90), tool_use(5, 410000)]),
])
os.utime(old, (now - 3600, now - 3600))
session("-home-tom", "new-session", [
    ("pending", "worker-high", [brief(30, "Item: smashcraft#7"), tool_use(1, 200000)]),
    ("result", "worker", [brief(30), tool_use(2, 150000), result(1), reminder(1)]),
    ("handoff", "worker-high", [brief(40), tool_use(2, 410000), result(2)]),
    ("stalled", "worker", [brief(60), tool_use(12, 90000)]),
    ("bgwait", "worker", [brief(30), tool_use(10, 80000, "bg1", True), result(10, "bg1"), text(9, 81000, "Waiting for the test run.")]),
    ("bgdone", "worker", [brief(30), tool_use(10, 80000, "bg2", True), result(10, "bg2"), text(9, 81000, "Waiting for the test run."), notice(1, "bg2")]),
    ("bgfinal", "worker", [brief(30), tool_use(10, 80000, "bg3", True), result(10, "bg3"), notice(8, "bg3"), text(7, 82000, "Done: waiting is over.")]),
    ("finished", "worker-high", [brief(30), tool_use(5, 300000), result(5), text(4, 301000, "Done: shipped.")]),
    ("interrupted", "worker", [brief(30), tool_use(5, 300000), result(5), {"type": "user", "timestamp": ts(4), "message": {"role": "user", "content": [{"type": "text", "text": "[Request interrupted by user]"}]}}]),
    ("explore", "Explore", [brief(30), tool_use(1, 500000)]),
])

def render(m, tid):
    return {"type": "assistant", "timestamp": ts(m), "message": {"role": "assistant", "usage": usage(90000),
            "content": [{"type": "tool_use", "id": tid, "name": "Bash", "input": {"command": "wisp-render shot.png"}}]}}
def failed(m, tid):
    return {"type": "user", "timestamp": ts(m), "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": tid, "is_error": True, "content": "exit 1"}]}}
dbg = "Item: smashcraft#5 desync. Category: debugging-unknown-cause."
bug = "Item: smashcraft#9 crash. Category: implementation."
waits = session("-home-tom", "wait-session", [
    ("dbg1", "worker", [brief(90, dbg), tool_use(80, 90000), result(80), text(70, 91000, "Not done: first idea failed.")]),
    ("dbg2", "worker", [brief(60, dbg), tool_use(50, 90000), result(50), text(40, 91000, "Not done: second idea failed.")]),
    ("dbg3", "worker", [brief(30, dbg), tool_use(1, 90000)]),
    ("bug1", "worker", [brief(90, bug), tool_use(80, 90000), result(80), text(70, 91000, "Not done: no repro.")]),
    ("bug2", "worker", [brief(60, bug), tool_use(50, 90000), result(50), text(40, 91000, "Not done: still crashes.")]),
    ("bug3", "worker", [brief(30, bug), tool_use(1, 90000)]),
    ("perf1", "worker", [brief(90, "Item: smashcraft#6 fps. Category: performance."), tool_use(80, 90000), result(80), text(70, 91000, "Not done: a.")]),
    ("perf2", "worker", [brief(60, "Item: smashcraft#6 fps. Category: performance."), tool_use(50, 90000), result(50), text(40, 91000, "Not done: b.")]),
    ("perf3", "worker", [brief(30, "Item: smashcraft#6 fps. Category: performance."), tool_use(1, 90000)]),
    ("rend", "worker", [brief(30), render(20, "r1"), failed(19, "r1"), render(2, "r2"), result(1, "r2"), tool_use(1, 90000, "t9")]),
])
os.utime(waits, (now - 1800, now - 1800))
galileo = session("-home-tom", "galileo-session", [
    ("galileo", "worker", [brief(240, "Item: galileo 4.14"), tool_use(230, 120000, "bajcx65gc", True), result(230, "bajcx65gc"),
                           notice(226, "bajcx65gc"), text(225, 121000, "Waiting on bajcx65gc."), text(220, 121500, "Waiting.")]),
    ("reported", "worker", [brief(240), tool_use(230, 120000), result(230), text(220, 121000, "Blocked: needs a GPU lease.")]),
])
os.utime(galileo, (now - 1800, now - 1800))
PY

pass=0 fail=0
check() {
  if eval "$2"; then pass=$((pass + 1)); printf 'PASS  %s\n' "$1"
  else fail=$((fail + 1)); printf 'FAIL  %s\n%s\n' "$1" "$out"; fi
}
row() { grep -E "^$1 " <<<"$out"; }

out=$("$sweep")
ids=$(grep -vE '^(WAIT|RECOVER|Follows|Evidence)' <<<"$out" | awk '{print $1}' | sort | tr '\n' ' ')
check 'newest session: running workers only' '[ "$ids" = "bgdone bgwait handoff pending result stalled " ]'
check 'STALLED after 12 idle minutes (10-minute limit)' 'row stalled | grep -q "idle=12m.*STALLED"'
check 'fresh worker has no flag' 'row pending | grep -q "worker-high.*ctx=200k.*ran=30m.*idle=1m" && ! row pending | grep -qE "STALLED|HANDOFF"'
check 'waiting on its own background job' 'row bgwait | grep -q waiting-bg'
check 'a pass records finished workers as runs in threads' '"$repo/bin/threads" run-ids | grep -qx finished'
check 'a pass makes a running worker the holder of its Item' '"$repo/bin/threads" list | grep -qE "^smashcraft#7 +pending "'

out=$("$sweep" --session old-session)
check '--session picks that session' '[ "$(grep -vE "^(WAIT|RECOVER|Follows|Evidence)" <<<"$out" | awk "{print \$1}")" = oldtool ]'

out=$(timeout 10 "$sweep" --wait)
status=$?
check '--wait exits at once with the flagged table' '[ "$status" -eq 0 ] && row handoff | grep -q HANDOFF'

queue=$XDG_STATE_HOME/safe-push/k.queue
mkdir -p "$queue"
bash -c 'sleep 60; :' safe-push &
waiter=$!
printf 'pid 1 in /lane/holder\n' >"$XDG_STATE_HOME/safe-push/k.lock"
printf '/lane/late\tlate\tabc\n' >"$queue/$(( ($(date +%s) - 900) * 1000000 ))-$waiter"
printf '/lane/fresh\tfresh\tabc\n' >"$queue/$(( ($(date +%s) - 60) * 1000000 ))-$$"
printf 'chrome --user-data-dir=/tmp/wisp-render-%s\n' 1 1 2 3 >"$scratch/ps"
"$sweep" --session wait-session >/dev/null
out=$("$sweep" --session wait-session)
kill "$waiter" 2>/dev/null
check 'landing: a lane queued 15 minutes is flagged' 'grep -q "^WAIT landing: safe-push $waiter for /lane/late has waited 15 minutes.*train" <<<"$out"'
check 'landing: a fresh lane is not' '! grep -q /lane/fresh <<<"$out"'
check 'actions: more than 4 queued runs' 'grep -q "^WAIT actions: tompassarelli/smashcraft has 5 queued runs, the oldest queued 3 minutes; cut duplicate" <<<"$out"'
check 'actions: a run queued over 10 minutes' 'grep -q "^WAIT actions: tompassarelli/wisp has 1 queued runs, the oldest queued 25 minutes" <<<"$out"'
check 'serial: debugging category with two Not done reports' 'grep -q "^WAIT serial: smashcraft#5 has one active worker (dbg3) after 2 Not done.*parallel hypothesis" <<<"$out"'
check 'serial: open bug issue with two Not done reports' 'grep -q "^WAIT serial: smashcraft#9 has one active worker (bug3)" <<<"$out"'
check 'serial: other categories are not flagged' '! grep -q "smashcraft#6" <<<"$out"'
check 'gpu: busy on two sweeps while a worker fails and rerenders' 'grep -q "^WAIT gpu: GPU busy at 95% then 95% on two sweeps while 1 render worker(s) (rend)" <<<"$out"'
check 'gpu: more than 2 headless renders' 'grep -q "^WAIT gpu: 3 headless wisp-render browsers.*cap renders at 2" <<<"$out"'

out=$("$sweep" --session galileo-session)
check 'galileo 4.14 scenario: a text-only turn with no report after its notified background job is STALLED' 'row galileo | grep -q "idle=220m.*STALLED" && ! row reported'

out=$("$sweep" --session missing 2>&1)
status=$?
check 'unknown session fails' '[ "$status" -eq 1 ]'

cat >"$scratch/codex-scenario.jsonl" <<'JSONL'
{"timestamp":"2026-10-10T19:26:50.158Z","type":"session_meta","payload":{"id":"codex-stalled","timestamp":"2026-10-10T19:26:50.158Z","source":{"subagent":{"thread_spawn":{"parent_thread_id":"codex-lead","agent_path":"/root/idle_worker"}}}},"fixtureOrigin":"recorded Codex rollout shapes; identifiers and text replaced, idle clock and context fault injected"}
{"timestamp":"2026-10-10T20:14:50.158Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":350000}}}}
{"timestamp":"2026-10-10T20:14:50.158Z","type":"response_item","payload":{"type":"function_call","name":"exec","call_id":"wait-call","arguments":"{\"cmd\":\"safe-push --to main\"}"}}
JSONL
python3 - "$scratch/codex-scenario.jsonl" <<'PYCODEX'
import json, os, sqlite3, sys, time
from datetime import datetime, timezone
from pathlib import Path

root = Path(os.environ["CODEX_CONFIG_DIR"])
sessions = root / "sessions/2026/10/10"
sessions.mkdir(parents=True)
now = float(int(time.time()))
origin = datetime.fromisoformat("2026-10-10T20:26:50.158+00:00").timestamp()
def ts(at):
    return datetime.fromtimestamp(round(at), timezone.utc).isoformat().replace("+00:00", "Z")
rows = [json.loads(line) for line in Path(sys.argv[1]).read_text().splitlines()]
for row in rows:
    row["timestamp"] = ts(now + datetime.fromisoformat(row["timestamp"].replace("Z", "+00:00")).timestamp() - origin)
rows[0]["payload"]["timestamp"] = rows[0]["timestamp"]
entries = []
for id, name in [("codex-stalled", "idle_worker"), ("codex-finished", "finished_worker")]:
    transcript = json.loads(json.dumps(rows))
    meta = transcript[0]["payload"]
    meta["id"] = id
    meta["source"]["subagent"]["thread_spawn"]["agent_path"] = f"/root/{name}"
    if id == "codex-finished":
        transcript.append({"timestamp": ts(now - 12 * 60), "type": "response_item", "payload": {"type": "message", "role": "assistant", "phase": "final", "content": [{"type": "output_text", "text": "Done: sanitized report."}]}})
        transcript.append({"timestamp": ts(now - 12 * 60), "type": "event_msg", "payload": {"type": "task_complete", "last_agent_message": "Done: sanitized report."}})
    path = sessions / f"rollout-{id}.jsonl"
    path.write_text("".join(json.dumps(row) + "\n" for row in transcript))
    entries.append((id, str(path), json.dumps(meta["source"]), "gpt-6.1-sol", "high", int(now)))
parent = sessions / "rollout-codex-lead.jsonl"
parent_rows = [{"timestamp": ts(now - 60 * 60), "type": "session_meta", "payload": {"id": "codex-lead", "timestamp": ts(now - 60 * 60), "source": "cli"}}]
for name in ["idle_worker", "finished_worker"]:
    parent_rows += [
        {"timestamp": ts(now - 60 * 60), "type": "response_item", "payload": {"type": "function_call", "name": "spawn_agent", "call_id": name, "arguments": json.dumps({"task_name": name, "message": "ETA: 40 minutes"})}},
        {"timestamp": ts(now - 60 * 60), "type": "response_item", "payload": {"type": "function_call_output", "call_id": name, "output": json.dumps({"task_name": f"/root/{name}"})}},
    ]
parent.write_text("".join(json.dumps(row) + "\n" for row in parent_rows))
entries.append(("codex-lead", str(parent), "cli", "gpt-6.1-sol", "high", int(now)))
(root / "sqlite").mkdir()
db = sqlite3.connect(root / "sqlite/state_5.sqlite")
db.execute("CREATE TABLE threads (id TEXT, rollout_path TEXT, source TEXT, model TEXT, reasoning_effort TEXT, updated_at INTEGER)")
db.executemany("INSERT INTO threads VALUES (?, ?, ?, ?, ?, ?)", entries)
db.commit()
db.close()
Path(os.environ["AGENTS_ORG_FILE"]).write_text(json.dumps({"nodes": [
    {"id": "claude-lead", "session": "old-session", "provider": "claude"},
    {"id": "codex-lead", "parent": "claude-lead", "session": "codex-lead", "provider": "codex", "role": "sub-lead", "state": "active"},
]}))
PYCODEX
out=$("$sweep" --session codex-lead)
check 'recorded Codex scenario: idle worker trips existing flags and keeps staffing ETA' 'row idle_worker | grep -q "gpt-6.1-sol high.*ctx=350k.*ran=60m.*idle=12m.*eta=40m.*STALLED,OVERTIME,HANDOFF,PARKED"'
check 'recorded Codex scenario: final report finishes without flags' 'row finished_worker | grep -q "ran=48m.*idle=12m.*eta=40m.*finished" && ! row finished_worker | grep -qE "STALLED|OVERTIME|HANDOFF|PARKED"'
check 'recorded Codex scenario: parked wait explanation' 'grep -q "^PARKED idle_worker: pending wait for 12m" <<<"$out"'
out=$("$sweep" --session old-session)
check 'Claude session preserves its worker and includes registered Codex sub-lead workers' 'row oldtool | grep -q "worker.*ctx=410k.*ran=90m.*idle=5m.*OVERTIME,HANDOFF" && row idle_worker | grep -q STALLED && row finished_worker | grep -q finished'
mv "$CODEX_CONFIG_DIR/sqlite/state_5.sqlite" "$scratch/codex-state.sqlite"
out=$("$sweep" --session codex-lead)
check 'Codex rollout-directory fallback uses the same recorded scenario' 'row idle_worker | grep -q "eta=40m.*STALLED" && row finished_worker | grep -q finished'

cat >"$scratch/bin/agents" <<'SH'
#!/usr/bin/env bash
[ "$*" = "plan --json" ] || exit 97
printf '{"escalation":["claude:haiku","codex:sol-medium","claude:opus-medium"],"capacity":{"codex":{"fallback":%s}},"tiers":{"claude:haiku":"claude-haiku-5-5 high","codex:sol-medium":"gpt-6.1-sol medium","codex:sol-high":"gpt-6.1-sol high","claude:opus-medium":"claude-opus-5-5 medium"}}\n' "$FAKE_FALLBACK"
SH
cat >"$scratch/bin/codex-lead" <<'SH'
#!/usr/bin/env bash
printf '%s %s %s %s\n' "$CODEX_LEAD_THREAD" "${CODEX_LEAD_MODEL:-}" "${CODEX_LEAD_EFFORT:-}" "$*" >>"$CODEX_LEAD_LOG"
SH
chmod +x "$scratch/bin/agents" "$scratch/bin/codex-lead"
export CODEX_LEAD_LOG=$scratch/codex-lead.log
# The capacity line is the task_complete recorded 2026-10-12T06:43:25Z in a pooled Codex rollout; ids and times replaced.
capacity_line='{"timestamp":"2026-10-12T06:43:25.408Z","type":"event_msg","payload":{"type":"task_complete","turn_id":"01a12eda-03a2-7590-86bc-a83582c7b81a","last_agent_message":null,"error":{"message":"Selected model is at capacity. Please try a different model.","codex_error_info":"server_overloaded"},"started_at":1791787402,"completed_at":1791787405,"duration_ms":3258}}'
turn() {
  python3 - "$CODEX_CONFIG_DIR/sessions/2026/10/10/rollout-$1.jsonl" "$1" "$2" "$3" "$4" "$5" "$capacity_line" <<'PYTURN'
import json, sys, time
from datetime import datetime, timezone
from pathlib import Path
path, tid, model, effort, outcome, ago, real = Path(sys.argv[1]), *sys.argv[2:]
at = lambda m: datetime.fromtimestamp(time.time() - float(m) * 60, timezone.utc).isoformat().replace("+00:00", "Z")
rows = [] if path.exists() else [{"timestamp": at(30), "type": "session_meta", "payload": {"id": tid, "timestamp": at(30),
        "source": {"subagent": {"thread_spawn": {"parent_thread_id": "codex-lead", "agent_path": f"/root/{tid}"}}}}}]
rows += [{"timestamp": at(ago), "type": "event_msg", "payload": {"type": "task_started"}},
         {"timestamp": at(ago), "type": "turn_context", "payload": {"model": model, "effort": effort}}]
done = json.loads(real) if outcome == "capacity" else {"type": "event_msg", "payload": {"type": "task_complete", "last_agent_message": "ok"}}
rows.append(dict(done, timestamp=at(ago)))
with path.open("a") as f:
    f.writelines(json.dumps(r) + "\n" for r in rows)
PYTURN
}
capstate=$XDG_STATE_HOME/agents/worker-sweep/capacity-codex-lead.json
age_capacity() { jq '.[] .at -= 600' "$capstate" >"$capstate.new" && mv "$capstate.new" "$capstate"; }

export FAKE_FALLBACK='"codex:astra"'
turn cap_sol gpt-6.1-sol high capacity 1.5
c1=$("$sweep" --session codex-lead)
turn cap_sol gpt-6-astra medium ok 0.5
c2=$("$sweep" --session codex-lead)
turn cap_sol gpt-6.1-sol high ok 0.2
c3=$("$sweep" --session codex-lead)
out="$c1"$'\n---\n'"$c2"$'\n---\n'"$c3"$'\n---\n'$(cat "$CODEX_LEAD_LOG")
check 'capacity scenario: Sol at capacity is CAPACITY, resent on gpt-6-astra medium, Sol probed next turn, back on Sol once it answers' '
  grep -qE "^cap_sol +gpt-6.1-sol high .*CAPACITY *$" <<<"$c1" && ! grep -qE "^cap_sol .*STALLED" <<<"$c1" &&
  grep -q "^CAPACITY cap_sol: retry 1/3 on gpt-6-astra medium: resent continue (capacity fallback)$" <<<"$c1" &&
  grep -q "^CAPACITY cap_sol: answered on gpt-6-astra; probing gpt-6.1-sol high on its next turn$" <<<"$c2" &&
  grep -q "^CAPACITY cap_sol: gpt-6.1-sol answered; back on gpt-6.1-sol high$" <<<"$c3" &&
  [ "$(cat "$CODEX_LEAD_LOG")" = "cap_sol gpt-6-astra medium send continue
cap_sol gpt-6.1-sol high model gpt-6.1-sol high" ]'

: >"$CODEX_LEAD_LOG"
export FAKE_FALLBACK=null
turn cap_floor gpt-6.1-sol high capacity 1.5
b1=$("$sweep" --session codex-lead); age_capacity
turn cap_floor gpt-6.1-sol high capacity 1
b2=$("$sweep" --session codex-lead)
turn cap_floor gpt-6.1-sol high capacity 3
b3=$("$sweep" --session codex-lead); age_capacity
turn cap_floor gpt-6.1-sol high capacity 5
b4=$("$sweep" --session codex-lead); age_capacity
turn cap_floor gpt-6.1-sol high capacity 1
b5=$("$sweep" --session codex-lead)
b6=$("$sweep" --session codex-lead)
summary=$("$repo/bin/worker-ledger" --summary 2>/dev/null | grep '^capacity_recovery:')
out="$b1"$'\n---\n'"$b2"$'\n---\n'"$b3"$'\n---\n'"$b4"$'\n---\n'"$b5"$'\n---\n'"$summary"
check 'capacity scenario: no Astra past the floor, Sol retried after 1, 2 and 4 minutes, then fall back to the next tier with Follows:' '
  grep -q "^CAPACITY cap_floor: retry 1/3 on gpt-6.1-sol high: resent continue$" <<<"$b1" &&
  grep -qE "^CAPACITY cap_floor: gpt-6.1-sol at capacity; retry 2/3 at [0-9]{2}:[0-9]{2}$" <<<"$b2" &&
  grep -q "^CAPACITY cap_floor: retry 2/3 on gpt-6.1-sol high" <<<"$b3" && grep -q "^CAPACITY cap_floor: retry 3/3 on gpt-6.1-sol high" <<<"$b4" &&
  grep -A1 "^CAPACITY cap_floor: fall back: restaff on claude:opus-medium (claude-opus-5-5 medium)" <<<"$b5" | grep -q "^Follows: cap_floor gpt-6.1-sol high$" &&
  grep -q "^CAPACITY cap_floor: fall back" <<<"$b6" && [ "$(grep -c "astra" "$CODEX_LEAD_LOG")" -eq 0 ] &&
  [ "$summary" = "capacity_recovery: fallback gpt-6-astra medium=1, restaff claude-opus-5-5 medium=1, retry gpt-6.1-sol high=3, switch_back gpt-6.1-sol high=1" ]'

for ago in 1.5 3 5; do turn cap_unknown gpt-6.1-sol "" capacity "$ago"; "$sweep" --session codex-lead >/dev/null; age_capacity; done
turn cap_unknown gpt-6.1-sol "" capacity 1
u4=$("$sweep" --session codex-lead)
out="$u4"
check 'capacity scenario: unknown effort is assumed the highest Sol tier, retried there and falls back above Sol, never to Haiku' '
  grep -q "^CAPACITY cap_unknown: effort unknown; assumed high$" <<<"$u4" &&
  grep -A1 "^CAPACITY cap_unknown: fall back: restaff on claude:opus-medium (claude-opus-5-5 medium)" <<<"$u4" | grep -q "^Follows: cap_unknown gpt-6.1-sol high$" &&
  grep -q "^cap_unknown gpt-6.1-sol high send continue$" "$CODEX_LEAD_LOG" && ! grep -q "^cap_unknown .* medium " "$CODEX_LEAD_LOG"'

for ago in 1.5 3 5; do turn cap_astra gpt-6-astra xhigh capacity "$ago"; "$sweep" --session codex-lead >/dev/null; age_capacity; done
turn cap_astra gpt-6-astra xhigh capacity 1
out=$("$sweep" --session codex-lead)
check 'capacity scenario: an on-request Astra xhigh lead restaffs above every Codex tier, never on Sol medium' '
  grep -A1 "^CAPACITY cap_astra: fall back: restaff on claude:opus-medium (claude-opus-5-5 medium)" <<<"$out" | grep -q "^Follows: cap_astra gpt-6-astra xhigh$"'

report=$XDG_STATE_HOME/agents/lane-gc/stale.json
cat >"$report" <<JSON
{"at":"$(date -u +%FT%TZ)","repos":{
  "north":{"remote_landed":["landed-a","landed-b"],"remote_stale":[{"branch":"idle-a","age_h":48,"owner":"unknown"}],"local_stale":[
    {"path":"/fixture/clean","branch":"clean","age_h":25,"dirty":false,"owner":"unknown","flagged_at":null,"retire_at":null},
    {"path":"/fixture/dirty","branch":"dirty","age_h":72,"dirty":true,"owner":"unknown","flagged_at":null,"retire_at":null},
    {"path":"/fixture/boundary","branch":"boundary","age_h":24,"dirty":true,"owner":"unknown","flagged_at":null,"retire_at":null}],
    "owned":[{"where":"~/code/north/worktrees/own","branch":"own","age_h":30,"refs":["north#6"],"owner":"north-lead"},
      {"where":"(branch)","branch":"loose","age_h":80,"refs":[],"owner":"north-lead"},
      {"where":"(branch)","branch":"theirs","age_h":90,"refs":[],"owner":"other-lead"}]},
  "south":{"remote_landed":[],"remote_stale":[],"local_stale":[{"path":"/fixture/south","branch":"idle","age_h":30,"dirty":false,"owner":"unknown","flagged_at":null,"retire_at":null}]},
  "empty":{"remote_landed":[],"remote_stale":[],"local_stale":[]}
}}
JSON
expected_stale=$'north: 3 3 2\nsouth: 0 1 0'
out=$("$sweep" --session codex-lead)
stale_counts=$(awk '/^STALE [^ ]+:/ { gsub(/[()]/, "", $13); print $2, $3, $6, $13 }' <<<"$out")
check 'shared stale report scenario: exact multi-repo counts include every flagged local lane' '[ "$(grep -c "^STALE " <<<"$out")" -eq 2 ] && [ "$stale_counts" = "$expected_stale" ]'
printf '{"nodes":[{"id":"north-lead","session":"codex-lead","role":"lead"}]}\n' >"$scratch/lead-org.json"
owned=$(AGENTS_ORG_FILE=$scratch/lead-org.json "$sweep" --session codex-lead | grep -E "^(OWNED|ALERT) ")
check 'owned lanes scenario: a lead tick lists only its own unlanded lanes, oldest first with issue, and alerts past 72 h' '[ "$owned" = "OWNED north loose 80h no issue: continue, land or archive
OWNED north own 30h north#6: continue, land or archive
ALERT north-lead: 2 owned unlanded lanes, oldest 80h; decide each before new work" ] && ! grep -q "^OWNED" <<<"$out"'
rm "$report"
out=$("$sweep" --session codex-lead)
check 'shared stale report scenario: missing report warns once' '[ "$(grep -c "^STALE " <<<"$out")" -eq 1 ] && ! grep -qE "^STALE [^ ]+:" <<<"$out"'
printf '{"at":"%s","repos":{}}\n' "$(date -u -d '-27 hours' +%FT%TZ)" >"$report"
out=$("$sweep" --session codex-lead)
check 'shared stale report scenario: report older than 26 hours warns once' '[ "$(grep -c "^STALE " <<<"$out")" -eq 1 ] && ! grep -qE "^STALE [^ ]+:" <<<"$out"'
printf '{"at":"%s","repos":{}}\n' "$(date -u +%FT%TZ)" >"$report"
out=$("$sweep" --session codex-lead)
check 'shared stale report scenario: fresh empty report is silent' '! grep -q ^STALE <<<"$out"'

export XDG_STATE_HOME=$scratch/stall-state
stall_dir=$CLAUDE_CONFIG_DIR/projects/-home-tom/stall-session/subagents
agent() {
  python3 - "$stall_dir" "$@" <<'PYAGENT'
import json, os, sys, time
from datetime import datetime, timezone
d, aid, mode, follows = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4] if len(sys.argv) > 4 else ""
now = time.time()
def ts(m): return datetime.fromtimestamp(now - m * 60, timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.000Z")
use = {"input_tokens": 10, "cache_read_input_tokens": 90000, "cache_creation_input_tokens": 0}
lines = [{"type": "user", "timestamp": ts(30), "message": {"role": "user", "content": f"Follows: {follows} worker\nbrief" if follows else "brief"}},
         {"type": "assistant", "timestamp": ts(12 if mode == "stalled" else 1), "message": {"role": "assistant", "usage": use,
          "content": [{"type": "tool_use", "id": "t1", "name": "Bash", "input": {"command": "x"}}]}}]
if mode == "done":
    lines += [{"type": "user", "timestamp": ts(1), "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "t1", "content": "ok"}]}},
              {"type": "assistant", "timestamp": ts(1), "message": {"role": "assistant", "usage": use, "content": [{"type": "text", "text": "Done: x."}]}}]
os.makedirs(d, exist_ok=True)
open(os.path.join(d, f"agent-{aid}.jsonl"), "w").writelines(json.dumps(l) + "\n" for l in lines)
json.dump({"agentType": "worker"}, open(os.path.join(d, f"agent-{aid}.meta.json"), "w"))
PYAGENT
}
recovery=$XDG_STATE_HOME/agents/worker-sweep/recovery-stall-session.json
age() { jq '.[] .at -= 660' "$recovery" >"$recovery.new" && mv "$recovery.new" "$recovery"; }
inbox=$LEDGER_HANDOFFS/stall-lead-inbox.md
jq '.nodes += [{"id": "stall-lead", "session": "stall-session", "provider": "claude", "role": "lead"}]' "$AGENTS_ORG_FILE" >"$scratch/org.new" && mv "$scratch/org.new" "$AGENTS_ORG_FILE"
for a in nudged handed escal reported; do agent "$a" stalled; done
mkdir -p "${recovery%/*}"
printf '{"ghost":{"stage":"nudged","worker":"ghost","at":%s}}\n' "$(( $(date +%s) - 3600 ))" >"$recovery"
s1=$("$sweep" --session stall-session)
agent nudged active; agent reported done; age
s2=$("$sweep" --session stall-session)
agent handed2 active handed; agent escal2 stalled escal; age
s3=$("$sweep" --session stall-session)
agent escal3 active escal2; age
s4=$("$sweep" --session stall-session)
out="$s1"$'\n---\n'"$s2"$'\n---\n'"$s3"$'\n---\n'"$s4"$'\n---\n'$(cat "$inbox" 2>/dev/null)
summary=$("$repo/bin/worker-ledger" --summary 2>/dev/null | grep '^stall_recovery:')
check 'stall recovery scenario: nudge once, Follows: handoff, one tier up, and a reported or stale entry clears' '
  [ "$(grep -c "^RECOVER nudge" <<<"$s1")" -eq 4 ] && grep -q "^RECOVER closed ghost at nudge" <<<"$s1" &&
  grep -q "^RECOVER closed nudged at nudge" <<<"$s2" && grep -q "^RECOVER closed reported at nudge" <<<"$s2" &&
  ! grep -qE "^RECOVER (nudge|handoff|escalate) (nudged|reported)" <<<"$s2" &&
  grep -q "^RECOVER handoff handed: spawn a fresh worker " <<<"$s2" && grep -q "^RECOVER handoff escal:" <<<"$s2" &&
  grep -q "^RECOVER closed handed at handoff" <<<"$s3" && ! grep -q "^RECOVER .* handed2" <<<"$s3" &&
  grep -q "^RECOVER escalate escal2: spawn a fresh worker-high whose brief starts:" <<<"$s3" &&
  grep -A1 "escalate escal2 -> fresh worker-high" "$inbox" | grep -q "spawn a fresh worker-high" &&
  grep -q "^Follows: escal2 worker$" "$inbox" && grep -q "^Follows: handed worker$" "$inbox" &&
  grep -q "^RECOVER closed escal at escalation" <<<"$s4" && [ "$(jq length "$recovery")" -eq 0 ] &&
  [ "$summary" = "stall_recovery: closed=5 nudge=3 handoff=1 escalation=1 lapsed=0 open=0 without_escalation=80% (goal 70% over 7 days)" ]'
out=$summary

printf 'worker-sweep: %d passed, %d failed\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
