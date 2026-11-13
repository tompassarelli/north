#!/usr/bin/env bash
# Replays the before-landing nextest check of the last COUNT landed commits on HOST and sets each beside its recorded run.
set -euo pipefail
host=${1:?usage: replay.sh HOST [COUNT]}
count=${2:-20}
root=$(git rev-parse --show-toplevel)
records=${REPLAY_RECORDS:-$HOME/.local/state/safe-push/check-timings/outcomes.tsv}
logs=$(mktemp -d)
common=$(realpath "$(git -C "$root" rev-parse --git-common-dir)")
scratch="${common%/*/.git}/worktrees"
trap 'rm -rf "$logs"' EXIT
git -C "$root" fetch --quiet origin main

replay_one() {
  local head=$1 parent declared tree selection job
  parent=$(git -C "$root" rev-parse "$head^")
  declared=$(git -C "$root" show "$head:.safe-push" | grep -F 'SAFE_PUSH_LAND_PACKAGES' | head -n 1 | sed 's/^[[:space:]]*check = [^:]* :: //')
  printf '%s\n' "$declared" >"$logs/$head.line"
  tree=$(mktemp -d "$scratch/replay-${head:0:12}.XXXXXX")
  git -C "$root" worktree add --detach --quiet "$tree" "$head"
  selection=$(cd "$tree" && north land selection --host "$host" --base "$parent" --head "$head" -- "$declared" 2>"$logs/$head.log") || selection=failed
  git -C "$root" worktree remove --force "$tree"
  if [ "$selection" = failed ]; then return; fi
  job="export SAFE_PUSH_LAND_BASE=$parent SAFE_PUSH_LAND_HEAD=$head"
  while IFS= read -r pair; do
    if [ -n "$pair" ]; then job+=" ${pair%%=*}=$(printf %q "${pair#*=}")"; fi
  done <<<"$selection"
  job+="
echo \"replay: selection \${SAFE_PUSH_LAND_PACKAGES:---workspace}\"
replay_started=\$SECONDS
replay_status=0
north land crate-tests -- $(printf %q "$declared") || replay_status=\$?
echo \"replay: exit \$replay_status seconds \$((SECONDS - replay_started))\"
git checkout --quiet -- . 2>/dev/null || true"
  north capacity offload run --host "$host" --base "$parent" --head "$head" --timeout 3600 -- "$job" >>"$logs/$head.log" 2>&1 || true
}

mapfile -t heads < <(git -C "$root" log --first-parent --format=%H -n "$count" origin/main)
for head in "${heads[@]}"; do
  while [ "$(jobs -rp | wc -l)" -ge 4 ]; do wait -n || true; done
  replay_one "$head" &
done
wait

percentiles() {
  sort -n | awk -v label="$1" '{ v[NR] = $1 } END { if (!NR) { print label ": none"; exit } p50 = v[int((NR * 50 + 99) / 100)]; p90 = v[int((NR * 90 + 99) / 100)]; printf "%s: p50 %s p90 %s (n=%d)\n", label, p50, p90, NR }'
}

touch "$logs/replay.s" "$logs/recorded.s"
printf 'commit\tselection\tbuck2\treplay_exit\treplay_s\trecorded_outcome\trecorded_exit\trecorded_s\n'
for head in "${heads[@]}"; do
  log=$logs/$head.log
  declared=$(cat "$logs/$head.line")
  selection=$(sed -n 's/^replay: selection //p' "$log" | tail -n 1)
  size=$(awk -v s="$selection" 'BEGIN { if (s == "") print "-"; else if (s == "--workspace") print "workspace"; else print gsub(/-p /, "", s) }')
  buck2=$(sed -n 's/^north land crate-tests: this host tests \([^ ]*\) with Buck2:.*/\1/p' "$log" | paste -sd, -)
  read -r exit seconds < <(sed -n 's/^replay: exit \([0-9]*\) seconds \([0-9]*\)$/\1 \2/p' "$log" | tail -n 1; echo "- -")
  IFS=$'\t' read -r outcome recorded_exit recorded_seconds < <(awk -F'\t' -v c="$head" -v l="$declared" '$2 == c && $8 == l { row = $4 "\t" $5 "\t" $6 } END { print (row == "" ? "-\t-\t-" : row) }' "$records")
  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "${head:0:12}" "$size" "${buck2:--}" "$exit" "$seconds" "$outcome" "$recorded_exit" "$recorded_seconds"
  if [ "$exit" = - ]; then
    echo "no replay verdict for ${head:0:12}:"; tail -n 5 "$log"; continue
  fi
  echo "$seconds" >>"$logs/replay.s"
  if [ "$recorded_seconds" != - ]; then echo "${recorded_seconds%.*}" >>"$logs/recorded.s"; fi
  if [ "$recorded_exit" != - ] && [ "$((exit == 0))" != "$((recorded_exit == 0))" ]; then
    echo "verdict differs on ${head:0:12} (replay exit $exit, recorded exit $recorded_exit); failing cases:"
    grep -E '^[[:space:]]+(FAIL|SIGSEGV|SIGABRT|TIMEOUT) |^error(\[E[0-9]+\])?:|Buck2 gave no verdict' "$log" | head -n 20 || true
  fi
done
percentiles replay_s <"$logs/replay.s"
percentiles recorded_s <"$logs/recorded.s"
