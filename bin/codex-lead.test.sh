#!/usr/bin/env bash
set -euo pipefail
unset NORTH_CODEX_CONVERSATION_HOME NORTH_CODEX_CONVERSATION_SQLITE_HOME NORTH_CODEX_POOLED_HOME

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
tool="$repo/bin/codex-lead"
scratch=$(mktemp -d "${TMPDIR:-/tmp}/codex-lead-test.XXXXXX")
owner=
trap 'if [ -n "$owner" ]; then kill "$owner"; wait "$owner" 2>/dev/null || true; fi; rm -rf "${scratch:?}"' EXIT

fail() { printf 'codex-lead.test.sh:%s: %s\n' "${BASH_LINENO[0]}" "$1" >&2; exit 1; }

org="$scratch/org.json"
cat >"$org" <<'JSON'
{"version":1,"priority":["galileo"],"nodes":[
 {"id":"muove-lead","role":"lead","domain":"galileo","parent":"proxy","provider":"claude","session":"claude-muove","depth":1,"budget":2,"state":"active"},
 {"id":"muove-codex-lead","role":"lead","domain":"galileo","parent":"proxy","provider":"codex","session":"thr-muove","depth":1,"budget":2,"state":"active"},
 {"id":"telemetry-lead","role":"lead","domain":"tooling","parent":"proxy","provider":"claude","session":"claude-telemetry","depth":1,"budget":2,"state":"active"},
 {"id":"telemetry-codex-lead","role":"sub-lead","domain":"tooling","parent":"telemetry-lead","provider":"codex","session":"thr-telemetry","depth":2,"budget":1,"state":"active"},
 {"id":"smashcraft-lead","role":"lead","domain":"smashcraft","parent":"proxy","provider":"claude","session":"claude-smashcraft","depth":1,"budget":2,"state":"active"},
 {"id":"smashcraft-codex-lead","role":"sub-lead","domain":"smashcraft","parent":"smashcraft-lead","provider":"codex","session":"thr-smashcraft","depth":2,"budget":1,"state":"active"},
 {"id":"old-codex-lead","role":"lead","domain":"galileo","parent":"proxy","provider":"codex","session":"thr-old","depth":1,"budget":2,"state":"finished"}
]}
JSON

# Binary path: a stub `ss` reports no daemon and logs each call, so a
# resolution that wrongly succeeds would be visible without any live send.
stubs="$scratch/stubs"
mkdir -p "$stubs"
printf '#!/bin/sh\necho called >>"%s"\n' "$scratch/ss.log" >"$stubs/ss"
chmod +x "$stubs/ss"

if env -u CODEX_LEAD_THREAD AGENTS_ORG_FILE="$org" AGENT_ORG_NAME=muove-lead PATH="$stubs:$PATH" \
    "$tool" send hi >/dev/null 2>"$scratch/send.err"; then
  fail "send with three live leads and no caller child exited 0"
fi
[ ! -e "$scratch/ss.log" ] || fail "send touched the daemon while refusing"
printf 'PASS ambiguous identity refuses before daemon lookup\n'

if env -u CODEX_LEAD_THREAD AGENTS_ORG_FILE="$org" AGENT_ORG_NAME=telemetry-lead PATH="$stubs:$PATH" \
    "$tool" send hi >/dev/null 2>"$scratch/send.err"; then
  fail "send reached a daemon stub that reports none"
fi
[ -s "$scratch/ss.log" ] || fail "resolved lead did not look up its daemon"
printf 'PASS caller child identity reaches daemon lookup\n'

mkdir -p "$scratch/home/sqlite"
db="$scratch/home/sqlite/state_5.sqlite"
sqlite3 "$db" "create table threads(id text, originator text, thread_source text,
  archived integer, created_at integer, first_user_message text);"
