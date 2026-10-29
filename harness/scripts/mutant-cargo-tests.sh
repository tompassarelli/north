#!/usr/bin/env bash
set -euo pipefail
root=${XDG_CACHE_HOME:-$HOME/.cache}/north/mutant-cargo
mkdir -p "$root"
exec 9>"$root/lock"
flock -w 3600 9
mapfile -t names <<<"$MUTANT_TESTS"
for name in "${names[@]}"; do
  [[ $name =~ ^[A-Za-z0-9_:]+$ ]] || { printf 'mutant-cargo-tests: %s is not a cargo test name\n' "$name" >&2; exit 2; }
done
while IFS= read -r tree; do
  rsync -rlpc --delete --exclude=.mutant-results "$tree/" "$root/src/" </dev/null
  log=$(cd "$root/src" && CARGO_TARGET_DIR="$root/target" CARGO_INCREMENTAL=0 cargo test --locked --offline --no-fail-fast --tests -- "${names[@]}" </dev/null 2>&1 9>&-) || true
  printf '%s\n' "$log"
  for name in "${names[@]}"; do
    printf '%s\n' "$log" | sed -nE "s/^test (.+::)?${name}( - should panic)? \.\.\. (ok|FAILED)$/\3/p" | while IFS= read -r verdict; do
      [ "$verdict" = ok ] && verdict=pass || verdict=fail
      printf '%s\t%s\n' "$verdict" "$name" >>"$tree/.mutant-results"
    done
  done
done <<<"$MUTANT_TREES"
