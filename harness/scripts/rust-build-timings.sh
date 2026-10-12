#!/usr/bin/env bash
set -euo pipefail

usage() { printf 'usage: %s [--dry-run] [--label NAME] REPO FILE\n' "${0##*/}" >&2; exit 2; }
dry=0 label=default args=("$@")
while [ $# -gt 0 ]; do
  case "$1" in
    --dry-run) dry=1; shift ;;
    --label) [ $# -ge 2 ] || usage; label="$2"; shift 2 ;;
    -*) usage ;;
    *) break ;;
  esac
done
[ $# -eq 2 ] || usage
repo="$(cd "$1" && pwd)"
file="$repo/${2#"$repo"/}"
[ -f "$file" ] || { printf 'no such file: %s\n' "$file" >&2; exit 2; }

if [ "$dry" = 0 ] && [ -z "${RUST_BUILD_TIMINGS_SHELL:-}" ]; then
  exec nix develop "$repo" -c env RUST_BUILD_TIMINGS_SHELL=1 bash "${BASH_SOURCE[0]}" "${args[@]}"
fi

target="$(mktemp -d "${TMPDIR:-/tmp}/rust-build-timings.XXXXXX")"
size="$(wc -c <"$file")"
trap 'truncate -s "$size" "$file"; rm -rf "$target"' EXIT
export CARGO_TARGET_DIR="$target"

step() {
  local name="$1"; shift
  if [ "$dry" = 1 ]; then
    printf '%s\tCARGO_TARGET_DIR=%s %s\n' "$name" "$target" "$*"
    return
  fi
  local start end rc=0
  start="$(date +%s.%N)"
  (cd "$repo" && "$@") >&2 || rc=$?
  end="$(date +%s.%N)"
  awk -v l="$label" -v n="$name" -v s="$start" -v e="$end" 'BEGIN { printf "%s\t%s\t%.1f\n", l, n, e - s }'
  return "$rc"
}

if [ "$dry" = 1 ]; then printf 'shell\tnix develop %s -c\n' "$repo"; fi
step clean-build cargo build --workspace --locked
if [ "$dry" = 1 ]; then printf 'touch\tprintf %s >> %s\n' "'\\n'" "$file"; else printf '\n' >>"$file"; fi
step incremental-build cargo build --workspace --locked
step test-no-run cargo test --workspace --locked --no-run
step test-run cargo test --workspace --locked --no-fail-fast
