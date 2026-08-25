#!/usr/bin/env bash
# Focused contract for the off/db/file/both communications seam.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
comms="$root/bin/north-comms"
north="$root/bin/north"
scratch="$(mktemp -d /tmp/north-comms-test.XXXXXX)"
trap 'rm -rf -- "${scratch:?}"' EXIT

home="$scratch/home"
state="$scratch/harness.conf"
file_root="$scratch/mail"
fake_bin="$scratch/bin"
bb_log="$scratch/bb.log"
ack_log="$scratch/acks.log"
mkdir -p "$home" "$fake_bin"
: >"$bb_log"
: >"$ack_log"

cat >"$fake_bin/bb" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >>"${BB_LOG:?}"
case "${1:-}" in
  *msg-cli.clj)
    case "${3:-}" in
      send)
        to="${5:-}"
        if [ "${4:-}" = "--dead-drop" ]; then to="${6:-}"; fi
        printf 'sent %s -> %s\n' \
          "${DB_MESSAGE_ID:-@msg:20260730-000000-00000000-0000-4000-8000-000000000001}" \
          "$to"
        ;;
      ack)
        printf '%s\n' "${5:-}" >>"${ACK_LOG:?}"
        printf '%s acked %s\n' "${4:-}" "${5:-}"
        ;;
      *) exit 2 ;;
    esac
    ;;
  *inbox-peek.clj)
    if [ -n "${DB_MESSAGE_ID:-}" ] &&
       grep -Fxq -- "$DB_MESSAGE_ID" "${ACK_LOG:?}"; then
      exit 0
    fi
    printf '%s' "${DB_POLL_TEXT:-}"
    ;;
  *north-listen.clj)
    printf 'WATCH\n'
    ;;
  -e)
    printf '%s\n' "${DB_PRESENCE_TEXT:-reachable db-recipient}"
    ;;
  *)
    printf 'unexpected bb invocation: %s\n' "$*" >&2
    exit 97
    ;;
esac
SH
chmod +x "$fake_bin/bb"

write_state() {
  printf '%s\n' "$@" >"$state"
}

run_comms() {
  HOME="$home" \
  NORTH_HOME="$root" \
  NORTH_HARNESS_STATE="$state" \
  NORTH_BB="$fake_bin/bb" \
  BB_LOG="$bb_log" \
  ACK_LOG="$ack_log" \
  DB_MESSAGE_ID="${DB_MESSAGE_ID:-}" \
  DB_POLL_TEXT="${DB_POLL_TEXT:-}" \
  DB_PRESENCE_TEXT="${DB_PRESENCE_TEXT:-}" \
  "$comms" "$@"
}

run_north_ack() {
  HOME="$home" \
  NORTH_HOME="$root" \
  NORTH_HARNESS_STATE="$state" \
  NORTH_BB="$fake_bin/bb" \
  BEAGLE_STORE_HOME="$scratch/store" \
  BEAGLE_STORE_BIN="$scratch/store/bin" \
  BEAGLE_STORE_OUT="$scratch/store/out" \
  BB_LOG="$bb_log" \
  ACK_LOG="$ack_log" \
  "$north" ack "$@"
}

assert_eq() {
  local label="$1" expected="$2" actual="$3"
  if [ "$expected" != "$actual" ]; then
    printf 'FAIL %s\nexpected: %q\nactual:   %q\n' \
      "$label" "$expected" "$actual" >&2
    exit 1
  fi
}

assert_contains() {
  local label="$1" haystack="$2" needle="$3"
  if [[ "$haystack" != *"$needle"* ]]; then
    printf 'FAIL %s: missing %q in %q\n' "$label" "$needle" "$haystack" >&2
    exit 1
  fi
}

db_case() {
  write_state "comms=db" "comms.enforcement=forced"
  local sent polled acked top_level_acked present watched
  sent="$(run_comms send sender target subject body)"
  polled="$(
    DB_POLL_TEXT=$'✉ from sender — subject\n  body\n' run_comms poll target
  )"
  acked="$(run_comms ack target @msg:fixture)"
  top_level_acked="$(run_north_ack target @msg:top-level-fixture)"
  present="$(run_comms presence target)"
  watched="$(run_comms watch target --once)"

  assert_eq "db send stdout is forwarded byte-for-byte" \
    "sent @msg:20260730-000000-00000000-0000-4000-8000-000000000001 -> target" \
    "$sent"
  assert_eq "db poll stdout is forwarded byte-for-byte" \
    $'✉ from sender — subject\n  body' "$polled"
  assert_eq "db ack stdout is forwarded byte-for-byte" \
    "target acked @msg:fixture" "$acked"
  assert_eq "north ack preserves the explicit actor and DB mail route" \
    "target acked @msg:top-level-fixture" "$top_level_acked"
  assert_eq "db presence" "reachable db-recipient" "$present"
  assert_eq "db watch" "WATCH" "$watched"
  printf 'north-comms db seam: PASS\n'
}

