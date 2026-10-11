#!/usr/bin/env bash
set -euo pipefail

repo=$(cd "$(dirname "$0")/.." && pwd)
target="$repo/bin/todo-mailbox-status"
scratch=$(mktemp -d "${TMPDIR:-/tmp}/todo-mailbox-status-test.XXXXXX")
trap 'rm -rf "${scratch:?}"' EXIT
mailbox="$scratch/agent-coord.md"

printf '%s\n' \
  '# mailbox fixture' \
  '- [2026-08-21T10:00:00+08:00][C-ONE][alice -> bob][OPEN] first request' \
  '- [2026-08-21T10:01:00+08:00][C-ONE][bob -> alice][ACK] received' \
  '- [2026-08-21T10:02:00+08:00][C-ONE][alice -> bob][UPDATE] checkpoint' \
  '- [2026-08-21T10:03:00+08:00][C-TWO][carol -> dave][OPEN] needs receipt' \
  '- [2026-08-21T10:04:00+08:00][C-ONE][bob -> alice][ACK] repeated receipt' \
  '- [2026-08-21T10:05:00+08:00][C-THREE][erin -> frank][DONE] settled' \
  '- [2026-08-21T10:06:00+08:00][C-BAD][gina -> hank][TRANSFER] rejected verb' \
  '- [2026-08-21T10:07:00+08:00][C-BAD-ROUTE][gina->hank][OPEN] rejected route' \
  '- [2026-08-21T10:08:00+08:00][C-BAD-BODY][gina -> hank][OPEN]' \
  >"$mailbox"

set +e
"$target" "$mailbox" >"$scratch/human"
status=$?
set -e
[ "$status" -eq 1 ]
grep -Fqx 'OPEN-WITHOUT-ACK C-TWO' "$scratch/human"
grep -Fqx 'WITHOUT-DONE C-ONE' "$scratch/human"
grep -Fqx 'WITHOUT-DONE C-TWO' "$scratch/human"
grep -Fq 'MALFORMED line=8 ' "$scratch/human"
grep -Fq 'MALFORMED line=9 ' "$scratch/human"
grep -Fq 'MALFORMED line=10 ' "$scratch/human"

set +e
"$target" --json "$mailbox" >"$scratch/report.json"
status=$?
set -e
[ "$status" -eq 1 ]
python3 - "$scratch/report.json" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as source:
    report = json.load(source)

assert report["open_without_ack"] == ["C-TWO"]
assert report["nonterminal_without_done"] == ["C-ONE", "C-TWO"]
claims = {claim["claim"]: claim for claim in report["claims"]}
assert claims["C-TWO"]["open_without_ack"] is True
assert claims["C-THREE"]["state"] == "done"
PY

printf '%s\n' \
  '- [2026-08-21T11:00:00+08:00][C-CLEAN][ivy -> jules][OPEN] request' \
  '- [2026-08-21T11:01:00+08:00][C-CLEAN][jules -> ivy][ACK] accepted' \
  '- [2026-08-21T11:02:00+08:00][C-CLEAN][jules -> ivy][DONE] landed' \
  >"$mailbox"
"$target" "$mailbox" >"$scratch/clean"

printf 'todo-mailbox-status tests: PASS (strict grammar, per-claim liveness, JSON)\n'
