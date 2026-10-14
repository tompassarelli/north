#!/usr/bin/env bash
set -uo pipefail

here=$(cd "$(dirname "$0")" && pwd)
tmp=$(mktemp -d)
trap 'rm -rf -- "$tmp"' EXIT
pass=0
fail=0

check() {
  local name=$1 want=$2 got=$3
  if [ "$got" = "$want" ]; then
    pass=$((pass + 1)); echo "PASS  $name"
  else
    fail=$((fail + 1)); echo "FAIL  $name"; echo "  want: $want"; echo "  got:  $got"
  fi
}

decide() {
  local state=$1 game=$2 pool=$3
  printf '{"state":"%s"}\n' "$state" > "$tmp/state.json"
  rm -rf -- "$tmp/pool"
  mkdir -p "$tmp/pool/pair-0/games/g1"
  if [ "$pool" = live ]; then
    touch "$tmp/pool/pair-0/games/g1/actions.log"
  else
    touch -d '-10 minutes' "$tmp/pool/pair-0/games/g1/actions.log"
  fi
  FARM_RUNNER_STATE=$tmp/state.json FARM_RUNNER_POOL_STATE=$tmp/pool FARM_RUNNER_GAME=$game \
    bash -c 'eval "$(sed -n "/^state_cache=/,/^units()/p" "$0" | head -n -1)"
      if frozen; then echo freeze; elif allowed; then echo run; else echo drain; fi' "$here/farm-runner"
}

check "an available laptop with no game and no pool game takes new jobs" "run" "$(decide available 'no-such-game-zz9' idle)"
check "a live pool capture drains: running jobs continue, no new registrations" "drain" "$(decide available 'no-such-game-zz9' live)"
check "Tom's own game freezes the runners even during a pool capture" "freeze" "$(decide available '[s]ystemd --user' live)"
check "a laptop that is not available freezes the runners" "freeze" "$(decide draining 'no-such-game-zz9' idle)"

echo "farm-runner: $pass passed, $fail failed"
[ "$fail" -eq 0 ]
