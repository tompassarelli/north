#!/usr/bin/env bash
# north-runtime selector bar: promote/rollback/status against a synthetic North
# checkout, with no coordinator and an isolated state root.
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
tool=$root/bin/north-runtime

work=$(mktemp -d)
trap 'chmod -R u+w "${work:?}" 2>/dev/null || true; rm -rf "${work:?}"' EXIT

origin=$work/origin
# Reached through a symlink on purpose: the real state root is one
# (~/.local/state/north -> ~/code/north-data), and an identity compared
# non-canonically against a resolved link fails there and nowhere else.
mkdir -p "$work/data"
ln -s data "$work/state"
state=$work/state
export NORTH_RUNTIME_STATE=$state
# No listener here on purpose: a coordinator that cannot be reached must warn,
# never block delivery.
export NORTH_PORT=1
export FRAM_LOG=$work/coordination.log

fail() { echo "FAIL: $1" >&2; exit 1; }
pass() { echo "ok: $1"; }

mkdir -p "$origin/bin" "$origin/cli"
printf '#!/usr/bin/env bash\ntrue\n' >"$origin/bin/north"
printf '#!/usr/bin/env bash\ntrue\n' >"$origin/bin/concern"
chmod +x "$origin/bin/north" "$origin/bin/concern"
printf '(println "reactor")\n' >"$origin/cli/north-reactor.clj"
printf '(ns north.coord)\n' >"$origin/cli/coord.clj"
git -C "$origin" init -q -b main
git -C "$origin" -c user.email=t@example.com -c user.name=t add -A
git -C "$origin" -c user.email=t@example.com -c user.name=t commit -qm first
first=$(git -C "$origin" rev-parse HEAD)
printf '(println "reactor two")\n' >"$origin/cli/north-reactor.clj"
git -C "$origin" -c user.email=t@example.com -c user.name=t commit -qam second
second=$(git -C "$origin" rev-parse HEAD)

"$tool" status | grep -Fqx 'promoted=none' || fail "an unpromoted selector must report none"
pass "unpromoted status is not an error"

"$tool" promote "$origin" "$first" --why "test first" >"$work/promote.out" 2>"$work/promote.err"
grep -Fqx "promoted=$first" "$work/promote.out" || fail "promote did not select the requested commit"
grep -Fq "promote record not published" "$work/promote.err" ||
  fail "an unreachable coordinator must warn"
pass "promote selects the exact commit and survives an unreachable coordinator"

[ "$(readlink -f "$state/current")" = "$(realpath "$state")/deployments/$first" ] ||
  fail "the stable selector does not resolve to the deployment"
[ "$(git -C "$state/deployments/$first" rev-parse HEAD)" = "$first" ] ||
  fail "the deployment is not checked out at the promoted commit"
grep -Fq '(println "reactor")' "$state/deployments/$first/cli/north-reactor.clj" ||
  fail "the deployment does not carry the promoted commit's content"
pass "the deployment is the exact commit, runnable in place"

grep -Fqx "REV $first" "$(readlink -f "$state/active")/record" ||
  fail "the generation record does not name the promoted revision"
grep -Fqx 'WHY test first' "$(readlink -f "$state/active")/record" ||
  fail "the generation record does not carry --why"
pass "the generation record attests kind/rev/why/ts"

before=$(readlink -f "$state/active")
"$tool" promote "$origin" "$first" --why "again" >/dev/null 2>&1
[ "$(readlink -f "$state/active")" = "$before" ] ||
  fail "re-promoting the selected commit created a generation"
pass "promoting the already-selected commit is idempotent"

"$tool" promote "$origin" "$second" --why "test second" >/dev/null 2>&1
[ "$(readlink -f "$state/current")" = "$(realpath "$state")/deployments/$second" ] ||
  fail "the second promote did not move the selector"
[ "$(readlink -f "$state/previous")" = "$(realpath "$state")/deployments/$first" ] ||
  fail "the second promote did not retain the previous deployment"
"$tool" status | grep -Fq "behind=0 commit(s)" || fail "status must report drift against the origin HEAD"
pass "promote retains the previous member and reports drift"

"$tool" rollback --why "test rollback" >/dev/null 2>&1
[ "$(readlink -f "$state/current")" = "$(realpath "$state")/deployments/$first" ] ||
  fail "rollback did not reselect the previous deployment"
grep -Fqx 'KIND rollback' "$(readlink -f "$state/active")/record" ||
  fail "rollback was not recorded as a rollback"
"$tool" status | grep -Fq "behind=1 commit(s)" || fail "a rolled-back runtime must read as behind"
pass "rollback reselects the retained previous deployment, recorded"

"$tool" rollback --why "test re-roll" >/dev/null 2>&1
[ "$(readlink -f "$state/current")" = "$(realpath "$state")/deployments/$second" ] ||
  fail "rollback is not itself rollback-able"
pass "rollback is itself rollback-able"

echo "PASS north-runtime-promote-test"
