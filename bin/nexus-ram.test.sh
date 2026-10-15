#!/usr/bin/env bash
# Recorded scenarios through the report: seven days of one-minute rows with a
# 15-minute PSI-full or low-MemAvailable episode on 3 days print UPGRADE; the
# quiet week, two episode days, a 9-minute episode on 3 days and an OOM kill of
# a non-agent print STAY; an OOM kill of an agent alone prints UPGRADE. Then
# `record` reads a fake /proc with a resident claude, a -p one-shot, a codex
# under bwrap with a child, and a spoofed-argv0 claude.
set -uo pipefail
target="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/nexus-ram"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/nexus-ram-test.XXXXXX")"
trap 'rm -rf "${scratch:?}"' EXIT
pass=0 fail=0
check() { if [ "$1" = ok ]; then pass=$((pass + 1)); printf 'PASS  %s\n' "$2"; else fail=$((fail + 1)); printf 'FAIL  %s\n      %s\n' "$2" "${3:-}"; fi; }

python3 - "$scratch" <<'PY'
import json, os, sys
from datetime import datetime, timedelta, timezone
root = sys.argv[1]
start = datetime(2026, 10, 7, 0, 0, tzinfo=timezone.utc)

def week(name, episode_days, minutes, kind):
    d = f"{root}/{name}"
    os.makedirs(d)
    for day in range(7):
        rows = []
        for m in range(1440):
            t = start + timedelta(days=day, minutes=m)
            hot = day in episode_days and 600 <= m < 600 + minutes
            rows.append({"ts": t.strftime("%Y-%m-%dT%H:%M:%SZ"),
                "mem_available_mib": 400 if hot and kind == "mem" else 2500 + m % 7,
                "swap_used_mib": 40, "zram_used_mib": 20, "zram_compressed_mib": 7,
                "psi_some_avg60": 4.0 if hot else 0.0, "psi_full_avg60": 2.5 if hot and kind == "psi" else 0.0,
                "agents": 5 if hot else 3, "agent_pss_mib": 2100 if hot else 1200})
        with open(f"{d}/{t.strftime('%Y-%m-%d')}.jsonl", "w") as out:
            out.writelines(json.dumps(r) + "\n" for r in rows)

week("psi3", {1, 3, 5}, 15, "psi")
week("mem3", {0, 2, 6}, 15, "mem")
week("quiet", set(), 0, "psi")
week("psi2", {1, 3}, 15, "psi")
week("short3", {1, 3, 5}, 9, "psi")
with open(f"{root}/journal-agent", "w") as out:
    out.write('2026-10-10T10:00:01+00:00 nexus kernel: Out of memory: Killed process 4242 (claude) total-vm:9000000kB\n')
    out.write('2026-09-01T10:00:01+00:00 nexus kernel: Out of memory: Killed process 1 (claude) total-vm:1kB\n')
with open(f"{root}/journal-other", "w") as out:
    out.write('2026-10-10T10:00:01+00:00 nexus kernel: Out of memory: Killed process 4243 (postgres) total-vm:9000000kB\n')
    out.write('2026-10-11T10:00:01+00:00 nexus earlyoom[519]: sending SIGTERM to process 77 uid 1000 "node": oom_score 300, VmRSS 900 MB, cmdline "node fixture-app"\n')
open(f"{root}/journal-none", "w").close()
PY

now=$(date -u -d 2026-10-14T00:00:00Z +%s)
report() { NEXUS_RAM_DIR="$scratch/$1" NEXUS_RAM_NOW=$now NEXUS_RAM_JOURNAL="$scratch/${2:-journal-none}" "$target" report --days 7; }
verdict() { report "$@" | rg '^verdict:' | cut -d' ' -f2; }

