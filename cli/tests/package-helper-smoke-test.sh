#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
TMP_ROOT="$(mktemp -d)"
TMP="$TMP_ROOT/state with spaces"
CHECKOUT="$TMP/north checkout"
HOME_DIR="$TMP/home"
HOST_PATH="$PATH"
REAL_STAT="$(command -v stat)"

cleanup() {
  rm -rf -- "${TMP_ROOT:?}"
}
trap cleanup EXIT

mkdir -p "$CHECKOUT/bin" "$HOME_DIR"
cp "$ROOT/bin/north-stream-sync" "$ROOT/bin/north-stream-sync-all" "$CHECKOUT/bin/"

SRC_ROOT="$TMP/source root"
SRC="$SRC_ROOT/project slug"
RAW="$TMP/raw"
SOURCE="$SRC/current-session.jsonl"
mkdir -p "$SRC"
printf '{"type":"current"}\n{"type":"tail"}\n' >"$SOURCE"

sync_current() {
  env -i HOME="$HOME_DIR" PATH="$HOST_PATH" \
    "$CHECKOUT/bin/north-stream-sync" \
      --src-dir "$SRC_ROOT" --raw-dir "$RAW" \
      --provider anthropic --source-namespace test-authority --layout claude
}

sync_current
CURSOR="$(find "$RAW" -maxdepth 1 -type f -name '.cursors.v4.*' -print -quit)"
[[ -n "$CURSOR" ]]
DEST="$RAW/$(cut -f8 "$CURSOR")"
cmp "$SOURCE" "$DEST"
[[ "$($REAL_STAT -c '%a' "$CURSOR")" == 600 ]]
[[ "$($REAL_STAT -c '%a' "$DEST")" == 600 ]]
awk -F'\t' '
  NF != 10 || $1 != "v4" || $2 !~ /^[0-9]+$/ ||
  $4 != "anthropic" || $5 != "test-authority" ||
  $7 !~ /^[0-9a-f]{64}$/ ||
  $8 !~ /^[A-Za-z0-9][A-Za-z0-9._-]*[.]jsonl$/ ||
  $9 !~ /^[0-9a-f]{64}$/ || $10 !~ /^[0-9a-f]{64}$/ { exit 1 }
' "$CURSOR"
CURSOR_HASH="$(sha256sum "$CURSOR")"
sync_current
[[ "$CURSOR_HASH" == "$(sha256sum "$CURSOR")" ]]

printf '{"type":"more"}\n' >>"$SOURCE"
sync_current
cmp "$SOURCE" "$DEST"

MISSING_RAW="$TMP/missing-authority"
if env -i HOME="$HOME_DIR" PATH="$HOST_PATH" \
  "$CHECKOUT/bin/north-stream-sync" \
    --src-dir "$SRC_ROOT" --raw-dir "$MISSING_RAW" >/dev/null 2>&1; then
  echo "sync accepted a source without provider authority" >&2
  exit 1
fi
[[ ! -e "$MISSING_RAW" ]]

LOCK_RAW="$TMP/lock raw"
mkdir -p "$LOCK_RAW/.stream-sync.lock"
if env -i HOME="$HOME_DIR" PATH="$HOST_PATH" \
  "$CHECKOUT/bin/north-stream-sync" \
    --src-dir "$SRC_ROOT" --raw-dir "$LOCK_RAW" \
    --provider anthropic --source-namespace lock-test --layout claude \
    >"$TMP/lock.out" 2>"$TMP/lock.err"; then
  echo "directory lock path was accepted" >&2
  exit 1
fi
grep -Fq "lock path is not a regular file" "$TMP/lock.err"
[[ -d "$LOCK_RAW/.stream-sync.lock" ]]

SHRINK_ROOT="$TMP/shrink source"
SHRINK_PROJECT="$SHRINK_ROOT/project"
SHRINK_RAW="$TMP/shrink raw"
mkdir -p "$SHRINK_PROJECT"
printf 'durable prefix\nand tail\n' >"$SHRINK_PROJECT/session.jsonl"
env -i HOME="$HOME_DIR" PATH="$HOST_PATH" \
  "$CHECKOUT/bin/north-stream-sync" \
    --src-dir "$SHRINK_ROOT" --raw-dir "$SHRINK_RAW" \
    --provider anthropic --source-namespace shrink-test --layout claude
SHRINK_CURSOR="$(find "$SHRINK_RAW" -maxdepth 1 -type f -name '.cursors.v4.*' -print -quit)"
SHRINK_HASH="$(sha256sum "$SHRINK_CURSOR")"
SHRINK_DEST="$SHRINK_RAW/$(cut -f8 "$SHRINK_CURSOR")"
printf 'short' >"$SHRINK_PROJECT/session.jsonl"
if env -i HOME="$HOME_DIR" PATH="$HOST_PATH" \
  "$CHECKOUT/bin/north-stream-sync" \
    --src-dir "$SHRINK_ROOT" --raw-dir "$SHRINK_RAW" \
    --provider anthropic --source-namespace shrink-test --layout claude; then
  echo "shrunk source was accepted" >&2
  exit 1
