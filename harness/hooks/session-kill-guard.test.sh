#!/usr/bin/env bash
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
runner="$HERE/../../target/debug/north"
[ -x "$runner" ] || runner="${CARGO_TARGET_DIR:-$HOME/.cache/cargo-target/north}/debug/north"
[ -x "$runner" ] || { printf 'FAIL no north build at %s; run cargo build\n' "$runner"; exit 1; }
scratch="$(mktemp -d)"
trap 'rm -rf -- "$scratch"' EXIT
mkdir "$scratch/bin" && ln -s "$runner" "$scratch/bin/north"
printf 'hook session-kill-guard\n' >"$scratch/activation.active"
export PATH="$scratch/bin:$PATH" NORTH_AGENT_ACTIVE="$scratch/activation.active" AGENT_NO_AUTHORING_HOOKS=0
decision() {
  jq -n --arg command "$1" '{hook_event_name: "PreToolUse", tool_name: "Bash", cwd: "/tmp", tool_input: {command: $command}}' \
    | "$HERE/session-kill-guard.sh" | jq -rs '.[0].hookSpecificOutput.permissionDecision // "allow"'
}
failed=0 count=0
while IFS= read -r -d '' command && IFS= read -r -d '' expected; do
  observed="$(decision "$command")"
  [ "$observed" = "$expected" ] || { printf 'FAIL %q: expected %s, got %s\n' "$command" "$expected" "$observed"; failed=1; }
  count=$((count + 1))
done < <(jq -j '.[] | .[0], "\u0000", .[1], "\u0000"' "$HERE/../../src/guard/fixtures/session_kill.json")
reason="$(jq -n '{tool_name: "Bash", tool_input: {command: "ssh -o BatchMode=yes root@gex45 '\''hostname; systemctl restart user@1000.service; echo rc $?'\''"}}' \
  | "$HERE/session-kill-guard.sh" | jq -r .hookSpecificOutput.permissionDecisionReason)"
case "$reason" in *'`firn host restart-user-manager gex45`'*) ;; *) printf 'FAIL refusal does not name the host command: %s\n' "$reason"; failed=1 ;; esac
off="$(jq -n '{tool_name: "Bash", tool_input: {command: "kill -1"}}' | AGENT_NO_AUTHORING_HOOKS=1 "$HERE/session-kill-guard.sh")"
[ -z "$off" ] || { printf 'FAIL kill-switch still denies: %s\n' "$off"; failed=1; }
malformed="$(printf '{"tool_name":' | "$HERE/session-kill-guard.sh")"
[ -z "$malformed" ] || { printf 'FAIL malformed payload denied: %s\n' "$malformed"; failed=1; }
printf 'session-kill-guard: %s fixtures through %s, failed=%s\n' "$count" "$runner" "$failed"
printf '%s\n' "$reason"
exit "$failed"
