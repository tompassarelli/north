#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
RUNNER="$ROOT/bin/test-suite"
scratch="$(mktemp -d -t north-test-suite-test.XXXXXX)"
cleanup() {
  rm -rf "${scratch:?}"
}
trap cleanup EXIT

normal_home="$scratch/normal-home"
mkdir -p "$normal_home"
# shellcheck disable=SC2016 # The child shell must read the runner's environment.
normal="$(env -u AGENT_MACHINERY_HOME HOME="$normal_home" "$RUNNER" -- \
  bash -c 'printf "%s|%s" "$HOME" "${NORTH_TEST_SANDBOX_HOME:-unset}"' \
  2>"$scratch/normal.err")"
[[ "$normal" == "$normal_home|unset" ]]
[[ ! -s "$scratch/normal.err" ]]

# The invocation is authoritative: an inherited sandbox flag never makes a
# normal-mode run report sandbox mode, which is what a nested run inside
# `test-suite --sandbox-home` does.
# shellcheck disable=SC2016 # The child shell must read the runner's environment.
nested="$(env -u AGENT_MACHINERY_HOME HOME="$normal_home" NORTH_TEST_SANDBOX_HOME=1 "$RUNNER" -- \
  bash -c 'printf "%s|%s" "$HOME" "${NORTH_TEST_SANDBOX_HOME:-unset}"' \
  2>"$scratch/nested.err")"
[[ "$nested" == "$normal_home|unset" ]]
[[ ! -s "$scratch/nested.err" ]]

probe="$scratch/probe.sh"
sed 's/^+//' >"$probe" <<'EOF'
+#!/usr/bin/env bash
+set -euo pipefail
+[[ -d "$HOME" ]]
+[[ -z "$(find "$HOME" -mindepth 1 -print -quit)" ]]
+[[ "${NORTH_TEST_SANDBOX_HOME:-}" == 1 ]]
+[[ -z "${XDG_CACHE_HOME:-}" ]]
+[[ -z "${XDG_CONFIG_HOME:-}" ]]
+[[ -z "${XDG_DATA_HOME:-}" ]]
+[[ -z "${XDG_STATE_HOME:-}" ]]
+[[ -z "${BEAGLE_STORE_LOG:-}" ]]
+[[ -z "${NORTH_STATE_ROOT:-}" ]]
+[[ -z "${CODEX_HOME:-}" ]]
+printf 'probe-home=%s\n' "$HOME"
EOF
chmod +x "$probe"

HOME="$normal_home" \
XDG_CACHE_HOME="$normal_home/cache" \
XDG_CONFIG_HOME="$normal_home/config" \
XDG_DATA_HOME="$normal_home/data" \
XDG_STATE_HOME="$normal_home/state" \
BEAGLE_STORE_LOG="$normal_home/facts.log" \
NORTH_STATE_ROOT="$normal_home/north-state" \
CODEX_HOME="$normal_home/codex" \
  env -u AGENT_MACHINERY_HOME "$RUNNER" --sandbox-home -- "$probe" >"$scratch/sandbox.out"
grep -Eq '^sandbox-home: HOME=/tmp/north-test-home\.[[:alnum:]]+$' "$scratch/sandbox.out"
grep -Eq '^probe-home=/tmp/north-test-home\.[[:alnum:]]+$' "$scratch/sandbox.out"

agent_machinery_source="$scratch/agent-machinery-source"
mkdir -p "$agent_machinery_source/staffing"
printf '{}\n' >"$agent_machinery_source/staffing/catalog.json"
projection_probe="$scratch/projection-probe.sh"
sed 's/^+//' >"$projection_probe" <<'EOF'
+#!/usr/bin/env bash
+set -euo pipefail
+[[ "$AGENT_MACHINERY_HOME" == "$HOME/.north-test-dependencies/agent-machinery" ]]
+[[ -L "$AGENT_MACHINERY_HOME" ]]
+[[ -r "$AGENT_MACHINERY_HOME/staffing/catalog.json" ]]
+printf 'agent-machinery-target=%s\n' "$(readlink "$AGENT_MACHINERY_HOME")"
EOF
chmod +x "$projection_probe"
AGENT_MACHINERY_HOME="$agent_machinery_source" \
  "$RUNNER" --sandbox-home -- "$projection_probe" >"$scratch/projection.out"
grep -Fxq "agent-machinery-target=$agent_machinery_source" "$scratch/projection.out"

set +e
env -u AGENT_MACHINERY_HOME \
  "$RUNNER" --sandbox-home -- bash -c 'exit 17' >"$scratch/fail.out" 2>"$scratch/fail.err"
status=$?
set -e
[[ "$status" == 17 ]]
failed_home="$(sed -n 's/^sandbox-home: HOME=//p' "$scratch/fail.out")"
[[ -n "$failed_home" && ! -e "$failed_home" ]]

printf 'test-suite: normal mode unchanged; sandbox HOME isolates state and projects an explicit package root; exit preserved: PASS\n'