else
  [[ "$?" -eq 2 ]]
fi
[[ "$SHRINK_HASH" == "$(sha256sum "$SHRINK_CURSOR")" ]]
grep -Fq 'durable prefix' "$SHRINK_DEST"
grep -Fq $'v1\tsource_shrank\t' "$SHRINK_RAW"/.stream-sync-errors.v4.*

XDG_STATE="$TMP/xdg state"
env -i HOME="$HOME_DIR" XDG_STATE_HOME="$XDG_STATE" \
  NORTH_PACKAGE_MODE=nix-store PATH="$HOST_PATH" \
  "$CHECKOUT/bin/north-stream-sync" \
    --src-dir "$SRC_ROOT" --provider anthropic \
    --source-namespace package-test --layout claude
PACKAGED_RAW="$XDG_STATE/north/streams/raw"
PACKAGED_CURSOR="$(find "$PACKAGED_RAW" -maxdepth 1 -type f -name '.cursors.v4.*' -print -quit)"
cmp "$SOURCE" "$PACKAGED_RAW/$(cut -f8 "$PACKAGED_CURSOR")"

# Provider-neutral discovery covers every Codex store plus provider profiles.
# Reusing one rollout basename in two authorities must still yield independent
# raw destinations and cursor ownership.
ALL_HOME="$TMP/all home"
ALL_STATE="$TMP/all state/north"
ALL_RAW="$TMP/all raw"
ALL_AMBIENT="$TMP/ambient codex"
ALL_ACCOUNT="$ALL_STATE/accounts/openai/codex-a/sessions/2026/07/29"
ALL_CLAUDE_A="$ALL_STATE/accounts/anthropic/claude-a/projects/shared-project"
ALL_CLAUDE_B="$ALL_STATE/accounts/anthropic/claude-b/projects/shared-project"
ALL_MANAGED_HOME="$ALL_STATE/managed-codex/north-managed-codex-20260729-x"
ALL_MANAGED="$ALL_MANAGED_HOME/sessions/2026/07/29"
ALL_PROFILE_CODEX="$ALL_STATE/profiles/codex-stock/sessions/2026/07/29"
ALL_PROFILE_CLAUDE="$ALL_STATE/profiles/stock/projects/project-a"
ALL_PROFILE_NESTED="$ALL_STATE/profiles/stock-nested/projects/project-b/session-b/subagents/workflows/wf-a"
ALL_AMBIENT_DAY="$ALL_AMBIENT/sessions/2026/07/29"
ALL_NAME="rollout-2026-07-29T00-00-00-aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa.jsonl"
mkdir -p "$ALL_HOME" "$ALL_ACCOUNT" "$ALL_MANAGED" \
  "$ALL_CLAUDE_A" "$ALL_CLAUDE_B" \
  "$ALL_PROFILE_CODEX" "$ALL_PROFILE_CLAUDE" "$ALL_PROFILE_NESTED" \
  "$ALL_AMBIENT_DAY"
printf '{"authority":"account"}\n' >"$ALL_ACCOUNT/$ALL_NAME"
printf '{"authority":"claude-account-a"}\n' >"$ALL_CLAUDE_A/same-session.jsonl"
printf '{"authority":"claude-account-b"}\n' >"$ALL_CLAUDE_B/same-session.jsonl"
printf '{"authority":"managed"}\n' >"$ALL_MANAGED/$ALL_NAME"
printf '{"authority":"profile-codex"}\n' \
  >"$ALL_PROFILE_CODEX/profile-codex.jsonl"
printf '{"authority":"profile-claude"}\n' \
  >"$ALL_PROFILE_CLAUDE/profile-claude.jsonl"
printf '{"authority":"profile-claude-subagent"}\n' \
  >"$ALL_PROFILE_NESTED/agent-nested.jsonl"
printf '{"authority":"ambient"}\n' >"$ALL_AMBIENT_DAY/ambient.jsonl"
printf '{"threadId":"managed-thread"}\n' \
  >"$ALL_MANAGED_HOME/north-launch.json"
# A sessions-root decoy is not a rollout partition and must not be ingested.
printf '{"decoy":true}\n' \
  >"$ALL_STATE/accounts/openai/codex-a/sessions/history.jsonl"

env -i HOME="$ALL_HOME" PATH="$HOST_PATH" \
  NORTH_STATE_ROOT="$ALL_STATE" NORTH_AMBIENT_CODEX_HOME="$ALL_AMBIENT" \
  "$CHECKOUT/bin/north-stream-sync-all" --raw-dir "$ALL_RAW"

mapfile -t all_cursors < <(find "$ALL_RAW" -maxdepth 1 -type f \
  -name '.cursors.v4.*' | sort)
[[ "${#all_cursors[@]}" -eq 8 ]]
[[ "$(find "$ALL_RAW" -maxdepth 1 -type f -name '*.jsonl' | wc -l)" -eq 8 ]]
for cursor in "${all_cursors[@]}"; do
  [[ "$(awk -F'\t' '{print NF}' "$cursor")" -eq 10 ]]
  all_source="$(cut -f3 "$cursor")"
  all_dest="$ALL_RAW/$(cut -f8 "$cursor")"
  cmp "$all_source" "$all_dest"
