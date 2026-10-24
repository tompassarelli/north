#!/usr/bin/env bash
set -euo pipefail
shopt -s inherit_errexit

usage() {
  echo "usage: cloud-sessions.sh list | events SESSION_ID OUT.jsonl | sweep OUT_DIR" >&2
  echo "sweep archives only cloud-run sessions (environment_kind anthropic_cloud) waiting on a human, never Remote Control bridges." >&2
  echo "Run on the host whose ~/.claude login owns the cloud sessions (south:agents/skills/claude-accounts/)." >&2
  exit 2
}

api() {
  local method=$1 path=$2
  curl -sS --fail-with-body --max-time 60 -X "$method" "https://api.anthropic.com$path" \
    -H "Authorization: Bearer $token" -H "anthropic-version: 2023-06-01" \
    -H "anthropic-beta: ccr-byoc-2025-07-29" -H "x-organization-uuid: $org" \
    -H "content-type: application/json" ${3:+-d "$3"}
}

sessions() {
  local after="" page
  while :; do
    page=$(api GET "/v1/sessions?beta=true&limit=100${after:+&after_id=$after}")
    jq -r '.data[] | [.id, .session_status, .environment_kind, .updated_at, .title] | @tsv' <<<"$page"
    after=$(jq -r 'if .has_more then .last_id // empty else empty end' <<<"$page")
    [ -n "$after" ] || break
  done
}

events() {
  local id=$1 out=$2 after="" page
  : > "$out"
  while :; do
    page=$(api GET "/v1/sessions/$id/events?beta=true&limit=1000${after:+&after_id=$after}")
    jq -c '.data[]' <<<"$page" >> "$out"
    after=$(jq -r 'if .has_more then .last_id // empty else empty end' <<<"$page")
    [ -n "$after" ] || break
  done
}

[ $# -ge 1 ] || usage
token=$(jq -er '.claudeAiOauth.accessToken' ~/.claude/.credentials.json)
org=$(jq -er '.oauthAccount.organizationUuid' ~/.claude.json)

case $1 in
  list)
    [ $# -eq 1 ] || usage
    sessions
    ;;
  events)
    [ $# -eq 3 ] || usage
    events "$2" "$3"
    echo "$(wc -l < "$3") events in $3"
    ;;
  sweep)
    [ $# -eq 2 ] || usage
    mkdir -p "$2"
    listing=$(sessions)
    found=0
    while IFS=$'\t' read -r id status kind _ title; do
      [ "$status" = requires_action ] && [ "$kind" = anthropic_cloud ] || continue
      found=1
      events "$id" "$2/$id.jsonl"
      api POST "/v1/sessions/$id/archive?beta=true" '{}' > /dev/null
      echo "stopped $id ($title): it was waiting on a human; transcript $2/$id.jsonl"
    done <<<"$listing"
    [ "$found" = 1 ] || echo "no cloud session is waiting on a human"
    ;;
  *) usage ;;
esac
