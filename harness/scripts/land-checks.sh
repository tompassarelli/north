#!/usr/bin/env bash
set -euo pipefail
base=${SAFE_PUSH_LAND_BASE:?}
head=${SAFE_PUSH_LAND_HEAD:?}
scratch=$(mktemp -d)
trap 'rm -rf "${scratch:?}"' EXIT
search() {
  local status=0
  rg "$@" >"$scratch/matches" || status=$?
  [ "$status" -le 1 ] || { printf 'land check: rg failed (status %s)\n' "$status" >&2; exit "$status"; }
}
git diff --name-only --no-renames -z "$base" "$head" >"$scratch/changes" || {
  printf 'land check: git diff failed\n' >&2; exit 1;
}
declare -A seen=()
selected=()
add_test() {
  [ -f "$1" ] || return 0
  [ -z "${seen[$1]:-}" ] || return 0
  seen[$1]=1
  selected+=("$1")
}
roots=(bin lib harness/hooks harness/lib harness/scripts)
search --files -0 "${roots[@]}" -g '*.test.py' -g '*.test.sh'
mapfile -d '' -t tests <"$scratch/matches"
while IFS= read -r -d '' path; do
  case "$path" in
    bin/*|lib/*|harness/hooks/*|harness/lib/*|harness/scripts/*) ;;
    *) continue ;;
  esac
  stem=${path%.sh}; stem=${stem%.py}; stem=${stem%.ts}; stem=${stem%.mjs}
  stem=${stem%.test}
  for test in "$stem.test.sh" "$stem.test.py"; do
    add_test "$test"
  done
  [ "${#tests[@]}" -gt 0 ] || continue
  search -l -0 -F -- "$path" "${tests[@]}"
  while IFS= read -r -d '' test; do add_test "$test"; done <"$scratch/matches"
  module=${path##*/}; module=${module%.py}
  if [[ "$path" = *.py && "$module" =~ ^[a-zA-Z_][a-zA-Z_0-9]*$ ]]; then
    imports="(^|[;:])[[:space:]]*(from[[:space:]]+$module[[:space:]]+import\b|import[[:space:]]+([[:alnum:]_.]+([[:space:]]+as[[:space:]]+[[:alnum:]_]+)?[[:space:]]*,[[:space:]]*)*$module([[:space:]]*(,|;|#|$)|[[:space:]]+as[[:space:]]))"
    search -l -0 -e "$imports" -- "${tests[@]}"
    while IFS= read -r -d '' test; do add_test "$test"; done <"$scratch/matches"
  fi
done <"$scratch/changes"
for test in "${selected[@]}"; do
  printf 'land check: %s\n' "$test"
  case "$test" in *.sh) bash "$test" ;; *.py) python3 "$test" ;; esac
done
