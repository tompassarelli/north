#!/usr/bin/env bash
# Spawn gate fixtures over a fake capacity probe: provisioned CPUs under the
# limit allow a local worker, at or over it (or protected pressure above 20)
# deny with the queue/farm/cloud message, high system PSI with few provisioned
# CPUs allows, an unreadable probe allows, a remote worker allows, and an URGENT
# brief passes and is logged. The Codex spawn_agent path through the behavior
# decider denies the same way. Delegation budget 0 (session env, or a subagent
# brief without a Delegation line) denies with the do-it-yourself message and
# budget 1 allows; a domain below the first in the project priority stops at 80%.
# A fresh usage-gate.json refusal (firn#12 money gate) denies with its own text.
# Pace hold: an account at or over its pace line holds a spawn whose project has
# no open target once now passes its [pace] enforce_from, warns before it, and a stale
# readout only warns; Tom's depth-0 proxy is only warned, and a whiterabbit
# Claude spawn reads claude:a, the account it bills.
set -uo pipefail
export PATH="/etc/codex/hooks/runtime:$PATH"

HERE="$(cd "$(dirname "$0")" && pwd)"
HOOK="$HERE/spawn-capacity-guard.sh"
SCRATCH="$(mktemp -d "${TMPDIR:-/tmp}/spawn-capacity-test.XXXXXX")"
trap 'rm -rf "${SCRATCH:?}"' EXIT
export AGENT_HOOK_ERRORS="$SCRATCH/errors.tsv"
ACTIVATION="$SCRATCH/activation.active"
STATUS="$SCRATCH/probe.json"
printf 'hook spawn-capacity-guard\nhook codex-behavior-guard\n' >"$ACTIVATION"
mkdir -p "$SCRATCH/home/.claude/projects/p/s/subagents"
: >"$SCRATCH/home/.claude/projects/p/s/subagents/agent-a.jsonl"
: >"$SCRATCH/home/.claude/projects/p/s/subagents/agent-b.jsonl"
GATE="$SCRATCH/home/.local/state/agents/usage-gate.json"
mkdir -p "${GATE%/*}"
# gate CLAUDE_REFUSAL: write a fresh money-gate verdict (codex always allowed).
gate() { printf '{"claude":{"epoch":%s,"stale_min":30,"refusal":%s},"codex":{"epoch":%s,"stale_min":30,"refusal":null}}\n' "$(date +%s)" "$1" "$(date +%s)" >"$GATE"; }
gate null

# status LEASED PROTECTED [SYSTEM_PSI]: write the probe fixture (limit 20 CPUs).
status() { printf '{"decision":"RUN","committedBatchCpus":%s,"leasedNativeCpus":12,"aggregateCpuLimit":20,"protectedCpuSomeAvg10":%s,"cpuSomeAvg10":%s}\n' "$1" "$2" "${3:-1}" >"$STATUS"; }

# call HOOK JSON: print the deny reason ("" when allowed).
call() {
  printf '%s' "$2" | env -u AGENT_NO_AUTHORING_HOOKS -u AGENT_ROLE -u AGENT_DELEGATION_BUDGET -u AGENT_ORG_NAME \
    AGENTS_ORG_FILE="$SCRATCH/org.json" AGENTS_ORCHESTRATION="$SCRATCH/orchestration.toml" "${CALL_ENV[@]}" HOME="$SCRATCH/home" SPAWN_CAPACITY_HOME="$SCRATCH/home" \
    SPAWN_CAPACITY_STATUS="$STATUS" XDG_STATE_HOME="$SCRATCH/home/.local/state" CODEX_BEHAVIOR_STATE="$SCRATCH/state" \
    CODEX_BEHAVIOR_CODE_ROOT="$SCRATCH/code" \
    NORTH_AGENT_ACTIVE="$ACTIVATION" NORTH_AGENT_PYTHON=/etc/codex/hooks/runtime/python3 \
    "$1" | python3 -c '
import json, sys
raw = sys.stdin.read().strip()
print(json.loads(raw)["hookSpecificOutput"].get("permissionDecision", "") if raw else "")'
}
agent() { printf '{"session_id":"s","cwd":"/tmp","hook_event_name":"PreToolUse","tool_name":"Agent","tool_input":%s}' "$1"; }

