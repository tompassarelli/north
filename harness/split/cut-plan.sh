#!/usr/bin/env bash
set -euo pipefail
usage="usage: cut-plan.sh PLAN [DAYS]  (PLAN: harness/split/plans/*.txt; prints crates, churn, rebuilt sets and DAG breaks)"
plan=${1:?$usage}; days=${2:-7}
plan=$(realpath "$plan")
cd "$(git rev-parse --show-toplevel)"
scratch=$(mktemp -d); trap 'rm -rf "${scratch:?}"' EXIT
git ls-files -z '*.rs' | xargs -0 awk '{n[FILENAME]++} /#\[cfg\(test\)\]/{t[FILENAME]=1} !t[FILENAME] && NF && $1 !~ /^\/\// {c[FILENAME]++} END{for (f in n) print n[f], c[f]+0, f}' >"$scratch/lines"
git log --since="$days.days" --format='C %h' --name-only -M </dev/null >"$scratch/log"
rg -n -o --no-heading 'crate::[a-z_]+(::[a-z_]+){0,2}' crates/north-core/src crates/north-tui/src </dev/null >"$scratch/refs" || true
rg -n -A1 --no-heading '^\s*#\[cfg\(test\)\]' crates/north-core/src crates/north-tui/src </dev/null \
  | rg -o '^[^:]+-[0-9]+-\s*(pub(\([a-z]+\))? )?mod [a-z_0-9]+ \{' | sed -E 's/-([0-9]+)-.*/ \1/' >"$scratch/inline" || true
awk -v LIB=0.053 -v TEST=0.071 -v ROOT=7.4 -v LINK=2.3 '
function norm(p) { if (p ~ /^crates\/north-core\/src\//) return "core:" substr(p, 23)
  if (p ~ /^src\/(cli|main\.rs|bin|fixtures|remote_serve_tests)/ || p ~ /^(tests|examples)\//) return "root:"
  if (p ~ /^src\//) return "core:" substr(p, 5)
  if (p ~ /^crates\/[a-z-]+\//) { split(p, q, "/"); return "crate:" q[2] }
  if (p ~ /^(Cargo\.toml|Cargo\.lock|rust-toolchain\.toml|build\.rs)$/) return "all:"; return "" }
function crate(p,  n, best, k) { n = norm(p); if (n == "") return ""; if (n == "root:") return "north"; if (n == "all:") return "ALL"
  if (n ~ /^crate:/) { k = substr(n, 7); return (k in C) ? k : "" }
  best = ""; for (k in P) if ((n == k || index(n, k "/") == 1 || index(n, k ".rs") == 1 || k == "core:") && length(k) > length(best)) best = k
  return best == "" ? "" : P[best] }
function testfile(f, l) { return f ~ /(^|\/)(tests|[a-z_0-9]*_tests)\.rs$|\/tests\// || (f in IN && l + 0 > IN[f]) }
function closure(  ch, k, x, y, q) { for (x in C) R[x, x] = 1; ch = 1
  while (ch) { ch = 0; for (k in D) { split(k, q, SUBSEP); for (y in C) if (R[q[2], y] && !R[q[1], y]) { R[q[1], y] = 1; ch = 1 } } }
  for (x in C) { TOT += L[x] / 1000; for (y in C) if (R[x, y]) DEPKL[x] += L[y] / 1000 } }
function cost(set,  a, kl, link) { kl = 0; link = 0; for (a in set) { kl += L[a] / 1000; link += LINK * DEPKL[a] / TOT }; KLC = kl; return kl * (LIB + TEST) + link + ROOT }
function flush(  x, a, n, any, cpu) { any = 0; for (x in T) if (x != "north") any = 1
  if (cur == "" || !any) return; delete S; n = 0
  for (x in T) { if (x == "ALL") { for (a in C) S[a] = 1; continue } if (x in C) for (a in C) if (R[a, x]) S[a] = 1 }
  for (a in S) n++; cpu = cost(S); commits++; KL += KLC; CPU += cpu; if (n == 1) single++
  if (KLC <= 40) b40++; else if (KLC <= 65) b65++; else if (KLC <= 110) b110++; else if (KLC <= 160) b160++; else bmax++
  for (x in T) if (x in C) N[x]++ }
FILENAME == ARGV[1] { if ($0 ~ /^#/ || NF < 3) next; C[$1] = 1; if ($2 != "-") { n = split($2, d, ","); for (i = 1; i <= n; i++) D[$1, d[i]] = 1 }
  for (i = 3; i <= NF; i++) P[$i] = $1; next }
FILENAME == ARGV[2] { if (!($1 in IN) || $2 < IN[$1]) IN[$1] = $2; next }
FILENAME == ARGV[3] { c = crate($3); if (c == "" || c == "north" || c == "ALL") next; L[c] += $1; if (!testfile($3, 0)) NT[c] += $2; next }
FILENAME == ARGV[4] { split($0, r, ":"); s = crate(r[1]); t = $0; sub(/^[^:]+:[0-9]+:crate::/, "", t); gsub(/::/, "/", t); t = crate("crates/north-core/src/" t)
  if (s == "" || t == "" || s == t) next; key = (testfile(r[1], r[2]) ? "test" : "prod") " " s " -> " t; BR[key] = BR[key] " " r[1] ":" r[2]; next }
FNR == 1 && FILENAME == ARGV[5] { closure() }
/^C / { flush(); cur = $2; delete T; next }
NF { c = crate($0); if (c != "") T[c] = 1 }
END { flush(); for (x in C) { deps = ""; nd = 0; delete S; for (a in C) { if (D[a, x]) nd++; if (R[a, x]) { S[a] = 1; if (a != x) deps = deps " " a } }
    printf "crate %s lines=%d nontest=%d direct_dependents=%d cap=%d commits=%d cpu_s=%.1f rebuilt_k=%.1f dependents=%s\n", x, L[x], NT[x], nd, nd >= 3 ? 10000 : 25000, N[x], cost(S), KLC, deps }
  for (k in BR) { split(k, q, " "); if (!R[q[2], q[4]]) printf "break %s:%s\n", k, BR[k] }
  printf "total commits=%d single_crate=%d kl_le40=%d kl_40_65=%d kl_65_110=%d kl_110_160=%d kl_gt160=%d rebuilt_klines=%.0f cpu_s=%.0f\n", commits, single, b40, b65, b110, b160, bmax, KL, CPU }
' "$plan" "$scratch/inline" "$scratch/lines" "$scratch/refs" "$scratch/log" | sort
