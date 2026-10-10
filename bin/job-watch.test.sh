#!/usr/bin/env bash
# job-watch: the scenario fixtures through the pure core, then one run against a scratch ledger,
# registry and peers file with a recording nexus-alert.
set -uo pipefail

repo=$(cd "$(dirname "$0")/.." && pwd)
scratch=$(mktemp -d "${TMPDIR:-/tmp}/job-watch-test.XXXXXX")
trap 'rm -rf "${scratch:?}"' EXIT
export THREADS_DB=$scratch/threads.db PATH=$scratch/bin:$PATH JOB_WATCH_HOSTS="laptop=lap nexus=hub"
unset JOB_WATCH_TOPIC

pass=0 fail=0 out=""
check() {
  if eval "$2"; then pass=$((pass + 1)); printf 'PASS  %s\n' "$1"
  else fail=$((fail + 1)); printf 'FAIL  %s\n%s\n' "$1" "$out"; fi
}

out=$("$repo/bin/job-watch" self-test 2>&1)
check '[scenario] fixtures: laptop off past a deadline, online but failing, recovered, suppressed, repeated after 24 h' \
  '[ "$(grep -c "^PASS" <<<"$out")" -eq 5 ] && grep -q "4 alerts; ok" <<<"$out"'

mkdir -p "$scratch/bin" "$scratch/reg"
printf '#!/usr/bin/env bash\nprintf "%%s|%%s|%%s\\n" "$@" >>"%s/sent"\necho id1\n' "$scratch" >"$scratch/bin/nexus-alert"
chmod +x "$scratch/bin/nexus-alert"
printf -- '---\nname: stale@nexus\nschedule: every 15 min\nplacement: nexus\ndeadline: 1h\n---\n' >"$scratch/reg/stale.md"
printf -- '---\nname: fresh\nschedule: hourly\nplacement: nexus\ndeadline: 1h\n---\n' >"$scratch/reg/fresh.md"
printf -- '---\nname: away\nschedule: daily\nplacement: laptop\ndeadline: 30min\n---\n' >"$scratch/reg/away.md"
printf -- '---\nname: unseen\nplacement: nexus\ndeadline: 1h\n---\n' >"$scratch/reg/unseen.md"
ago() { date -u -d "@$(($(date +%s) - $1))" +%Y-%m-%dT%H:%M:%SZ; }
"$repo/bin/threads" job-run stale --host hub --started "$(ago 7300)" --finished "$(ago 7200)" --rc 0
"$repo/bin/threads" job-run stale --host hub --started "$(ago 600)" --finished "$(ago 590)" --rc 1
"$repo/bin/threads" job-run fresh --host hub --started "$(ago 300)" --finished "$(ago 290)" --rc 0
"$repo/bin/threads" job-run away --host lap --started "$(ago 3700)" --finished "$(ago 3600)" --rc 0
printf '{"peers":{"laptop":{"handshake":%s,"online_since":null}}}\n' "$(($(date +%s) - 1800))" >"$scratch/peers.json"
jw() { "$repo/bin/job-watch" run --registry "$scratch/reg" --peers "$scratch/peers.json" --state "$scratch/state.json" 2>&1; }
out=$(jw; jw)
check '[integration] a run alerts each overdue ledger job once on the urgent topic, skips fresh and unrecorded jobs, and a second run sends nothing' \
  '[ "$(wc -l <"$scratch/sent")" -eq 2 ] && grep -q "^urgent|stale on nexus missed its deadline (1h)" "$scratch/sent" && grep -q "^urgent|away missed its deadline; laptop offline since .*; it runs on wake|Nexus: away overdue$" "$scratch/sent" && grep -q "4 jobs with deadlines, 3 with runs, 0 alerts" <<<"$out"'

printf 'job-watch: %d passed, %d failed\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
