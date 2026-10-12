#!/usr/bin/env bash
# Tests for launch-critical-worktree-guard.sh.
#
# The two properties that matter, in order:
#   1. It denies writes to the PRIMARY checkout of a launch-critical repo.
#   2. It never denies anything else — a guard that blocks ordinary work gets
#      switched off, and then it protects nothing. The worktree cases and the
#      ~/code/north-data sibling are the ones that would regress in practice.
set -uo pipefail
# Hooks run with the managed hook runtime first on PATH; so do their tests.
export PATH="/etc/codex/hooks/runtime:$PATH"

HOOK="$(cd "$(dirname "$0")" && pwd)/launch-critical-worktree-guard.sh"
python3 "$(dirname "$HOOK")/lib/shellcmd.property.py" --launch || exit $?
ERRORS="$(mktemp)"
export AGENT_HOOK_ERRORS="$ERRORS"
pass=0 fail=0

# Guards are OFF in harness.conf on this machine, so every case forces them live
# with AGENT_NO_AUTHORING_HOOKS=0. That is the force-live value, not the
# disable value — see lib/authoring-killswitch.sh.
decide() {
  printf '{"tool_input":{"file_path":"%s"}}' "$1" \
    | AGENT_NO_AUTHORING_HOOKS=0 LAUNCH_CRITICAL_CODE_ROOT="${ROOT:-}" "$HOOK" 2>/dev/null
}

bash_decide() { # bash_decide <command> <cwd>
  printf '{"tool_name":"Bash","tool_input":{"command":"%s"},"cwd":"%s"}' "$1" "$2" \
    | AGENT_NO_AUTHORING_HOOKS=0 LAUNCH_CRITICAL_CODE_ROOT="${ROOT:-}" "$HOOK" 2>/dev/null
}

apply_decide() { # apply_decide <envelope-with-\n> <cwd>
  printf '{"tool_name":"apply_patch","tool_input":{"input":"%s"},"cwd":"%s"}' "$1" "$2" \
    | AGENT_NO_AUTHORING_HOOKS=0 LAUNCH_CRITICAL_CODE_ROOT="${ROOT:-}" "$HOOK" 2>/dev/null
}

check() { # check <expect deny|allow> <path> <why>
  local want=$1 path=$2 why=$3 out got
  out="$(decide "$path")"
  case "$out" in
    *'"permissionDecision": "deny"'*|*'"permissionDecision":"deny"'*) got=deny ;;
    "") got=allow ;;
    *) got="malformed:$out" ;;
  esac
  if [ "$got" = "$want" ]; then
    pass=$((pass + 1))
  else
    fail=$((fail + 1))
    printf 'FAIL  want=%s got=%s  %s\n      %s\n' "$want" "$got" "$path" "$why" >&2
  fi
}

# --- 3b. every container's main, on a fixture layout -------------------------
# Dynamic detection: a project this guard has never heard of, its client-nested
# sibling, and the near-misses that must stay writable. The container's three
# slots are all present, because the whole rule is positional.
FIXTURE="$(mktemp -d)"
ROOT="$FIXTURE/code"
PIN_OID=0123456789abcdef0123456789abcdef01234567
PIN_SIDECAR="$ROOT/proj/pins/$PIN_OID.pin"
mkdir -p "$ROOT/proj/main/.git" "$ROOT/proj/worktrees/x" "$ROOT/proj/worktrees/main" \
         "$ROOT/proj/pins/$PIN_OID" \
         "$ROOT/client/acme/app/main/.git" "$ROOT/resources/upstream/main/.git" \
         "$ROOT/runtime-data/.git"
printf 'The vendored upstream checkout. Consumers: gjoa:.envrc, the docs build.\n' \
  > "$PIN_SIDECAR"
# A real repo inside the pin: `git check-ignore` needs one for the T6 case, and
# both live pins are exactly this shape — full of ignored build artifacts.
git init -q "$ROOT/proj/pins/$PIN_OID" 2>/dev/null
printf 'build/\n' > "$ROOT/proj/pins/$PIN_OID/.gitignore"
mkdir -p "$ROOT/proj/pins/$PIN_OID/build"

