#!/usr/bin/env bash
set -euo pipefail

shim_main() {
  local tool="${0##*/}"
  local tool_trace="${SAFE_PUSH_TEST_TRACE:?}"
  [ "$tool" != north ] || tool_trace="${SAFE_PUSH_TEST_LANGUAGE_TRACE:-/dev/null}"
  {
    printf '%s' "$tool"
    printf ' <%s>' "$@"
    printf '\n'
  } >>"$tool_trace"

  if [[ "$tool" == betterleaks && -n "${SAFE_PUSH_TEST_REAL_BETTERLEAKS:-}" && " $* " == *" --config "* ]]; then
    exec "$SAFE_PUSH_TEST_REAL_BETTERLEAKS" "$@"
  fi

  if [[ "$tool" == betterleaks && -n "${SAFE_PUSH_TEST_SCAN_GATE:-}" ]]; then
    if mkdir "$SAFE_PUSH_TEST_SCAN_GATE.scan-started" 2>/dev/null; then
      read -r _ <"$SAFE_PUSH_TEST_SCAN_GATE.scan-release"
    fi
  fi

  case "$tool:${1:-}" in
    north:denylist)
      exec "${SAFE_PUSH_TEST_REAL_NORTH:?}" "$@"
      ;;
    north:land)
      case "${2:-}" in
        checks|push|route|scan) exec "${SAFE_PUSH_TEST_REAL_NORTH:?}" "$@" ;;
      esac
      return 2
      ;;
    north:safe-push) exec "${SAFE_PUSH_TEST_REAL_NORTH:?}" "$@" ;;
    north:safe-push-result|north:capacity|north:mutant|north:stale-revert|north:bin-references|north:build-cache-gate|north:runtime-floor-gate) exec "${SAFE_PUSH_TEST_REAL_NORTH:?}" "$@" ;;
    north:language-gate)
      if [ -n "${SAFE_PUSH_TEST_LANGUAGE_DENY_OID_FILE:-}" ]; then
        local denied_oid
        read -r denied_oid <"$SAFE_PUSH_TEST_LANGUAGE_DENY_OID_FILE"
        printf '%s\n' "${7:-}" >>"${SAFE_PUSH_TEST_LANGUAGE_HEAD_TRACE:?}"
        [ "${7:-}" != "$denied_oid" ] || {
          printf '%s: Python files are forbidden\n' "$denied_oid"
          return 1
        }
      fi
      [ -z "${SAFE_PUSH_TEST_LANGUAGE_DENY:-}" ] || {
        printf '%s\n' "$SAFE_PUSH_TEST_LANGUAGE_DENY"
        return 1
      }
      ;;
    betterleaks:git)
      [ "${SAFE_PUSH_TEST_BETTERLEAKS_LEAK:-0}" != 1 ] || { printf 'leaks found: 1\n'; return 1; }
      ;;
    betterleaks:dir) ;;
    gh:repo)
      [ "${SAFE_PUSH_TEST_VISIBILITY:-PUBLIC}" != fail ] || return 1
      printf '%s\n' "${SAFE_PUSH_TEST_VISIBILITY:-PUBLIC}"
      ;;
    *) return 2 ;;
  esac
}

if [ "${SAFE_PUSH_TEST_SHIM:-}" = 1 ]; then
  shim_main "$@"
  exit $?
fi

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="$REPO/bin/safe-push"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/safe-push-test.XXXXXX")"
owned_sleeper_file=''
cleanup_sleeper() {
  local sleeper
  if [ -n "$owned_sleeper_file" ] && [ -f "$owned_sleeper_file" ]; then
    read -r sleeper <"$owned_sleeper_file"
    kill "$sleeper" 2>/dev/null || true
  fi
  owned_sleeper_file=''
}
trap 'cleanup_sleeper; rm -rf "${scratch:?}"' EXIT
live_push_log="${XDG_STATE_HOME:-$HOME/.local/state}/safe-push/pushes.tsv"
live_push_log_size="$(stat -c %s "$live_push_log" 2>/dev/null || echo 0)"
export XDG_STATE_HOME="$scratch/state"
printf '[init]\n	defaultBranch = main\n' >"$scratch/gitconfig"
export GIT_CONFIG_GLOBAL="$scratch/gitconfig" GIT_CONFIG_NOSYSTEM=1
trace="$scratch/trace"
: >"$trace"
# The real deny-list must not leak into the suite; cases opt in with a fixture.
export SAFE_PUSH_DENYLIST="$scratch/absent-denylist.toml"
export SAFE_PUSH_SOUTH="$scratch/absent-south"
export THREADS_DB="$scratch/threads.db"
unset SAFE_PUSH_LAND_CAPACITY SAFE_PUSH_LAND_OFFLOAD_HOST SAFE_PUSH_LAND_RUNNER SAFE_PUSH_LAND_CHECK_TIMEOUT NORTH_LAND_SPANS

case_output=''
case_status=0

fail() {
  printf 'FAIL: %s\n' "$1" >&2
  printf 'status: %s\noutput:\n%s\ntrace:\n' "$case_status" "$case_output" >&2
  sed 's/^/  /' "$trace" >&2
  exit 1
}

expect_status() {
  local expected="$1"
  if [ "$expected" = zero ]; then
    [ "$case_status" -eq 0 ] || fail "expected exit 0"
  else
    [ "$case_status" -ne 0 ] || fail "expected nonzero exit"
  fi
}

expect_output() {
  grep -Fq -- "$1" <<<"$case_output" || fail "missing output: $1"
}

real_git="$(command -v git)"
real_north=${NORTH_BIN:-$(cd "$REPO" && realpath "${CARGO_TARGET_DIR:-target}")/debug/north}
[ -x "$real_north" ] || { echo "safe-push.test.sh: build north from this tree first; $real_north is missing" >&2; exit 1; }
export SAFE_PUSH_TEST_REAL_NORTH="$real_north"
new_real_bin() {
  real_bin="$scratch/real-bin${1:-}"
  mkdir -p "$real_bin"
  ln -s "$(readlink -f "${BASH_SOURCE[0]}")" "$real_bin/betterleaks"
  ln -s "$(readlink -f "${BASH_SOURCE[0]}")" "$real_bin/north"
}
new_real_bin

