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

export AGENTS_ORG_FILE="$scratch/domains-org.json"
org add smashcraft-lead --role lead --domain smashcraft
org add galileo-sub-lead --role sub-lead --domain galileo --parent smashcraft-lead
org add chattest-lead --role lead --domain chattest
jq -n --slurpfile org "$AGENTS_ORG_FILE" '{org:$org[0],server:"example.test",
  spaces:{socrates:"!root:example.test",smashcraft:"!smashcraft:example.test",galileo:"!galileo:example.test",wisp:"!wisp:example.test",nexus:"!nexus:example.test",north:"!north:example.test",tooling:"!tooling:example.test",mail:"!mail:example.test"},
  rooms:[$org[0].nodes[]|{node:.id,host:"fixture",room_id:("!node-"+.id+":example.test"),state:"active",tom:"join"}],
  children:[$org[0].nodes[]|{parent:"socrates",room_id:("!node-"+.id+":example.test"),content:{via:["example.test"],suggested:true}}]}' >"$scratch/layout.json"
bun --no-install "$repo/bin/socrates-chat" replay "$scratch/layout.json" >"$scratch/layout-actions.json"
check "$(jq -c '[.[]|select(.kind=="space" and (.room_id|startswith("!node-")))|[.parent,.room_id,.content.order//null]]' "$scratch/layout-actions.json")" \
  '[["socrates","!node-smashcraft-lead:example.test",null],["socrates","!node-galileo-sub-lead:example.test",null],["smashcraft","!node-smashcraft-lead:example.test","smashcraft-lead"],["galileo","!node-galileo-sub-lead:example.test","galileo-sub-lead"],["socrates","!node-chattest-lead:example.test","99-chattest-lead"]]' 'domain rooms move; unknown domain stays in Socrates'
jq --slurpfile actions "$scratch/layout-actions.json" '.children=[$actions[0][]|select(.kind=="space" and .content.via)|del(.kind)]' "$scratch/layout.json" >"$scratch/layout-settled.json"
check "$(bun --no-install "$repo/bin/socrates-chat" replay "$scratch/layout-settled.json")" '[]' 'unchanged domain snapshot has no actions'
