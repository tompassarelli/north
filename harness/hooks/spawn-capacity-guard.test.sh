#!/usr/bin/env bash
# Spawn gate fixtures over a fake capacity probe: provisioned CPUs under the
# limit allow a local worker, at or over it (or protected pressure above 20)
# deny with the queue/farm/cloud message, high system PSI with few provisioned
# CPUs allows, an unreadable probe allows, a remote worker allows, and an URGENT
# brief passes and is logged. The Codex spawn_agent path through the behavior
# decider denies the same way. Fleet task references provide project attribution
# for the priority headroom rule.
# A fresh usage-gate.json refusal (firn#12 money gate) denies with its own text.
# No pace hold: an account at or over its pace line with no target or drain grant
# admits the spawn; only the money refusal and the out-rule (5h at 95%, 5h burn over
# 50%/h, provider-exhausted week) hold it, and a signed-in Claude spawn reads its
# account label, the account it bills.
set -uo pipefail
if [ "${1:-}" != --isolated ]; then
  # shellcheck source-path=SCRIPTDIR source=../lib/tree-north.sh
  . "$(dirname "$0")/../lib/tree-north.sh" && exec env -i PATH="$PATH" NORTH_PACE_BIN="$NORTH_PACE_BIN" bash "$0" --isolated
fi
export PATH="/etc/codex/hooks/runtime:$PATH"

HERE="$(cd "$(dirname "$0")" && pwd)"
HOOK="$HERE/spawn-capacity-guard.sh"
SCRATCH="$(mktemp -d "${TMPDIR:-/tmp}/spawn-capacity-test.XXXXXX")"
trap 'rm -rf "${SCRATCH:?}"' EXIT
export HOME="$SCRATCH/home" XDG_STATE_HOME="$SCRATCH/home/.local/state" XDG_RUNTIME_DIR="$SCRATCH/run"
export XDG_CONFIG_HOME="$SCRATCH/home/.config" XDG_DATA_HOME="$SCRATCH/home/.local/share"
export CODEX_HOME="$SCRATCH/home/.codex" AGENTS_OPERATOR_DIR="$SCRATCH"
export TZ=UTC
mkdir -p "$XDG_RUNTIME_DIR" "$SCRATCH/offline-bin"
mkdir -p "$SCRATCH/bin"
cat >"$SCRATCH/bin/north" <<'SH'
#!/usr/bin/env bash
[[ "$*" == "fleet list --json" ]] || { [[ "$1 $2" == "threads snapshot-pull" || "$1 $2" == "threads calibration-runs" ]] && exec "$NORTH_PACE_BIN" "$@"; exit 2; }
cat "$FLEET_JSON"
SH
chmod +x "$SCRATCH/bin/north"
export NORTH_BIN="$SCRATCH/bin/north" FLEET_JSON="$SCRATCH/fleet.json"
printf '[]\n' >"$FLEET_JSON"
cat >"$SCRATCH/offline-bin/agents" <<'SH'
#!/usr/bin/env bash
if [ "$1 $2 $3" = "flow row makgora" ] && [ -f "${FLOW_CACHE:-}" ]; then
  jq -c '{row: ., line: "\(.repo): pending \(.pending), landed \(.landed_per_hour)/h over 3h, push-to-land median \(.push_to_land_minutes)m p90 \(.push_to_land_p90_minutes)m over 24h"}' "$FLOW_CACHE"
  exit 0
fi
exit 1
SH
chmod +x "$SCRATCH/offline-bin/agents"
ln -s agents "$SCRATCH/offline-bin/claude"
ln -s agents "$SCRATCH/offline-bin/ssh"
ln -s "$HERE/.." "$SCRATCH/harness"
export PATH="$SCRATCH/offline-bin:$PATH"
export AGENT_HOOK_ERRORS="$SCRATCH/errors.tsv"
ACTIVATION="$SCRATCH/activation.active"
STATUS="$SCRATCH/probe.json"
printf 'hook spawn-capacity-guard\nhook codex-behavior-guard\n' >"$ACTIVATION"
mkdir -p "$SCRATCH/home/.claude/projects/p/s/subagents"
export CLAUDE_CONFIG_DIR="$SCRATCH/home"
printf '%s\n' '{"cachedUsageUtilization":{"accountUuid":"fixture-primary"}}' >"$SCRATCH/home/.claude.json"
: >"$SCRATCH/home/.claude/projects/p/s/subagents/agent-a.jsonl"
: >"$SCRATCH/home/.claude/projects/p/s/subagents/agent-b.jsonl"
touch -d "@$(date +%s)" "$SCRATCH/home/.claude/projects/p/s/subagents/agent-"{a,b}.jsonl
GATE="$SCRATCH/home/.local/state/agents/usage-gate.json"
mkdir -p "${GATE%/*}"
# gate CLAUDE_REFUSAL [REFUSALS_JSON]: write a fresh money-gate verdict (codex always allowed).
gate() { printf '{"claude":{"epoch":%s,"stale_min":30,"refusal":%s,"refusals":%s},"codex":{"epoch":%s,"stale_min":30,"refusal":null}}\n' "$(date +%s)" "$1" "${2-{\}}" "$(date +%s)" >"$GATE"; }
gate null