done
[[ -f "$ALL_MANAGED_HOME/north-stream-mirrored" ]]
[[ "$("$REAL_STAT" -c '%a' "$ALL_MANAGED_HOME/north-stream-mirrored")" == 600 ]]
grep -Fq '"managed-thread"' "$ALL_RAW"/source-receipt.*.json

# An unchanged second pass must use the metadata fast path: hashing a dormant
# transcript prefix would invoke head and make this probe fail.
ALL_FAST_SHIM="$TMP/all fast shim"
mkdir -p "$ALL_FAST_SHIM"
cat >"$ALL_FAST_SHIM/head" <<'EOF'
#!/usr/bin/env bash
echo "provider no-op unexpectedly rehashed transcript bytes" >&2
exit 99
EOF
chmod +x "$ALL_FAST_SHIM/head"
all_cursor_hash="$(
  for cursor in "${all_cursors[@]}"; do sha256sum "$cursor"; done |
    sha256sum
)"
env -i HOME="$ALL_HOME" PATH="$ALL_FAST_SHIM:$HOST_PATH" \
  NORTH_STATE_ROOT="$ALL_STATE" NORTH_AMBIENT_CODEX_HOME="$ALL_AMBIENT" \
  "$CHECKOUT/bin/north-stream-sync-all" --raw-dir "$ALL_RAW"
[[ "$all_cursor_hash" == "$(
  for cursor in "${all_cursors[@]}"; do sha256sum "$cursor"; done |
    sha256sum
)" ]]

for cursor in "${all_cursors[@]}"; do
  printf '{"appended":true}\n' >>"$(cut -f3 "$cursor")"
done
env -i HOME="$ALL_HOME" PATH="$HOST_PATH" \
  NORTH_STATE_ROOT="$ALL_STATE" NORTH_AMBIENT_CODEX_HOME="$ALL_AMBIENT" \
  "$CHECKOUT/bin/north-stream-sync-all" --raw-dir "$ALL_RAW"
for cursor in "${all_cursors[@]}"; do
  cmp "$(cut -f3 "$cursor")" "$ALL_RAW/$(cut -f8 "$cursor")"
done

# TERM must interrupt the parent while its child copy is still blocked, then
# forward TERM to that child. This is the service-stop shape: completion here
# means systemd need not escalate to SIGKILL after TimeoutStopSec.
TERM_HOME="$TMP/term home"
TERM_READY="$TMP/term-ready"
TERM_SYNC="$TMP/term-sync"
mkdir -p "$TERM_HOME/.claude/projects/project"
cat >"$TERM_SYNC" <<'EOF'
#!/usr/bin/env bash
trap 'exit 0' TERM INT
: >"$STREAM_SYNC_TERM_READY"
while :; do sleep 1; done
EOF
chmod +x "$TERM_SYNC"
env HOME="$TERM_HOME" PATH="$HOST_PATH" \
  NORTH_STREAM_SYNC_BIN="$TERM_SYNC" STREAM_SYNC_TERM_READY="$TERM_READY" \
  "$CHECKOUT/bin/north-stream-sync-all" &
term_pid=$!
for _ in $(seq 1 100); do
  [[ -e "$TERM_READY" ]] && break
  sleep 0.01
done
[[ -e "$TERM_READY" ]]
term_started="$(date +%s%N)"
kill -TERM "$term_pid"
while kill -0 "$term_pid" 2>/dev/null; do
  [[ $(( $(date +%s%N) - term_started )) -lt 5000000000 ]]
  sleep 0.01
done
wait "$term_pid" 2>/dev/null || term_status=$?
[[ "${term_status:-0}" -eq 143 ]]

# Restart after a TERM-stop still advances the persisted cursor exactly once.
printf '{"type":"cursor-before-restart"}\n' >"$TERM_HOME/.claude/projects/project/restart.jsonl"
env HOME="$TERM_HOME" PATH="$HOST_PATH" \
  "$CHECKOUT/bin/north-stream-sync-all"
term_raw="$CHECKOUT/streams/raw"
term_cursor_before="$(
  awk -F'\t' '$3 ~ /restart[.]jsonl$/ { print $2 }' \
    "$term_raw"/.cursors.v4.*
)"
printf '{"type":"cursor-after-restart"}\n' >>"$TERM_HOME/.claude/projects/project/restart.jsonl"
env HOME="$TERM_HOME" PATH="$HOST_PATH" \
  "$CHECKOUT/bin/north-stream-sync-all"
term_dest="$term_raw/$(
  awk -F'\t' '$3 ~ /restart[.]jsonl$/ { print $8 }' \
    "$term_raw"/.cursors.v4.*
)"
term_cursor_after="$(
  awk -F'\t' '$3 ~ /restart[.]jsonl$/ { print $2 }' \
    "$term_raw"/.cursors.v4.*
)"
[[ "$term_cursor_after" -gt "$term_cursor_before" ]]
cmp "$TERM_HOME/.claude/projects/project/restart.jsonl" "$term_dest"

echo "package helper smoke tests: PASS"
