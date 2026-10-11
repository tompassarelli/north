#!/usr/bin/env bash
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/socrates-chat-test.XXXXXX")"
trap 'rm -rf "${scratch:?}"' EXIT
export AGENTS_ORG_FILE="$scratch/org.json"
export AGENTS_ORCHESTRATION="$scratch/orchestration.toml"
printf '[projects]\n' >"$AGENTS_ORCHESTRATION"
org() { "$repo/bin/agents" org "$@" >/dev/null; }
replay() { local live="${3:-}"; [[ -n "$live" ]] || live='{}'; jq -n --slurpfile org "$1" --argjson rooms "$2" --argjson live "$live" '{org:$org[0],rooms:$rooms,live:$live}' | bun --no-install "$repo/bin/socrates-chat" replay; }
check() { if [[ "$1" != "$2" ]]; then printf 'FAIL %s\ngot: %s\nwant: %s\n' "$3" "$1" "$2" >&2; exit 1; fi; printf 'PASS %s\n' "$3"; }

org add chattest-room --role lead --domain chattest
org add chattest-child --role sub-lead --domain chattest --parent chattest-room
org add chattest-worker --role worker --domain chattest --parent chattest-child
cp "$AGENTS_ORG_FILE" "$scratch/new.json"
active='[{"node":"chattest-room","host":"fixture","room_id":"!room:example.test","state":"active","tom":"join"},{"node":"chattest-child","host":"fixture","room_id":"!child:example.test","state":"active","tom":"join"}]'
check "$(replay "$scratch/new.json" '[]' | jq -c '[.[]|[.kind,.node.id]]')" '[["open","chattest-room"],["open","chattest-child"]]' 'new leads open; worker has no room'
check "$(replay "$scratch/new.json" "$active")" '[]' 'unchanged snapshot is idempotent'

org finish chattest-room
cp "$AGENTS_ORG_FILE" "$scratch/finished.json"
check "$(replay "$scratch/finished.json" "$active" | jq -c '[.[]|[.kind,.node]]')" '[["close","chattest-room"]]' 'finished room closes'
closed="$(jq -c 'map(if .node=="chattest-room" then .state="finished"|.tom="leave" else . end)' <<<"$active")"
check "$(replay "$scratch/finished.json" "$closed")" '[]' 'finished snapshot is idempotent'

org remove chattest-room
cp "$AGENTS_ORG_FILE" "$scratch/gone.json"
check "$(replay "$scratch/gone.json" "$active" | jq -c '[.[]|[.kind,.node]]')" '[["close","chattest-room"],["close","chattest-child"]]' 'gone nodes close'

org add chattest-room --role lead --domain chattest
org exit chattest-room
cp "$AGENTS_ORG_FILE" "$scratch/died.json"
one="$(jq -c '.[0:1]' <<<"$active")"
check "$(replay "$scratch/died.json" "$one" | jq -c '[.[]|[.kind,.node]]')" '[["died","chattest-room"]]' 'dead session emits one notice'
dead="$(jq -c 'map(.state="died")' <<<"$one")"
check "$(replay "$scratch/died.json" "$dead")" '[]' 'death notice is not repeated'
check "$(replay "$scratch/died.json" "$(jq -c 'map(.tom="leave")' <<<"$dead")" | jq -c '[.[].kind]')" '["open"]' 'pending force-join does not repeat the death notice'

org set chattest-room --state active
cp "$AGENTS_ORG_FILE" "$scratch/returning.json"
check "$(replay "$scratch/returning.json" "$dead" | jq -c '[.[]|[.kind,.node]]')" '[["revived","chattest-room"]]' 'returning session retains its room'
check "$(replay "$scratch/returning.json" "$(jq -c '.[0:1]' <<<"$closed")" | jq -c '[.[]|[.kind,.node.id]]')" '[["open","chattest-room"]]' 'returning name reopens its finished room'
check "$(replay "$scratch/returning.json" "$one" '{"chattest-room":false}' | jq -c '[.[].kind]')" '["died"]' 'pid liveness is supplied to the pure core'
mkdir -p "$scratch/mutant/bin" "$scratch/mutant/socrates/lib"
cp "$repo/bin/socrates-chat" "$scratch/mutant/bin/socrates-chat"
sed 's/node.state === "finished"/false/' "$repo/socrates/lib/chat-core.ts" >"$scratch/mutant/socrates/lib/chat-core.ts"
fault="$(jq -n --slurpfile org "$scratch/finished.json" --argjson rooms "$active" '{org:$org[0],rooms:$rooms,live:{}}' | bun --no-install "$scratch/mutant/bin/socrates-chat" replay | jq -c '[.[]|[.kind,.node]]')"
if [[ "$fault" == '[["close","chattest-room"]]' ]]; then printf 'FAIL finished-node fault survived\n' >&2; exit 1; fi
printf 'PASS injected finished-node fault detected\n'
printf '5 recorded lifecycle cases passed\n'
