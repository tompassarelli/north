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
    printf 'fixture sync error\n'
    exit "$TEST_SYNC_STATUS" ;;
  event)
    [[ "$(readlink "$XDG_STATE_HOME/north/current")" == "runtimes/$TEST_PREVIOUS" ]] || exit 92
    printf '%s\n' "$@" >>"$TEST_ALERTS" ;;
  *) exit 93 ;;
esac
STUB
chmod +x "$scratch/bin/git" "$scratch/bin/agents"
export PATH="$scratch/bin:$PATH" HOME="$scratch/home"
export TEST_TARGET=1111111111111111111111111111111111111111
export TEST_PREVIOUS=2222222222222222222222222222222222222222
for mode in rollback success; do
  export XDG_STATE_HOME="$scratch/$mode" TEST_ALERTS="$scratch/$mode-alerts"
  state="$XDG_STATE_HOME/north"
  mkdir -p "$state/runtimes/$TEST_TARGET" "$state/runtimes/$TEST_PREVIOUS"
  touch "$state/runtimes/$TEST_TARGET/north" "$state/runtimes/$TEST_PREVIOUS/north"
  chmod +x "$state/runtimes/$TEST_TARGET/north"
  ln -s "runtimes/$TEST_PREVIOUS" "$state/current"
  export TEST_SYNC_STATUS=0
  [[ "$mode" != rollback ]] || TEST_SYNC_STATUS=42
  status=0
  bash "$repo/bin/north-runtime-update" >"$scratch/$mode.log" 2>&1 || status=$?
  if [[ "$mode" == rollback ]]; then
    [[ "$status" == 1 && "$(readlink "$state/current")" == "runtimes/$TEST_PREVIOUS" ]]
    [[ "$(wc -l <"$TEST_ALERTS")" == 8 ]]
    mapfile -t alert <"$TEST_ALERTS"
    [[ "${alert[0]}" == event && "${alert[1]}" == STUCK && "${alert[2]}" == --from-node && "${alert[3]}" == north-runtime-update ]]
    [[ "${alert[4]}" == --to-node && "${alert[5]}" == proxy && "${alert[6]}" == --body ]]
    [[ "${alert[7]}" == *"failed step agents sync on $TEST_TARGET; rolled back to runtimes/$TEST_PREVIOUS; error (exit 42): fixture sync error" ]]
    printf 'PASS runtime rollback: restored previous revision and emitted exactly one proxy alert with step, revisions and error\n'
  else
    [[ "$status" == 0 && "$(readlink "$state/current")" == "runtimes/$TEST_TARGET" && ! -e "$TEST_ALERTS" ]]
    printf 'PASS runtime success: promoted revision and emitted no alert\n'
  fi
done