announce() {
  local handle="$1" alias="${2:-}"
  if [ -n "$alias" ]; then
    NORTH_COMMS_ALIAS="$alias" run_comms presence "$handle" --announce >/dev/null
  else
    run_comms presence "$handle" --announce >/dev/null
  fi
}

message_id_from() {
  local output="$1"
  if [[ "$output" =~ (@msg:[A-Za-z0-9][A-Za-z0-9._:-]*) ]]; then
    printf '%s\n' "${BASH_REMATCH[1]}"
  else
    printf 'missing message id in %q\n' "$output" >&2
    exit 1
  fi
}

file_case() {
  write_state \
    "comms=file" \
    "comms.enforcement=forced" \
    "comms.file.root=$file_root" \
    "comms.file.poll=hook"
  announce sender
  announce recipient repo-role

  local sent id first second ack1 ack2 absent_err dead broadcast b1 b2 self
  sent="$(run_comms send sender repo-role hello body)"
  id="$(message_id_from "$sent")"
  assert_contains "file send resolves the alias" "$sent" "-> recipient"
  [ -L "$file_root/.aliases/repo-role" ]
  [ "$(readlink "$file_root/.aliases/repo-role")" = "../recipient" ]
  [ -f "$file_root/recipient/new/${id#@msg:}.msg" ]
  if find "$file_root/tmp" -mindepth 1 -print -quit | grep -q .; then
    printf 'FAIL atomic publication left a temp artifact\n' >&2
    exit 1
  fi

  first="$(run_comms poll recipient)"
  second="$(run_comms poll recipient)"
  assert_eq "file render matches the DB renderer" \
    $'✉ from sender — hello\n  body' "$first"
  assert_eq "file poll acks after output" "" "$second"
  [ -f "$file_root/recipient/cur/${id#@msg:}.msg" ]

  ack1="$(run_comms ack recipient "$id")"
  ack2="$(run_comms ack recipient "$id")"
  assert_eq "file ack first replay" "recipient acked $id" "$ack1"
  assert_eq "file ack is idempotent" "$ack1" "$ack2"

  if absent_err="$(run_comms send sender absent no-drop body 2>&1)"; then
    printf 'FAIL unreachable file send succeeded without --dead-drop\n' >&2
    exit 1
  fi
  assert_contains "unreachable diagnostic" "$absent_err" "--dead-drop"
  dead="$(run_comms send sender absent dead body --dead-drop)"
  announce absent
  assert_eq "dead-drop becomes deliverable once the identity appears" \
    $'✉ from sender — dead\n  body' "$(run_comms poll absent)"
  message_id_from "$dead" >/dev/null

  announce peer-a
  announce peer-b
  broadcast="$(run_comms send sender '*' broadcast body --broadcast)"
  assert_contains "broadcast reports a finite snapshot" \
    "$broadcast" "snapshotted recipients"
  b1="$(run_comms poll peer-a)"
  b2="$(run_comms poll peer-b)"
  self="$(run_comms poll sender)"
  assert_eq "broadcast reaches first snapshotted peer" \
    $'✉ from sender — broadcast\n  body' "$b1"
  assert_eq "broadcast reaches second snapshotted peer" "$b1" "$b2"
  assert_eq "broadcast excludes sender" "" "$self"

  local too_large_subject too_large_body cap_body cap_sent cap_id cap_poll
  announce cap-target
  cap_body="$(head -c 24550 /dev/zero | tr '\0' c)"
  cap_sent="$(run_comms send sender cap-target cap "$cap_body")"
  cap_id="$(message_id_from "$cap_sent")"
  cap_poll="$(run_comms poll cap-target)"
  assert_eq "poll leaves a rendering over 24 KiB pending" "" "$cap_poll"
  [ -f "$file_root/cap-target/new/${cap_id#@msg:}.msg" ]

  too_large_subject="$(head -c 16385 /dev/zero | tr '\0' s)"
  too_large_body="$(head -c 131073 /dev/zero | tr '\0' b)"
  if run_comms send sender recipient "$too_large_subject" body >/dev/null 2>&1; then
    printf 'FAIL oversized subject was accepted\n' >&2
    exit 1
  fi
  if run_comms send sender recipient subject "$too_large_body" >/dev/null 2>&1; then
    printf 'FAIL oversized body was accepted\n' >&2
    exit 1
  fi
  if run_comms presence '../bad' --announce >/dev/null 2>&1; then
    printf 'FAIL invalid handle was accepted\n' >&2
    exit 1
  fi
  touch -d '2 hours ago' "$file_root/recipient/cur/${id#@msg:}.msg"
  write_state \
    "comms=file" \
    "comms.enforcement=forced" \
    "comms.file.root=$file_root" \
    "comms.file.poll=hook" \
    "comms.file.retain-hours=1"
  run_comms presence recipient --announce >/dev/null
  [ ! -e "$file_root/recipient/cur/${id#@msg:}.msg" ]
  [ ! -e "$file_root/audit" ]
  printf 'north-comms file invariants: PASS\n'
}

