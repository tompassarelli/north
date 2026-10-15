#!/usr/bin/env bash
set -uo pipefail

trap 'cat >/dev/null' EXIT
export NORTH_HOOK_ID=language-guard
hook_lib="$(dirname "$0")/lib"
[ -r "$hook_lib/authoring-killswitch.sh" ] || hook_lib="$(dirname "$0")/../lib"
. "$hook_lib/authoring-killswitch.sh" 2>/dev/null || exit 0
authoring_guards_off && exit 0
north guard language 2>/dev/null || true