# status LEASED PROTECTED [SYSTEM_PSI]: write the probe fixture (limit 20 CPUs).
status() { printf '{"decision":"RUN","committedBatchCpus":%s,"leasedNativeCpus":12,"aggregateCpuLimit":20,"protectedCpuSomeAvg10":%s,"cpuSomeAvg10":%s}\n' "$1" "$2" "${3:-1}" >"$STATUS"; }

# call HOOK JSON: print the deny reason ("" when allowed).
call() {
  printf '%s' "$2" | env -u AGENT_NO_AUTHORING_HOOKS -u CLAUDE_CODE_ENTRYPOINT \
    AGENTS_ORCHESTRATION="$SCRATCH/orchestration.toml" "${CALL_ENV[@]}" HOME="$SCRATCH/home" SPAWN_CAPACITY_HOME="$SCRATCH/home" \
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
expect_deny "$(agent '{"subagent_type":"worker-sonnet","prompt":"p"}')" 'worker-sonnet is a local Claude worker and denied over the limit'
expect_allow "$(agent '{"subagent_type":"worker","isolation":"remote","prompt":"p"}')" 'remote worker allows over the limit'
expect_allow "$(agent '{"subagent_type":"claude-code-guide","prompt":"p"}')" 'non-worker agent type allows'
expect_allow "$(agent '{"subagent_type":"worker","prompt":"CASE=URGENT FACT=\"prod login is down for every user\" fix it"}')" 'URGENT brief passes over the limit'
grep -q '"gate": "spawn-capacity"' "$SCRATCH/state/verify-overrides.jsonl" 2>/dev/null \
  && check ok 'URGENT override is logged' || check bad 'URGENT override is logged' "no log line"
expect_allow "$(agent '{"subagent_type":"worker","prompt":"Category: tooling\nLanding-path: yes\nPick and run checks for each queued landing."}')" 'a Landing-path brief is admitted at the CPU cap'
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
cat >"$SCRATCH/orchestration.toml" <<'TOML'
[accounts.claude]
labels = { "9b3a7fb5" = "a" }
[projects.galileo]
priority = 1
[projects.makgora]
priority = 2
TOML
status 17 5
printf '[{"name":"sc","conversation":"s","task_ref":"makgora#7","status":"working"}]\n' >"$FLEET_JSON"
dodeny "$(agent "$worker")" 'a lower-priority domain stops at 80% of the limit'
printf '[{"name":"mu","conversation":"s","task_ref":"galileo#9","status":"working"}]\n' >"$FLEET_JSON"
expect_allow "$(agent "$worker")" 'the first-priority domain keeps the headroom'

status 2 5
gate '"No: a new claude worker could spend money: test."'
dodeny "$(agent "$worker")" 'fresh money-gate refusal denies'
gate '"No: a new claude worker could spend money: no usage snapshot for claude could be read."' '{"claude:a":null}'
expect_allow "$(agent "$worker")" 'a refusal written by a process with no account does not refuse a session on account a, which is judged on its own entry'
gate null '{"claude:a":"No: a new claude:a worker could spend money: test."}'
dodeny "$(agent "$worker")" 'the refusal entry for the session account denies though the shared refusal is null'
gate null

