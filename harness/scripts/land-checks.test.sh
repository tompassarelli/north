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
printf 'import sys\nsys.path.insert(0, "harness/lib")\ndef check():\n    from events import value\n    import events\ncheck()\n' >harness/lib/indented.test.py
printf 'import sys\nsys.path.insert(0, "harness/lib")\nif True: import events\n' >harness/lib/inline.test.py
printf '%s\n' 'python3 - <<"PY"' 'import sys' 'sys.path.insert(0, "harness/lib")' 'from events import value' 'PY' >harness/hooks/embedded.test.sh
printf 'import sys\nsys.path.insert(0, "harness/lib")\nimport bridge\nprint("transitive importer ran")\n' >harness/lib/transitive.test.py
printf 'import events_extra\nraise RuntimeError("unrelated test selected")\n' >harness/lib/unrelated.test.py
git add harness/lib/events.py harness/lib/landing-health.test.py harness/lib/bridge.py harness/lib/from.test.py harness/lib/multi.test.py harness/lib/indented.test.py harness/lib/inline.test.py harness/lib/transitive.test.py harness/lib/unrelated.test.py harness/hooks/embedded.test.sh
git commit -qm base
base=$(git rev-parse HEAD)
printf 'value = 2\n' >harness/lib/events.py
git add harness/lib/events.py
git commit -qm 'change events only'
output=$(SAFE_PUSH_LAND_BASE="$base" SAFE_PUSH_LAND_HEAD=HEAD bash "$script")
printf '%s\n' "$output"
for test in landing-health from multi indented inline; do
  [ "$(printf '%s\n' "$output" | grep -Fxc "land check: harness/lib/$test.test.py")" -eq 1 ]
done
[[ "$output" == *'importer ran'* && "$output" == *'transitive importer ran'* && "$output" != *unrelated.test.py* ]]
[[ "$output" == *'land check: harness/hooks/embedded.test.sh'* ]]
mkdir fake-bin
printf '#!/usr/bin/env bash\nfor arg; do [[ "$arg" != "$FAIL_RG" ]] || exit 2; done\nexec %q "$@"\n' "$(command -v rg)" >fake-bin/rg
chmod +x fake-bin/rg
for phase in --files -l -e; do
  if output=$(PATH="$scratch/fake-bin:$PATH" FAIL_RG="$phase" SAFE_PUSH_LAND_BASE="$base" SAFE_PUSH_LAND_HEAD=HEAD bash "$script" 2>&1); then
    printf 'FAIL: rg %s error was ignored\n' "$phase" >&2; exit 1
  fi
  [[ "$output" == *'land check: rg failed (status 2)'* ]]
done
if output=$(SAFE_PUSH_LAND_BASE=missing-ref SAFE_PUSH_LAND_HEAD=HEAD bash "$script" 2>&1); then
  printf 'FAIL: git error was ignored\n' >&2; exit 1
fi
[[ "$output" == *'land check: git diff failed'* ]]
printf 'printf "tool ran\\n"\n' >bin/tool; printf 'bash bin/tool\n' >lib/invoker.test.sh
printf 'bash bin/tool\n' >bin/tool.test.sh
printf 'printf "script ran\\n"\n' >harness/scripts/task.sh
printf 'bash harness/scripts/task.sh\n' >harness/hooks/caller.test.sh
git add bin/tool bin/tool.test.sh lib/invoker.test.sh harness/scripts/task.sh harness/hooks/caller.test.sh; git commit -qm 'add tool and script'
base=$(git rev-parse HEAD)
printf 'printf "tool changed\\n"\n' >bin/tool; printf 'printf "script changed\\n"\n' >harness/scripts/task.sh
git add bin/tool harness/scripts/task.sh; git commit -qm 'change tool and script'
output=$(SAFE_PUSH_LAND_BASE="$base" SAFE_PUSH_LAND_HEAD=HEAD bash "$script")
printf '%s\n' "$output"
for test in bin/tool.test.sh lib/invoker.test.sh harness/hooks/caller.test.sh; do
  [ "$(printf '%s\n' "$output" | grep -Fxc "land check: $test")" -eq 1 ]
done
printf 'PASS: land checks run direct and transitive importers and path callers once, excluding unrelated tests\n'
printf 'PASS: indented and inline importers selected; rg and git errors fail closed\n'

