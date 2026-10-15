#!/usr/bin/env bash
set -euo pipefail
script=$(realpath "$(dirname "$0")/land-checks.sh")
scratch=$(mktemp -d)
trap 'rm -rf "${scratch:?}"' EXIT
git -C "$scratch" init -q
git -C "$scratch" config user.name 'Land selection fixture'
git -C "$scratch" config user.email 'fixture@example.invalid'
git -C "$scratch" config core.hooksPath /dev/null
cd "$scratch"
mkdir -p bin lib harness/hooks harness/lib harness/scripts
printf 'value = 1\n' >harness/lib/events.py
printf 'import sys\nsys.path.insert(0, "harness/lib")\nimport events\nprint("importer ran")\n' >harness/lib/landing-health.test.py
printf 'from events import value\n' >harness/lib/bridge.py
printf 'import sys\nsys.path.insert(0, "harness/lib")\nfrom events import value\n' >harness/lib/from.test.py
printf 'import sys\nsys.path.insert(0, "harness/lib")\nimport os, events as renamed\n' >harness/lib/multi.test.py
printf '%s\n' 'python3 - <<"PY"' 'import sys' 'sys.path.insert(0, "harness/lib")' 'from events import value' 'PY' >harness/hooks/embedded.test.sh
printf 'import bridge\nraise RuntimeError("transitive importer selected")\n' >harness/lib/transitive.test.py
printf 'import events_extra\nraise RuntimeError("unrelated test selected")\n' >harness/lib/unrelated.test.py
git add harness/lib/events.py harness/lib/landing-health.test.py harness/lib/bridge.py harness/lib/from.test.py harness/lib/multi.test.py harness/lib/transitive.test.py harness/lib/unrelated.test.py harness/hooks/embedded.test.sh
git commit -qm base
base=$(git rev-parse HEAD)
printf 'value = 2\n' >harness/lib/events.py
git add harness/lib/events.py
git commit -qm 'change events only'
output=$(SAFE_PUSH_LAND_BASE="$base" SAFE_PUSH_LAND_HEAD=HEAD bash "$script")
printf '%s\n' "$output"
for test in landing-health from multi; do
  [ "$(printf '%s\n' "$output" | grep -Fxc "land check: harness/lib/$test.test.py")" -eq 1 ]
done
[[ "$output" == *'importer ran'* && "$output" != *transitive.test.py* && "$output" != *unrelated.test.py* ]]
[[ "$output" == *'land check: harness/hooks/embedded.test.sh'* ]]
printf 'printf "tool ran\\n"\n' >bin/tool
printf 'bash bin/tool\n' >lib/invoker.test.sh
printf 'bash bin/tool\n' >bin/tool.test.sh
printf 'printf "script ran\\n"\n' >harness/scripts/task.sh
printf 'bash harness/scripts/task.sh\n' >harness/hooks/caller.test.sh
git add bin/tool bin/tool.test.sh lib/invoker.test.sh harness/scripts/task.sh harness/hooks/caller.test.sh
git commit -qm 'add tool and script'
base=$(git rev-parse HEAD)
printf 'printf "tool changed\\n"\n' >bin/tool
printf 'printf "script changed\\n"\n' >harness/scripts/task.sh
git add bin/tool harness/scripts/task.sh
git commit -qm 'change tool and script'
output=$(SAFE_PUSH_LAND_BASE="$base" SAFE_PUSH_LAND_HEAD=HEAD bash "$script")
printf '%s\n' "$output"
for test in bin/tool.test.sh lib/invoker.test.sh harness/hooks/caller.test.sh; do
  [ "$(printf '%s\n' "$output" | grep -Fxc "land check: $test")" -eq 1 ]
done
printf 'PASS: land checks run direct importers and path callers once, excluding unrelated and transitive tests\n'
