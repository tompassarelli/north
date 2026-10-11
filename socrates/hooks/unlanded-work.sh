#!/usr/bin/env bash
# Claude Code SessionStart hook: one line naming how many unlanded lanes
# (worktrees or branches with work not on origin/main) are older than 24 h in
# the hourly lane-gc report, so an agent continues one instead of redoing it.
set -uo pipefail

report="${LANE_GC_UNLANDED:-$HOME/.local/state/agents/lane-gc/unlanded.txt}"

authoring_killswitch="$(dirname "$0")/lib/authoring-killswitch.sh"
[ -r "$authoring_killswitch" ] \
  || authoring_killswitch="$(dirname "$0")/../lib/authoring-killswitch.sh"
# shellcheck disable=SC1090,SC1091
. "$authoring_killswitch" 2>/dev/null || true
type authoring_guards_off >/dev/null 2>&1 && authoring_guards_off && exit 0
if [ -r "$report" ]; then
  count="$(awk -F'\t' '!/^#/ && $1+0 >= 24 { n++ } END { print n+0 }' "$report")"
  if [ "$count" -gt 0 ]; then
    printf '%s unlanded worktrees/branches older than 24 h are listed in %s (lane-gc --unlanded). Before starting an issue, continue an existing lane that references it instead of starting over.\n' \
      "$count" "${report/#$HOME/\~}"
  fi
fi
payload="$(head -c 65536)"
lib="$(dirname "$(readlink -f "$0")")/../lib/thread_archives.py"
[ -r "$lib" ] || lib="$(dirname "$(readlink -f "$(command -v threads)")")/../socrates/lib/thread_archives.py"
[ -r "$lib" ] || exit 0
printf '%s' "$payload" | "${NORTH_AGENT_PYTHON:-python3}" "$lib"
