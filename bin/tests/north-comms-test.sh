#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
comms="$root/bin/north-comms"
actor_key="$root/bin/north-actor-key"
scratch="$(mktemp -d /tmp/north-comms-test.XXXXXX)"
trap 'rm -rf -- "${scratch:?}"' EXIT

fail() {
  printf 'FAIL %s\n' "$1" >&2
  exit 1
}

expected_actor_digest=d658337d4f3304da57f99bc30a6de75aa08ffdcbb7f1e0466497291e8c30d312
actual_actor_digest="$(env -u NORTH_BB "$actor_key" agent codex:test)"
[[ $actual_actor_digest == "$expected_actor_digest" ]] ||
  fail "actor-key vector changed: expected $expected_actor_digest, got $actual_actor_digest"

fake_bb="$scratch/bb"
bb_log="$scratch/bb.log"
: >"$bb_log"
cat >"$fake_bb" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
{
  printf '%s' "${1:-}"
  shift || true
  printf ' %s' "$@"
  printf '\n'
} >>"${BB_LOG:?}"
SH
chmod +x "$fake_bb"

run_comms() {
  BB_LOG="$bb_log" \
  NORTH_BB="$fake_bb" \
  NORTH_HOME="$root" \
  NORTH_PORT=7977 \
  NORTH_COMMS_DIR="$scratch/project" \
  NORTH_COMMS_SESSION_ID=session-1 \
    "$comms" "$@"
}

run_comms send sender target subject body
run_comms mention sender target body
run_comms interrupt sender target urgent
run_comms ack target message-reference
run_comms poll target
run_comms lease target
run_comms lease target --announce
run_comms watch target
run_comms watch target --once

expected_dispatches="$root/cli/msg-cli.clj 7977 send sender target subject body
$root/cli/msg-cli.clj 7977 mention sender target body
$root/cli/msg-cli.clj 7977 interrupt sender target urgent
$root/cli/msg-cli.clj 7977 ack target message-reference
$root/cli/inbox-peek.clj 7977 target
$root/cli/msg-cli.clj 7977 presence target
$root/cli/presence-cli.clj 7977 register target $scratch/project session-1
$root/cli/north-listen.clj 7977 target --ack
$root/cli/north-listen.clj 7977 target --once --ack"
actual_dispatches="$(<"$bb_log")"
[[ $actual_dispatches == "$expected_dispatches" ]] || {
  printf 'FAIL Store dispatches differ\nexpected:\n%s\nactual:\n%s\n' \
    "$expected_dispatches" "$actual_dispatches" >&2
  exit 1
}

forbidden='@msg|message[-_]?id|sha(1|224|256|384|512)|acked[-_]by|delivery[-_](eligible|rejected)|valid[-_](message|handle)|canonical[-_]message|comms\.file|file[-_](backend|message|root)|listener[-_](fence|resource)|role[-_]slug|resolve[-_](alias|recipient|role)|dead[-_]letter|message[-_]routing|route[-_]message|recipient[-_](live|reachable)'
if matches="$(LC_ALL=C grep -Ein "$forbidden" "$comms")"; then
  printf 'FAIL north-comms contains message-domain authority:\n%s\n' \
    "$matches" >&2
  exit 1
fi

printf 'north-comms Store dispatch authority: PASS\n'