make_real_fixture() {
  local label="$1"
  real_fixture="$scratch/real-$label"
  real_repo="$real_fixture/repo"
  real_remote="$real_fixture/remote.git"
  real_other="$real_fixture/other"
  real_trace="$real_fixture/trace"
  real_state="$real_fixture/state"
  if [ -d "$scratch/fixture-template" ]; then
    cp -a "$scratch/fixture-template" "$real_fixture"
    "$real_git" -C "$real_repo" remote set-url origin "$real_remote"
    "$real_git" -C "$real_other" remote set-url origin "$real_remote"
    if [ "$label" = test-integrity ]; then
      : >"$real_repo/retry.test.ts"
      "$real_git" -C "$real_repo" add retry.test.ts
      "$real_git" -C "$real_repo" commit -qm 'existing retry test file'
      "$real_git" -C "$real_repo" push -q origin HEAD:main
    fi
    return
  fi
  mkdir -p "$real_fixture"
  : >"$real_trace"
  : >"$real_state"

  "$real_git" init --bare -q "$real_remote"
  "$real_git" init -q -b main "$real_repo"
  "$real_git" -C "$real_repo" config user.name safe-push-test
  "$real_git" -C "$real_repo" config user.email safe-push-test@example.invalid
  printf 'initial\n' >"$real_repo/content.txt"
  "$real_git" -C "$real_repo" add content.txt
  "$real_git" -C "$real_repo" commit -qm initial
  real_base_oid="$("$real_git" -C "$real_repo" rev-parse HEAD)"
  "$real_git" -C "$real_repo" remote add origin "$real_remote"
  "$real_git" -C "$real_repo" push -q -u origin main
  "$real_git" -C "$real_repo" push -q origin HEAD:refs/heads/other
  "$real_git" -C "$real_repo" commit --allow-empty -qm outgoing

  "$real_git" init -q -b main "$real_other"
  "$real_git" -C "$real_other" config user.name safe-push-test
  "$real_git" -C "$real_other" config user.email safe-push-test@example.invalid
  "$real_git" -C "$real_other" commit --allow-empty -qm other
  "$real_git" -C "$real_other" remote add origin "$real_remote"
  "$real_git" -C "$real_other" push -q origin HEAD:refs/heads/race-target
  real_destination_oid="$("$real_git" -C "$real_other" rev-parse HEAD)"
  cp -a "$real_fixture" "$scratch/fixture-template"
}

real_remote_state() {
  "$real_git" --git-dir="$real_remote" for-each-ref \
    --format='%(refname) %(objectname)' | sort
}

run_generated_rewrite_case() {
  set +e
  case_output="$(cd "$real_repo" && "$real_north" land scan generated-rewrite "$("$real_git" rev-parse refs/remotes/origin/main)..$("$real_git" rev-parse HEAD)" 2>&1)"
  case_status=$?
  set -e
}

run_real_case() {
  printf "%s\n" run_real_case >>"$scratch/cases"
  : >"$real_trace"
  : >"$real_state"
  trace="$real_trace"
  set +e
  case_output="$(
    cd "$real_repo"
    SAFE_PUSH_TEST_SHIM=1 \
      SAFE_PUSH_TEST_TRACE="$real_trace" \
      SAFE_PUSH_TEST_STATE="$real_state" \
      SAFE_PUSH_TEST_REPO="$real_repo" \
      SAFE_PUSH_TEST_REMOTE="$real_remote" \
      XDG_STATE_HOME="$xdg_state" \
      PATH="${SAFE_PUSH_TEST_PATH:-$real_bin:$PATH}" \
      "$TARGET" "$@" 2>&1
  )"
  case_status=$?
  set -e
}

# A branch landing to a public destination runs the deny-list gate with real
# betterleaks before it pushes; the scanner and stage cases live in
# crates/north-landing/src/landing/denylist_publication_tests.rs and land/scans/outgoing/{denylist,email}_tests.rs.
fixture_denylist="$scratch/denylist.toml"
cat >"$fixture_denylist" <<'TOML'
title = "safe-push test deny-list"
[[rules]]
id = "fake-private-term"
description = "fake private identity term"
regex = '''zebra-quokka-7731'''
[[rules]]
id = "fake-credit-lane"
description = "fake credit-lane login"
regex = '''((?i:proton\s+(?:credit|account|login)))'''
[[rules]]
id = "fake-skill-host"
description = "fake host named in a skill"
regex = '''quokka-host-42'''
path = '''^harness/skills/'''
TOML
real_betterleaks="$(command -v betterleaks)"
gh_bin="$scratch/gh-bin"
mkdir -p "$gh_bin"
ln -s "$(readlink -f "${BASH_SOURCE[0]}")" "$gh_bin/gh"

make_real_fixture template

run_denylist_case() {
  PATH="$gh_bin:$PATH" SAFE_PUSH_TEST_VISIBILITY="$1" \
    SAFE_PUSH_TEST_REAL_BETTERLEAKS="$real_betterleaks" run_real_case "${@:2}"
}

sections=()