# pace PACE_JSON: write a fresh claude verdict carrying routing's pace reading.
pace() { printf '{"claude":{"epoch":%s,"stale_min":30,"refusal":null,"pace":%s}}\n' "$(date +%s)" "$1" >"$GATE"; }
# said JSON: the deny reason, else the warning shown while the spawn proceeds.
said() {
  printf '%s' "$1" | env -u AGENT_NO_AUTHORING_HOOKS -u CLAUDE_CODE_ENTRYPOINT \
    AGENTS_ORCHESTRATION="$SCRATCH/orchestration.toml" "${CALL_ENV[@]}" HOME="$SCRATCH/home" SPAWN_CAPACITY_HOME="$SCRATCH/home" \
    SPAWN_CAPACITY_STATUS="$STATUS" XDG_STATE_HOME="$SCRATCH/home/.local/state" CODEX_BEHAVIOR_STATE="$SCRATCH/state" \
    CODEX_BEHAVIOR_CODE_ROOT="$SCRATCH/code" NORTH_AGENT_ACTIVE="$ACTIVATION" NORTH_AGENT_PYTHON=/etc/codex/hooks/runtime/python3 "${2:-$HOOK}" | python3 -c '
import json, sys
raw = sys.stdin.read().strip()
out = json.loads(raw) if raw else {}
print(out.get("hookSpecificOutput", {}).get("permissionDecision", "allow") + "|" +
      (out.get("hookSpecificOutput", {}).get("permissionDecisionReason") or out.get("systemMessage") or
      out.get("hookSpecificOutput", {}).get("additionalContext") or ""))'
}
expect_said() { local out; out="$(said "$1" "${4:-}")"; [ "$out" = "$2" ] && check ok "$3" || check bad "$3" "$out"; }
status 2 5
cat >"$SCRATCH/orchestration.toml" <<'TOML'
[accounts.claude]
labels = { "9b3a7fb5" = "a" }
[projects.galileo]
priority = 1
target = 2999-11-01
drain = { "claude:b" = "tom: 2026-10-12 fixture grant to drain claude:b for this target", "codex" = "tom: 2026-10-12 fixture grant to drain codex for this target" }
[projects.makgora]
priority = 2
[projects.north]
priority = 3
target = 2026-01-01
[projects.hellfire]
priority = 4
target = 2999-12-01
[pace]
enforce_from = { "claude:b" = "2000-01-01T00:00:00Z", "claude:a" = "2999-01-01T00:00:00Z" }
TOML
over='{"account":"claude:b","state":"at or over pace","used_pct":97.0,"pace_pct":56.8,"stale":null}'
pace "$over"
printf '[{"name":"mu","conversation":"s","task_ref":"firn#19","status":"working"}]\n' >"$FLEET_JSON"
expect_said "$(agent '{"subagent_type":"worker","prompt":"Item: firn#19\nFix it."}')" 'allow|' 'an enforced at-or-over-pace reading with no target or drain grant never holds a spawn'
printf '[]\n' >"$FLEET_JSON"
CALL_ENV=()
gate null

flow_cache="$SCRATCH/home/.local/state/agents/flow/makgora.json"
export FLOW_CACHE="$flow_cache"
mkdir -p "${flow_cache%/*}"
flow_status() { printf '{"repo":"makgora","epoch":%s,"pending":%s,"landed_per_hour":%s,"push_to_land_minutes":12,"push_to_land_p90_minutes":20}\n' "$(date +%s)" "$1" "$2" >"$flow_cache"; }
flow_brief() { agent "{\"subagent_type\":\"worker\",\"prompt\":\"Lane: ~/code/makgora/worktrees/fix. $1\"}"; }
flow_status 3 1
expect_deny "$(flow_brief 'Fix the fighter.')" 'landing WIP refuses makgora above the floor of two'
mkdir -p "$SCRATCH/projected/lib" "$SCRATCH/bin"
cp "$HOOK" "$SCRATCH/projected/spawn-capacity-guard.sh"
cp "$HERE/lib/spawn_capacity.py" "$SCRATCH/projected/lib/"
cp "$HERE/../defaults.toml" "$SCRATCH/projected/lib/"
cp "$HERE/../lib/authoring-killswitch.sh" "$SCRATCH/projected/lib/"
cp "$HERE/../lib/north-agent-activation.sh" "$SCRATCH/projected/lib/"
ln -s "$SCRATCH/offline-bin/agents" "$SCRATCH/bin/agents"
PATH="$SCRATCH/bin:$PATH" expect_deny "$(flow_brief 'Fix the fighter.')" \
  'projected Claude hook resolves flow from the installed agents command' "$SCRATCH/projected/spawn-capacity-guard.sh"
expect_allow "$(flow_brief 'Fix Autoland.')" 'landing-path brief passes the landing WIP limit'
expect_allow "$(flow_brief 'CASE=URGENT Fix the fighter.')" 'CASE=URGENT passes the landing WIP limit'
expect_allow "$(flow_brief '\nLanding-path: yes\nPick checks per landing.')" 'a Landing-path brief passes the landing WIP limit'
expect_allow "$(flow_brief '\nClosing: yes\nRerun Done when on origin/main and tick the boxes.')" 'a closing pass that pushes no lane passes the landing WIP limit'
expect_deny "$(flow_brief 'Closing: the fighter fix.')" 'only a Closing: yes line exempts a brief'
expect_deny "$(agent '{"subagent_type":"worker","prompt":"Read /home/tom/code/north/main/agent-machinery/skills/verification/SKILL.md. Lane: ~/code/makgora/worktrees/fix. Fix the fighter."}')" 'the lane path, not an earlier north path, names the repository'
expect_deny '{"session_id":"s","cwd":"/home/test/code/makgora/worktrees/fix","hook_event_name":"PreToolUse","tool_name":"spawn_agent","tool_input":{"message":"Fix the fighter."}}' \
  'Codex cwd uses the same landing WIP limit' "$HERE/codex-behavior-guard.sh"
