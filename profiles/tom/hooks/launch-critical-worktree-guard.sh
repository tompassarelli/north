#!/usr/bin/env bash
# PreToolUse guard — agents edit launch-critical repos in a worktree, never the
# primary checkout.
# ============================================================================
# STATUS: LIVE. Wired into settings.json (PreToolUse, Edit|Write|MultiEdit|Bash).
#
# MECHANISM: Bash carries tool_input.command, Edit/Write/MultiEdit carry
# tool_input.file_path — both are inspected so every entrance is covered.
#
# `north up` refuses to launch on a tracked-dirty Fram checkout: a coordinator
# serving a half-edited engine is worse than one that refuses. Policy lives in
# ~/code/AGENTS.md ("Launch-critical repos"); enforcement lives here —
# PreToolUse is the only event that can refuse a call before the write lands.
#
# SCOPE — deliberately narrow, and it must stay narrow
#   Denies ONLY a write whose realpath sits inside one of the PRIMARY checkouts
#   named in LAUNCH_CRITICAL below. Every other path returns empty stdout, which
#   the harness reads as "no opinion".
#
#   Worktrees are the sanctioned destination and must never be caught: a
#   worktree lives at ~/code/<project>/wt-<slug> and is carved out by name, so
#   the rule holds both today (checkout at ~/code/<project>) and after the move
#   to ~/code/<project>/main. Nothing to keep in sync, nothing to sequence.
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

# Cheap bash pre-filter — this runs on EVERY Edit/Write, so the overwhelmingly
# common case (a path in none of these repos) must not pay a python3 startup.
#
# Safe because it only ever SKIPS work: if not one of these literals appears
# anywhere in the payload, no file_path under those roots can be present either,
# so the precise check below could not have denied. Worktree paths
# (~/code/worktrees/fram/...) do not contain "/code/fram", so they stay cheap too.
case "$payload" in
  */code/fram*|*/code/north*|*/code/beagle*|*/code/nixos-config*) ;;
  *) exit 0 ;;
esac

# A GITIGNORED path can never make the tree tracked-dirty, so it is exempt:
# `north up` refuses on a tracked-dirty Fram checkout, and `firn rebuild`
# snapshots COMMITTED state — neither failure mode applies to an ignored path.
#
# Runs only after the pre-filter has already matched one of the four repos, so
# the git call is off the common path. Fail-open: if git is unavailable or
# errors, fall through to the precise check below rather than admitting blindly.
if command -v git >/dev/null 2>&1; then
  _lc_path="$(printf '%s' "$payload" | python3 -c 'import json,sys
try: print((json.load(sys.stdin).get("tool_input") or {}).get("file_path") or "")
except Exception: print("")' 2>/dev/null)"
  if [ -n "$_lc_path" ] && git -C "$(dirname "$_lc_path")" check-ignore -q "$_lc_path" 2>/dev/null; then
    exit 0
  fi
  unset _lc_path
fi

command -v python3 >/dev/null 2>&1 || exit 0

# The decision lives in lib/launch_critical_decide.py so it can be tested
# directly (launch-critical-worktree-guard.test.py) rather than through a
# heredoc. Fail-open on any error, as everywhere else in this guard.
printf '%s' "$payload" | python3 "$(dirname "$0")/lib/launch_critical_decide.py" 2>/dev/null || exit 0
exit 0
