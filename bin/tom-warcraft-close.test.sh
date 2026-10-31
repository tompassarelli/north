#!/usr/bin/env bash
set -uo pipefail

here=$(cd "$(dirname "$0")" && pwd)
tmp=$(mktemp -d)
standins=()
trap 'kill -KILL "${standins[@]}" 2>/dev/null; rm -rf -- "$tmp"' EXIT
pass=0 fail=0
appid=999999$$

check() {
  local name=$1 want=$2 got=$3
  if [ "$got" = "$want" ]; then
    pass=$((pass + 1)); echo "PASS  $name"
  else
    fail=$((fail + 1)); echo "FAIL  $name"; echo "  want: $want"; echo "  got:  $got"
  fi
}

alive() {
  local pid count=0
  for pid in "$@"; do kill -0 "$pid" 2>/dev/null && count=$((count + 1)); done
  echo "$count"
}

close() {
  TOM_WARCRAFT_COMPAT=$tmp/compat TOM_WARCRAFT_APPID=$appid "$here/tom-warcraft-close" "$@"
}

pad=$(head -c 60000 /dev/zero | tr '\0' x)
sleep=$(command -v sleep)
env -i STEAM_COMPAT_DATA_PATH="$tmp/compat" PAD1="$pad" PAD2="$pad" "$sleep" 300 &
standins+=($!)
# shellcheck disable=SC2016
env -i STEAM_COMPAT_DATA_PATH="$tmp/compat" PAD1="$pad" bash -c '"$0" -i "$1" 300 & wait' "$(command -v env)" "$sleep" &
standins+=($!)
bash -c 'sleep 300 & wait' reaper SteamLaunch "AppId=$appid" -- &
standins+=($!)
env -i STEAM_COMPAT_DATA_PATH="$tmp/other-compat" "$sleep" 300 &
bystander=$!
standins+=("$bystander")
sleep 0.5
launch=$(close --list)
check "the launch is every process with Tom's compat prefix in a large environment, the reaper's AppId, and their descendants" \
  "5" "$(wc -w <<<"$launch")"
out=$(close 2>&1)
check "close exits 0 and says a process check found none left" \
  "0 tom-warcraft-close: closed 5 processes of Tom's Steam launch; a process check finds none left" "$? $out"
# shellcheck disable=SC2086
check "no process of the launch survives" "0" "$(alive $launch)"
check "a process of another prefix is left running" "1" "$(alive "$bystander")"

proc=$tmp/proc
mkdir -p "$proc/4194399"
printf '4194399 (Battle.net.exe) S 1 1 1 0 -1 0\n' > "$proc/4194399/stat"
printf 'STEAM_COMPAT_DATA_PATH=%s\0' "$tmp/compat" > "$proc/4194399/environ"
printf 'C:\\Program Files (x86)\\Battle.net\\Battle.net.exe\0' > "$proc/4194399/cmdline"
err=$(TOM_WARCRAFT_PROC=$proc close 2>&1 >/dev/null)
check "close fails when a process of the launch survives KILL" "1" "$?"
check "the failure names the survivor" "1" "$(rg -c '^  4194399 C:\\Program Files \(x86\)\\Battle\.net\\Battle\.net\.exe $' <<<"$err")"

echo "tom-warcraft-close: $pass passed, $fail failed"
[ "$fail" -eq 0 ]
