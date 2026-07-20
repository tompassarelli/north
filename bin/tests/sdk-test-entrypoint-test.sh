#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
RUNNER="$ROOT/sdk/test/support/run-suite.sh"

grep -Fq '"test": "bash ./test/support/run-suite.sh"' \
  "$ROOT/sdk/package.json"
grep -Fq 'cd ~/code/north/sdk && bun run check && bun run test' "$ROOT/AGENTS.md"
grep -Eq '^[[:space:]]+bun run test[[:space:]]*$' "$ROOT/.github/workflows/ci.yml"
grep -Fq "find ./test -maxdepth 1 -type f -name '*.test.ts' -print0" "$RUNNER"
grep -Fq 'SDK test runner accepts no passthrough arguments' "$RUNNER"
# These are literal source contracts, not expansions in this process.
# shellcheck disable=SC2016
grep -Fq 'timeout --kill-after="${kill_after_s}s" "${file_timeout_s}s"' "$RUNNER"
# shellcheck disable=SC2016
grep -Fq -- '--only-failures "$file" >"$log" 2>&1' "$RUNNER"
grep -Fq 'NORTH_RUN_INSTALLED_CODEX_SIGNAL_SMOKE' "$RUNNER"

if grep -Eq '^[[:space:]]+bun test[[:space:]]*$' "$ROOT/.github/workflows/ci.yml"; then
  echo "CI bypasses the SDK package's hermetic test entrypoint" >&2
  exit 1
fi
if grep -Fq 'bun test ./test' "$ROOT/AGENTS.md"; then
  echo "AGENTS.md recommends a non-hermetic SDK test command" >&2
  exit 1
fi

tmp="$(mktemp -d -t north-sdk-entrypoint.XXXXXX)"
cleanup() {
  rm -rf "${tmp:?}"
}
trap cleanup EXIT

mkdir -p "$tmp/bin" "$tmp/sdk/test/support"
cp "$RUNNER" "$tmp/sdk/test/support/run-suite.sh"
touch "$tmp/sdk/test/a.test.ts" "$tmp/sdk/test/b.test.ts"
# The generated fake expands these variables when the runner invokes it.
# shellcheck disable=SC2016
printf '%s\n' \
  '#!/usr/bin/env bash' \
  'printf "%s\n" "$*" >> "${NORTH_SDK_TEST_TRACE:?}"' \
  'file="${*: -1}"' \
  'case "${NORTH_SDK_FAKE_MODE:-pass}:$file" in' \
  '  hang:*hang.test.ts)' \
  '    printf "%s\n" "$$" > "${NORTH_SDK_TEST_PID:?}"' \
  "    trap '' TERM" \
  '    while :; do sleep 30; done' \
  '    ;;' \
  '  skip:*)' \
  '    printf "bun test v1.3.13\n\n%s:\n(skip) fixture\n\n 0 pass\n 1 skip\n 0 fail\nRan 1 test across 1 file. [1.00ms]\n" "$file"' \
  '    ;;' \
  '  *)' \
  '    printf "bun test v1.3.13\n\n%s:\n\n 1 pass\n 0 fail\n 2 expect() calls\nRan 1 test across 1 file. [1.00ms]\n" "$file"' \
  '    ;;' \
  'esac' \
  >"$tmp/bin/bun"
chmod +x "$tmp/bin/bun"

trace="$tmp/trace"
if ! PATH="$tmp/bin:$PATH" NORTH_SDK_TEST_TRACE="$trace" \
  bash "$tmp/sdk/test/support/run-suite.sh" >"$tmp/pass.out"; then
  cat "$tmp/pass.out" >&2
  exit 1
fi
[[ "$(wc -l <"$trace")" -eq 2 ]]
grep -Fxq 'test --isolate --preload ./test/support/hermetic-preload.ts --only-failures ./test/a.test.ts' "$trace"
grep -Fxq 'test --isolate --preload ./test/support/hermetic-preload.ts --only-failures ./test/b.test.ts' "$trace"
grep -Eq '^SDK tests: 2 files · 2 pass · 0 skip · 0 fail · 4 expects · [0-9]+s$' "$tmp/pass.out"

mv "$tmp/sdk/test/b.test.ts" "$tmp/sdk/test/hang.test.ts"
: >"$trace"
start_s="$SECONDS"
set +e
PATH="$tmp/bin:$PATH" NORTH_SDK_TEST_TRACE="$trace" NORTH_SDK_TEST_PID="$tmp/hang.pid" \
  NORTH_SDK_FAKE_MODE=hang NORTH_TEST_SDK_FILE_TIMEOUT_SECONDS=1 \
  NORTH_TEST_SDK_KILL_AFTER_SECONDS=1 \
  bash "$tmp/sdk/test/support/run-suite.sh" >"$tmp/hang.out" 2>"$tmp/hang.err"
status=$?
set -e
((status == 124 || status == 137))
((SECONDS - start_s < 10))
grep -Eq '^FAILED \./test/hang\.test\.ts \(status (124|137)\)$' "$tmp/hang.err"
hang_pid="$(<"$tmp/hang.pid")"
[[ "$hang_pid" =~ ^[1-9][0-9]*$ ]]
if kill -0 "$hang_pid" 2>/dev/null; then
  echo "SDK runner left its timed-out child alive: $hang_pid" >&2
  exit 1
fi

PATH="$tmp/bin:$PATH" NORTH_SDK_TEST_TRACE="$trace" NORTH_SDK_FAKE_MODE=skip \
  bash "$tmp/sdk/test/support/run-suite.sh" >"$tmp/local-skip.out"
grep -Eq '^SDK tests: 2 files · 0 pass · 2 skip · 0 fail · 0 expects · [0-9]+s$' \
  "$tmp/local-skip.out"

set +e
PATH="$tmp/bin:$PATH" NORTH_SDK_TEST_TRACE="$trace" NORTH_SDK_FAKE_MODE=skip \
  NORTH_RUN_INSTALLED_CODEX_SIGNAL_SMOKE=1 \
  bash "$tmp/sdk/test/support/run-suite.sh" >"$tmp/skip.out" 2>"$tmp/skip.err"
status=$?
set -e
((status != 0))
grep -Fq 'SDK tests require zero skips with the installed-Codex smoke' "$tmp/skip.err"

set +e
PATH="$tmp/bin:$PATH" NORTH_SDK_TEST_TRACE="$trace" \
  NORTH_RUN_INSTALLED_CODEX_SIGNAL_SMOKE=yes \
  bash "$tmp/sdk/test/support/run-suite.sh" >"$tmp/invalid.out" 2>"$tmp/invalid.err"
status=$?
set -e
((status == 2))
grep -Fq 'NORTH_RUN_INSTALLED_CODEX_SIGNAL_SMOKE must be 0 or 1' "$tmp/invalid.err"