check deny  "$ROOT/proj/main/src/x.py"                "an unheard-of project's main"
check deny  "$ROOT/client/acme/app/main/src/x.py"      "a client project's nested main"
check allow "$ROOT/proj/worktrees/x/src/x.py"         "a lane is the destination (T1)"
check allow "$ROOT/proj/worktrees/main/x.py"          "a lane whose slug is literally 'main' is still a lane (T8)"
check allow "$ROOT/proj/scratch.txt"                  "the container root is not a checkout"
check allow "$ROOT/proj/worktrees"                    "the worktrees/ root itself must stay writable (T5)"
check allow "$ROOT/proj/pins"                         "the pins/ root itself must stay writable (T5)"
check deny  "$ROOT/proj/pins/$PIN_OID/index.html"     "a content-addressed pin is externally consumed (T3)"
check allow "$ROOT/proj/pins/$PIN_OID.pin"            "the manifest is agent-writable metadata (T7, AMB-6 as amended)"
check deny  "$ROOT/proj/pins/$PIN_OID/nested.pin"     "a .pin path inside the checkout is content, still denied"
check deny  "$ROOT/proj/pins/$PIN_OID/build/out.js"   "gitignore does NOT exempt a pin (T6)"
check allow "$ROOT/runtime-data/state.json"           "bare .git, no main/: runtime state stays writable"
check allow "$ROOT/resources/upstream/main/README.md" "resource checkouts are read-only context"

rm -rf "${FIXTURE:?}"
unset ROOT FIXTURE pin_out

# This runs without LAUNCH_CRITICAL_CODE_ROOT so it proves the production
# pre-filter admits the demonstrated root-level Cargo spill to the decoder.
case "$(bash_decide "CARGO_TARGET_DIR=$HOME/code/clause/target-agent-a cargo test" "$HOME")" in
  *'"deny"'*) pass=$((pass + 1)) ;;
  *) fail=$((fail + 1)); echo "FAIL  project-container CARGO_TARGET_DIR must be denied" >&2 ;;
esac
case "$(bash_decide "cargo test --target-dir $HOME/code/clause/target-agent-b" "$HOME")" in
  *'"deny"'*) pass=$((pass + 1)) ;;
  *) fail=$((fail + 1)); echo "FAIL  project-container --target-dir must be denied" >&2 ;;
esac
case "$(bash_decide "CARGO_TARGET_DIR=$HOME/code/clause/worktrees/lane/target cargo test" "$HOME")" in
  "") pass=$((pass + 1)) ;;
  *) fail=$((fail + 1)); echo "FAIL  lane-local CARGO_TARGET_DIR must remain allowed" >&2 ;;
esac
case "$(bash_decide "cargo test --target-dir /tmp/clause-target-agent-a" "$HOME")" in
  "") pass=$((pass + 1)) ;;
  *) fail=$((fail + 1)); echo "FAIL  /tmp Cargo target must remain allowed" >&2 ;;
esac

case "$(apply_decide "*** Begin Patch\n*** Update File: code/north/main/cli/x.clj\n@@\n-a\n+b\n*** End Patch" "/home/tom")" in
  *'"deny"'*) pass=$((pass + 1)) ;;
  *) fail=$((fail + 1)); echo "FAIL  relative apply_patch must bypass the cheap pre-filter" >&2 ;;
esac

# --- 4. fail-open ------------------------------------------------------------
check allow "" "empty file_path"
printf 'not json at all' | AGENT_NO_AUTHORING_HOOKS=0 "$HOOK" >/dev/null 2>&1 \
  && pass=$((pass + 1)) \
  || { fail=$((fail + 1)); echo "FAIL  malformed payload must exit 0" >&2; }

# --- 5. kill-switch ----------------------------------------------------------
# The path must be one the guard WOULD deny, or the case passes for the wrong
# reason: `~/code/clause/x.rkt` is the container root and is not protected.
out="$(printf '{"tool_input":{"file_path":"%s"}}' "$HOME/code/clause/main/x.rkt" \
       | AGENT_NO_AUTHORING_HOOKS=1 "$HOOK" 2>/dev/null)"
if [ -z "$out" ]; then pass=$((pass + 1)); else
  fail=$((fail + 1)); echo "FAIL  AGENT_NO_AUTHORING_HOOKS=1 must disable the guard" >&2
fi

# Injected fault: a decider exception allows silently on the wire but must
# leave exactly one errors.tsv row.
: >"$ERRORS"
out="$(printf '{"tool_name":"Edit","tool_input":["%s/code/x/main"]}' "$HOME" \
  | AGENT_NO_AUTHORING_HOOKS=0 "$HOOK" 2>&1)"; rc=$?
error_kind="$(cut -f 3 "$ERRORS")"
if [ "$rc" -eq 0 ] && [ -z "$out" ] && [ "$error_kind" = decider-exception ]; then pass=$((pass + 1)); else
  fail=$((fail + 1)); echo "FAIL  forced decider exception: exit=$rc out=$out kind=$error_kind" >&2
fi
rm -f "$ERRORS"

printf '%s\n' "launch-critical-worktree-guard: $pass passed, $fail failed"
[ "$fail" -eq 0 ]
