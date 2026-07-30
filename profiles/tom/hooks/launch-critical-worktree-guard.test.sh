#!/usr/bin/env bash
# Tests for launch-critical-worktree-guard.sh.
#
# The two properties that matter, in order:
#   1. It denies writes to the PRIMARY checkout of a launch-critical repo.
#   2. It never denies anything else — a guard that blocks ordinary work gets
#      switched off, and then it protects nothing. The worktree cases and the
#      ~/code/north-data sibling are the ones that would regress in practice.
set -uo pipefail

HOOK="$(cd "$(dirname "$0")" && pwd)/launch-critical-worktree-guard.sh"
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

# --- 1. primaries are denied -------------------------------------------------
check deny "$HOME/code/fram/main/coord_daemon.clj"       "fram primary: dirty tree makes north up refuse"
check deny "$HOME/code/north/main/cli/dashboard-cli.clj" "north primary"
check deny "$HOME/code/beagle/main/bin/beagle-build"     "beagle primary"
check deny "$HOME/code/nixos-config/main/flake.nix"      "nixos-config primary"
check deny "$HOME/code/fram/main"                        "the checkout root itself, not only files under it"

# --- 2. the sanctioned destinations are NOT denied ---------------------------
check allow "$HOME/code/worktrees/fram/topic/coord_daemon.clj" "durable worktree is where agents are TOLD to work"
check allow "/tmp/north-lane-abc123/cli/x.clj"                 "managed lane worktree"
check allow "/tmp/fram-indexed-show-lane/bin/fram-fast.clj"    "ad-hoc lane worktree"

# --- 2b. gitignored paths are exempt ----------------------------------------
# A gitignored file can never make the tree tracked-dirty, so it cannot cause
# either failure this guard prevents (`north up` refusing a dirty Fram checkout,
# `firn rebuild` snapshotting only committed state). Blocking them bought
# nothing and stopped agents writing docs/private/ notes — which is exactly
# where policy says internal notes belong.
check allow "$HOME/code/north/docs/private/overnight-notes.md" \
  "docs/private is gitignored: cannot dirty the tree, and policy REQUIRES notes there"
check allow "$HOME/code/fram/docs/private/scratch.md" \
  "same exemption in fram"

# ...but a TRACKED file in the same repo is still denied.
check deny "$HOME/code/north/main/cli/trace-cli.clj" \
  "tracked source in a launch-critical primary stays denied"

# --- 3. near-miss paths must not be swept in ---------------------------------
# north-data is a SIBLING of north; a naive prefix test without a separator
# check denies it and breaks all runtime-state writes.
check allow "$HOME/code/north-data/threads/x.md"   "north-data is runtime state, not the north repo"
check allow "$HOME/code/framework/x.clj"           "framework != fram"
check allow "$HOME/code/gjoa/x.clj"                "unrelated project"
check allow "/tmp/scratch.txt"                     "outside ~/code entirely"

check deny "$HOME/code/nixos-config/main/modules/x.nix" "nixos-config primary, the 2026-07-30 near-miss"

# --- 3b. every container's main, on a fixture layout -------------------------
# Dynamic detection: a project this guard has never heard of, its client-nested
# sibling, and the near-misses that must stay writable.
FIXTURE="$(mktemp -d)"
ROOT="$FIXTURE/code"
mkdir -p "$ROOT/proj/main/.git" "$ROOT/proj/wt-x" \
         "$ROOT/client/msa/app/main/.git" "$ROOT/reference/upstream/main/.git" \
         "$ROOT/runtime-data/.git"

check deny  "$ROOT/proj/main/src/x.py"                "an unheard-of project's main"
check deny  "$ROOT/client/msa/app/main/src/x.py"      "a client project's nested main"
check allow "$ROOT/proj/wt-x/src/x.py"                "its worktree is the destination"
check allow "$ROOT/proj/scratch.txt"                  "the container root is not a checkout"
check allow "$ROOT/runtime-data/state.json"           "bare .git, no main/: runtime state stays writable"
check allow "$ROOT/reference/upstream/main/README.md" "reference checkouts are read-only context"

# Bash entrance on the same fixture: WIP destruction vs the landing flow.
case "$(bash_decide "git -C $ROOT/proj/main reset --hard HEAD~1" "$HOME")" in
  *'"deny"'*) pass=$((pass + 1)) ;;
  *) fail=$((fail + 1)); echo "FAIL  reset --hard in a container main must be denied" >&2 ;;
