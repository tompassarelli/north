#!/usr/bin/env bash
# Runs Buck2 alone, on HOST, for the crates of FILE each of the last COUNT landed commits reaches, beside its recorded Cargo run.
set -euo pipefail
host=${1:?usage: replay-buck2.sh HOST FILE [COUNT]}
listed=${2:?usage: replay-buck2.sh HOST FILE [COUNT]}
count=${3:-20}
root=$(git rev-parse --show-toplevel)
records=${REPLAY_RECORDS:-$HOME/.local/state/safe-push/check-timings/outcomes.tsv}
declared() { git -C "$root" show "$1:.safe-push" | grep -F 'SAFE_PUSH_LAND_PACKAGES' | head -n 1 | sed 's/^[[:space:]]*check = [^:]* :: //'; }
logs=$(mktemp -d)
trap 'rm -rf "$logs"' EXIT
git -C "$root" fetch --quiet origin main

reached() {
  local parent=$1 head=$2 changed edges
  changed=$(git -C "$root" diff --name-only "$parent" "$head")
  if grep -qE '^(Cargo\.(toml|lock)|rust-toolchain(\.toml)?|BUCK|\.buckconfig|build/|toolchains/|platforms/)' <<<"$changed"; then cat "$listed"; return; fi
  edges=$(for manifest in $(git -C "$root" ls-tree --name-only "$head" crates/ | sed 's|$|/Cargo.toml|'); do git -C "$root" show "$head:$manifest" 2>/dev/null | sed -n 's|.*path = "\.\./\([^"]*\)".*|\1|p' | sed "s|\$| ${manifest#crates/}|; s|/Cargo.toml\$||"; done)
  sed -n 's|^crates/\([^/]*\)/.*|\1|p' <<<"$changed" | sort -u | awk -v edges="$edges" 'BEGIN { n = split(edges, e, "\n"); for (i = 1; i <= n; i++) { split(e[i], p, " "); users[p[1]] = users[p[1]] " " p[2] } } { queue[++q] = $1; seen[$1] = 1 } END { for (i = 1; i <= q; i++) { m = split(users[queue[i]], u, " "); for (j = 1; j <= m; j++) if (!seen[u[j]]) { seen[u[j]] = 1; queue[++q] = u[j] } } for (c in seen) print c }' | grep -Fxf "$listed" || true
}

replay_one() {
  local head=$1 parent crates job
  parent=$(git -C "$root" rev-parse "$head^")
  crates=$(reached "$parent" "$head" | sort -u)
  printf '%s\n' "$crates" | grep -c . >"$logs/$head.size" || true
  if [ -z "$crates" ]; then return; fi
  job="replay_started=\$SECONDS; replay_status=0
north capacity dev-shell run -- buck2 test --remote-only --unstable-allow-compatible-tests-on-re $(sed 's|.*|//crates/&:|' <<<"$crates" | paste -sd' ' -) || replay_status=\$?
echo \"replay: exit \$replay_status seconds \$((SECONDS - replay_started))\"
north capacity dev-shell run -- buck2 log show | grep -F '\"TestResult\"' | sed 's/^/replay-result: /' || true"
  north capacity offload run --host "$host" --base "$parent" --head "$head" --timeout 2100 -- "$job" >"$logs/$head.log" 2>&1 || true
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
printf 'commit\tcrates\tbuck2_exit\tbuck2_s\tpassed\tfailed\tsilent\tcargo_outcome\tcargo_exit\tcargo_s\n'
for head in "${heads[@]}"; do
  log=$logs/$head.log
  touch "$log"
  read -r exit seconds < <(sed -n 's/^replay: exit \([0-9]*\) seconds \([0-9]*\)$/\1 \2/p' "$log" | tail -n 1; echo "- -")
  verdicts=$(sed -n 's/^replay-result: //p' "$log" | jq -r '.Event.data.Instant.data.TestResult | [(.target_label.label.package | sub("^root//crates/"; "")), (if .status == 1 then "pass" else "fail" end), ([.details | scan("FAIL \\[[^]]*\\] +[^ ]+ +([^\\n]+)") | .[0]] | join(";"))] | @tsv' 2>/dev/null | sort)
  passed=$(awk -F'\t' '{ s[$1] = s[$1] $2 } END { for (c in s) if (s[c] !~ /fail/) print c }' <<<"$verdicts" | sort | paste -sd, -)
  failed=$(awk -F'\t' '$2 == "fail" { f[$1] = f[$1] (f[$1] ? ";" : "") $3 } END { for (c in f) print c "(" f[c] ")" }' <<<"$verdicts" | sort | paste -sd, -)
  silent=$(comm -23 <(reached "$(git -C "$root" rev-parse "$head^")" "$head" | sort -u) <(cut -f1 <<<"$verdicts" | sort -u) | paste -sd, -)
  IFS=$'\t' read -r outcome cargo_exit cargo_seconds < <(awk -F'\t' -v c="$head" -v l="$(declared "$head")" '$2 == c && $8 == l { row = $4 "\t" $5 "\t" $6 } END { print (row == "" ? "-\t-\t-" : row) }' "$records")
  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "${head:0:12}" "$(cat "$logs/$head.size")" "$exit" "$seconds" "${passed:--}" "${failed:--}" "${silent:--}" "$outcome" "$cargo_exit" "$cargo_seconds"
  if [ "$exit" != - ]; then echo "$seconds" >>"$logs/buck2.s"; fi
  if [ "$cargo_seconds" != - ]; then echo "${cargo_seconds%.*}" >>"$logs/recorded.s"; fi
  if [ "$exit" != - ] && [ -z "$verdicts" ]; then echo "no test result for ${head:0:12}:"; grep -v '^offload: waiting' "$log" | tail -n 8; fi
done
percentiles buck2_s <"$logs/buck2.s"
percentiles cargo_s <"$logs/recorded.s"
