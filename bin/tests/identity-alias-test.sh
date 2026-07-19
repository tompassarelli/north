#!/usr/bin/env bash
# Regression test for the presence-identity ALIASING bug (thread 019f2496-23f2,
# 2026-07-03): a parent session's NORTH_AGENT_ID env pin leaked through the process
# environment into its Claude subagents (SubagentSessionStart inherits the parent
# env but carries the subagent's OWN session_id). Both spawn + tooluse preferred the
# env pin over the per-session cache, so every subagent aliased the parent id — the
# roster, concern ledger, and peer-mail inbox all attributed several workstreams to
# one name, and mail was answered by whichever actor peeked first (cc-fram-d5523b3b,
# under the incident-era cc- prefix; native ids now carry a full typed digest).
#
# INVARIANT under test: one live actor == one id. A given id is renewed/registered
# ONLY by the session that first acquired it. An env pin is honored only when no OTHER
# session already owns it; an inherited pin yields a distinct per-sid id instead.
#
# Fully hermetic + fast: isolates XDG_RUNTIME_DIR (the id cache) into a temp dir and
# shims `bb` on PATH to record the id each hook registers — no JVM, no :7977, no net.
#   ./identity-alias-test.sh
set -uo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
BIN="$(cd "$HERE/.." && pwd)"
SPAWN="$BIN/north-on-spawn"
TOOLUSE="$BIN/north-on-tooluse"
ACTOR_KEY="$BIN/north-actor-key"

TMP="$(mktemp -d)"
trap 'rm -rf -- "${TMP:?}"' EXIT
export XDG="$TMP/xdg"; mkdir -p "$XDG/north-agent-ids"
CACHE="$XDG/north-agent-ids"
export BB_REG_LOG="$TMP/registered.log"; : > "$BB_REG_LOG"