pass=0 fail=0
CALL_ENV=()
check() {
  if [ "$1" = ok ]; then pass=$((pass + 1)); printf 'PASS  %s\n' "$2"
  else fail=$((fail + 1)); printf 'FAIL  %s\n      out=%s\n' "$2" "$3"; fi
}
expect_allow() { local out; out="$(call "$HOOK" "$1")"; [ -z "$out" ] && check ok "$2" || check bad "$2" "$out"; }
expect_deny() {
  local out; out="$(call "${3:-$HOOK}" "$1")"
  [ "$out" = deny ] && check ok "$2" || check bad "$2" "$out"
}

worker='{"subagent_type":"worker","description":"d","prompt":"do it"}'
status 19 5
expect_allow "$(agent "$worker")" '19 of 20 provisioned CPUs allows a local worker'
status 2 5 95
expect_allow "$(agent "$worker")" 'system PSI 95 with 2 provisioned CPUs allows a local worker'
status 20 5
DENY_LEASED=20 expect_deny "$(agent "$worker")" '20 of 20 provisioned CPUs (at the limit) denies with the message'
status 4 20.5
DENY_LEASED=4 DENY_PROTECTED=20.5 expect_deny "$(agent "$worker")" 'protected pressure 20.5 denies with few provisioned CPUs'
status 4 20
expect_allow "$(agent "$worker")" 'protected pressure exactly 20 allows'
printf '{"decision":"RUN","leasedBatchCpus":10,"committedBatchCpus":21,"aggregateCpuLimit":20,"protectedCpuSomeAvg10":5}\n' >"$STATUS"
DENY_LEASED=21 expect_deny "$(agent "$worker")" 'measured lease use plus unleased heavy load past the limit denies'
printf '{"decision":"RUN","leasedBatchCpus":24,"committedBatchCpus":19,"aggregateCpuLimit":20,"protectedCpuSomeAvg10":5}\n' >"$STATUS"
expect_allow "$(agent "$worker")" 'committed CPUs, when reported, replace reserved ones'
printf '{"decision":"RUN","profile":"unattended","committedBatchCpus":4,"aggregateCpuLimit":20,"protectedCpuSomeAvg10":40}\n' >"$STATUS"
expect_allow "$(agent "$worker")" 'unattended profile (Tom away) skips the protected-pressure refusal, as the helper does'
printf '{"decision":"RUN","profile":"unattended","committedBatchCpus":20,"aggregateCpuLimit":20,"protectedCpuSomeAvg10":40}\n' >"$STATUS"
DENY_LEASED=20 DENY_PROTECTED=0 expect_deny "$(agent "$worker")" 'unattended profile still denies at the provisioned limit'
status 24 5
expect_deny "$(agent "$worker")" '24 of 20 provisioned CPUs (over the limit) denies with the message'
expect_deny "$(agent '{"description":"d","prompt":"p"}')" 'omitted subagent_type (general-purpose) is local and denied'
expect_allow "$(agent '{"subagent_type":"worker","isolation":"remote","prompt":"p"}')" 'remote worker allows over the limit'
expect_allow "$(agent '{"subagent_type":"claude-code-guide","prompt":"p"}')" 'non-worker agent type allows'
expect_allow "$(agent '{"subagent_type":"worker","prompt":"CASE=URGENT FACT=\"prod login is down for every user\" fix it"}')" 'URGENT brief passes over the limit'
grep -q '"gate": "spawn-capacity"' "$SCRATCH/state/verify-overrides.jsonl" 2>/dev/null \
  && check ok 'URGENT override is logged' || check bad 'URGENT override is logged' "no log line"
