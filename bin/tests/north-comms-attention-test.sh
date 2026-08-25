#!/usr/bin/env bash
# Deterministic, coordinator-free contract for Attention over the comms dial.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
comms="$root/bin/north-comms"
north_under_test="${NORTH_TEST_NORTH_BIN:-$root/bin/north}"
scratch="$(mktemp -d /tmp/north-comms-attention-test.XXXXXX)"
trap 'rm -rf -- "${scratch:?}"' EXIT

home="$scratch/home"
state="$scratch/harness.conf"
file_root="$scratch/mail"
fake_bin="$scratch/bin"
bb_log="$scratch/bb.log"
mkdir -p "$home" "$fake_bin"
: >"$bb_log"

cat >"$fake_bin/bb" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >>"${BB_LOG:?}"
case "${1:-}" in
  *msg-cli.clj)
    verb="${3:-}"
    case "$verb" in
      mention|interrupt)
        printf 'sent %s -> %s\n' \
          "${DB_MESSAGE_ID:-@msg:20260730-000000-00000000-0000-4000-8000-000000000101}" \
          "${5:-}"
        ;;
      *)
        printf 'unexpected msg-cli verb: %s\n' "$*" >&2
        exit 97
        ;;
    esac
    ;;
  *presence-cli.clj)
    printf 'registered %s\n' "${4:-unknown}"
    ;;
  *)
    printf 'unexpected bb invocation: %s\n' "$*" >&2
    exit 98
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
  DB_MESSAGE_ID="${DB_MESSAGE_ID:-}" \
  "$comms" "$@"
}

run_north() {
  HOME="$home" \
  XDG_CONFIG_HOME="$scratch/config" \
  NORTH_HARNESS_STATE="$state" \
  NORTH_BB="$fake_bin/bb" \
  BB_LOG="$bb_log" \
  AGENT_ID=agent-first \
  NORTH_AGENT_ID=north-second \
  NORTH_AUTHOR=author-third \
  "$north_under_test" "$@"
}

assert_eq() {
  local label="$1" expected="$2" actual="$3"
  if [[ $expected != "$actual" ]]; then
    printf 'FAIL %s\nexpected: %q\nactual:   %q\n' \
      "$label" "$expected" "$actual" >&2
    exit 1
  fi
}

assert_contains() {
  local label="$1" haystack="$2" needle="$3"
  if [[ $haystack != *"$needle"* ]]; then
    printf 'FAIL %s: missing %q in %q\n' \
      "$label" "$needle" "$haystack" >&2
    exit 1
  fi
}

assert_bb_quiet() {
  local label="$1"
  if [[ -s $bb_log ]]; then
    printf 'FAIL %s: unexpected DB invocation\n' "$label" >&2
    sed -n '1,20p' "$bb_log" >&2
    exit 1
  fi
}

latest_message() {
  local handle="$1"
  find "$file_root/$handle/new" -mindepth 1 -maxdepth 1 \
    -type f -name '*.msg' -print | LC_ALL=C sort | tail -n 1
}

decoded_field() {
  local message="$1" field="$2" line encoded
  line="$(grep -E "^${field}=" "$message" | tail -n 1)"
  [[ $line == "$field="* ]] || return 1
  encoded="${line#*=}"
  printf '%s' "$encoded" | base64 -d
}

assert_attention_envelope() {
  local label="$1" message="$2" kind="$3" delivery="$4" about="$5"
  assert_eq "$label kind" "$kind" \
    "$(decoded_field "$message" attention_kind)"
  assert_eq "$label delivery" "$delivery" \
    "$(decoded_field "$message" delivery_class)"
  assert_eq "$label acknowledgement" true \
    "$(decoded_field "$message" requires_ack)"
  assert_eq "$label about" "$about" \
    "$(decoded_field "$message" about)"
}