esac
case "$(bash_decide "git stash" "$ROOT/proj/main")" in
  *'"deny"'*) pass=$((pass + 1)) ;;
  *) fail=$((fail + 1)); echo "FAIL  stash in a container main must be denied" >&2 ;;
esac
case "$(bash_decide "git -C $ROOT/proj/wt-x reset --hard HEAD~1" "$HOME")" in
  "") pass=$((pass + 1)) ;;
  *) fail=$((fail + 1)); echo "FAIL  reset --hard in a worktree must be allowed" >&2 ;;
esac
case "$(bash_decide "git -C $ROOT/proj/main merge --ff-only slug && git -C $ROOT/proj/main branch -d slug && git -C $ROOT/proj/main worktree prune && safe-push --to main" "$HOME")" in
  "") pass=$((pass + 1)) ;;
  *) fail=$((fail + 1)); echo "FAIL  the landing flow must run from main" >&2 ;;
esac
# wt-rescue is the sanctioned remediation the deny message points at.
case "$(bash_decide "wt-rescue $ROOT/proj/main" "$HOME")" in
  "") pass=$((pass + 1)) ;;
  *) fail=$((fail + 1)); echo "FAIL  wt-rescue must be allowed against a main" >&2 ;;
esac
case "$(bash_decide "git -C $ROOT/proj/main checkout -- ." "$HOME")" in
  *wt-rescue*) pass=$((pass + 1)) ;;
  *) fail=$((fail + 1)); echo "FAIL  checkout -- in a main must deny and name wt-rescue" >&2 ;;
esac

rm -rf "${FIXTURE:?}"
unset ROOT FIXTURE

# --- 4. fail-open ------------------------------------------------------------
check allow "" "empty file_path"
printf 'not json at all' | AGENT_NO_AUTHORING_HOOKS=0 "$HOOK" >/dev/null 2>&1 \
  && pass=$((pass + 1)) \
  || { fail=$((fail + 1)); echo "FAIL  malformed payload must exit 0" >&2; }

# --- 5. kill-switch ----------------------------------------------------------
out="$(printf '{"tool_input":{"file_path":"%s"}}' "$HOME/code/fram/x.clj" \
       | AGENT_NO_AUTHORING_HOOKS=1 "$HOOK" 2>/dev/null)"
if [ -z "$out" ]; then pass=$((pass + 1)); else
  fail=$((fail + 1)); echo "FAIL  AGENT_NO_AUTHORING_HOOKS=1 must disable the guard" >&2
fi

# --- 6. cost -----------------------------------------------------------------
# This fires on EVERY Edit/Write. An unconditional python3 start measured 4.6ms
# per call; the bash pre-filter must keep the common path well under that.
start=$(date +%s%N)
for _ in $(seq 50); do
  printf '{"tool_input":{"file_path":"/tmp/x.txt"}}' \
    | AGENT_NO_AUTHORING_HOOKS=0 "$HOOK" >/dev/null 2>&1
done
per_ms=$(( ( $(date +%s%N) - start ) / 50 / 1000000 ))
if [ "$per_ms" -le 25 ]; then pass=$((pass + 1)); else
  fail=$((fail + 1)); echo "FAIL  allow path ${per_ms}ms/call — too slow for PreToolUse" >&2
fi

# A path under ~/code with no `main` component cannot be protected either, so
# the second pre-filter stage must keep it off python3 as well.
start=$(date +%s%N)
for _ in $(seq 50); do
  printf '{"tool_input":{"file_path":"%s/code/gjoa/docs/x.md"}}' "$HOME" \
    | AGENT_NO_AUTHORING_HOOKS=0 "$HOOK" >/dev/null 2>&1
done
per_ms=$(( ( $(date +%s%N) - start ) / 50 / 1000000 ))
if [ "$per_ms" -le 25 ]; then pass=$((pass + 1)); else
  fail=$((fail + 1)); echo "FAIL  ~/code non-main path ${per_ms}ms/call — pre-filter regressed" >&2
fi

printf '%s\n' "launch-critical-worktree-guard: $pass passed, $fail failed"
[ "$fail" -eq 0 ]