expect_allow "$(agent '{"subagent_type":"worker","prompt":"Delegation: role=worker budget=0\nLanding-path: yes\nPick and run checks for each queued landing."}')" 'a Landing-path brief is admitted at the CPU cap'
expect_allow "$(agent '{"subagent_type":"worker","name":"lander","prompt":"pick checks per landing"}')" 'the lander by name is admitted at the CPU cap'
expect_deny "$(agent '{"subagent_type":"worker","prompt":"Landing-path: no\nadd a feature"}')" 'an ordinary brief is still denied at the CPU cap'
expect_allow "$(agent '{"subagent_type":"worker-haiku","prompt":"[routine:watchdog] Run `agents routines show watchdog` and follow it."}')" 'the capacity watchdog brief is exempt over the limit'
expect_deny "$(agent '{"subagent_type":"worker","prompt":"CASE=URGENT FACT=\"short\" go"}')" 'URGENT with a too-short fact is denied'
rm -f "$STATUS"
expect_allow "$(agent "$worker")" 'missing probe output allows'
printf 'machine-capacity: shared admission lock remained busy\n' >"$STATUS"
expect_allow "$(agent "$worker")" 'unreadable probe output allows'
printf '{"decision":"RUN"}\n' >"$STATUS"
expect_allow "$(agent "$worker")" 'probe output without lease fields allows'
status 24 5
expect_deny '{"session_id":"s","cwd":"/tmp","hook_event_name":"PreToolUse","tool_name":"spawn_agent","tool_input":{"message":"fix the bug and land it"}}' \
  'Codex spawn_agent over the limit denies through the behavior guard' "$HERE/codex-behavior-guard.sh"
