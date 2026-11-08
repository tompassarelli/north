#!/usr/bin/env bash
# usage: [HOOK=1] grounding-equivalence.sh OLD_NORTH NEW_NORTH SNAPSHOT_DIR
set -euo pipefail
old=$1 new=$2 snapshot=$3
queries=${QUERIES:-$(dirname "$0")/fixtures/grounding-equivalence-queries.txt}
work=$(mktemp -d)
trap 'rm -rf -- "$work"' EXIT
for side in old new; do
  mkdir -p "$work/$side/grounding"
  cp "$snapshot/graph.db" "$work/$side/graph.db"
  cp "$snapshot/grounding/index.db" "$work/$side/grounding/index.db"
done
NORTH_GRAPH_DB=$work/new/graph.db NORTH_GROUNDING_STATE=$work/new/grounding "$new" graph ground import >"$work/import.json"
refs() {
  grep -v '^Mentions:' "$1" | grep -oE '(north|south|wisp|makgora):[^ ,]+|convo:[0-9]+|ledger:L[0-9a-f]+|github [^ ]+#[0-9]+|~/[^ ,]+' | awk '!seen[$0]++' | head -10 | sort
}
query() {
  local side=$1 binary=$2 text=$3 started ended
  started=$(date +%s%N)
  NORTH_GRAPH_DB=$work/$side/graph.db NORTH_GROUNDING_STATE=$work/$side/grounding "$binary" graph ground query ${HOOK:+--hook} --query "$text" >"$work/$side.out"
  ended=$(date +%s%N)
  echo $(((ended - started) / 1000000)) >>"$work/$side.ms"
  refs "$work/$side.out" >"$work/$side.refs"
}
while IFS= read -r text; do
  [ -n "$text" ] || continue
  query old "$old" "$text"
  query new "$new" "$text"
  shared=$(comm -12 "$work/old.refs" "$work/new.refs" | wc -l)
  echo "$shared" >>"$work/overlap"
  printf '%s/10 old %sms new %sms  %s\n' "$shared" "$(tail -1 "$work/old.ms")" "$(tail -1 "$work/new.ms")" "$text"
done <"$queries"
quantile() {
  sort -n "$1" | awk -v q="$2" '{v[NR]=$1} END {i=int(q*(NR-1)+0.5)+1; print v[i]}'
}
echo "median overlap $(quantile "$work/overlap" 0.5)/10 over $(wc -l <"$work/overlap") queries"
echo "old p50 $(quantile "$work/old.ms" 0.5) ms p90 $(quantile "$work/old.ms" 0.9) ms"
echo "new p50 $(quantile "$work/new.ms" 0.5) ms p90 $(quantile "$work/new.ms" 0.9) ms"
echo "import $(cat "$work/import.json")"