{ xdg_state="$scratch/xdg-state-1"; new_real_bin -1
make_real_fixture language-gate-boundary
language_remote_before=$(real_remote_state)
language_base=$("$real_git" --git-dir="$real_remote" rev-parse main)
language_head=$("$real_git" -C "$real_repo" rev-parse HEAD)
language_trace="$real_fixture/language-trace"
SAFE_PUSH_TEST_LANGUAGE_TRACE="$language_trace" SAFE_PUSH_TEST_LANGUAGE_DENY='new.py: Python files are forbidden' run_real_case --dry-run --to main
expect_status nonzero
expect_output 'new.py: Python files are forbidden'
[ "$(real_remote_state)" = "$language_remote_before" ] || fail 'language refusal published'
grep -Eq 'blocking check\(s\) failed: (.*; )?language gate' <<<"$case_output" || fail "the refusal did not name the language gate: $case_output"
grep -Fxq "north <language-gate> <--repo> <$real_repo> <--base> <$language_base> <--head> <$language_head>" "$language_trace" || fail 'language gate lost repository or landing oids'
printf 'PASS: language gate refusal blocks every repository\n'

run_real_case --to other
expect_status zero
[ "$("$real_git" --git-dir="$real_remote" rev-parse other)" = "$language_head" ] || fail 'language pass did not publish'
printf 'PASS: language gate pass publishes\n'


make_real_fixture language-initial
while read -r ref _; do
  "$real_git" --git-dir="$real_remote" update-ref -d "$ref"
done < <(real_remote_state)
language_head=$("$real_git" -C "$real_repo" rev-parse HEAD)
language_empty_tree=$("$real_git" -C "$real_repo" hash-object -t tree /dev/null)
language_trace="$real_fixture/language-trace"
SAFE_PUSH_TEST_LANGUAGE_TRACE="$language_trace" SAFE_PUSH_TEST_LANGUAGE_DENY='initial push denied by language gate' run_real_case --dry-run --to main
expect_status nonzero
expect_output 'initial push denied by language gate'
grep -Fxq "north <language-gate> <--repo> <$real_repo> <--base> <$language_empty_tree> <--head> <$language_head>" "$language_trace" || fail 'initial push did not use the empty-tree base'
[ -z "$(real_remote_state)" ] || fail 'initial language refusal published'
printf 'PASS: initial push uses the empty tree as the language gate base\n'

make_real_fixture unreachable-fetch
printf '#!/bin/sh\ncat >/dev/null\n' >"$real_repo/.git/hooks/pre-push"
chmod +x "$real_repo/.git/hooks/pre-push"
"$real_git" -C "$real_repo" config remote.origin.pushurl "$real_remote"
"$real_git" -C "$real_repo" config remote.origin.url "$real_fixture/unreachable.git"
fetch_remote_before=$(real_remote_state)
fetch_language_trace="$real_fixture/language-trace"
SAFE_PUSH_TEST_LANGUAGE_TRACE="$fetch_language_trace" run_real_case --to main
expect_status nonzero
expect_output 'safe-push: could not fetch origin/main; NOT pushed.'
! grep -Fq 'R1' <<<"$case_output" || fail 'a failed fetch was reported as a language refusal'
! grep -Fq '<language-gate>' "$fetch_language_trace" || fail 'a failed fetch reached the language gate'
[ "$(real_remote_state)" = "$fetch_remote_before" ] || fail 'a failed fetch published'
printf 'PASS: a failed fetch of the destination is refused before any check runs\n'

make_real_fixture fresh-north-worktree
land_home="$real_fixture/home"
mkdir -p "$land_home"
ln -s remote.git "$real_fixture/north.git"
"$real_git" -C "$real_repo" config remote.origin.pushurl "$real_remote"
"$real_git" -C "$real_repo" config remote.origin.url "$real_fixture/north.git"
mkdir -p "$real_repo/agent-machinery"
printf '{}\n' >"$real_repo/agent-machinery/package.json"
printf '{\n  "lockfileVersion": 1,\n  "workspaces": {"": {"name": "agent-machinery"}},\n  "packages": {}\n}\n' >"$real_repo/agent-machinery/bun.lock"
"$real_git" -C "$real_repo" config --file .safe-push --add land.check 'agent-machinery/ :: test -d agent-machinery/node_modules'
"$real_git" -C "$real_repo" add .safe-push agent-machinery
"$real_git" -C "$real_repo" commit -qm 'agent-machinery project'
printf '%s\n' "#!/usr/bin/env bash" "[ \"\$*\" = \"install --frozen-lockfile\" ] || exit 2" "mkdir -p node_modules" >"$real_bin/bun"
chmod +x "$real_bin/bun"
HOME="$land_home" run_real_case --dry-run --to main
rm -f "$real_bin/bun"
expect_status zero
expect_output "safe-push: no node_modules — bun install --frozen-lockfile in $real_repo/agent-machinery"
expect_output 'safe-push: ✓ clean (--dry-run)'
printf 'PASS: a fresh north worktree installs agent-machinery before its land check\n'

} >"$scratch/section-1.log" 2>&1 &
sections+=($!)

