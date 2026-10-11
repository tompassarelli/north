#!/usr/bin/env bash
set -euo pipefail
base=${SAFE_PUSH_LAND_BASE:?}
head=${SAFE_PUSH_LAND_HEAD:?}
declare -A seen=()
while IFS= read -r -d '' path; do
  case "$path" in
    bin/*|lib/*|socrates/hooks/*|socrates/lib/*|socrates/scripts/*) ;;
    *) continue ;;
  esac
  stem=${path%.sh}; stem=${stem%.py}; stem=${stem%.ts}; stem=${stem%.mjs}
  stem=${stem%.test}
  for test in "$stem.test.sh" "$stem.test.py"; do
    [ -f "$test" ] || continue
    [ -z "${seen[$test]:-}" ] || continue
    seen[$test]=1
    printf 'land check: %s\n' "$test"
    case "$test" in *.sh) bash "$test" ;; *.py) python3 "$test" ;; esac
  done
done < <(git diff --name-only --no-renames -z "$base" "$head")