printf '#!/bin/sh\nexit 1\n' >"$stubs/agents"
printf '#!/bin/sh\nprintf "%%s\\0" "$@" >"%s"\n' "$scratch/launch.args" >"$stubs/spawn-quiet"
printf '#!/bin/sh\necho 2000\n' >"$stubs/date"
chmod +x "$stubs/agents" "$stubs/spawn-quiet" "$stubs/date"
brief="$scratch/brief.md"
printf 'Follow this brief.\n' >"$brief"
prompt=$'read Tom\'s brief\nand follow it'
for times in '2000 2001' '2001 2000' '2000 2000'; do
  read -r matching_time other_time <<<"$times"
  sqlite3 "$db" "delete from threads;
    insert into threads values ('matching','codex-tui','user',0,$matching_time,'read Tom''s brief
and follow it'),('unrelated','codex-tui','user',0,$other_time,'read another brief');"
  actual=$(env -u AGENT_ORG_NAME -u CODEX_LEAD_RESUME NORTH_CODEX_POOLED_HOME="$scratch/home" \
    CODEX_LEAD_HOST=ghostty CODEX_LEAD_PROMPT="$prompt" PATH="$stubs:$PATH" "$tool" start "$brief")
  [ "$actual" = matching ] || fail "start bound $actual with created_at $times"
  mapfile -d '' -t launch_args <"$scratch/launch.args"
  [ "${launch_args[${#launch_args[@]}-1]}" = "$prompt" ] || fail "launch changed the exact prompt"
done
sqlite3 "$db" "update threads set created_at=2001 where id='unrelated';"
actual=$(NORTH_CODEX_POOLED_HOME="$scratch/home" "$tool" id)
[ "$actual" = unrelated ] || fail "id stopped selecting the newest TUI thread"
actual=$(env -u AGENT_ORG_NAME NORTH_CODEX_POOLED_HOME="$scratch/home" CODEX_LEAD_RESUME=resume-thread \
  CODEX_LEAD_HOST=ghostty PATH="$stubs:$PATH" "$tool" start "$brief")
[ "$actual" = resume-thread ] || fail "start changed the resume thread"
printf 'PASS concurrent start binds the exact quoted prompt in either creation order\n'

cat >"$stubs/agents" <<'SH'
#!/usr/bin/env bash
[[ "${2:-}" = --burn ]] || exit 1
exec python3 -I "$BURN_USAGE" "${@:2}"
SH
chmod +x "$stubs/agents"
for expiry in 2099-01-01T00:00:00Z 2000-01-01T00:00:00Z; do
  printf '[burn]\ncodex = { until = "%s", rate = "13%%/h" }\n' "$expiry" >"$scratch/burn.toml"
  model=gpt-6.1-sol; effort=high
  actual=$(env -u AGENT_ORG_NAME NORTH_CODEX_POOLED_HOME="$scratch/home" CODEX_LEAD_RESUME=resume-thread \
    BURN_USAGE="$repo/harness/lib/usage.py" AGENTS_ORCHESTRATION="$scratch/burn.toml" THREADS_DB="$scratch/burn.db" \
    CODEX_LEAD_HOST=ghostty PATH="$stubs:$PATH" "$tool" start "$brief")
  mapfile -d '' -t launch_args <"$scratch/launch.args"
  printf '%s\n' "${launch_args[@]}" >"$scratch/burn.args"
  grep -Fxq "$model" "$scratch/burn.args" || fail "lead start lost burn/default model"
  grep -Fxq "model_reasoning_effort=$effort" "$scratch/burn.args" || fail "lead start lost burn/default effort"
done
printf 'PASS lead start consumes active burn and restores defaults after expiry\n'

actual=$(env -u AGENT_ORG_NAME -u CODEX_LEAD_RESUME NORTH_CODEX_POOLED_HOME="$scratch/home" \
  CODEX_LEAD_HOST=ghostty CODEX_LEAD_PROMPT="$prompt" PATH="$stubs:$PATH" "$tool" start "$brief" \
  --model gpt-6.1-sol -c model_reasoning_effort=medium -c service_tier=default)
[ "$actual" = matching ] || fail "explicit model start lost its prompt identity"
mapfile -d '' -t launch_args <"$scratch/launch.args"
model_count=0
for arg in "${launch_args[@]:7}"; do
  case "$arg" in -m|--model|-m?*|--model=*) model_count=$((model_count + 1)) ;; esac
