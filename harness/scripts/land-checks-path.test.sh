#!/usr/bin/env bash
set -euo pipefail
script=$(realpath "$(dirname "$0")/land-checks.sh")
scratch=$(mktemp -d)
trap 'rm -rf "${scratch:?}"' EXIT
git -C "$scratch" init -q
git -C "$scratch" config user.name 'Land path fixture'
git -C "$scratch" config user.email 'fixture@example.invalid'
git -C "$scratch" config core.hooksPath /dev/null
cd "$scratch"
mkdir -p bin lib harness/hooks harness/lib harness/scripts
git_dir=$(dirname "$(command -v git)")
ln -s "$git_dir" "$scratch/profile-bin"
printf 'value = 1\n' >bin/vanish
printf 'set -e\nrm -f "$PROFILE_LINK"\ngit --version >/dev/null\nrg --version >/dev/null\nprintf "tools survived the switch\\n"\n' >bin/vanish.test.sh
git add bin/vanish bin/vanish.test.sh
git commit -qm 'add vanish'
base=$(git rev-parse HEAD)
printf 'value = 2\n' >bin/vanish
git add bin/vanish
git commit -qm 'change vanish'
trimmed=$(printf %s "$PATH" | tr : '\n' | grep -vxF "$git_dir" | paste -sd:)
output=$(PROFILE_LINK="$scratch/profile-bin" PATH="$scratch/profile-bin:$trimmed" SAFE_PUSH_LAND_BASE="$base" SAFE_PUSH_LAND_HEAD=HEAD bash "$script" 2>&1)
printf '%s\n' "$output"
[[ "$output" == *'tools survived the switch'* ]]
printf 'PASS: land checks keep resolving git and their tools after a profile directory on PATH is removed mid-run\n'
