#!/usr/bin/env bash
set -euo pipefail
# A home or system switch replaces the profile symlinks on PATH mid-run; pin every directory to its immutable target.
pinned=()
IFS=: read -ra path_dirs <<<"$PATH"
for dir in "${path_dirs[@]}"; do pinned+=("$(readlink -f -- "$dir" 2>/dev/null || printf '%s' "$dir")"); done
PATH=$(IFS=:; printf '%s' "${pinned[*]}")
export PATH
unset THREADS_ACTOR
mode=${1:-}
all=${2:-}
case "$mode:$all" in :|portable:|machine-bound:|:--all|portable:--all|machine-bound:--all) ;; *) printf "usage: land-checks.sh [portable|machine-bound] [--all]\n" >&2; exit 2 ;; esac
declare -A machine_bound=()
while IFS= read -r test; do machine_bound[$test]=1; done < <(grep -v "^#" "$(dirname "$0")/../machine-bound-tests")
missing=$(for test in "${!machine_bound[@]}"; do [ -f "$(dirname "$0")/../../$test" ] || printf "%s\n" "$test"; done)
[ -z "$missing" ] || { printf "land check: harness/machine-bound-tests lists files that do not exist: %s\n" "$missing" >&2; exit 1; }
scratch=$(mktemp -d)
trap 'rm -rf "${scratch:?}"' EXIT
search() {
  local status=0
  rg "$@" >"$scratch/matches" || status=$?
  [ "$status" -le 1 ] || { printf 'land check: rg failed (status %s)\n' "$status" >&2; exit "$status"; }
}
if [ -n "$all" ]; then : >"$scratch/changes"; else git diff --name-only --no-renames -z "${SAFE_PUSH_LAND_BASE:?}" "${SAFE_PUSH_LAND_HEAD:?}" >"$scratch/changes" || { printf "land check: git diff failed\n" >&2; exit 1; }; fi
declare -A seen=() visited=()
selected=() queue=()
add_test() {
  [ -f "$1" ] || return 0
  [ -z "${seen[$1]:-}" ] || return 0
  case "$mode:${machine_bound[$1]:+bound}" in portable:bound|machine-bound:) return 0 ;; esac
  seen[$1]=1
  selected+=("$1")
}
enqueue() { [ -n "${visited[$1]:-}" ] || { visited[$1]=1; queue+=("$1"); }; }
roots=(bin lib harness/hooks harness/lib harness/scripts)
search --files -0 "${roots[@]}"
mapfile -d '' -t files <"$scratch/matches"
search --files -0 --max-depth 1 -g '*.toml' harness
mapfile -d '' -t -O "${#files[@]}" files <"$scratch/matches"
while IFS= read -r -d '' path; do
  case "$path" in
    bin/*|lib/*|harness/hooks/*|harness/lib/*|harness/scripts/*|harness/*.toml) enqueue "$path" ;;
    src/*|crates/*|Cargo.toml|Cargo.lock|build.rs|rust-toolchain.toml) enqueue "$path"; enqueue NORTH_PACE_BIN; enqueue NORTH_BIN; enqueue debug/north ;;
  esac
done <"$scratch/changes"
[ -z "$all" ] || for path in "${files[@]}"; do case "$path" in *.test.sh|*.test.py) add_test "$path" ;; esac; done
for ((next = 0; next < ${#queue[@]}; next++)); do
  path=${queue[next]}
  stem=${path%.sh}; stem=${stem%.py}; stem=${stem%.ts}; stem=${stem%.mjs}
  stem=${stem%.test}
  for test in "$stem.test.sh" "$stem.test.py"; do
    add_test "$test"
  done
  [ "${#files[@]}" -gt 0 ] || continue
  name=${path##*/}
  [[ "$name" = *.* || "$path" != */* ]] || { dir=${path%/*}; name=${dir##*/}/$name; }
  patterns=(-e "(^|[^[:alnum:]_.-])${name//./\\.}([^[:alnum:]_.-]|$)")
  module=${path##*/}; module=${module%.py}
  if [[ "$path" = *.py && "$module" =~ ^[a-zA-Z_][a-zA-Z_0-9]*$ ]]; then
    patterns+=(-e "(^|[;:])[[:space:]]*(from[[:space:]]+${module}[[:space:]]+import\b|import[[:space:]]+([[:alnum:]_.]+([[:space:]]+as[[:space:]]+[[:alnum:]_]+)?[[:space:]]*,[[:space:]]*)*${module}([[:space:]]*(,|;|#|$)|[[:space:]]+as[[:space:]]))")
  fi
  search -l -0 "${patterns[@]}" -- "${files[@]}"
  while IFS= read -r -d '' file; do
    case "$file" in *.test.sh|*.test.py) add_test "$file" ;; esac
    enqueue "$file"
  done <"$scratch/matches"
done
if [ "${#selected[@]}" -gt 0 ] && [ -f Cargo.toml ]; then NORTH_PACE_BIN=$(./bin/head-north --path); export NORTH_PACE_BIN; fi
runner=${NORTH_PACE_BIN:-$(dirname "$0")/../../target/debug/north}
[ "${#selected[@]}" -eq 0 ] || { printf "land check: running %s land-tests %s\n" "$runner" "${selected[*]}"; "$runner" land-tests "${selected[@]}"; }