[ "$(verdict psi3)" = UPGRADE ] && check ok '15-minute PSI-full episode on 3 days prints UPGRADE' || check bad 'psi3' "$(report psi3)"
[ "$(verdict mem3)" = UPGRADE ] && check ok '15-minute MemAvailable < 600 MiB on 3 days prints UPGRADE' || check bad 'mem3' "$(report mem3)"
[ "$(verdict quiet)" = STAY ] && check ok 'quiet week prints STAY' || check bad 'quiet' "$(report quiet)"
[ "$(verdict psi2)" = STAY ] && check ok 'episodes on only 2 days print STAY' || check bad 'psi2' "$(report psi2)"
[ "$(verdict short3)" = STAY ] && check ok '9-minute episodes on 3 days print STAY' || check bad 'short3' "$(report short3)"
{ [ "$(verdict quiet journal-agent)" = UPGRADE ] && report quiet journal-agent | rg -q '^OOM kills: 1 \(1 hit an agent\)'; } \
  && check ok 'an OOM kill of claude in the window prints UPGRADE and the out-of-window one is ignored' || check bad 'oom agent' "$(report quiet journal-agent)"
{ [ "$(verdict quiet journal-other)" = STAY ] && report quiet journal-other | rg -q '^OOM kills: 2 \(0 hit an agent\)'; } \
  && check ok 'OOM kills of non-agents print STAY' || check bad 'oom other' "$(report quiet journal-other)"

full="$(report psi3)"
for pattern in '^window: .* 10080 rows' '^agent CLIs: p50 3 p95 3 max 5' '^agent PSS MiB: p50 1200 p95 1200 max 2100' \
  '^min MemAvailable MiB: 2500' '^minutes with PSI full avg60 > 1%: 45 '; do
  rg -q -- "$pattern" <<<"$full" && check ok "report line $pattern" || check bad "report line $pattern" "$full"
done

proc="$scratch/proc"
mkdir -p "$proc/pressure"
printf 'MemTotal: 8000000 kB\nMemAvailable: 6144000 kB\nSwapTotal: 12000000 kB\nSwapFree: 11900000 kB\n' >"$proc/meminfo"
printf 'some avg10=0.00 avg60=1.50 avg300=0.00 total=1\nfull avg10=0.00 avg60=0.25 avg300=0.00 total=1\n' >"$proc/pressure/memory"
printf '104857600 52428800 62914560 0 70000000 74 0\n' >"$scratch/mm_stat"
fakeproc() {
  local pid=$1 ppid=$2 comm=$3 pss=$4; shift 4
  mkdir -p "$proc/$pid"
  printf '%s (%s) S %s 1 1 0\n' "$pid" "$comm" "$ppid" >"$proc/$pid/stat"
  printf '%s\0' "$@" >"$proc/$pid/cmdline"
  printf 'Rss: 1 kB\nPss: %s kB\n' "$pss" >"$proc/$pid/smaps_rollup"
}
fakeproc 100 1 claude 204800 /x/bin/claude --resume abc
fakeproc 101 100 bash 10240 bash -c sleep
fakeproc 104 101 claude 10240 /x/bin/claude --nested
fakeproc 102 1 claude 99999 /x/bin/claude -p one-shot
fakeproc 103 1 claude 99999 ugrep -G pattern
fakeproc 200 1 bash 1024 bash /x/codex
fakeproc 201 200 bwrap 2048 bwrap -- codex
fakeproc 202 201 codex 307200 /x/bin/codex -c a=b
fakeproc 203 202 codex-code-mode 51200 /x/codex-code-mode-host
fakeproc 300 1 codex 99999 /x/bin/codex exec task
fakeproc 400 1 claude 99999 /x/bin/claude --cloud task
NEXUS_RAM_DIR="$scratch/rec" NEXUS_RAM_PROC="$proc" NEXUS_RAM_ZRAM="$scratch/mm_stat" NEXUS_RAM_NOW=1792000000 "$target" record
file="$(ls "$scratch"/rec/*.jsonl 2>/dev/null)"
{ [ "$(basename "$file")" = 2026-10-14.jsonl ] && [ "$(wc -l <"$file")" = 1 ]; } && check ok 'record appends one row to the UTC day file' || check bad 'record file' "$file"
expected='{"ts":"2026-10-14T17:46:40Z","mem_available_mib":6000,"swap_used_mib":97,"zram_used_mib":100,"zram_compressed_mib":50,"psi_some_avg60":1.5,"psi_full_avg60":0.25,"agents":2,"agent_pss_mib":570}'
[ "$(cat "$file")" = "$expected" ] && check ok 'row counts 2 resident agents with tree PSS 570 MiB and a nested claude not counted twice, excluding -p, exec, --cloud and spoofed argv0' || check bad 'record row' "$(cat "$file")"

printf '\n%s passed, %s failed\n' "$pass" "$fail"
[ "$fail" = 0 ]
