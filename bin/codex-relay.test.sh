#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
state="$(mktemp -d)"
trap 'rm -rf "$state"' EXIT
mkdir "$state/bin" "$state/mailbox"
cat >"$state/bin/niri" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
if [[ "$*" == 'msg --json focused-window' ]]; then
  printf '{"id":7}\n'
else
  printf '%s\n' "$*" >>"$RELAY_NIRI_LOG"
fi
SH
cat >"$state/bin/wtype" <<'SH'
#!/usr/bin/env bash
exit 99
SH
chmod +x "$state/bin/niri" "$state/bin/wtype"
export PATH="$state/bin:$PATH" CODEX_BEHAVIOR_STATE="$state/mailbox" RELAY_WAIT=0 RELAY_NIRI_LOG="$state/niri.log"
status=0
printf '' | "$here/codex-relay" 'session/one' 999999 >"$state/out" 2>"$state/err" || status=$?
[[ "$status" == 2 && ! -e "$state/mailbox/relay-session_one.txt" ]]
printf 'earlier\n' >"$state/mailbox/relay-session_one.txt"
status=0
printf 'new line\nsecond line\n' | "$here/codex-relay" 'session/one' 999999 >"$state/out" 2>"$state/err" || status=$?
[[ "$status" == 1 ]]
printf 'earlier\nnew line\nsecond line\n' >"$state/expected"
cmp "$state/expected" "$state/mailbox/relay-session_one.txt"
[[ "$(cat "$state/niri.log")" == 'msg action focus-window --id 999999' ]]
printf 'codex-relay: mailbox preserves pending bytes when external focus fails; empty input never writes\n'