# --- bb shim: record the id passed to `presence-cli register`, swallow inbox-peek ---
SHIM="$TMP/shim"; mkdir -p "$SHIM"
cat > "$SHIM/bb" <<'EOF'
#!/usr/bin/env bash
# test double for `bb`: log the id from a `... register <ID> ...` call; else no-op.
a=("$@")
for ((i=0; i<${#a[@]}; i++)); do
  if [ "${a[$i]}" = "register" ]; then printf '%s\n' "${a[$((i+1))]}" >> "$BB_REG_LOG"; fi
done
exit 0
EOF
chmod +x "$SHIM/bb"

# fram-named non-git cwd -> hooks fall back to REPO=cwd, RN=fram (mirrors the incident)
REPO_DIR="$TMP/fram"; mkdir -p "$REPO_DIR"

PASS=0; FAIL=0
ok()   { PASS=$((PASS+1)); printf '  ok   %s\n' "$1"; }
bad()  { FAIL=$((FAIL+1)); printf '  FAIL %s\n     expected [%s] got [%s]\n' "$1" "$2" "$3"; }
eq()   { if [ "$2" = "$3" ]; then ok "$1"; else bad "$1" "$2" "$3"; fi; }
ne()   { if [ "$2" != "$3" ]; then ok "$1"; else bad "$1" "not $2" "$3"; fi; }

json() {
  if [ -n "${4:-}" ]; then
    printf '{"session_id":"%s","cwd":"%s","hook_event_name":"%s","agent_id":"%s"}' "$1" "$2" "$3" "$4"
  else
    printf '{"session_id":"%s","cwd":"%s","hook_event_name":"%s"}' "$1" "$2" "$3"
  fi
}

# run a hook with a controlled env; returns the id it registered (via the shim).
# usage: reg=$(run_hook <hookpath> <sid> <evt> [PIN] [AGENT_ID])
run_hook() {
  local hook="$1" sid="$2" evt="$3" pin="${4:-}" agent_id="${5:-}"
  : > "$BB_REG_LOG"
  if [ -n "$pin" ]; then
    json "$sid" "$REPO_DIR" "$evt" "$agent_id" | env -i HOME="$HOME" PATH="$SHIM:$PATH" \
      XDG_RUNTIME_DIR="$XDG" BB_REG_LOG="$BB_REG_LOG" NORTH_PORT=1 \
      NORTH_AGENT_ID="$pin" bash "$hook" >/dev/null 2>&1
  else
    json "$sid" "$REPO_DIR" "$evt" "$agent_id" | env -i HOME="$HOME" PATH="$SHIM:$PATH" \
      XDG_RUNTIME_DIR="$XDG" BB_REG_LOG="$BB_REG_LOG" NORTH_PORT=1 \
      bash "$hook" >/dev/null 2>&1
  fi
  # PostToolUse renewals are asynchronous and singleflight-coalesced. Lock
  # removal is the completion signal; wait for it before reading the shim log.
  for _ in $(seq 1 100); do
    if ! find "$XDG" -type d -name '*.lock' -print -quit 2>/dev/null | grep -q .; then
      break
    fi
    sleep 0.01
  done
  tail -n1 "$BB_REG_LOG" 2>/dev/null || true
}
key_of() { "$ACTOR_KEY" "$1" "$2"; }
id_of() { printf 'native-%s' "$(key_of "$1" "$2")"; }
cache_of() { cat "$CACHE/$(key_of "$1" "$2")" 2>/dev/null || true; }

PARENT="d5523b3b-47bb-446d-b5ae-f08ff8c0eba4"
SUB="aaaa1111-0000-4000-8000-000000000001"
SUB2="bbbb2222-0000-4000-8000-000000000002"
FRESH="cccc3333-0000-4000-8000-000000000003"
NOCACHE="dddd4444-0000-4000-8000-000000000004"
PARENT_ID="$(id_of session "$PARENT")"
SUB_ID="$(id_of session "$SUB")"
SUB2_ID="$(id_of session "$SUB2")"

echo "== 1. parent SessionStart (no env pin) acquires its derived id =="
reg="$(run_hook "$SPAWN" "$PARENT" SessionStart)"
eq "parent registers its full provider identity" "$PARENT_ID" "$reg"
eq "parent cache file == its id"        "$PARENT_ID" "$(cache_of session "$PARENT")"

echo "== 2. subagent SubagentSessionStart w/ INHERITED env pin gets its OWN id =="
# The subagent inherits the parent's full pin but has its own session_id.
reg="$(run_hook "$SPAWN" "$SUB" SubagentSessionStart "$PARENT_ID")"
ne "subagent does NOT alias parent id"  "$PARENT_ID" "$reg"
eq "subagent gets own derived id"       "$SUB_ID" "$reg"
eq "subagent cache file == own id"      "$SUB_ID" "$(cache_of session "$SUB")"
eq "parent cache untouched"             "$PARENT_ID" "$(cache_of session "$PARENT")"

echo "== 2b. provider SubagentStart parent session_id + unique agent_id stays distinct =="
NATIVE_AGENT="agent-eeee5555-0000-4000-8000-000000000005"
NATIVE_ID="$(id_of agent "$NATIVE_AGENT")"
reg="$(run_hook "$SPAWN" "$PARENT" SubagentStart "$PARENT_ID" "$NATIVE_AGENT")"
eq "native subagent derives from full agent_id, not parent session_id" "$NATIVE_ID" "$reg"
eq "native subagent cache is keyed by agent_id" "$NATIVE_ID" "$(cache_of agent "$NATIVE_AGENT")"
reg="$(run_hook "$TOOLUSE" "$PARENT" PostToolUse "$PARENT_ID" "$NATIVE_AGENT")"
eq "native subagent tooluse renews its full agent_id row" "$NATIVE_ID" "$reg"

echo "== 3. dispatch-style fresh process: env pin, NO prior owner -> keeps pin =="
reg="$(run_hook "$SPAWN" "$FRESH" SessionStart "sdk-custom-xyz")"
eq "fresh process keeps its env pin"    "sdk-custom-xyz" "$reg"
eq "fresh cache file == pinned id"      "sdk-custom-xyz" "$(cache_of session "$FRESH")"

echo "== 4. tooluse renews the CACHE id even when env pin is set (cache > env) =="
# The subagent still inherits the parent env pin.
reg="$(run_hook "$TOOLUSE" "$SUB" PostToolUse "$PARENT_ID")"
eq "tooluse renews cached subagent id"  "$SUB_ID" "$reg"
ne "tooluse ignores inherited env pin"  "$PARENT_ID" "$reg"

echo "== 5. tooluse env FALLBACK when no cache (SDK-dispatched, spawn hook unfired) =="
reg="$(run_hook "$TOOLUSE" "$NOCACHE" PostToolUse "sdk-fallback")"
eq "tooluse falls back to env pin"      "sdk-fallback" "$reg"

echo "== 6. concurrent siblings both inherit pin -> both derive DISTINCT ids =="
# Two siblings spawn with the inherited parent pin.
r1="$(run_hook "$SPAWN" "$SUB"  SubagentSessionStart "$PARENT_ID")"
r2="$(run_hook "$SPAWN" "$SUB2" SubagentSessionStart "$PARENT_ID")"
ne "sibling A not aliased to parent"    "$PARENT_ID" "$r1"
ne "sibling B not aliased to parent"    "$PARENT_ID" "$r2"
ne "siblings not aliased to each other" "$r1" "$r2"
eq "sibling A own id"                   "$SUB_ID" "$r1"
eq "sibling B own id"                   "$SUB2_ID" "$r2"

echo "== 7. matching eight-character prefixes retain full collision entropy =="
COLLIDE_A="agent-deadbeef-0000-4000-8000-000000000001"
COLLIDE_B="agent-deadbeef-0000-4000-8000-000000000002"
COLLIDE_A_ID="$(id_of agent "$COLLIDE_A")"
COLLIDE_B_ID="$(id_of agent "$COLLIDE_B")"
c1="$(run_hook "$SPAWN" "$PARENT" SubagentStart "$PARENT_ID" "$COLLIDE_A")"
c2="$(run_hook "$SPAWN" "$PARENT" SubagentStart "$PARENT_ID" "$COLLIDE_B")"
eq "first colliding-prefix actor keeps its complete ID" "$COLLIDE_A_ID" "$c1"
eq "second colliding-prefix actor keeps its complete ID" "$COLLIDE_B_ID" "$c2"
ne "matching readable prefixes cannot alias control identity" "$c1" "$c2"

echo "== 8. sanitized aliases, namespaces, and max-size actors remain distinct =="
PUNCT_A="actor:a"
PUNCT_B="actor_a"
punct_a_id="$(id_of agent "$PUNCT_A")"
punct_b_id="$(id_of agent "$PUNCT_B")"
p1="$(run_hook "$SPAWN" "$PARENT" SubagentStart "$PARENT_ID" "$PUNCT_A")"
p2="$(run_hook "$SPAWN" "$PARENT" SubagentStart "$PARENT_ID" "$PUNCT_B")"
eq "colon actor resolves to its full digest" "$punct_a_id" "$p1"
eq "underscore actor resolves to its full digest" "$punct_b_id" "$p2"
ne "sanitize-equivalent raw actors cannot alias" "$p1" "$p2"

SAME_RAW="same-actor-bytes"
session_same="$(run_hook "$SPAWN" "$SAME_RAW" SessionStart)"
agent_same="$(run_hook "$SPAWN" "$PARENT" SubagentStart "" "$SAME_RAW")"
eq "session namespace owns its typed digest" "$(id_of session "$SAME_RAW")" "$session_same"
eq "agent namespace owns its typed digest" "$(id_of agent "$SAME_RAW")" "$agent_same"
ne "equal raw agent/session bytes are domain-separated" "$session_same" "$agent_same"

MAX_ACTOR="$(printf 'a%.0s' $(seq 1 512))"
max_id="$(run_hook "$SPAWN" "$PARENT" SubagentStart "" "$MAX_ACTOR")"
eq "512-byte actor derives a bounded full-digest ID" "$(id_of agent "$MAX_ACTOR")" "$max_id"
eq "512-byte actor cache uses a NAME_MAX-safe key" "$max_id" "$(cache_of agent "$MAX_ACTOR")"

DOT_ACTOR=".."
DOT_ID="$(run_hook "$SPAWN" "$PARENT" SubagentStart "" "$DOT_ACTOR")"
eq "dot path-component bytes are content, never a filesystem path" \
  "$(id_of agent "$DOT_ACTOR")" "$DOT_ID"
eq "dot actor cache is stored only under its bounded digest" \
  "$DOT_ID" "$(cache_of agent "$DOT_ACTOR")"
if "$ACTOR_KEY" agent '../escape' >/dev/null 2>&1; then
  bad "slash-bearing path component is rejected before filesystem use" "rejected" "accepted"
else
  ok "slash-bearing path component is rejected before filesystem use"
fi

echo
echo "identity-alias-test: $PASS passed, $FAIL failed"
[ "$FAIL" -eq 0 ]
