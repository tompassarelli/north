#!/usr/bin/env bash
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
agents="$repo/bin/agents"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/agents-test.XXXXXX")"
trap 'rm -rf "${scratch:?}"' EXIT

if AGENTS_NORTH_BIN="$scratch/bin/missing" "$agents" status \
  >"$scratch/missing.out" 2>"$scratch/missing.err"; then
  printf 'agents accepted a missing North CLI\n' >&2
  exit 1
fi

printf 'ok: agents is a thin North CLI client\n'