policy_case() {
  write_state \
    "comms=file" \
    "comms.enforcement=forced" \
    "comms.file.root=$file_root" \
    "comms.file.poll=hook"
  announce sender
  announce recipient

  local file_sent shared_id combined rejected both_sent
  file_sent="$(run_comms send sender recipient duplicate body)"
  shared_id="$(message_id_from "$file_sent")"
  : >"$ack_log"
  write_state \
    "comms=both" \
    "comms.enforcement=forced" \
    "comms.file.root=$file_root" \
    "comms.file.poll=hook"
  DB_MESSAGE_ID="$shared_id" \
    DB_POLL_TEXT=$'✉ from sender — duplicate\n  body\n' \
    combined="$(run_comms poll recipient)"
  assert_eq "both poll dedupes a shared id" \
    $'✉ from sender — duplicate\n  body' "$combined"
  grep -Fxq -- "$shared_id" "$ack_log"

  write_state \
    "comms=db" \
    "comms.enforcement=forced" \
    "comms.file.root=$file_root"
  if rejected="$(
    export NORTH_COMMS_REQUESTED_PROTOCOL=file
    run_comms send sender recipient x y 2>&1
  )"; then
    printf 'FAIL forced mismatch was accepted\n' >&2
    exit 1
  fi
  assert_contains "forced mismatch names compliant protocol" "$rejected" "db"
  assert_contains "forced mismatch names compliant entry point" \
    "$rejected" "north-comms"

  write_state \
    "comms=both" \
    "comms.enforcement=forced" \
    "comms.file.root=$file_root"
  DB_MESSAGE_ID="@msg:20260730-000000-00000000-0000-4000-8000-000000000099" \
    both_sent="$(run_comms send sender recipient both body)"
  assert_contains "both send retains DB stdout" "$both_sent" "@msg:20260730-"
  [ -f "$file_root/recipient/new/20260730-000000-00000000-0000-4000-8000-000000000099.msg" ]

  write_state \
    "comms=db" \
    "comms.enforcement=biased" \
    "comms.file.root=$file_root" \
    "comms.file.poll=hook"
  DB_MESSAGE_ID='' \
    DB_POLL_TEXT=$'✉ from db — db-only\n  body\n' \
    combined="$(run_comms poll recipient)"
  assert_contains "biased reads the file backend" "$combined" "both"
  assert_contains "biased reads the DB backend" "$combined" "db-only"
  printf 'north-comms selection policy: PASS\n'
}

burst_case() {
  write_state \
    "comms=file" \
    "comms.enforcement=forced" \
    "comms.file.root=$file_root" \
    "comms.file.poll=hook"
  : >"$bb_log"
  announce sender
  local start_ms end_ms elapsed_ms handle
  start_ms="$(date +%s%3N)"
  for index in $(seq 1 10); do
    handle="burst-$index"
    announce "$handle"
    run_comms send sender "$handle" burst body >/dev/null
  done
  for index in $(seq 1 10); do
    run_comms poll "burst-$index" >/dev/null
  done
  end_ms="$(date +%s%3N)"
  elapsed_ms=$((end_ms - start_ms))
  local jvm_starts
  jvm_starts="$(wc -l <"$bb_log")"
  [ "$jvm_starts" -eq 0 ]
  [ "$elapsed_ms" -le 10000 ]
  printf 'north-comms 10-handle burst: PASS elapsed_ms=%s jvm_starts=%s\n' \
    "$elapsed_ms" "$jvm_starts"
}

callsites_case() {
  grep -Fq "\"\$NORTH_HOME/bin/north-comms\" presence" "$root/bin/north-on-tooluse"
  grep -Fq "\"\$NORTH_HOME/bin/north-comms\" poll" "$root/bin/north-on-tooluse"
  grep -Fq 'bin/north-comms send' "$root/bin/north-on-spawn"
  # shellcheck disable=SC2016
  grep -Fq 'exec "$NORTH/bin/north-comms"' "$root/bin/north"
  grep -Fq './bin/north-comms' "$root/flake.nix"
  grep -Fq 'bin/north-comms' "$root/flake.nix"
  printf 'north-comms call sites and package census: PASS\n'
}

case "${1:-all}" in
  db) db_case ;;
  file) file_case ;;
  policy) policy_case ;;
  burst) burst_case ;;
  callsites) callsites_case ;;
  all)
    db_case
    rm -rf -- "${file_root:?}"
    file_case
    rm -rf -- "${file_root:?}"
    policy_case
    rm -rf -- "${file_root:?}"
    burst_case
    callsites_case
    ;;
  *)
    printf 'usage: %s [db|file|policy|burst|callsites|all]\n' "$0" >&2
    exit 2
    ;;
esac