{ xdg_state="$scratch/xdg-state-2"; new_real_bin -2
make_real_fixture stale-revert-before-land-checks
land_home="$real_fixture/home"
mkdir -p "$land_home"
"$real_git" -C "$real_repo" config --file .safe-push --add land.check "content.txt :: printf 'ran\\n' >> check-runs"
"$real_git" -C "$real_repo" add .safe-push
"$real_git" -C "$real_repo" commit -qm 'checks on content.txt'
"$real_git" clone -q "$real_remote" "$real_fixture/mover"
"$real_git" -C "$real_fixture/mover" config user.name safe-push-test
"$real_git" -C "$real_fixture/mover" config user.email safe-push-test@example.invalid
printf 'moved\n' >"$real_fixture/mover/content.txt"
"$real_git" -C "$real_fixture/mover" commit -qam 'main moves content.txt'
"$real_git" -C "$real_fixture/mover" push -q origin main
"$real_git" -C "$real_repo" fetch -q origin
"$real_git" -C "$real_repo" reset -q --soft origin/main
"$real_git" -C "$real_repo" commit -qm 'squash without a rebase'
stale_remote_before=$(real_remote_state)
HOME="$land_home" run_real_case --dry-run --to main
expect_status nonzero
expect_output 'safe-push: ✗ this landing undoes recent commits on main'
expect_output 'stale revert of newer main'
[ -e "$real_repo/check-runs" ] || fail 'a stale-revert refusal skipped the land checks instead of reporting them too'
[ "$(real_remote_state)" = "$stale_remote_before" ] || fail 'a stale-revert refusal published'
printf 'PASS: a landing that undoes recent main is refused, with its land checks still run and reported\n'

make_real_fixture all-at-once
land_home="$real_fixture/home"
mkdir -p "$land_home"
"$real_git" -C "$real_repo" config --file .safe-push --add land.check 'file.txt :: printf "FAIL boundary\\n" >&2; exit 9'
"$real_git" -C "$real_repo" add .safe-push
"$real_git" -C "$real_repo" commit -qm 'a test that fails'
printf 'changed\n' >"$real_repo/file.txt"
"$real_git" -C "$real_repo" add file.txt
"$real_git" -C "$real_repo" commit -qm 'change under test beside a secret fixture'
all_remote_before=$(real_remote_state)
SAFE_PUSH_TEST_BETTERLEAKS_LEAK=1 HOME="$land_home" run_real_case --dry-run --to main
expect_status nonzero
expect_output 'safe-push: ✗ 2 blocking check(s) failed: land checks; secret scan; NOT pushed.'
expect_output 'safe-push: ✗ SECRET DETECTED'
[ "$(grep -c '✗ [0-9]* blocking check(s) failed' <<<"$case_output")" -eq 1 ] || fail "the refusal was not reported once: $case_output"
[ "$(real_remote_state)" = "$all_remote_before" ] || fail 'a refused landing published'
all_log=${case_output##*exited 9; log: }
rm -f -- "${all_log%%; NOT pushed.*}"
printf 'PASS: a landing with a failing test and a secret is refused once, naming both\n'

} >"$scratch/section-2.log" 2>&1 &
sections+=($!)

{ xdg_state="$scratch/xdg-state-3"; new_real_bin -3
make_real_fixture land-check-boundary
land_home="$real_fixture/home"
land_clean_trace="$real_fixture/land-clean-trace"
mkdir -p "$land_home"
land_remote_before=$(real_remote_state)
land_command='printf "ran\\n" >> check-runs; printf "PASS before\\n"; printf "FAIL named-land-case\\n" >&2; printf "PASS after\\n"; exit 23'
"$real_git" -C "$real_repo" config --file .safe-push --add land.check "file.txt,other.txt :: $land_command"
"$real_git" -C "$real_repo" config --file .safe-push --add land.check 'unmatched/ :: exit 71'
"$real_git" -C "$real_repo" add .safe-push
"$real_git" -C "$real_repo" commit -qm 'affected checks'
printf 'changed\n' >"$real_repo/file.txt"
"$real_git" -C "$real_repo" add file.txt
"$real_git" -C "$real_repo" commit -qm 'changed input'
: >"$land_clean_trace"
watch_calls="$real_fixture/watch-calls"
printf '#!/usr/bin/env bash\nprintf "%%s\\n" "$*" >>"%s"\n' "$watch_calls" >"$real_fixture/watch-stub"
chmod +x "$real_fixture/watch-stub"
SAFE_PUSH_TEST_LANGUAGE_TRACE="$land_clean_trace" SAFE_PUSH_WATCH="$real_fixture/watch-stub" HOME="$land_home" run_real_case --dry-run --to main
expect_status nonzero
land_log=${case_output##*exited 23; log: }
land_log=${land_log%%; NOT pushed.*}
expect_output "safe-push: land check $land_command exited 23; log: $land_log; NOT pushed."
[[ "$land_log" = "$xdg_state/safe-push/land-check-"*.log ]] || fail 'failure did not name an absolute state log'
[ -f "$land_log" ] || fail 'failure log does not exist'
printf 'PASS before\nFAIL named-land-case\nPASS after\n' >"$real_fixture/expected-log"
while IFS= read -r line; do
  grep -Fxq "$line" "$land_log" || fail "failure log lost line: $line"
done <"$real_fixture/expected-log"
[ "$(wc -c <"$land_log")" -eq "$(wc -c <"$real_fixture/expected-log")" ] || fail 'failure log changed output bytes'
[ "$(wc -l <"$real_repo/check-runs")" -eq 1 ] || fail 'matching check did not run exactly once'
! grep -Fq '<land-clean>' "$land_clean_trace" || fail 'a failed land check released the lane build output its retry reuses'
[ "$(real_remote_state)" = "$land_remote_before" ] || fail 'failing land check published'
events="$xdg_state/safe-push/events.jsonl"
grep -Fq "\"stage\":\"land check\",\"status\":\"failed\",\"detail\":\"" "$events" || fail 'the failed land check wrote no event'
grep -Fq "log: $land_log\"" "$events" || fail 'the land check event did not name its log'
grep -Fq '"stage":"exit","status":"refused"' "$events" || fail 'the refusal wrote no event'
for _ in $(seq 50); do [ -s "$watch_calls" ] && break; sleep 0.1; done
grep -Fxq 'watch --once' "$watch_calls" || fail 'a failed landing did not start north watch at once'
printf 'PASS: a failed land check and its refusal write events and start north watch at once\n'
"$real_git" -C "$real_repo" config --file .safe-push --unset-all land.check
land_command='printf "PASS stdout\\n"; printf "PASS stderr\\n" >&2'
"$real_git" -C "$real_repo" config --file .safe-push --add land.check "file.txt :: $land_command"
"$real_git" -C "$real_repo" add .safe-push
"$real_git" -C "$real_repo" commit -qm 'successful check'
stale_land_log="$xdg_state/safe-push/land-check-old.log"
unrelated_land_log="$xdg_state/safe-push/unrelated.log"
printf old >"$stale_land_log"
printf unrelated >"$unrelated_land_log"
touch -d '16 days ago' "$stale_land_log" "$unrelated_land_log"
: >"$land_clean_trace"
SAFE_PUSH_TEST_LANGUAGE_TRACE="$land_clean_trace" HOME="$land_home" run_real_case --dry-run --to main
expect_status zero
expect_output "safe-push: land check: $land_command"
expect_output 'PASS stdout'
expect_output 'PASS stderr'
expect_output 'safe-push: ✓ clean (--dry-run)'
! grep -Fq 'log:' <<<"$case_output" || fail 'successful check added visible logging output'
grep -Fxq "north <land-clean> <$real_repo>" "$land_clean_trace" || fail 'a passing land check left the lane build output'
: >"$land_clean_trace"
SAFE_PUSH_TEST_LANGUAGE_TRACE="$land_clean_trace" HOME="$land_home" run_real_case --dry-run --to main --keep-lane
expect_status zero
! grep -Fq '<land-clean>' "$land_clean_trace" || fail '--keep-lane released the lane build output'
: >"$land_clean_trace"
SAFE_PUSH_TEST_LANGUAGE_TRACE="$land_clean_trace" HOME="$land_home" run_real_case --dry-run --to other
expect_status zero
! grep -Fq '<land-clean>' "$land_clean_trace" || fail 'a push to a non-main branch released the lane build output'
land_logs=("$xdg_state/safe-push/land-check-"*.log)
[ "${#land_logs[@]}" -eq 1 ] && [ "${land_logs[0]}" = "$land_log" ] || fail 'successful check retained a log or removed failure log'
[ ! -e "$stale_land_log" ] || fail 'old failure log was not pruned'
[ -f "$unrelated_land_log" ] || fail 'pruning removed an unrelated log'
"$real_git" -C "$real_repo" config --file .safe-push --unset-all land.check
land_command='sleep 30 >/dev/null 2>&1 & printf "%s\n" "$!" > sleeper.pid'
"$real_git" -C "$real_repo" config --file .safe-push --add land.check "file.txt :: $land_command"
"$real_git" -C "$real_repo" add .safe-push
"$real_git" -C "$real_repo" commit -qm 'background check'
owned_sleeper_file="$real_repo/sleeper.pid"
inherited_descriptors="$(for descriptor in /proc/$$/fd/*; do readlink "$descriptor" || true; done)"
HOME="$land_home" run_real_case --dry-run --to main
expect_status zero
[ -f "$owned_sleeper_file" ] || fail 'background check did not start the sleeper'
read -r sleeper_pid <"$owned_sleeper_file"
kill -0 "$sleeper_pid" 2>/dev/null || fail 'background check sleeper ended before its descriptors were read'
leaked_descriptors="$(ls -1 "/proc/$sleeper_pid/fd" | awk '$1 > 2' | while read -r descriptor; do target="$(readlink "/proc/$sleeper_pid/fd/$descriptor")"; grep -Fxq -- "$target" <<<"$inherited_descriptors" || printf '%s->%s ' "$descriptor" "$target"; done)"
[ -z "$leaked_descriptors" ] || fail "background check inherited private descriptors: $leaked_descriptors"
cleanup_sleeper
expect_output 'safe-push: ✓ clean (--dry-run)'
[ "$(real_remote_state)" = "$land_remote_before" ] || fail 'background dry-run published'
"$real_git" -C "$real_repo" config --file .safe-push --unset-all land.check
"$real_git" -C "$real_repo" config --file .safe-push --add land.check 'unmatched/ :: exit 71'
"$real_git" -C "$real_repo" add .safe-push
"$real_git" -C "$real_repo" commit -qm 'unmatched check'
printf 'uncommitted land.check must not run\n' >>"$real_repo/.safe-push"
: >"$land_clean_trace"
SAFE_PUSH_TEST_LANGUAGE_TRACE="$land_clean_trace" HOME="$land_home" run_real_case --dry-run --to main
expect_status zero
[ "$(wc -l <"$real_repo/check-runs")" -eq 1 ] || fail 'unmatched or uncommitted check ran'
! grep -Fq '<land-clean>' "$land_clean_trace" || fail 'a push that ran no land check released the lane build output'
"$real_git" -C "$real_repo" checkout -- .safe-push
"$real_git" -C "$real_repo" rm -q .safe-push
"$real_git" -C "$real_repo" commit -qm 'no land.check'
HOME="$land_home" run_real_case --dry-run --to main
expect_status zero
expect_output "safe-push: ✓ clean (--dry-run)"

make_real_fixture land-publish
"$real_git" -C "$real_repo" config --file .safe-push land.publish farm/
"$real_git" -C "$real_repo" config --file .safe-push --add land.check "file.txt :: printf '%s\\n' \"\$SAFE_PUSH_LAND_REF\" > '$real_fixture/published-ref'; '$real_git' --git-dir='$real_remote' rev-parse \"refs/heads/\$SAFE_PUSH_LAND_REF\" > '$real_fixture/published-head'"
printf 'published\n' >"$real_repo/file.txt"
"$real_git" -C "$real_repo" add .safe-push file.txt
"$real_git" -C "$real_repo" commit -qm 'publish for the land check'
HOME="$real_fixture" run_real_case --check-candidate --base "$("$real_git" -C "$real_repo" rev-parse origin/main)"
expect_status zero
expect_output "safe-push: verdict green"
publish_head=$("$real_git" -C "$real_repo" rev-parse HEAD)
[ "$(cat "$real_fixture/published-ref")" = "farm/${publish_head:0:12}" ] || fail 'land check did not get the published ref'
[ "$(cat "$real_fixture/published-head")" = "$publish_head" ] || fail 'published ref did not hold the lane head while the land check ran'
! "$real_git" --git-dir="$real_remote" rev-parse -q --verify "refs/heads/farm/${publish_head:0:12}" >/dev/null || fail 'published ref outlived the land checks'
printf 'PASS: land.publish pushes the scanned lane to a scratch ref for the land checks and deletes it afterwards\n'

# ~/.local/bin links firn dotfiles/bin/ and north bin/ tools out-of-store, so a
# removed tool needs a same-name replacement in either repository.
} >"$scratch/section-3.log" 2>&1 &
sections+=($!)

{ xdg_state="$scratch/xdg-state-4"; new_real_bin -4
make_real_fixture tool-removal
mv "$real_remote" "$real_fixture/firn.git"
real_remote="$real_fixture/firn.git"
"$real_git" -C "$real_repo" remote set-url origin "$real_remote"
mkdir -p "$real_repo/dotfiles/bin"
for tool in shared-tool lone-tool; do
  printf '#!/bin/sh\n' >"$real_repo/dotfiles/bin/$tool"
  chmod +x "$real_repo/dotfiles/bin/$tool"
done
"$real_git" -C "$real_repo" add dotfiles/bin
"$real_git" -C "$real_repo" commit -qm tools
"$real_git" -C "$real_repo" push -q origin HEAD:main
tool_home="$real_fixture/home"
"$real_git" init -q -b main "$tool_home/code/north/main"
mkdir -p "$tool_home/code/north/main/bin"
printf '#!/bin/sh\n' >"$tool_home/code/north/main/bin/shared-tool"
chmod +x "$tool_home/code/north/main/bin/shared-tool"
"$real_git" -C "$tool_home/code/north/main" add bin
"$real_git" -C "$tool_home/code/north/main" -c user.name=t -c user.email=t@example.invalid commit -qm peer
"$real_git" -C "$tool_home/code/north/main" update-ref refs/remotes/origin/main HEAD
"$real_git" -C "$real_repo" rm -q dotfiles/bin/lone-tool
"$real_git" -C "$real_repo" commit -qm 'drop lone-tool'
HOME="$tool_home" run_real_case --dry-run
expect_status nonzero
"$real_git" -C "$real_repo" reset -q --hard HEAD~1
"$real_git" -C "$real_repo" rm -q dotfiles/bin/shared-tool
"$real_git" -C "$real_repo" commit -qm 'drop shared-tool'
HOME="$tool_home" run_real_case --dry-run
expect_status zero

tool_base=$("$real_git" -C "$real_repo" rev-parse HEAD~1)
"$real_git" -C "$real_repo" reset -q --hard "$tool_base"
for name in agents threads safe-push capacity-watchdog retired-tool helper.test.sh; do
  printf '#!/bin/sh\n' >"$real_repo/dotfiles/bin/$name"
  chmod +x "$real_repo/dotfiles/bin/$name"
done
"$real_git" -C "$real_repo" add dotfiles/bin
"$real_git" -C "$real_repo" commit -qm 'all core tools'
"$real_git" -C "$real_repo" push -q origin HEAD:main
tool_base=$("$real_git" -C "$real_repo" rev-parse HEAD)
remote_before="$(real_remote_state)"
for name in agents threads safe-push capacity-watchdog retired-tool helper.test.sh; do
  "$real_git" -C "$real_repo" reset -q --hard "$tool_base"
  "$real_git" -C "$real_repo" rm -q "dotfiles/bin/$name"
  "$real_git" -C "$real_repo" commit -qm "retire $name" -m "Removes-tool: $name"
  HOME="$tool_home" run_real_case --dry-run
  case "$name" in
    safe-push|retired-tool|helper.test.sh) expect_status zero ;;
    *) expect_status nonzero; expect_output 'blocking check(s) failed: tool removal' ;;
  esac
  [ "$(real_remote_state)" = "$remote_before" ] || fail 'tool-removal dry-run changed origin'
done
"$real_git" -C "$real_repo" reset -q --hard "$tool_base"
"$real_git" -C "$real_repo" rm -q dotfiles/bin/helper.test.sh
"$real_git" -C "$real_repo" commit -qm 'remove test helper without trailer'
HOME="$tool_home" run_real_case --dry-run
expect_status zero

"$real_git" -C "$real_repo" reset -q --hard "$tool_base"
printf '#!/bin/sh\n' >"$real_repo/dotfiles/bin/origin-only"
chmod +x "$real_repo/dotfiles/bin/origin-only"
"$real_git" -C "$real_repo" add dotfiles/bin/origin-only
"$real_git" -C "$real_repo" commit -qm 'remote-only tool'
remote_tool_head=$("$real_git" -C "$real_repo" rev-parse HEAD)
"$real_git" -C "$real_repo" reset -q --hard "$tool_base"
"$real_git" -C "$real_repo" rm -q dotfiles/bin/helper.test.sh
"$real_git" -C "$real_repo" commit -qm 'local test helper removal'
local_tool_head=$("$real_git" -C "$real_repo" rev-parse HEAD)
(cd "$real_repo"; HOME="$tool_home" "$real_north" land scan tool-removal "$remote_tool_head..$local_tool_head") || fail 'tool scan compared with destination instead of merge base'

} >"$scratch/section-4.log" 2>&1 &
sections+=($!)

