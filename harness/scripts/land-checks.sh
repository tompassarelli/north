#!/usr/bin/env bash
set -euo pipefail
base=${SAFE_PUSH_LAND_BASE:?}
head=${SAFE_PUSH_LAND_HEAD:?}
declare -A seen=()
selected=()
add_test() {
  [ -f "$1" ] || return 0
  [ -z "${seen[$1]:-}" ] || return 0
  seen[$1]=1
  selected+=("$1")
}
roots=(bin lib harness/hooks harness/lib harness/scripts)
mapfile -d '' -t tests < <(rg --files -0 "${roots[@]}" -g '*.test.py' -g '*.test.sh')
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
  while IFS= read -r -d '' test; do add_test "$test"; done < <(
    rg -l -0 -F -- "$path" "${tests[@]}"
  )
  module=${path##*/}; module=${module%.py}
  if [[ "$path" = *.py && "$module" =~ ^[a-zA-Z_][a-zA-Z_0-9]*$ ]]; then
    imports="(^|;)[[:space:]]*(from[[:space:]]+$module[[:space:]]+import\b|import[[:space:]]+([[:alnum:]_.]+([[:space:]]+as[[:space:]]+[[:alnum:]_]+)?[[:space:]]*,[[:space:]]*)*$module([[:space:]]*(,|;|#|$)|[[:space:]]+as[[:space:]]))"
    while IFS= read -r -d '' test; do add_test "$test"; done < <(
      rg -l -0 -e "$imports" -- "${tests[@]}"
    )
  fi
done < <(git diff --name-only --no-renames -z "$base" "$head")
for test in "${selected[@]}"; do
  printf 'land check: %s\n' "$test"
  case "$test" in *.sh) bash "$test" ;; *.py) python3 "$test" ;; esac
done