boundary_case() {
  write_state \
    "comms=file" \
    "comms.native=file" \
    "comms.enforcement=forced" \
    "comms.file.root=$file_root" \
    "comms.file.poll=hook"
  : >"$bb_log"
  run_comms presence reviewer --announce >/dev/null

  local mention interrupt
  mention="$(run_north mention reviewer --about @thread:x body)"
  interrupt="$(run_north interrupt reviewer urgent)"

  assert_contains "north mention uses the selected file transport" \
    "$mention" "-> reviewer"
  assert_contains "north interrupt uses the selected file transport" \
    "$interrupt" "-> reviewer"
  assert_bb_quiet "north Attention entrypoint bypassed north-comms"
  [[ $(find "$file_root/reviewer/new" -type f -name '*.msg' | wc -l) -eq 2 ]]
  printf 'north Attention entrypoint comms boundary: PASS\n'
}

semantics_case() {
  local output error message mention_id interrupt_id

  write_state
  : >"$bb_log"
  output="$(run_comms mention sender default-reviewer body)"
  assert_contains "missing comms config falls back to DB" \
    "$output" "-> default-reviewer"
  grep -Fq -- "msg-cli.clj 7977 mention sender default-reviewer body" "$bb_log"

  write_state "comms=off" "comms.enforcement=forced"
  : >"$bb_log"
  if error="$(run_comms mention sender absent body 2>&1)"; then
    printf 'FAIL comms=off accepted mention\n' >&2
    exit 1
  fi
  assert_contains "off mode explains the compliant state" \
    "$error" "disabled by comms=off"
  assert_bb_quiet "off mode touched DB"
  [[ ! -e $file_root ]]

  write_state \
    "comms=file" \
    "comms.enforcement=forced" \
    "comms.file.root=$file_root" \
    "comms.file.poll=hook"
  : >"$bb_log"
  output="$(run_comms mention sender offline-reviewer \
    --about @thread:review body)"
  mention_id="$(sed -n 's/^sent \(@msg:[^ ]*\) ->.*/\1/p' <<<"$output")"
  message="$file_root/offline-reviewer/new/${mention_id#@msg:}.msg"
  [[ -f $message ]]
  assert_attention_envelope \
    "file mention" "$message" mention inbox @thread:review
  assert_bb_quiet "file mention touched DB"

  run_comms presence live-reviewer --announce >/dev/null
  output="$(run_comms interrupt sender live-reviewer urgent)"
  interrupt_id="$(sed -n 's/^sent \(@msg:[^ ]*\) ->.*/\1/p' <<<"$output")"
  message="$file_root/live-reviewer/new/${interrupt_id#@msg:}.msg"
  [[ -f $message ]]
  assert_attention_envelope \
    "file interrupt" "$message" interrupt interrupt ""
  assert_bb_quiet "file interrupt touched DB"
  if run_comms interrupt sender absent urgent >/dev/null 2>&1; then
    printf 'FAIL file interrupt admitted an absent recipient\n' >&2
    exit 1
  fi

  write_state \
    "comms=db" \
    "comms.enforcement=forced" \
    "comms.file.root=$file_root"
  : >"$bb_log"
  output="$(run_comms mention sender db-reviewer --about @thread:db body)"
  assert_contains "db mention keeps msg-cli stdout" "$output" "-> db-reviewer"
  output="$(run_comms interrupt sender db-reviewer urgent)"
  assert_contains "db interrupt keeps msg-cli stdout" "$output" "-> db-reviewer"
  assert_eq "db Attention invokes exact verbs" \
    "mention,interrupt" \
    "$(sed -n 's/.*msg-cli\.clj [0-9][0-9]* \(mention\|interrupt\).*/\1/p' \
        "$bb_log" | paste -sd, -)"
  grep -Fq -- \
    "msg-cli.clj 7977 mention sender db-reviewer --about @thread:db body" \
    "$bb_log"

  write_state \
    "comms=both" \
    "comms.enforcement=forced" \
    "comms.file.root=$file_root" \
    "comms.file.poll=hook"
  run_comms presence dual-reviewer --announce >/dev/null
  : >"$bb_log"
  output="$(
    DB_MESSAGE_ID="@msg:20260730-000000-00000000-0000-4000-8000-000000000202" \
      run_comms interrupt sender dual-reviewer urgent
  )"
  assert_contains "both retains DB stdout" "$output" "@msg:20260730-"
  [[ -f "$file_root/dual-reviewer/new/20260730-000000-00000000-0000-4000-8000-000000000202.msg" ]]
  grep -Fq -- "msg-cli.clj 7977 interrupt sender dual-reviewer urgent" "$bb_log"

  write_state \
    "comms=file" \
    "comms.enforcement=forced" \
    "comms.file.root=$file_root"
  : >"$bb_log"
  if error="$(
    NORTH_COMMS_REQUESTED_PROTOCOL=db \
      run_comms mention sender denied body 2>&1
  )"; then
    printf 'FAIL forced file mode accepted a DB request\n' >&2
    exit 1
  fi
  assert_contains "forced mismatch names the selected protocol" "$error" "file"
  assert_bb_quiet "forced mismatch touched DB"
  [[ ! -e "$file_root/denied" ]]

  write_state \
    "comms=db" \
    "comms.native=file" \
    "comms.managed=db" \
    "comms.enforcement=forced" \
    "comms.file.root=$file_root"
  : >"$bb_log"
  output="$(
    NORTH_COMMS_SURFACE=native \
      run_comms mention sender native-reviewer body
  )"
  assert_contains "native override selects file" "$output" "-> native-reviewer"
  assert_bb_quiet "native file override touched DB"
  output="$(
    NORTH_COMMS_SURFACE=managed \
      run_comms mention sender managed-reviewer body
  )"
  assert_contains "managed override selects DB" "$output" "-> managed-reviewer"
  grep -Fq -- "msg-cli.clj 7977 mention sender managed-reviewer body" "$bb_log"

  printf 'north Attention transport semantics: PASS\n'
}

