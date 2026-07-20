#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

if (($# != 0)); then
  echo "SDK test runner accepts no passthrough arguments; use the canonical bounded run" >&2
  exit 2
fi

file_timeout_s="${NORTH_TEST_SDK_FILE_TIMEOUT_SECONDS:-180}"
kill_after_s="${NORTH_TEST_SDK_KILL_AFTER_SECONDS:-5}"
installed_smoke="${NORTH_RUN_INSTALLED_CODEX_SIGNAL_SMOKE:-0}"
for setting in \
  "NORTH_TEST_SDK_FILE_TIMEOUT_SECONDS:$file_timeout_s" \
  "NORTH_TEST_SDK_KILL_AFTER_SECONDS:$kill_after_s"; do
  name="${setting%%:*}"
  value="${setting#*:}"
  if [[ ! "$value" =~ ^[1-9][0-9]*$ ]]; then
    echo "$name must be a positive integer" >&2
    exit 2
  fi
done
if [[ "$installed_smoke" != 0 && "$installed_smoke" != 1 ]]; then
  echo "NORTH_RUN_INSTALLED_CODEX_SIGNAL_SMOKE must be 0 or 1" >&2
  exit 2
fi
if ! command -v timeout >/dev/null 2>&1; then
  echo "SDK tests require GNU timeout for per-file process containment" >&2
  exit 2
fi

mapfile -d '' -t files < <(
  find ./test -maxdepth 1 -type f -name '*.test.ts' -print0 | LC_ALL=C sort -z
)
if ((${#files[@]} == 0)); then
  echo "SDK test runner found no ./test/*.test.ts files" >&2
  exit 2
fi

scratch="$(mktemp -d -t north-sdk-tests.XXXXXX)"
cleanup() {
  rm -rf "${scratch:?}"
}
trap cleanup EXIT

summary_count() {
  local file="$1" pattern="$2" default_value="$3" label="$4"
  local matches=()
  mapfile -t matches < <(grep -E "$pattern" "$file" || true)
  if ((${#matches[@]} == 0)); then
    printf '%s\n' "$default_value"
    return 0
  fi
  if ((${#matches[@]} != 1)); then
    echo "ambiguous $label summary" >&2
    return 1
  fi
  if [[ "${matches[0]}" =~ ([0-9]+) ]]; then
    printf '%s\n' "${BASH_REMATCH[1]}"
    return 0
  fi
  echo "malformed $label summary" >&2
  return 1
}

started_s="$SECONDS"
total_pass=0
total_skip=0
total_expect=0
total_tests=0
index=0

for file in "${files[@]}"; do
  ((index += 1))
  printf -v log '%s/%03d.log' "$scratch" "$index"
  set +e
  timeout --kill-after="${kill_after_s}s" "${file_timeout_s}s" \
    bun test --isolate --preload ./test/support/hermetic-preload.ts \
      --only-failures "$file" >"$log" 2>&1
  status=$?
  set -e
  if ((status != 0)); then
    printf 'FAILED %s (status %d)\n' "$file" "$status" >&2
    cat "$log" >&2
    exit "$status"
  fi

  normalized="$scratch/$index.normalized"
  tr -d '\r' <"$log" >"$normalized"
  if ! pass="$(summary_count "$normalized" '^[[:space:]]*[0-9]+ pass$' -1 pass)" || \
     ! skip="$(summary_count "$normalized" '^[[:space:]]*[0-9]+ skip$' 0 skip)" || \
     ! fail="$(summary_count "$normalized" '^[[:space:]]*[0-9]+ fail$' -1 fail)" || \
     ! expect="$(summary_count "$normalized" '^[[:space:]]*[0-9]+ expect\(\) calls?$' 0 expect)" || \
     ! ran="$(summary_count "$normalized" '^Ran [0-9]+ tests? across 1 file\. \[[^]]+\]$' -1 ran)"; then
    printf 'FAILED %s (unparseable Bun summary)\n' "$file" >&2
    cat "$log" >&2
    exit 1
  fi
  if ((pass < 0 || fail != 0 || ran < 0 || pass + skip != ran)); then
    printf 'FAILED %s (incoherent Bun summary: pass=%d skip=%d fail=%d ran=%d)\n' \
      "$file" "$pass" "$skip" "$fail" "$ran" >&2
    cat "$log" >&2
    exit 1
  fi

  total_pass=$((total_pass + pass))
  total_skip=$((total_skip + skip))
  total_expect=$((total_expect + expect))
  total_tests=$((total_tests + ran))
  printf 'ok %02d %s pass=%d skip=%d\n' "$index" "$file" "$pass" "$skip"
done

if [[ "$installed_smoke" == 1 ]] && \
   ((total_skip != 0 || total_pass != total_tests)); then
  printf 'SDK tests require zero skips with the installed-Codex smoke: pass=%d skip=%d tests=%d\n' \
    "$total_pass" "$total_skip" "$total_tests" >&2
  exit 1
fi

elapsed_s=$((SECONDS - started_s))
printf 'SDK tests: %d files · %d pass · %d skip · 0 fail · %d expects · %ds\n' \
  "${#files[@]}" "$total_pass" "$total_skip" "$total_expect" "$elapsed_s"