{ xdg_state="$scratch/xdg-state-5"; new_real_bin -5
make_real_fixture vendored-tree
mkdir -p "$real_repo/repos/upstream" "$real_repo/vendor/lib"
printf '{"vendor": "repos/upstream"}\n' >"$real_repo/effect-kit.json"
printf '{"prefix": "vendor/lib"}\n' >"$real_repo/repos/lib.json"
printf 'x\n' >"$real_repo/repos/upstream/README"
printf 'x\n' >"$real_repo/vendor/lib/README"
"$real_git" -C "$real_repo" add effect-kit.json repos vendor
"$real_git" -C "$real_repo" commit -qm 'declare vendored trees'
"$real_git" -C "$real_repo" push -q origin HEAD:main
vendored_base=$("$real_git" -C "$real_repo" rev-parse HEAD)
for vendored_case in kit-tree repo-json-tree both-trees first-party sibling-prefix self-declared; do
  "$real_git" -C "$real_repo" reset -q --hard "$vendored_base"
  mkdir -p "$real_repo/repos/upstream/test" "$real_repo/vendor/lib/test" "$real_repo/tests" "$real_repo/repos/upstream-extra/test"
  case "$vendored_case" in
    kit-tree) printf '%s.skip("upstream boundary", () => {})\n' it >"$real_repo/repos/upstream/test/Dns.integration.test.ts" ;;
    repo-json-tree) printf '%s.skip("library boundary", () => {})\n' it >"$real_repo/vendor/lib/test/Lib.test.ts" ;;
    both-trees)
      printf '%s.skip("upstream boundary", () => {})\n' it >"$real_repo/repos/upstream/test/Dns.integration.test.ts"
      printf '%s.skip("library boundary", () => {})\n' it >"$real_repo/vendor/lib/test/Lib.test.ts"
      ;;
    first-party)
      printf '%s.skip("upstream boundary", () => {})\n' it >"$real_repo/repos/upstream/test/Dns.integration.test.ts"
      printf '%s.skip("first party boundary", () => {})\n' it >"$real_repo/tests/Own.test.ts"
      case_path=tests/Own.test.ts
      ;;
    sibling-prefix)
      printf '%s.skip("sibling boundary", () => {})\n' it >"$real_repo/repos/upstream-extra/test/Own.test.ts"
      case_path=repos/upstream-extra/test/Own.test.ts
      ;;
    self-declared)
      printf '{"vendor": "repos/upstream", "other": "own"}\n' >"$real_repo/effect-kit.json"
      printf '{"prefix": "own"}\n' >"$real_repo/repos/own.json"
      mkdir -p "$real_repo/own"
      printf '%s.skip("own boundary", () => {})\n' it >"$real_repo/own/Own.test.ts"
      case_path=own/Own.test.ts
      ;;
  esac
  "$real_git" -C "$real_repo" add -A .
  "$real_git" -C "$real_repo" commit -qm "$vendored_case"
  run_real_case --dry-run
  case "$vendored_case" in
    *-tree*) expect_status zero ;;
    *)
      expect_status nonzero
      expect_output "safe-push: ✗ $case_path adds a skip or focus:"
      ;;
  esac
