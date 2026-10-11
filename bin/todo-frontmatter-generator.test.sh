#!/usr/bin/env bash
set -euo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
generator="$repo/bin/todo-frontmatter-generator"
lint="$repo/bin/todo-lint"
scratch=$(mktemp -d "${TMPDIR:-/tmp}/todo-frontmatter-generator.XXXXXX")
trap 'rm -rf "${scratch:?}"' EXIT

todo="$scratch/todo"
mkdir "$todo"
printf '%s\n' '# A genuine record' '' 'body' >"$todo/genuine-record.md"
printf '%s\n' '+++' 'id = "existing-record"' 'title = "Existing"' '+++' '' 'body' >"$todo/existing-record.md"
printf '%s\n' '# Reserved' >"$todo/AGENTS.md"
printf '%s\n' '# Board' >"$todo/agent-coord.md"
printf '%s\n' '# Commander' >"$todo/COMMANDER-RESUME.md"
printf '%s\n' '# Ledger' >"$todo/model-assignment-ledger.md"
printf '\n' >"$todo/empty.md"

touch -d '2026-10-11 00:00:00 UTC' "$todo/genuine-record.md"

before_existing=$(sha256sum "$todo/existing-record.md" | cut -d' ' -f1)
before_reserved=$(sha256sum "$todo/AGENTS.md" | cut -d' ' -f1)
before_body=$(sha256sum "$todo/genuine-record.md" | cut -d' ' -f1)
output=$("$generator" --dry-run "$todo")
jq -e ' .stamped == 1' <<<"$output" >/dev/null
test "$(sha256sum "$todo/genuine-record.md" | cut -d' ' -f1)" = "$before_body"
output=$("$generator" "$todo")
jq -e ' .stamped == 1' <<<"$output" >/dev/null
test "$(sha256sum "$todo/existing-record.md" | cut -d' ' -f1)" = "$before_existing"
test "$(sha256sum "$todo/AGENTS.md" | cut -d' ' -f1)" = "$before_reserved"
diff -u "$repo/bin/fixtures/todo-frontmatter-expected.md" "$todo/genuine-record.md"
output=$("$generator" "$todo")
jq -e ' .stamped == 0' <<<"$output" >/dev/null
set +e
"$lint" --strict --mtime-drift-seconds 999999999 "$todo" >/dev/null
status=$?
set -e
test "$status" -eq 1

printf 'todo-frontmatter-generator tests: PASS\n'