parser_case() {
  local store classpath expression parsed error
  store="${BEAGLE_STORE_TEST_CHECKOUT:-/home/tom/code/beagle/main/store}"
  classpath="$root/out:$store/out"
  expression='(System/setProperty "north.msg-cli.lib" "1")
(System/setProperty "babashka.file" (System/getenv "NORTH_MSG_CLI"))
(load-file (System/getenv "NORTH_MSG_CLI"))
(prn (parse-directed-attention! "mention" *command-line-args*))'

  parsed="$(
    NORTH_MSG_CLI="$root/cli/msg-cli.clj" \
      bb -cp "$classpath" -e "$expression" -- \
      sender reviewer --about @thread:x body
  )"
  assert_contains "mention parser accepts options before its body" \
    "$parsed" ':about "@thread:x"'
  assert_contains "mention parser preserves the single body" \
    "$parsed" ':body "body"'

  if error="$(
    NORTH_MSG_CLI="$root/cli/msg-cli.clj" \
      bb -cp "$classpath" -e "$expression" -- sender reviewer 2>&1
  )"; then
    printf 'FAIL mention parser accepted no body\n' >&2
    exit 1
  fi
  assert_contains "mention parser rejects no body" \
    "$error" "requires exactly one non-option body argument"

  if error="$(
    NORTH_MSG_CLI="$root/cli/msg-cli.clj" \
      bb -cp "$classpath" -e "$expression" -- sender reviewer body extra 2>&1
  )"; then
    printf 'FAIL mention parser accepted multiple bodies\n' >&2
    exit 1
  fi
  assert_contains "mention parser rejects multiple bodies" \
    "$error" "requires exactly one body argument after options"

  printf 'north mention parser contract: PASS\n'
}

case "${1:-all}" in
  boundary) boundary_case ;;
  semantics) semantics_case ;;
  parser) parser_case ;;
  all)
    boundary_case
    rm -rf -- "${file_root:?}"
    semantics_case
    parser_case
    ;;
  *)
    printf 'usage: %s [boundary|semantics|parser|all]\n' "$0" >&2
    exit 2
    ;;
esac