done

make_real_fixture mutant-record
"$real_git" -C "$real_repo" commit --allow-empty -qm 'historical claim' -m 'Test-kind: old boundary 3 kills historical-missing'
"$real_git" -C "$real_repo" push -q origin HEAD:main
mutant_base=$("$real_git" -C "$real_repo" rev-parse HEAD)
for mutant_case in empty real new-code shared-clone; do
  "$real_git" -C "$real_repo" reset -q --hard "$mutant_base"
  patch="$xdg_state/agents/mutants/remote/$mutant_case.patch"
  mkdir -p "$(dirname "$patch")"
  if [ "$mutant_case" = new-code ]; then
    printf 'new code\n' >"$real_repo/new.txt"
    "$real_git" -C "$real_repo" add new.txt
    "$real_git" -C "$real_repo" commit -qm 'new code'
  fi
  case "$mutant_case" in
    empty)
      sed -i 's/does-not-match/mutated/' "$real_repo/content.txt"
      case_status=0
      case_output=$(cd "$real_repo"; XDG_STATE_HOME="$xdg_state" "$real_north" mutant record "$mutant_case" 2>&1) || case_status=$?
      expect_status nonzero
      [ ! -e "$patch" ] || fail 'no-op sed recorded a patch'
      : >"$patch"
      ;;
    real|new-code)
      mutant_path=content.txt
      [ "$mutant_case" != new-code ] || mutant_path=new.txt
      printf 'mutated\n' >"$real_repo/$mutant_path"
      (cd "$real_repo"; XDG_STATE_HOME="$xdg_state" "$real_north" mutant record "$mutant_case")
      "$real_git" -C "$real_repo" restore "$mutant_path"
      ;;
    shared-clone)
      "$real_git" clone -q --shared "$real_repo" "$real_fixture/shared-clone"
      printf 'mutated\n' >"$real_fixture/shared-clone/content.txt"
      (cd "$real_fixture/shared-clone"; XDG_STATE_HOME="$xdg_state" "$real_north" mutant record "$mutant_case")
      ;;
  esac
  "$real_git" -C "$real_repo" commit --allow-empty -qm 'mutant evidence' -m "Test-kind: recorded boundary 3 kills $mutant_case"
  lane_before=$("$real_git" -C "$real_repo" status --porcelain)
  case_status=0
  case_output=$(cd "$real_repo"; XDG_STATE_HOME="$xdg_state" "$real_north" mutant verify "$mutant_base" HEAD 2>&1) || case_status=$?
  case "$mutant_case" in
    real|new-code|shared-clone) expect_status zero ;;
    *)
      expect_status nonzero
      expect_output "safe-push: ✗ Test-kind recorded boundary claims mutant $mutant_case, which has no recorded non-empty patch; record it with mutant record; NOT pushed."
      ;;
  esac
  [ "$("$real_git" -C "$real_repo" status --porcelain)" = "$lane_before" ] || fail 'mutant check changed the lane'
done
printf 'mutant evidence: PASS (historical main claim accepted)\n'

make_real_fixture test-integrity
integrity_base=$("$real_git" -C "$real_repo" rev-parse HEAD)
remote_before="$(real_remote_state)"
printf 'answer = 41  # %s\n' "MUT""ANT:r1" >"$real_repo/product.py"
"$real_git" -C "$real_repo" add product.py
"$real_git" -C "$real_repo" commit -qm 'product mutant' -m 'Test-change: none'
for args_text in '--dry-run' '--dry-run --to other'; do
  read -r -a parsed_args <<<"$args_text"
  run_real_case "${parsed_args[@]}"
  expect_status nonzero
  [ "$(real_remote_state)" = "$remote_before" ] || fail 'mutant refusal changed origin'