done
[ "$model_count" = 1 ] || fail "explicit model start forwarded $model_count model options"
expected=(--model gpt-6.1-sol -c model_reasoning_effort=medium -c service_tier=default "$prompt")
for ((i=0; i<${#expected[@]}; i++)); do
  [ "${launch_args[${#launch_args[@]}-${#expected[@]}+i]}" = "${expected[i]}" ] || fail "explicit model start changed caller argv"
done
printf 'PASS recorded explicit-model startup keeps one caller model and exact caller argv\n'

key=$(printf '%s\n%s' "$scratch/home" "$scratch/home/sqlite" | sha256sum)
python3 -c 'import signal; signal.pause()' app-server --listen "unix://${XDG_RUNTIME_DIR:-/run/user/$UID}/codex-shared-${key:0:16}/app-server.sock" & owner=$!
cat >"$stubs/ss" <<SH
#!/bin/sh
printf '%s\n' '/tmp/codex-daemon-$UID/abcdef users:(("codex",pid=$owner,fd=31))'
SH
cat >"$stubs/websocat" <<'PY'
#!/usr/bin/env python3
import json, os, sys
from pathlib import Path
for line in sys.stdin:
    request = json.loads(line)
    if request.get("id") != 2:
        continue
    method, params = request["method"], request["params"]
    assert params["threadId"] == "fixture-worker", request
    if method == "thread/turns/list":
        result = {"data": [] if os.environ.get("NO_ACTIVE_TURN") else [{"id": "fixture-turn", "status": "inProgress"}]}
    elif method == "turn/interrupt":
        assert method == "turn/interrupt" and params == {"threadId": "fixture-worker", "turnId": "fixture-turn"}, request
        Path(os.environ["INTERRUPT_LOG"]).write_text(json.dumps(request))
        result = {}
    else:
        expected = "<north_machine_relay/> " + os.environ["RELAY_MESSAGE"]
        if method in ("turn/steer", "turn/start"):
            assert params["input"] == [{"type": "text", "text": expected, "text_elements": []}], request
            if method == "turn/steer":
                assert params["expectedTurnId"] == "fixture-turn", request
            result = {"turnId": "fixture-turn", "turn": {"id": "fixture-turn"}}
        else:
            assert method == "thread/goal/set" and params["objective"] == expected and params["status"] == "active", request
            result = {"goal": {"status": "active", "objective": params["objective"]}}
        Path(os.environ["RELAY_LOG"]).write_text(json.dumps(request))
    print(json.dumps({"id": 2, "result": result}), flush=True)
    break
PY
chmod +x "$stubs/ss" "$stubs/websocat"
actual=$(NORTH_CODEX_POOLED_HOME="$scratch/home" CODEX_LEAD_THREAD=fixture-worker INTERRUPT_LOG="$scratch/interrupt.json" PATH="$stubs:$PATH" "$tool" interrupt)
[ "$actual" = 'interrupted turn fixture-turn' ] && [ -s "$scratch/interrupt.json" ] || fail 'interrupt did not target the explicit worker active turn'
if env -u CODEX_LEAD_THREAD PATH="$stubs:$PATH" "$tool" interrupt >"$scratch/interrupt.out" 2>"$scratch/interrupt.err"; then
  fail 'interrupt without an explicit worker thread succeeded'
fi
[ "$(cat "$scratch/interrupt.err")" = 'codex-lead: interrupt requires CODEX_LEAD_THREAD' ] || fail 'interrupt did not refuse ambiguous identity before RPC'
printf 'PASS worker interrupt targets its active turn and requires explicit thread identity\n'

export NORTH_CODEX_POOLED_HOME="$scratch/home" CODEX_LEAD_THREAD=fixture-worker RELAY_MESSAGE='From the proxy, STOP NOW: never run window actions' RELAY_LOG="$scratch/relay.json"
for active in yes no; do
  if [ "$active" = yes ]; then unset NO_ACTIVE_TURN; else export NO_ACTIVE_TURN=1; fi
  actual=$(PATH="$stubs:$PATH" "$tool" send "$RELAY_MESSAGE")
  if [ "$active" = yes ]; then expected='delivered into running turn fixture-turn'; method=turn/steer; else expected='delivered as new turn fixture-turn'; method=turn/start; fi
  [ "$actual" = "$expected" ] && [ "$(jq -r .method "$RELAY_LOG")" = "$method" ] || fail 'send did not mark the relayed prompt'
done
actual=$(PATH="$stubs:$PATH" "$tool" goal "$RELAY_MESSAGE")
[ "$actual" = "active: <north_machine_relay/> $RELAY_MESSAGE" ] && [ "$(jq -r .method "$RELAY_LOG")" = thread/goal/set ] || fail 'goal did not mark the relayed objective'
printf 'PASS send and goal preserve message text with machine relay origin\n'

echo "codex-lead.test.sh: all assertions passed (7 cases)"
