#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
WATCH="$ROOT/bin/north-wip-watch"
TMP="$(mktemp -d)"
trap 'rm -rf -- "${TMP:?}"' EXIT
FAKE="$TMP/fake-bin"
STATE="$TMP/state"
MAIL="$TMP/mail"
mkdir -p "$FAKE"

fail() { printf 'north-wip-watch-test: %s\n' "$*" >&2; exit 1; }

cat >"$FAKE/north" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
state="${WIP_FIXTURE_STATE:?}"
case "$(<"$state")" in
  ok) exit 0 ;;
  short) echo 'WIP 1/3 — SHORTFALL: pull @ready-a @ready-b'; exit 3 ;;
  unavailable) echo "north: unknown command 'wip'" >&2; exit 2 ;;
  *) exit 9 ;;
esac
EOF
cat >"$FAKE/bb" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$1" "$2" "$3" "$4" "$5" "$6" "$7" >>"${WIP_FIXTURE_MAIL:?}"
EOF
chmod 0755 "$FAKE/north" "$FAKE/bb"

run_once() {
  PATH="$FAKE:$PATH" NORTH_WIP_WATCH_STATE="$STATE" WIP_FIXTURE_STATE="$TMP/wip-state" \
    WIP_FIXTURE_MAIL="$MAIL" "$WATCH" --once coordinator-test
}

mail_count() {
  if [ -f "$MAIL" ]; then
    wc -l <"$MAIL"
  else
    printf '0\n'
  fi
}

printf 'short\n' >"$TMP/wip-state"
run_once
[ "$(mail_count)" -eq 0 ] || fail 'single shortfall sent mail'
run_once
[ "$(mail_count)" -eq 1 ] || fail 'two shortfalls did not send exactly one mail'
grep -F 'WIP 1/3 — SHORTFALL: pull @ready-a @ready-b' "$MAIL" >/dev/null || fail 'mail omitted shortfall line'
run_once
[ "$(mail_count)" -eq 1 ] || fail 'degraded state repeated mail'

printf 'ok\n' >"$TMP/wip-state"
run_once
printf 'short\n' >"$TMP/wip-state"
run_once
[ "$(mail_count)" -eq 1 ] || fail 'first post-recovery shortfall sent mail'
run_once
[ "$(mail_count)" -eq 2 ] || fail 'recovery did not re-arm watcher'

printf 'unavailable\n' >"$TMP/wip-state"
set +e
run_once >"$TMP/unavailable.out" 2>"$TMP/unavailable.err"
status=$?
set -e
[ "$status" -eq 4 ] || fail "unavailable verb returned $status"
grep -Fx 'wip verb unavailable' "$TMP/unavailable.err" >/dev/null || fail 'unavailable verb message changed'

echo 'north-wip-watch-test: PASS'