done
for trailer in '' 'Test-change: the retry rule was removed'; do
  "$real_git" -C "$real_repo" reset -q --hard "$integrity_base"
  printf '%s"retries", () => {})\n' "it.sk""ip(" >"$real_repo/retry.test.ts"
  "$real_git" -C "$real_repo" add retry.test.ts
  "$real_git" -C "$real_repo" commit -qm 'skip retry test' ${trailer:+-m "$trailer"}
  run_real_case --dry-run
  if [ -z "$trailer" ]; then expect_status nonzero
  else expect_status zero; fi
done

} >"$scratch/section-5.log" 2>&1 &
sections+=($!)

{ xdg_state="$scratch/xdg-state-6"; new_real_bin -6
make_real_fixture generated-rewrite
record_path=ts/scripts/wisp/cue-models.tsv
mkdir -p "$real_repo/ts/scripts/wisp"
primary_inputs=(
  ts/scripts/wisp/effectVisibility.ts ts/scripts/cueModels.ts ts/scripts/wisp/cueModels.ts
  ts/scripts/prePush.ts ts/src/platform/shell/view.ts ts/src/game/presentation/specialCues.ts
  ts/src/game/presentation/attackCues.ts ts/src/game/presentation/projectileArt.ts
  ts/src/game/presentation/projectilePose.ts ts/wisp.lock ts/bun.lock ts/package.json
  typescript-toolchain.lock ts/src/game/render/specialCueEffects.ts
  ts/src/game/render/projectilePresentation.ts ts/src/game/render/hitAreaEffects.ts
  ts/src/game/render/specialEffects.ts ts/src/game/render/combatEffects.ts
  ts/src/game/render/placedObjectEffects.ts ts/src/game/render/summonPresentation.ts
  ts/src/game/sim/codes.ts ts/src/game/sim/heroes/registry.ts
  ts/src/game/presentation/disjointCues.ts ts/src/game/sim/ultimates.ts
  ts/src/game/presentation/summonClipInfo.ts ts/src/game/presentation/cueEmitterInfo.ts
  ts/src/game/presentation/hitPresentation.ts
  build-inputs/animation-assets build-inputs/base build-inputs/container
  build-inputs/fighter-renders build-inputs/hero-models build-inputs/illidan-animation
  build-inputs/impact-assets build-inputs/imported-models build-inputs/model-sounds
  build-inputs/original-clips-static-lights build-inputs/selection-assets
  build-inputs/stage-assets build-inputs/summon
)
for path in "${primary_inputs[@]}"; do
  mkdir -p "$real_repo/$(dirname "$path")"
  case "$path" in
    ts/package.json|ts/bun.lock) printf '{}\n' >"$real_repo/$path" ;;
    *) printf 'original primary input\n' >"$real_repo/$path" ;;
  esac
  "$real_git" -C "$real_repo" config --file .safe-push --add "generated.$record_path.input" "$path"
done
for ((row=0; row<1000; row++)); do printf 'classic-use\t%s\tvisible\toriginal-provenance\n' "$row"; done >"$real_repo/$record_path"
cp "$real_repo/$record_path" "$real_repo/authored.tsv"
"$real_git" -C "$real_repo" add .safe-push ts build-inputs typescript-toolchain.lock authored.tsv
"$real_git" -C "$real_repo" commit -qm 'recorded generated primary inventory'
"$real_git" -C "$real_repo" push -q origin HEAD:main
generated_base=$("$real_git" -C "$real_repo" rev-parse HEAD)
remote_before=$(real_remote_state)
for journey in churn proportional five-percent full-input missing removed-authority untracked-input authored added-input removed-input unchanged-missing unchanged-removed absent-output; do
  "$real_git" -C "$real_repo" reset -q --hard "$generated_base"
  count=1000; inputs_changed=0; output="$record_path"
  case "$journey" in
    churn) inputs_changed=1 ;;
    proportional) count=500 ;;
    five-percent) inputs_changed=2 ;;
    full-input) inputs_changed=${#primary_inputs[@]} ;;
    missing) "$real_git" -C "$real_repo" config --file .safe-push --replace-all "generated.$record_path.input" '' ;;
    removed-authority) "$real_git" -C "$real_repo" config --file .safe-push --remove-section "generated.$record_path" ;;
    untracked-input) "$real_git" -C "$real_repo" config --file .safe-push --add "generated.$record_path.input" absent.ts ;;
    authored) output=authored.tsv ;;
    added-input)
      printf 'new primary\n' >"$real_repo/new-primary.ts"
      "$real_git" -C "$real_repo" config --file .safe-push --add "generated.$record_path.input" new-primary.ts
      ;;
    removed-input) "$real_git" -C "$real_repo" rm -q "${primary_inputs[0]}" ;;
    unchanged-missing)
      count=0
      "$real_git" -C "$real_repo" config --file .safe-push --replace-all "generated.$record_path.input" ''
      ;;
    unchanged-removed)
      count=0
      "$real_git" -C "$real_repo" config --file .safe-push --remove-section "generated.$record_path"
      ;;
    absent-output)
      count=0
      "$real_git" -C "$real_repo" config --file .safe-push --add generated.absent.tsv.input "${primary_inputs[0]}"
      ;;
  esac
  for ((index=0; index<inputs_changed; index++)); do
    printf '\n' >>"$real_repo/${primary_inputs[index]}"
  done
  python3 - "$real_repo/$output" "$count" <<'PYRECORD'
from pathlib import Path
import sys
record = Path(sys.argv[1])
rows = record.read_text().splitlines()
record.write_text("\n".join(row.replace("original-provenance", "changed-provenance") if index < int(sys.argv[2]) else row for index, row in enumerate(rows)) + "\n")
PYRECORD
  "$real_git" -C "$real_repo" add .safe-push ts build-inputs typescript-toolchain.lock authored.tsv
  [ "$journey" != added-input ] || "$real_git" -C "$real_repo" add new-primary.ts
  "$real_git" -C "$real_repo" commit -qm "generated journey $journey"
  if [ "$journey" = churn ]; then
    run_real_case --dry-run --to main
    expect_status zero
  fi
  run_generated_rewrite_case
  case "$journey" in
    churn|missing|removed-authority|untracked-input|added-input|removed-input|unchanged-missing|unchanged-removed|absent-output)
      expect_status nonzero
      expect_output 'NOT pushed.'
      if [ "$journey" = absent-output ]; then expect_output absent.tsv; else expect_output "$record_path"; fi
      ;;
    *) expect_status zero ;;
  esac
  [ "$(real_remote_state)" = "$remote_before" ] || fail 'generated check changed origin'
done
printf 'Generated rewrite recorded journey: PASS (after landing, a churning rewrite does not refuse the landing)\n'

make_real_fixture native-ref
"$real_git" -C "$real_repo" commit -q --allow-empty -m 'query scroll target for ops#17'
run_real_case --dry-run
expect_status nonzero
expect_output 'names a native NorthThread item'
printf 'PASS: a commit message naming a native item refuses before landing\n'

