#!/usr/bin/env bash
set -euo pipefail
root=${XDG_CACHE_HOME:-$HOME/.cache}/north/mutant-cargo
slots=${MUTANT_SLOTS:-4}
mkdir -p "$root"
mapfile -t claimed <<<"$MUTANT_TESTS"
names=() bun=()
for name in "${claimed[@]}"; do
  if [[ $name =~ ^[A-Za-z0-9_:]+$ ]]; then names+=("$name"); else bun+=("$name"); fi
done
while IFS= read -r tree; do
  for name in "${bun[@]}"; do
    mapfile -t files < <(cd "$tree" && grep -rlF --include='*.test.ts' -- "$name" .)
    ((${#files[@]})) || continue
    log=$(cd "$tree" && bun test -t "$(sed 's/[][\\.*^$()+?{}|]/\\&/g' <<<"$name")\$" "${files[@]}" </dev/null 2>&1) || true
    printf '%s\n' "$log"
    if grep -qE '^ *[1-9][0-9]* fail$' <<<"$log"; then verdict=fail; elif grep -qE '^ *[1-9][0-9]* pass$' <<<"$log"; then verdict=pass; else continue; fi
    printf '%s\t%s\n' "$verdict" "$name" >>"$tree/.mutant-results"
  done
done <<<"$MUTANT_TREES"
((${#names[@]})) || exit 0
if [ -d "$root/target" ] && [ ! -e "$root/slot-0" ]; then
  exec {legacy}>"$root/lock"
  if flock -n "$legacy" && [ -d "$root/target" ] && [ ! -e "$root/slot-0" ]; then mkdir "$root/slot-0" && mv "$root/target" "$root/slot-0/target"; fi
  exec {legacy}>&-
fi
slot=
for ((i = 0; i < slots; i++)); do
  exec {held}>"$root/slot-$i.lock"
  if flock -n "$held"; then slot=$i; break; fi
  exec {held}>&-
done
if [ -z "$slot" ]; then
  slot=$((RANDOM % slots))
  exec {held}>"$root/slot-$slot.lock"
  flock -w 3600 "$held"
fi
dir=$root/slot-$slot
mkdir -p "$dir/src"
seed=$root/slot-seed/target
if [ ! -d "$dir/target" ] && [ -d "$seed" ]; then
  cp -a --reflink=auto "$seed" "$dir/target.seed" && mv "$dir/target.seed" "$dir/target" || rm -rf "${dir:?}/target.seed"
fi
while IFS= read -r tree; do
  rsync -rlpc --delete --exclude=.mutant-results "$tree/" "$dir/src/" </dev/null
  log=$(cd "$dir/src" && CARGO_TARGET_DIR="$dir/target" CARGO_INCREMENTAL=1 cargo test --locked --offline --no-fail-fast --tests -- "${names[@]}" </dev/null 2>&1 {held}>&-) || true
  printf '%s\n' "$log"
  for name in "${names[@]}"; do
    printf '%s\n' "$log" | sed -nE "s/^test (.+::)?${name}( - should panic)? \.\.\. (ok|FAILED)$/\3/p" | while IFS= read -r verdict; do
      [ "$verdict" = ok ] && verdict=pass || verdict=fail
      printf '%s\t%s\n' "$verdict" "$name" >>"$tree/.mutant-results"
    done
  done
done <<<"$MUTANT_TREES"
if [ ! -d "$seed" ]; then
  mkdir -p "$root/slot-seed" && cp -a --reflink=auto "$dir/target" "$root/slot-seed/target.$$" && mv -T "$root/slot-seed/target.$$" "$seed" || rm -rf "${root:?}/slot-seed/target.$$"
fi