expect_allow '{"session_id":"s","cwd":"/home/test/code/makgora/main","hook_event_name":"PreToolUse","tool_name":"spawn_agent","tool_input":{"message":"Read the firn tests and judge ten of them."}}' \
  'a spawn from a main checkout with no lane is not a code worker' "$HERE/codex-behavior-guard.sh"
expect_allow "$(agent '{"subagent_type":"worker","prompt":"Category: research\nLane: ~/code/makgora/worktrees/fix. Survey the tests."}')" 'a research brief is outside the landing WIP limit'
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
assert any("makgora: pending 3, landed 1/h over 3h, push-to-land median 12m" in reason
           and "widen the landing path first" in reason for reason in reasons)
PY

for n in 1 2 3; do touch "$SCRATCH/home/.claude/projects/p/s/subagents/agent-flight-$n.jsonl"; done
status 2 5
sed -i 's/^enforce_from = .*/enforce_from = { "claude:a" = "2000-01-01T00:00:00Z" }/' "$SCRATCH/orchestration.toml"
sed -i '/^\[accounts.claude\]/a target_left = 5' "$SCRATCH/orchestration.toml"
RESET=$(( $(date +%s) + 3600 ))
meter_gate() {
  AGENTS_ORCHESTRATION="$SCRATCH/orchestration.toml" XDG_STATE_HOME="$SCRATCH/home/.local/state" \
    python3 - "$HERE/../lib" "$SCRATCH/meter.db" "$RESET" "$@" <<'PYMETER'
import json, os, sqlite3, subprocess, sys
from datetime import datetime, timezone
sys.path.insert(0, sys.argv[1])
import usage
now = datetime.now(timezone.utc)
reset = datetime.fromtimestamp(int(sys.argv[3]), timezone.utc).isoformat()
five, burn, week, extra = float(sys.argv[4]), float(sys.argv[5]), float(sys.argv[6]), int(sys.argv[7])
rows = [dict(ts=now.isoformat(), provider="claude", account="a", kind="window", name=name, used_pct=used, window_min=minutes,
             resets_at=reset, source="sanitized one out-rule spawn journey") for name, used, minutes in (("5h", five, 300), ("week", week, 10080))]
rows += [dict(ts=now.isoformat(), provider="claude", account="a", kind="billing", name=name, amount=extra if name == "extra_usage" else 0,
              source="sanitized one out-rule spawn journey") for name in ("extra_usage", "spend")]
with sqlite3.connect(sys.argv[2]) as db:
    usage.record(db, rows)
    verdicts = usage.write_gate(db, ["claude"])
verdict = verdicts["claude"]
for windows in verdict["projections"].values():
    for window in windows:
        window["burn_pct_h"] = burn if window["window"] == "5h" else 0.0
verdict["controls"] = {account: json.loads(subprocess.run([os.environ.get("NORTH_PACE_BIN", "north"), "pace"], input=json.dumps(dict(op="advance", previous={}, windows=windows, workers=5, at=now.timestamp(), initial=64, margin=10, target=None)), capture_output=True, text=True, check=True).stdout)
                       for account, windows in verdict["projections"].items()}
usage.gate_path().write_text(json.dumps(verdicts))
PYMETER
}
reset_at="$(date -u -d "@$RESET" '+%-I:%M %p')"
worker_brief='{"subagent_type":"worker","model":"opus","effort":"medium","prompt":"Item: firn#19"}'
meter_gate 30 5 97 0
expect_said "$(agent "$worker_brief")" 'allow|' 'a plan-only account at week 97% with 5 workers in flight and no ledger calibration is admitted'
meter_gate 95 5 60 0
expect_said "$(agent "$worker_brief")" "deny|Hold: claude:a exhausted: 5h 95% >= 95%; the 5h window resets at $reset_at." 'the 5h window at 95% holds and names its reset'
meter_gate 30 50.5 60 0
expect_said "$(agent "$worker_brief")" "deny|Hold: claude:a burning: 5h 50.5%/h > the 50%/h cap that keeps the window two hours; the 5h window resets at $reset_at." '5h burn over 50%/h holds and names its reset'
meter_gate 30 5 100 0
expect_said "$(agent "$worker_brief")" "deny|Hold: claude:a exhausted: provider week 100%; the week window resets at $reset_at." 'a provider-reported exhausted week holds and names its reset'
meter_gate 30 5 93 1
out="$(said "$(agent "$worker_brief")")"
[[ "$out" == "deny|No: a new claude"*" worker could spend money: "*"(ceiling 92%) and extra_usage would bill money past it."* ]] && check ok 'an account whose overflow bills money past the money ceiling is denied by the usage-gate money refusal' || check bad 'an account whose overflow bills money past the money ceiling is denied by the usage-gate money refusal' "$out"
gate null

printf 'spawn-capacity-guard: %d passed, %d failed\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
