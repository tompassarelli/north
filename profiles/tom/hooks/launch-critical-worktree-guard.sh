#!/usr/bin/env bash
# PreToolUse guard — agents edit in a worktree, never in a `main` checkout.
# ============================================================================
# STATUS: LIVE. Wired into settings.json (PreToolUse, Edit|Write|MultiEdit|Bash).
#
# MECHANISM: Bash carries tool_input.command, Edit/Write/MultiEdit carry
# tool_input.file_path — both are inspected so every entrance is covered.
# apply_patch carries targets in its envelope; tool-call and shell entrances are
# both parsed.
#
# Policy lives in ~/code/AGENTS.md (repository layout, launch-critical repos);
# enforcement lives here — PreToolUse is the only event that can refuse a call
# before the write lands.
#
# SCOPE
#   Denies a write whose realpath sits inside a protected `main` checkout — see
#   lib/launch_critical_paths.py for the derivation — and, separately, a git
#   call that would discard uncommitted work in one. Every other path returns
#   empty stdout, which the harness reads as "no opinion".
#
#   Worktrees are the sanctioned destination and must never be caught: a
#   worktree lives at ~/code/<project>/wt-<slug> and is carved out by name.
#   `wt-rescue`, the remediation the deny message recommends, is allowlisted for
#   the same reason — see SANCTIONED_TOOLS in lib/launch_critical_decide.py.
#
#   The human is unaffected: they edit through their editor, not through the
#   harness's Edit/Write tools, so this code never runs for them.
#
# FAIL-OPEN
#   Missing python3, unreadable payload, unresolvable path, malformed JSON, or
#   any unexpected error -> print nothing, exit 0, edit proceeds. A guard that
#   blocks work when it is itself broken is worse than the leak it prevents.
#
# KILL-SWITCH (opt-out, consistent with the other authoring guards)
#   `north config guards off`, or launch with AGENT_NO_AUTHORING_HOOKS set to
#   anything but 0/false. Do NOT un-wire it from settings.json.
# ============================================================================
set -uo pipefail

# Drain stdin before any decision — an un-drained payload can block the writer.
payload=""
while :; do
  chunk=""
  IFS= read -r -N 65536 chunk
  status=$?
  [ -n "$chunk" ] && payload+="$chunk"
  [ "$status" -eq 0 ] || break
done

# Kill-switch — the shared implementation, so this guard can never disagree with
# what `north config guards` reports. Sourced, never shelled out to: this runs on
# EVERY Edit/Write, and a north CLI call here would add per-tool subprocess cost.
# shellcheck disable=SC1090,SC1091
. "$(dirname "$0")/lib/authoring-killswitch.sh" 2>/dev/null || true
type authoring_guards_off >/dev/null 2>&1 && authoring_guards_off && exit 0

# Cheap bash pre-filter — this runs on EVERY Edit/Write, so a payload that
# cannot possibly name a protected checkout must not pay a python3 startup.
# Safe because a protected path is either under a `main` component or inside a
# pre-migration launch-critical container. Skipped when a test fixture root is
# in play.
if [ -z "${LAUNCH_CRITICAL_CODE_ROOT:-}" ]; then
  case "$payload" in
    *apply_patch*|*'Begin Patch'*) ;;   # envelope paths may be relative; python must resolve them
    *)
      case "$payload" in
        */code/*) ;;
        *) exit 0 ;;
      esac
      case "$payload" in
        *main*|*/code/fram*|*/code/north*|*/code/beagle*|*/code/nixos-config*) ;;
        *) exit 0 ;;
      esac
      ;;
  esac
fi

command -v python3 >/dev/null 2>&1 || exit 0

# The decision lives in lib/launch_critical_decide.py so it can be tested
# directly (launch-critical-worktree-guard.test.py) rather than through a
# heredoc. Fail-open on any error, as everywhere else in this guard.
decision="$(printf '%s' "$payload" | python3 "$(dirname "$0")/lib/launch_critical_decide.py" 2>/dev/null)" || exit 0
[ -n "$decision" ] || exit 0

# A GITIGNORED path can never make the tree tracked-dirty and holds no human
# WIP worth protecting, so it is exempt. Checked only on the deny path, where
# the git subprocess costs nothing that matters. Fail-open on git errors.
if command -v git >/dev/null 2>&1; then
  _lc_path="$(printf '%s' "$payload" | python3 -c 'import json,sys
try: print((json.load(sys.stdin).get("tool_input") or {}).get("file_path") or "")
except Exception: print("")' 2>/dev/null)"
  if [ -n "$_lc_path" ] && git -C "$(dirname "$_lc_path")" check-ignore -q "$_lc_path" 2>/dev/null; then
    exit 0
  fi
  unset _lc_path
fi

printf '%s\n' "$decision"
exit 0