make_real_fixture skill-without-review
ln -s remote.git "$real_fixture/north.git"
"$real_git" -C "$real_repo" config remote.origin.pushurl "$real_remote"
"$real_git" -C "$real_repo" config remote.origin.url "$real_fixture/north.git"
mkdir -p "$real_repo/harness/skills/example"
printf 'changed\n' >>"$real_repo/harness/skills/example/SKILL.md"
"$real_git" -C "$real_repo" add harness/skills/example/SKILL.md
"$real_git" -C "$real_repo" commit -qm 'change a skill'
run_real_case --dry-run --to main
expect_status zero
expect_output 'safe-push: ✓ clean (--dry-run)'
printf 'PASS: a north skill change lands with no Reviewed trailer\n'

} >"$scratch/section-6.log" 2>&1 &
sections+=($!)

{ xdg_state="$scratch/xdg-state-7"; new_real_bin -7
make_real_fixture policy-sweep
mkdir -p "$real_repo/agents/replay"
printf 'Policy\n\n- Keep it short.\n' >"$real_repo/agents/AGENTS.md"
printf '{"id":"answer-first-line"}\n' >"$real_repo/agents/replay/answer-first-line.json"
printf '{}\n' >"$real_repo/agents/replay/cleanup.json"
printf 'notes\n' >"$real_repo/NOTES.md"
"$real_git" -C "$real_repo" add agents NOTES.md
"$real_git" -C "$real_repo" commit -qm 'operator policy'
"$real_git" -C "$real_repo" push -q origin HEAD:main
policy_base=$("$real_git" -C "$real_repo" rev-parse HEAD)
for policy_case in no-row row row-one-trial row-other-sha row-in-state-only no-policy-change; do
  "$real_git" -C "$real_repo" reset -q --hard "$policy_base"
  if [ "$policy_case" = no-policy-change ]; then
    printf 'more notes\n' >>"$real_repo/NOTES.md"
  else
    printf -- '- Ask once.\n' >>"$real_repo/agents/AGENTS.md"
    policy_sha=$(sha256sum "$real_repo/agents/AGENTS.md" | cut -d' ' -f1)
    case "$policy_case" in
      row) printf '{"variant":"base","policy_sha256":"%s","scenarios":{"answer-first-line":[3,3],"cleanup":[2,3]}}\n' "$policy_sha" \
        >"$real_repo/agents/replay/sweep-2026-10-28.jsonl" ;;
      row-one-trial) printf '{"variant":"base","policy_sha256":"%s","scenarios":{"answer-first-line":[1,1],"cleanup":[1,1]}}\n' "$policy_sha" \
        >"$real_repo/agents/replay/sweep-2026-10-28.jsonl" ;;
      row-other-sha) printf '{"variant":"base","policy_sha256":"%064d","scenarios":{"answer-first-line":[3,3],"cleanup":[3,3]}}\n' 0 \
        >"$real_repo/agents/replay/sweep-2026-10-28.jsonl" ;;
      row-in-state-only) mkdir -p "$real_fixture/state-replay"
        printf '{"variant":"base","policy_sha256":"%s","scenarios":{"answer-first-line":[3,3],"cleanup":[3,3]}}\n' "$policy_sha" \
        >"$real_fixture/state-replay/sweep-2026-10-28.jsonl" ;;
    esac
  fi
  "$real_git" -C "$real_repo" add -A agents NOTES.md
  "$real_git" -C "$real_repo" commit -qm "policy sweep case $policy_case"
  run_real_case --dry-run
  case "$policy_case" in
    row | no-policy-change) expect_status zero ;;
    *)
      expect_status nonzero
      expect_output 'makes agents/AGENTS.md'
      ;;
  esac
done
printf 'Policy sweep gate: PASS (no row, row, one trial, other sha, state-only row, no policy change)\n'

} >"$scratch/section-7.log" 2>&1 &
sections+=($!)




{ xdg_state="$scratch/xdg-state-8"; new_real_bin -8
make_real_fixture dep-gate
printf '{"dependencies":{"unowned-package":"1.0.0"}}\n' >"$real_repo/package.json"
"$real_git" -C "$real_repo" add package.json
"$real_git" -C "$real_repo" commit -qm 'add dependency without reason'
remote_before="$(real_remote_state)"
run_real_case --dry-run
expect_status nonzero
expect_output 'blocking check(s) failed: dependency gate'
[ "$(real_remote_state)" = "$remote_before" ] || fail 'dependency refusal changed origin'

# Cross-branch publication is available only through --to and scans the exact
# remote-destination..HEAD range before an object+lease-bound update.
make_real_fixture explicit-main
"$real_git" -C "$real_repo" switch -q -c feature
"$real_git" -C "$real_repo" commit --allow-empty -qm feature
run_real_case --to other
expect_status zero
feature_oid="$("$real_git" -C "$real_repo" rev-parse feature)"
[ "$("$real_git" --git-dir="$real_remote" rev-parse refs/heads/other)" = "$feature_oid" ] \
  || fail '--to other did not advance the explicit destination'
if "$real_git" -C "$real_repo" rev-parse --verify '@{upstream}' >/dev/null 2>&1; then
  fail '--to unexpectedly changed local branch tracking'
fi

make_real_fixture denylist-public-finding
printf 'owner: zebra-quokka-7731\n' >"$real_repo/identity.txt"
"$real_git" -C "$real_repo" add identity.txt
"$real_git" -C "$real_repo" commit -qm 'add identity'
remote_before="$(real_remote_state)"
SAFE_PUSH_DENYLIST="$fixture_denylist" run_denylist_case PUBLIC --dry-run
expect_status nonzero
expect_output 'fake-private-term'
expect_output 'identity.txt'
if grep -Fq 'zebra-quokka-7731' <<<"$case_output"; then fail 'deny-list refusal printed the matched value'; fi
[ "$(real_remote_state)" = "$remote_before" ] || fail 'deny-list refusal mutated the remote'

} >"$scratch/section-8.log" 2>&1 &
sections+=($!)

status=0
for i in "${!sections[@]}"; do
  wait "${sections[$i]}" || status=1
  cat "$scratch/section-$((i + 1)).log"
done
[ "$status" -eq 0 ] || exit 1
while IFS=$'\t' read -r _ _ _ oid _; do
  for git_dir in "$scratch"/*/*.git; do
    ! "$real_git" --git-dir="$git_dir" cat-file -e "$oid^{commit}" 2>/dev/null \
      || fail "the suite recorded its fixture push $oid in the live push log $live_push_log"
  done
done < <(tail -c "+$((live_push_log_size + 1))" "$live_push_log" 2>/dev/null)

printf 'safe-push tests: PASS (%s boundary invocations)\n' "$(wc -l <"$scratch/cases")"
