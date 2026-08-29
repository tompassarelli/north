#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)
tool=$root/bin/north-store-runtime
output=${1:-}

[ -n "$output" ] || {
  echo "usage: north-store-runtime-test.sh OUT" >&2
  exit 2
}

work=$(mktemp -d)
trap 'chmod -R u+w "${work:?}" 2>/dev/null || true; rm -rf "${work:?}"' EXIT

export NORTH_STORE_RUNTIME_STATE=$work/store-runtime
bb_command=${NORTH_BB:-bb}

fail() {
  echo "FAIL: $1" >&2
  exit 1
}

pass() {
  echo "ok: $1"
}

status=$work/status

"$tool" status >"$status"
grep -Fqx 'generation=none' "$status" ||
  fail "empty isolated state did not report generation=none"
pass "status is read-only before first promotion"

"$tool" promote "$output" >"$status"
grep -Fqx 'current.kind=jvm' "$status" ||
  fail "promotion did not select the accepted JVM"
grep -Fqx 'previous.kind=native' "$status" ||
  fail "promotion did not retain the accepted Native recovery member"
active_before=$(readlink "$NORTH_STORE_RUNTIME_STATE/active")
record=$NORTH_STORE_RUNTIME_STATE/$active_before/generation.edn
[ -f "$record" ] || fail "selected generation record is missing"
pass "promotion publishes one complete JVM/Native generation"

"$tool" promote "$output" >"$status"
[ "$(readlink "$NORTH_STORE_RUNTIME_STATE/active")" = "$active_before" ] ||
  fail "re-promoting the selected JVM created a new generation"
pass "promotion is idempotent"

"$tool" rollback >"$status"
grep -Fqx 'current.kind=native' "$status" ||
  fail "rollback did not select Native"
grep -Fqx 'previous.kind=jvm' "$status" ||
  fail "rollback did not retain the JVM"
rollback_selector=$(readlink "$NORTH_STORE_RUNTIME_STATE/active")
pass "rollback swaps the complete generation pair"

if "$bb_command" -cp "$root/out" -e '
  (load-file (first *command-line-args*))
  (binding [north.store-runtime-generation/*after-selector-move!*
            (fn [_] (throw (ex-info "synthetic cutover failure" {})))]
    (north.store-runtime-generation/restore!
     (north.store-runtime-generation/environment)))' \
    -- "$root/cli/store-runtime-generation.clj" >/dev/null 2>&1; then
  fail "synthetic post-selector failure unexpectedly succeeded"
fi
[ "$(readlink "$NORTH_STORE_RUNTIME_STATE/active")" = "$rollback_selector" ] ||
  fail "failed cutover did not restore the prior selector"
pass "failed cutover restores the prior selector"

"$tool" restore >"$status"
grep -Fqx 'current.kind=jvm' "$status" ||
  fail "restore did not return JVM to current"
restored_selector=$(readlink "$NORTH_STORE_RUNTIME_STATE/active")
"$tool" restore >"$status"
[ "$(readlink "$NORTH_STORE_RUNTIME_STATE/active")" = "$restored_selector" ] ||
  fail "restoring an already-current JVM created a generation"
pass "restore is deterministic and idempotent"

echo "PASS north-store-runtime-test"