out="$(call "$HERE/codex-behavior-guard.sh" '{"session_id":"s","cwd":"/tmp","hook_event_name":"PreToolUse","tool_name":"spawn_agent","tool_input":"{\\"items\\":[{\\"type\\":\\"text\\",\\"text\\":\\"CASE=URGENT FACT=\\\\\\"main red blocks every landing (#242)\\\\\\" fix it\\"}]}"}')"
[ -z "$out" ] && check ok 'Codex URGENT inside serialized JSON items allows' || check bad 'Codex URGENT inside serialized JSON items allows' "$out"
out="$(call "$HERE/codex-behavior-guard.sh" '{"session_id":"s","cwd":"/tmp","hook_event_name":"PreToolUse","tool_name":"spawn_agent","tool_input":{"message":"CASE=URGENT FACT=\"main red blocks every landing (#242)\" fix it"}}')"
[ -z "$out" ] && check ok 'Codex URGENT in message allows' || check bad 'Codex URGENT in message allows' "$out"
enc='{"session_id":"s","cwd":"/tmp","hook_event_name":"PreToolUse","tool_name":"collaborationspawn_agent","tool_input":{"message":"gAAAAABqx6PNencrypted","name":"hellfire_base_293_high","model":"gpt-6.1-sol","reasoning_effort":"high"}}'
out="$(call "$HERE/codex-behavior-guard.sh" "$enc")"
[ -n "$out" ] && check ok 'encrypted Codex spawn without a spawn-urgent line is denied' || check bad 'encrypted Codex spawn without a spawn-urgent line is denied' "allowed"
mkdir -p "$SCRATCH/home/.local/state/agents"
printf '%s\thellfire_base_293_high\tmain red blocks every landing (#242)\n' "$(date +%s)" >"$SCRATCH/home/.local/state/agents/spawn-urgent.tsv"
out="$(call "$HERE/codex-behavior-guard.sh" "$enc")"
[ -z "$out" ] && check ok 'encrypted Codex spawn named in a fresh spawn-urgent line allows' || check bad 'encrypted Codex spawn named in a fresh spawn-urgent line allows' "$out"
printf '%s\thellfire_base_293_high\tmain red blocks every landing (#242)\n' "$(( $(date +%s) - 1000 ))" >"$SCRATCH/home/.local/state/agents/spawn-urgent.tsv"
out="$(call "$HERE/codex-behavior-guard.sh" "$enc")"
[ -n "$out" ] && check ok 'a spawn-urgent line older than 15 minutes is denied' || check bad 'a spawn-urgent line older than 15 minutes is denied' "allowed"
status 10 5
out="$(call "$HERE/codex-behavior-guard.sh" '{"session_id":"s","cwd":"/tmp","hook_event_name":"PreToolUse","tool_name":"spawn_agent","tool_input":{"message":"fix the bug and land it"}}')"
[ -z "$out" ] && check ok 'Codex spawn_agent under the limit allows' || check bad 'Codex spawn_agent under the limit allows' "$out"

status 10 5
dodeny() { local out; out="$(call "$HOOK" "$1")"; [ "$out" = deny ] && check ok "$2" || check bad "$2" "$out"; }
CALL_ENV=(AGENT_DELEGATION_BUDGET=0); dodeny "$(agent "$worker")" 'session budget 0 denies with the do-it-yourself message'
CALL_ENV=(AGENT_DELEGATION_BUDGET=0); dodeny "$(agent '{"subagent_type":"claude-code-guide","isolation":"remote","prompt":"p"}')" 'budget 0 denies remote and non-worker spawns too'
CALL_ENV=(AGENT_DELEGATION_BUDGET=1); expect_allow "$(agent "$worker")" 'session budget 1 allows'
CALL_ENV=(AGENT_ROLE=worker); dodeny "$(agent "$worker")" 'role worker without a budget defaults to 0'
CALL_ENV=()
sub="$SCRATCH/home/.claude/projects/p/s/subagents"
printf '{"type":"user","message":{"role":"user","content":"Item: x. Do it."}}\n' >"$sub/agent-w0.jsonl"
printf '{"type":"user","message":{"role":"user","content":"Delegation: role=sub-lead depth=2 budget=1\\nItem: x."}}\n' >"$sub/agent-w1.jsonl"
subagent() { printf '{"session_id":"s","transcript_path":"%s","agent_id":"%s","agent_type":"worker","cwd":"/tmp","hook_event_name":"PreToolUse","tool_name":"Agent","tool_input":%s}' "$SCRATCH/home/.claude/projects/p/s.jsonl" "$1" "$worker"; }
dodeny "$(subagent w0)" 'a subagent whose brief has no Delegation line is a worker and is denied'
expect_allow "$(subagent w1)" 'a subagent briefed with budget=1 may spawn'
printf '{"version":1,"nodes":[{"id":"sc","domain":"Smashcraft"},{"id":"mu","domain":"galileo"}]}\n' >"$SCRATCH/org.json"
cat >"$SCRATCH/orchestration.toml" <<'TOML'
[projects.galileo]
priority = 1
[projects.smashcraft]
priority = 2
TOML
status 17 5
out="$(CALL_ENV=(AGENT_ORG_NAME=sc); call "$HOOK" "$(agent "$worker")")"
[ "$out" = deny ] && check ok 'a lower-priority domain stops at 80% of the limit' || check bad 'a lower-priority domain stops at 80% of the limit' "$out"
CALL_ENV=(AGENT_ORG_NAME=mu); expect_allow "$(agent "$worker")" 'the first-priority domain keeps the headroom'
CALL_ENV=()

status 19 5
input="$(agent "$worker")"
status 2 5
gate '"No: a new claude worker could spend money: test."'
out="$(call "$HOOK" "$(agent "$worker")")"
[ "$out" = deny ] && check ok 'fresh money-gate refusal denies' || check bad 'fresh money-gate refusal denies' "$out"
gate null

# pace PACE_JSON: write a fresh claude verdict carrying routing's pace reading.
pace() { printf '{"claude":{"epoch":%s,"stale_min":30,"refusal":null,"pace":%s}}\n' "$(date +%s)" "$1" >"$GATE"; }
# said JSON: the deny reason, else the warning shown while the spawn proceeds.
said() {
  printf '%s' "$1" | env -u AGENT_NO_AUTHORING_HOOKS -u AGENT_ROLE -u AGENT_DELEGATION_BUDGET -u AGENT_ORG_NAME \
    AGENTS_ORG_FILE="$SCRATCH/org.json" AGENTS_ORCHESTRATION="$SCRATCH/orchestration.toml" "${CALL_ENV[@]}" HOME="$SCRATCH/home" SPAWN_CAPACITY_HOME="$SCRATCH/home" \
    SPAWN_CAPACITY_STATUS="$STATUS" XDG_STATE_HOME="$SCRATCH/home/.local/state" CODEX_BEHAVIOR_STATE="$SCRATCH/state" \
    NORTH_AGENT_ACTIVE="$ACTIVATION" NORTH_AGENT_PYTHON=/etc/codex/hooks/runtime/python3 "$HOOK" | python3 -c '
import json, sys
raw = sys.stdin.read().strip()
out = json.loads(raw) if raw else {}
print(out.get("hookSpecificOutput", {}).get("permissionDecision", "allow") + "|" +
      (out.get("hookSpecificOutput", {}).get("permissionDecisionReason") or out.get("systemMessage") or ""))'
}
expect_said() { local out; out="$(said "$1")"; [ "$out" = "$2" ] && check ok "$3" || check bad "$3" "$out"; }
status 2 5
cat >"$SCRATCH/orchestration.toml" <<'TOML'
[projects.galileo]
priority = 1
target = 2026-11-01
[projects.smashcraft]
priority = 2
[projects.north]
priority = 3
target = 2026-01-01
[pace]
enforce_from = { "claude:b" = "2000-01-01T00:00:00Z", "claude:a" = "2999-01-01T00:00:00Z" }
TOML
over='{"account":"claude:b","state":"at or over pace","used_pct":97.0,"pace_pct":56.8,"stale":null}'
held='Hold: claude:b is at or over its pace line (used 97.0% vs pace 56.8%); firn has no approved deadline. Queue it, or ask Tom through the proxy for a dated goal.'
pace "$over"
expect_said "$(agent '{"subagent_type":"Explore","prompt":"Find where X is defined"}')" "allow|${held/firn/this project}" \
  'enforced pace hold: a proxy spawn with no Item is only warned'
CALL_ENV=(AGENT_ROLE=lead)
expect_said "$(agent '{"subagent_type":"worker","prompt":"Item: firn#19\nFix it."}')" "deny|$held" 'enforced pace hold: at or over pace, no target denies with the exact text'
expect_said "$(agent '{"subagent_type":"worker","prompt":"Item: north#35"}')" "deny|${held/firn/north}" 'enforced pace hold: a target in the past no longer exempts'
printf '{"claude":{"epoch":%s,"stale_min":30,"refusal":null,"pace":%s,"paces":{"claude:a":%s,"claude:b":%s}}}\n' "$(date +%s)" \
  '{"account":"claude:b","state":"under pace","used_pct":40.0,"pace_pct":56.8,"stale":null}' "${over/claude:b/claude:a}" \
  '{"account":"claude:b","state":"under pace","used_pct":40.0,"pace_pct":56.8,"stale":null}' >"$GATE"
CALL_ENV=(AGENT_ROLE=lead SPAWN_CAPACITY_HOST=whiterabbit)
expect_said "$(agent '{"subagent_type":"worker","prompt":"Item: firn#19"}')" "allow|${held/claude:b/claude:a}" \
  'a whiterabbit Claude spawn reads claude:a, the account it bills, not the routed claude:b'
pace "$over"
CALL_ENV=(AGENT_ROLE=lead AGENT_ORG_NAME=mu)
expect_said "$(agent "$worker")" 'allow|' 'enforced pace hold: the org domain galileo has a target and passes'
CALL_ENV=(AGENT_ROLE=lead)
expect_said "$(agent '{"subagent_type":"worker","prompt":"CASE=URGENT FACT=\"main red blocks every landing\"\nItem: firn#19"}')" 'allow|' 'enforced pace hold: CASE=URGENT passes'
pace '{"account":"claude:b","state":"under pace","used_pct":40.0,"pace_pct":56.8,"stale":null}'
expect_said "$(agent '{"subagent_type":"worker","prompt":"Item: firn#19"}')" 'allow|' 'enforced pace hold: under pace passes'
pace '{"account":"claude:a","state":"at or over pace","used_pct":61.0,"pace_pct":24.0,"stale":null}'
expect_said "$(agent '{"subagent_type":"worker","prompt":"Item: smashcraft#7"}')" \
  'allow|Hold: claude:a is at or over its pace line (used 61.0% vs pace 24.0%); smashcraft has no approved deadline. Queue it, or ask Tom through the proxy for a dated goal.' \
  'before enforce_from the pace hold only warns'
pace '{"account":"claude:b","stale":"account claude:b STALE: usage 45 minutes old; run agents usage --refresh"}'
expect_said "$(agent '{"subagent_type":"worker","prompt":"Item: firn#19"}')" 'allow|account claude:b STALE: usage 45 minutes old; run agents usage --refresh' \
  'a stale readout never holds and prints the stale warning'
CALL_ENV=()
gate null

flow_cache="$SCRATCH/home/.local/state/agents/flow/smashcraft.json"
mkdir -p "${flow_cache%/*}"
flow_status() { printf '{"repo":"smashcraft","epoch":%s,"pending":%s,"landed_per_hour":%s,"push_to_land_minutes":12,"push_to_land_p90_minutes":20}\n' "$(date +%s)" "$1" "$2" >"$flow_cache"; }
flow_brief() { agent "{\"subagent_type\":\"worker\",\"prompt\":\"Lane: ~/code/smashcraft/worktrees/fix. $1\"}"; }
flow_status 3 1
expect_deny "$(flow_brief 'Fix the fighter.')" 'landing WIP refuses smashcraft above the floor of two'
mkdir -p "$SCRATCH/projected/lib" "$SCRATCH/bin"
cp "$HOOK" "$SCRATCH/projected/spawn-capacity-guard.sh"
cp "$HERE/lib/spawn_capacity.py" "$SCRATCH/projected/lib/"
cp "$HERE/../defaults.toml" "$SCRATCH/projected/lib/"
cp "$HERE/../lib/authoring-killswitch.sh" "$SCRATCH/projected/lib/"
cp "$HERE/../lib/north-agent-activation.sh" "$SCRATCH/projected/lib/"
ln -s "$HERE/../../bin/agents" "$SCRATCH/bin/agents"
PATH="$SCRATCH/bin:$PATH" expect_deny "$(flow_brief 'Fix the fighter.')" \
  'projected Claude hook resolves flow from the installed agents command' "$SCRATCH/projected/spawn-capacity-guard.sh"
expect_allow "$(flow_brief 'Fix Autoland.')" 'landing-path brief passes the landing WIP limit'
expect_allow "$(flow_brief 'CASE=URGENT Fix the fighter.')" 'CASE=URGENT passes the landing WIP limit'
expect_allow "$(flow_brief '\nLanding-path: yes\nPick checks per landing.')" 'a Landing-path brief passes the landing WIP limit'
expect_allow "$(flow_brief '\nClosing: yes\nRerun Done when on origin/main and tick the boxes.')" 'a closing pass that pushes no lane passes the landing WIP limit'
expect_deny "$(flow_brief 'Closing: the fighter fix.')" 'only a Closing: yes line exempts a brief'
expect_deny "$(agent '{"subagent_type":"worker","prompt":"Read /home/tom/code/north/main/socrates/skills/testing/SKILL.md. Lane: ~/code/smashcraft/worktrees/fix. Fix the fighter."}')" 'the lane path, not an earlier north path, names the repository'
expect_deny '{"session_id":"s","cwd":"/home/test/code/smashcraft/worktrees/fix","hook_event_name":"PreToolUse","tool_name":"spawn_agent","tool_input":{"message":"Fix the fighter."}}' \
  'Codex cwd uses the same landing WIP limit' "$HERE/codex-behavior-guard.sh"
expect_allow '{"session_id":"s","cwd":"/home/test/code/smashcraft/main","hook_event_name":"PreToolUse","tool_name":"spawn_agent","tool_input":{"message":"Read the firn tests and judge ten of them."}}' \
  'a spawn from a main checkout with no lane is not a code worker' "$HERE/codex-behavior-guard.sh"
expect_allow "$(agent '{"subagent_type":"worker","prompt":"Category: research\nLane: ~/code/smashcraft/worktrees/fix. Survey the tests."}')" 'a research brief is outside the landing WIP limit'
expect_allow "$(agent '{"subagent_type":"worker","prompt":"Fix the fighter in ~/code/north/worktrees/fix."}')" 'the brief lane takes precedence over unrelated repositories'
flow_status 4 4
expect_allow "$(flow_brief 'Fix the fighter.')" 'pending exactly at one hour of landing throughput allows'
flow_status 5 4
expect_deny "$(flow_brief 'Fix the fighter.')" 'pending above one hour of landing throughput refuses'
flow_status 2 1
expect_allow "$(flow_brief 'Fix the fighter.')" 'pending at the floor of two allows'
python3 - "$SCRATCH/home/.local/state/agents/spawn-gate-denials.jsonl" <<'PY' \
  && check ok 'landing WIP denial logs the repository and all three measurements' \
  || check bad 'landing WIP denial logs the repository and all three measurements' 'missing reason'
import json, sys
reasons = [json.loads(line).get("reason") or "" for line in open(sys.argv[1])]
assert any("smashcraft: pending 3, landed 1/h over 3h, push-to-land median 12m" in reason
           and "widen the landing path first" in reason for reason in reasons)
PY

python3 - "$HERE/../lib/flow.py" <<'PY' \
  && check ok 'GitHub Autoland push/status recording, 2026-10-11: median and p90' \
  || check bad 'GitHub Autoland push/status recording, 2026-10-11: median and p90' 'latency mismatch'
import importlib.util, json, sys
spec = importlib.util.spec_from_file_location("flow", sys.argv[1])
flow = importlib.util.module_from_spec(spec)
spec.loader.exec_module(flow)
pushes = json.loads('''
[
  {"createdAt": "2026-10-11T12:39:54Z", "headBranch": "claude/land-recorded-1", "headSha": "1111111111111111111111111111111111111111"},
  {"createdAt": "2026-10-11T12:37:05Z", "headBranch": "claude/land-recorded-2", "headSha": "2222222222222222222222222222222222222222"},
  {"createdAt": "2026-10-11T12:34:26Z", "headBranch": "claude/land-recorded-3", "headSha": "3333333333333333333333333333333333333333"},
  {"createdAt": "2026-10-11T12:27:43Z", "headBranch": "claude/land-recorded-4", "headSha": "4444444444444444444444444444444444444444"},
  {"createdAt": "2026-10-11T14:13:01Z", "headBranch": "claude/land-recorded-5", "headSha": "5555555555555555555555555555555555555555"}
]
''')
response = json.loads('''
{
  "data": {"repository": {
    "tip0": {"oid": "1111111111111111111111111111111111111111", "status": {"contexts": [{"context": "autoland", "state": "SUCCESS", "description": "landed as 6666666666666666666666666666666666666666", "createdAt": "2026-10-11T13:51:47Z"}]}},
    "tip1": {"oid": "2222222222222222222222222222222222222222", "status": {"contexts": [{"context": "autoland", "state": "SUCCESS", "description": "landed as 7777777777777777777777777777777777777777", "createdAt": "2026-10-11T13:35:25Z"}]}},
    "tip2": {"oid": "3333333333333333333333333333333333333333", "status": {"contexts": [{"context": "autoland", "state": "SUCCESS", "description": "landed as 7777777777777777777777777777777777777777", "createdAt": "2026-10-11T13:35:23Z"}]}},
    "tip3": {"oid": "4444444444444444444444444444444444444444", "status": {"contexts": [{"context": "autoland", "state": "SUCCESS", "description": "landed as 8888888888888888888888888888888888888888", "createdAt": "2026-10-11T13:17:56Z"}]}},
    "tip4": {"oid": "5555555555555555555555555555555555555555", "status": {"contexts": [{"context": "autoland", "state": "PENDING", "description": "waiting to test alone", "createdAt": "2026-10-11T14:13:12Z"}]}}
  }}
}
''')
now = flow.timestamp("2026-10-11T14:20:00Z")
pushes.extend([dict(pushes[0], createdAt="2026-10-11T12:40:00Z"),
               dict(pushes[0], createdAt="2026-10-09T12:39:54Z"),
               dict(pushes[0], headBranch="main", createdAt="2026-10-11T12:00:00Z")])
response["data"]["repository"]["missing"] = None
samples, _ = flow.autoland_samples(pushes, response["data"]["repository"], now)
assert samples == [4313 / 60, 3500 / 60, 3657 / 60, 3013 / 60], samples
median, p90 = flow.percentiles(samples)
assert median == (3500 + 3657) / 120 and p90 == 4313 / 60, (median, p90)
row = {"repo": "smashcraft", "pending": 23, "landed_per_hour": 8,
       "push_to_land_minutes": median, "push_to_land_p90_minutes": p90}
assert "push-to-land median 59.6417m p90 71.8833m over 24h" in flow.format_row(row)
PY

printf 'spawn-capacity-guard: %d passed, %d failed\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