printf 'value = 1\n' >bin/widget; printf 'printf "widget test ran\\n"\n' >bin/widget.test.sh; printf 'raise SystemExit("longer filename selected")\nlauncher = "bin/widget-server"\n' >lib/long-name.test.py; git add bin/widget bin/widget.test.sh lib/long-name.test.py; git commit -qm 'add namespaced paths'; base=$(git rev-parse HEAD)
printf 'value = 2\n' >bin/widget; git add bin/widget; git commit -qm 'change widget'; output=$(SAFE_PUSH_LAND_BASE="$base" SAFE_PUSH_LAND_HEAD=HEAD bash "$script"); printf '%s\n' "$output"
[[ "$output" == *'land check: bin/widget.test.sh'* && "$output" != *'land check: lib/long-name.test.py'* ]] && printf 'PASS: path selection excludes longer filenames with a shared prefix\n'

mkdir -p harness/hooks/lib src
printf 'python3 "$(dirname "$0")/lib/decide.py"\n' >harness/hooks/gate.sh; printf 'import usage\n' >harness/hooks/lib/decide.py
printf 'value = 1\n' >harness/lib/usage.py; printf 'printf "gate test ran\\n"\n' >harness/hooks/gate.test.sh
printf 'subprocess.run([os.environ.get("NORTH_PACE_BIN", "north"), "pace"])\n' >harness/lib/pace_client.py; printf 'import pace_client\n' >harness/hooks/lib/paced.py
printf 'python3 "$(dirname "$0")/lib/paced.py"\n' >harness/hooks/paced.sh; printf 'printf "paced test ran\\n"\n' >harness/hooks/paced.test.sh
printf 'fn main() {}\n' >src/main.rs; printf 'north="$(dirname "$0")/../../target/debug/north"; printf "direct test ran\\n"\n' >harness/hooks/direct.test.sh; printf 'consumers = ["src/main.rs"]\n' >harness/registry.toml; printf 'print(open("harness/registry.toml").read())\n' >harness/lib/registry.test.py
printf 'binary_variable = "NORTH_BIN"\n' >harness/lib/ledger_client.py; printf 'import sys\nsys.path.insert(0, "harness/lib")\nimport ledger_client\nprint("ledger test ran")\n' >harness/lib/ledger.test.py
git add harness/hooks/gate.sh harness/hooks/lib/decide.py harness/lib/usage.py harness/hooks/gate.test.sh harness/lib/pace_client.py harness/hooks/lib/paced.py harness/hooks/paced.sh harness/hooks/paced.test.sh harness/hooks/direct.test.sh harness/lib/ledger_client.py harness/lib/ledger.test.py harness/registry.toml harness/lib/registry.test.py src/main.rs; git commit -qm 'add hooks over a library and the binary'; base=$(git rev-parse HEAD)
printf 'value = 2\n' >harness/lib/usage.py; git add harness/lib/usage.py; git commit -qm 'change the library a hook reaches'; output=$(SAFE_PUSH_LAND_BASE="$base" SAFE_PUSH_LAND_HEAD=HEAD bash "$script"); printf '%s\n' "$output"
[[ "$output" == *'land check: harness/hooks/gate.test.sh'* && "$output" != *'land check: harness/hooks/paced.test.sh'* ]]
base=$(git rev-parse HEAD); printf 'fn main() { }\n' >src/main.rs; git add src/main.rs; git commit -qm 'change the binary'; output=$(SAFE_PUSH_LAND_BASE="$base" SAFE_PUSH_LAND_HEAD=HEAD bash "$script"); printf '%s\n' "$output"
[[ "$output" == *'land check: harness/hooks/paced.test.sh'* && "$output" != *'land check: harness/hooks/gate.test.sh'* && "$output" == *'land check: harness/hooks/direct.test.sh'* && "$output" == *'land check: harness/lib/ledger.test.py'* && "$output" == *'land check: harness/lib/registry.test.py'* ]]
base=$(git rev-parse HEAD); printf 'exit 1\n' >harness/hooks/gate.test.sh; printf 'value = 3\n' >harness/lib/usage.py; git add harness/lib/usage.py harness/hooks/gate.test.sh; git commit -qm 'change the library while its hook test is red'
if SAFE_PUSH_LAND_BASE="$base" SAFE_PUSH_LAND_HEAD=HEAD bash "$script" >/dev/null 2>&1; then printf 'FAIL: a red hook test reached through a library passed\n' >&2; exit 1; fi
printf 'PASS: a hook test runs when a library its hook imports or the north binary it calls changes, and a red one fails the check\n'
