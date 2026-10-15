#!/usr/bin/env bash
set -euo pipefail
repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
scratch="$(mktemp -d)"
trap 'rm -rf -- "${scratch:?}"' EXIT
mkdir -p "$scratch/bin"
cat >"$scratch/bin/git" <<'STUB'
#!/usr/bin/env bash
case "$3" in
  fetch) exit 0 ;;
  rev-parse) printf '%s\n' "$TEST_TARGET" ;;
  *) exit 90 ;;
esac
STUB
cat >"$scratch/bin/agents" <<'STUB'
#!/usr/bin/env bash
case "$1" in
  sync)
    [[ "$(readlink "$XDG_STATE_HOME/north/current")" == "runtimes/$TEST_TARGET" ]] || exit 91
    printf 'fixture sync error: Authorization: Bearer %s\n' "$TEST_SECRET"
    exit "$TEST_SYNC_STATUS" ;;
  *) exit 93 ;;
esac
STUB
cat >"$scratch/bin/north" <<'STUB'
#!/usr/bin/env bash
[[ "$1 $2 $3" == "fleet send proxy" ]] || exit 94
    [[ "$(readlink "$XDG_STATE_HOME/north/current")" == "runtimes/$TEST_PREVIOUS" ]] || exit 92
    printf '%s\n' "$@" >>"$TEST_ALERTS"
exit "$TEST_EVENT_STATUS"
STUB
chmod +x "$scratch/bin/git" "$scratch/bin/agents" "$scratch/bin/north"
export PATH="$scratch/bin:$PATH" HOME="$scratch/home"
export TEST_TARGET=1111111111111111111111111111111111111111
export TEST_PREVIOUS=2222222222222222222222222222222222222222
export TEST_SECRET=synthetic-fixture-bearer-token
for mode in rollback alert-failure success; do
  export XDG_STATE_HOME="$scratch/$mode" TEST_ALERTS="$scratch/$mode-alerts"
  state="$XDG_STATE_HOME/north"
  mkdir -p "$state/runtimes/$TEST_TARGET" "$state/runtimes/$TEST_PREVIOUS"
  touch "$state/runtimes/$TEST_TARGET/north" "$state/runtimes/$TEST_TARGET/verify-north" "$state/runtimes/$TEST_PREVIOUS/north"
  chmod +x "$state/runtimes/$TEST_TARGET/north" "$state/runtimes/$TEST_TARGET/verify-north"
  ln -s "runtimes/$TEST_PREVIOUS" "$state/current"
  export TEST_SYNC_STATUS=42 TEST_EVENT_STATUS=0
  [[ "$mode" != success ]] || TEST_SYNC_STATUS=0
  [[ "$mode" != alert-failure ]] || TEST_EVENT_STATUS=43
  status=0
  bash "$repo/bin/north-runtime-update" >"$scratch/$mode.log" 2>&1 || status=$?
  if [[ "$mode" != success ]]; then
    [[ "$status" == 1 && "$(readlink "$state/current")" == "runtimes/$TEST_PREVIOUS" ]]
    [[ "$(wc -l <"$TEST_ALERTS")" == 4 ]]
    mapfile -t alert <"$TEST_ALERTS"
    [[ "${alert[0]}" == fleet && "${alert[1]}" == send && "${alert[2]}" == proxy ]]
    [[ "${alert[3]}" == *"failed step agents sync on $TEST_TARGET; rolled back to runtimes/$TEST_PREVIOUS; exit status 42; local log: "* ]]
    [[ "${alert[3]}" != *"$TEST_SECRET"* && "${alert[3]}" != *'fixture sync error'* ]]
    sync_log="${alert[3]##*local log: }"
    [[ -f "$sync_log" && "$(stat -c %a "$sync_log")" == 600 ]]
    rg -qF "$TEST_SECRET" "$sync_log"
    if [[ "$mode" == alert-failure ]]; then
      rg -qF 'rollback alert delivery failed' "$scratch/$mode.log"
    fi
    printf 'PASS runtime %s: restored previous revision; one alert with step, revisions, exit status and private log; bearer token stays local\n' "$mode"
  else
    [[ "$status" == 0 && "$(readlink "$state/current")" == "runtimes/$TEST_TARGET" && ! -e "$TEST_ALERTS" ]]
    printf 'PASS runtime success: promoted revision and emitted no alert\n'
  fi
done
