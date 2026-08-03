#!/usr/bin/env bash
# orchestration doctrine injector — SessionStart hook body (see .claude-plugin/plugin.json).
# ============================================================================
# Emits the SESSION DIGEST of doctrine.md: everything except the spans marked
# <!-- orchestration:full-only -->, with the marked SPAWN SURFACES block swapped
# for the ACTIVE spawn adapter's block. Two goals, one file:
#   - flip the config -> the injected instructions change (adapter splice),
#   - the always-injected surface stays small; the full doctrine is read from
#     disk at dispatch time (the digest's closing pointer says exactly that).
# doctrine.md remains the single source; the digest is a mechanical extract,
# never a hand-maintained copy.
#
#   adapter resolution:  $ORCHESTRATION_SPAWN_ADAPTER  >  ~/.local/state/north/harness.conf
#                        (dispatch=)            >  native (default)
#   adapter blocks:      docs/adapters/<adapter>.md   (generated from PRESETS by
#                        scripts/build-agents.mjs — do not hand-edit)
#   native:              the block lives inline in doctrine.md (markers stripped);
#                        no external file, so orchestration works standalone for anyone.
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
adapter="${ORCHESTRATION_SPAWN_ADAPTER:-}"
if [ -z "$adapter" ]; then
  disp="$(grep -E '^dispatch=' "$HOME/.local/state/north/harness.conf" 2>/dev/null | tail -1 | cut -d= -f2-)"
  case "$disp" in
    managed|north) adapter="north" ;;
    *)    adapter="native" ;;
  esac
fi

block="$ROOT/docs/adapters/${adapter}.md"

# Splice the active adapter block (native keeps the inline block), then strip
# the full-only spans and every remaining marker line. Fail-open: with no
# adapter file the doctrine still emits with its inline native block.
if [ "$adapter" = "native" ] || [ ! -f "$block" ]; then
  cat "$DOCTRINE"
else
  awk -v blockfile="$block" '
    /^<!-- orchestration:spawn-surfaces/    { while ((getline line < blockfile) > 0) print line; skip=1; next }
    /^<!-- \/orchestration:spawn-surfaces/  { skip=0; next }
    skip != 1                       { print }
  ' "$DOCTRINE"
fi | awk '
  /^<!-- orchestration:full-only/    { skip=1; next }
  /^<!-- \/orchestration:full-only/  { skip=0; next }
  /^<!-- \/?orchestration:spawn-surfaces/ { next }
  skip != 1 { print }
'

printf '\nDIGEST NOTE — this injection is the session digest of the doctrine.\nBefore any nontrivial dispatch decision, read %s in full:\northogonal axes, resource policy, orchestrator/worker jurisdiction law, the\nstop-rule, workflow stage pins, and the bespoke-composition spec live there.\n' "$DOCTRINE"
