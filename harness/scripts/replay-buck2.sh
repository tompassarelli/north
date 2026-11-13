#!/usr/bin/env bash
# Runs Buck2 alone, on HOST, for the crates of FILE in each of the last COUNT landed commits' selection, beside its recorded Cargo run.
set -euo pipefail
host=${1:?usage: replay-buck2.sh HOST FILE [COUNT]}
listed=${2:?usage: replay-buck2.sh HOST FILE [COUNT]}
count=${3:-20}
root=$(git rev-parse --show-toplevel)
records=${REPLAY_RECORDS:-$HOME/.local/state/safe-push/check-timings/outcomes.tsv}
declared() { git -C "$root" show "$1:.safe-push" | grep -F 'SAFE_PUSH_LAND_PACKAGES' | head -n 1 | sed 's/^[[:space:]]*check = [^:]* :: //'; }
targets() { while read -r crate; do git -C "$root" grep -l "^name = \"$crate\"" "$1" -- '*Cargo.toml' | sed "s|^$1:Cargo.toml\$|//:|; s|^$1:\(.*\)/Cargo.toml\$|//\1:|"; done | paste -sd' ' -; }
named() { awk -F"\t" -v OFS="\t" -v names="$(tr , "\n" <"$2" | while read -r c; do git -C "$root" grep -l "^name = \"$c\"" "$1" -- "*Cargo.toml" | sed "s|^$1:||; s|^Cargo.toml$|\t$c|; s|/Cargo.toml$|\t$c|"; done)" 'BEGIN { n = split(names, l, "\n"); for (i = 1; i <= n; i++) { split(l[i], p, "\t"); m[p[1]] = p[2] } } { if ($1 in m) $1 = m[$1]; print }'; }
logs=$(mktemp -d)
echo "replay: logs in $logs" >&2
git -C "$root" fetch --quiet origin main

selected() {
  local parent=$1 head=$2 tree packages
  tree=$(mktemp -d "${TMPDIR:-/tmp}/replay-${head:0:12}.XXXXXX")
  git -C "$root" worktree add --detach --quiet "$tree" "$head"
  packages=$(cd "$tree" && north land selection --host "$host" --base "$parent" --head "$head" -- "$(declared "$head")" 2>>"$logs/$head.log" | sed -n 's/^SAFE_PUSH_LAND_PACKAGES=//p') || packages=failed
  git -C "$root" worktree remove --force "$tree" || true
  echo "$packages" >"$logs/$head.selection"
  case $packages in
    failed) ;;
    --workspace) cat "$listed" ;;
    *) tr ' ' '\n' <<<"$packages" | grep -v '^-p$' | grep -Fxf "$listed" || true ;;
  esac
}

place() {
  local log=$1 tries=0 status
  shift
  while [ "$tries" -lt 12 ]; do
    tries=$((tries + 1)) status=0
    north capacity offload run "$@" >>"$log" 2>&1 || status=$?
    if [ "$status" != 96 ] || grep -q "is closed to offloaded checks" "$log"; then return; fi
  done
}

replay_one() {
  local head=$1 parent crates job
  parent=$(git -C "$root" rev-parse "$head^")
  crates=$(selected "$parent" "$head" | sort -u)
  printf '%s\n' "$crates" | grep . | paste -sd, - >"$logs/$head.crates" || true
  if [ -z "$crates" ]; then return; fi
  job="replay_started=\$SECONDS; replay_status=0
north capacity dev-shell run -- buck2 test --remote-only --unstable-allow-compatible-tests-on-re $(targets "$head" <<<"$crates") || replay_status=\$?
echo \"replay: exit \$replay_status seconds \$((SECONDS - replay_started))\"
north capacity dev-shell run -- buck2 log show | grep -F '\"TestResult\"' | sed 's/^/replay-result: /' || true"
  place "$logs/$head.log" --host "$host" --base "$parent" --head "$head" --timeout 2100 -- "$job"
}

mapfile -t heads < <(git -C "$root" log --first-parent --format=%H -n "$count" origin/main)
for head in "${heads[@]}"; do
  while [ "$(jobs -rp | wc -l)" -ge 4 ]; do wait -n || true; done
  replay_one "$head" &
done
wait

percentiles() {
  sort -n | awk -v label="$1" '{ v[NR] = $1 } END { if (!NR) { print label ": none"; exit } printf "%s: p50 %s p90 %s (n=%d)\n", label, v[int((NR * 50 + 99) / 100)], v[int((NR * 90 + 99) / 100)], NR }'
}

touch "$logs/buck2.s" "$logs/recorded.s"
printf 'commit\tselection\treplayed\tbuck2_exit\tbuck2_s\tpassed\tfailed\tsilent\tcargo_outcome\tcargo_exit\tcargo_s\n'
for head in "${heads[@]}"; do
  log=$logs/$head.log
  touch "$log" "$logs/$head.selection" "$logs/$head.crates"
  read -r exit seconds < <(sed -n 's/^replay: exit \([0-9]*\) seconds \([0-9]*\)$/\1 \2/p' "$log" | tail -n 1; echo "- -")
  verdicts=$(sed -n 's/^replay-result: //p' "$log" | jq -r '.Event.data.Instant.data.TestResult | [(.target_label.label.package | sub("^root//"; "")), (if .status == 1 then "pass" else "fail" end), ([.details | scan("FAIL \\[[^]]*\\] +[^ ]+ +([^\\n]+)") | .[0]] | join(";"))] | @tsv' 2>/dev/null | named "$head" "$logs/$head.crates" | sort)
  passed=$(awk -F'\t' '{ s[$1] = s[$1] $2 } END { for (c in s) if (s[c] !~ /fail/) print c }' <<<"$verdicts" | sort | paste -sd, -)
  failed=$(awk -F'\t' '$2 == "fail" { f[$1] = f[$1] (f[$1] ? ";" : "") $3 } END { for (c in f) print c "(" f[c] ")" }' <<<"$verdicts" | sort | paste -sd, -)
  silent=$(comm -23 <(tr , '\n' <"$logs/$head.crates" | grep . | sort) <(cut -f1 <<<"$verdicts" | sort -u) | paste -sd, -)
  selection=$(sed 's/-p //g' "$logs/$head.selection")
  IFS=$'\t' read -r outcome cargo_exit cargo_seconds < <(awk -F'\t' -v c="$head" -v l="$(declared "$head")" '$2 == c && $8 == l { row = $4 "\t" $5 "\t" $6 } END { print (row == "" ? "-\t-\t-" : row) }' "$records")
  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "${head:0:12}" "${selection:--}" "$(cat "$logs/$head.crates")" "$exit" "$seconds" "${passed:--}" "${failed:--}" "${silent:--}" "$outcome" "$cargo_exit" "$cargo_seconds"
  if [ "$exit" != - ]; then echo "$seconds" >>"$logs/buck2.s"; fi
  if [ "$cargo_seconds" != - ]; then echo "${cargo_seconds%.*}" >>"$logs/recorded.s"; fi
  if [ "$exit" != - ] && [ -z "$verdicts" ]; then echo "no test result for ${head:0:12}:"; grep -v '^offload: waiting' "$log" | tail -n 8; fi
done
percentiles buck2_s <"$logs/buck2.s"
percentiles cargo_s <"$logs/recorded.s"
