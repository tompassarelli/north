#!/usr/bin/env bash
# gaffer doctrine injector — SessionStart hook body (see .claude-plugin/plugin.json).
# ============================================================================
# Prints doctrine.md, but swaps the marked SPAWN SURFACES block for the ACTIVE
# spawn adapter's block. This is what makes "flip the config -> the injected
# instructions change" work, instead of baking a substrate into anyone's
# provider adapter's bootstrap instruction file.
#
#   adapter resolution:  $GAFFER_SPAWN_ADAPTER  >  ~/.claude/my-config.state
#                        (dispatch=)            >  native (default)
#   adapter blocks:      docs/adapters/<adapter>.md   (generated from PRESETS by
#                        scripts/build-agents.mjs — do not hand-edit)
#   native:              the block lives inline in doctrine.md (markers stripped);
#                        no external file, so gaffer works standalone for anyone.
#
# POSIX sh + awk on purpose — node is NOT guaranteed on PATH at SessionStart, and
# a failure here would silently drop the doctrine every session. Generation stays
# in node (build-agents.mjs, run by hand); this hot path stays dependency-free.
# ============================================================================
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DOCTRINE="$ROOT/doctrine.md"

# Resolve the active adapter: explicit env wins, else the shared dispatch state
# (same switch the agent-spawn-guard reads, so enforcement + doctrine agree),
# else native.
adapter="${GAFFER_SPAWN_ADAPTER:-}"
if [ -z "$adapter" ]; then
  disp="$(grep -E '^dispatch=' "$HOME/.claude/my-config.state" 2>/dev/null | tail -1 | cut -d= -f2-)"
  case "$disp" in
    north) adapter="north" ;;
    *)    adapter="native" ;;
  esac
fi

block="$ROOT/docs/adapters/${adapter}.md"

# native, or an unknown adapter with no block file -> emit the base doctrine with
# the inline (native) block, markers stripped. Fail-open to a valid doctrine.
if [ "$adapter" = "native" ] || [ ! -f "$block" ]; then
  grep -v -E '^<!-- /?gaffer:spawn-surfaces' "$DOCTRINE"
  exit 0
fi

# Otherwise splice the adapter's block in place of the marked native block.
awk -v blockfile="$block" '
  /^<!-- gaffer:spawn-surfaces/    { while ((getline line < blockfile) > 0) print line; skip=1; next }
  /^<!-- \/gaffer:spawn-surfaces/  { skip=0; next }
  skip != 1                       { print }
' "$DOCTRINE"
