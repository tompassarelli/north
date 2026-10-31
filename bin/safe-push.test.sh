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

  if [[ "$tool" == gitleaks && -n "${SAFE_PUSH_TEST_REAL_GITLEAKS:-}" && " $* " == *" --config "* ]]; then
    exec "$SAFE_PUSH_TEST_REAL_GITLEAKS" "$@"
  fi

  if [[ "$tool" == gitleaks && -n "${SAFE_PUSH_TEST_REAL_RACE:-}" \
        && ! -s "${SAFE_PUSH_TEST_STATE:?}" ]]; then
    case "$SAFE_PUSH_TEST_REAL_RACE" in
      branch)
        git -C "${SAFE_PUSH_TEST_REPO:?}" switch -q -c raced-branch
        ;;
      commit)
        git -C "${SAFE_PUSH_TEST_REPO:?}" commit --allow-empty -qm 'raced commit'
        ;;
      destination)
        git --git-dir="${SAFE_PUSH_TEST_REMOTE:?}" update-ref \
          refs/heads/main "${SAFE_PUSH_TEST_DESTINATION_OID:?}"
        ;;
      worktree)
        mv "${SAFE_PUSH_TEST_REPO:?}/.git" "${SAFE_PUSH_TEST_REPO:?}/.git.safe-push-original"
        printf 'gitdir: %s\n' "${SAFE_PUSH_TEST_OTHER_GIT_DIR:?}" \
          >"${SAFE_PUSH_TEST_REPO:?}/.git"
        ;;
      *) return 2 ;;
    esac
    printf 'raced\n' >"${SAFE_PUSH_TEST_STATE:?}"
  fi

  if [[ "$tool" == gitleaks && -n "${SAFE_PUSH_TEST_SCAN_GATE:-}" ]]; then
    if mkdir "$SAFE_PUSH_TEST_SCAN_GATE.scan-started" 2>/dev/null; then
      read -r _ <"$SAFE_PUSH_TEST_SCAN_GATE.scan-release"
    fi
  fi

  case "$tool:${1:-}" in
    north:dep-gate)
      "${SAFE_PUSH_TEST_REAL_NORTH:?}" "$@"
      local status=$?
      if [ "${SAFE_PUSH_TEST_REMOVE_NORTH_ON_DEP_GATE:-0}" = 1 ]; then rm -f -- "$0"; fi
      return "$status"
      ;;
    north:provenance)
      "${SAFE_PUSH_TEST_REAL_NORTH:?}" "$@"
      local status=$?
      if [ "${SAFE_PUSH_TEST_REMOVE_NORTH_AFTER_PROVENANCE:-0}" = 1 ]; then rm -f -- "$0"; fi
      return "$status"
      ;;
    north:denylist)
      if [ "${2:-}" = attest ]; then
        printf '%s\n' "$*" >>"${SAFE_PUSH_TEST_ATTEST_TRACE:?}"
        return "${SAFE_PUSH_TEST_ATTEST_STATUS:-0}"
      fi
      exec "${SAFE_PUSH_TEST_REAL_NORTH:?}" "$@"
      ;;
    north:autoland-status|north:autoland-wait|north:safe-push-result|north:capacity|north:review-gate|north:mutant|north:stale-revert|north:bin-references) exec "${SAFE_PUSH_TEST_REAL_NORTH:?}" "$@" ;;
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
    gitleaks:detect|gitleaks:dir) ;;
    gh:repo)
      [ "${SAFE_PUSH_TEST_VISIBILITY:-PUBLIC}" != fail ] || return 1
      printf '%s\n' "${SAFE_PUSH_TEST_VISIBILITY:-PUBLIC}"
      ;;
    gh:run)
      [ "${2:-}" = list ] || return 2
      if [[ " $* " == *" autoland.yml "* ]]; then
        printf '%s\n' "${SAFE_PUSH_TEST_AUTOLAND_RUNS-}"
        return 0
      fi
      cat "${SAFE_PUSH_TEST_CI_FIXTURE:?}/runs.json"
      ;;
    gh:api)
      case "${2:-}" in
        repos/*/commits/*/status)
          if [[ "${SAFE_PUSH_TEST_AUTOLAND_STATUS:-}" == success$'\t'* ]]; then
            local landed_sha="${2#*/commits/}"
            landed_sha="${landed_sha%/status}"
            git --git-dir="${SAFE_PUSH_TEST_REMOTE:?}" update-ref refs/heads/main "$landed_sha"
          fi
          printf '%s\n' "${SAFE_PUSH_TEST_AUTOLAND_STATUS:?}"
          return 0
          ;;
        repos/*/actions/runs/*/jobs\?*) ;;
        *) return 2 ;;
      esac
      local id="${2%/jobs*}"
      cat "${SAFE_PUSH_TEST_CI_FIXTURE:?}/${id##*/}.json"
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
trace="$scratch/trace"
: >"$trace"
# The real deny-list must not leak into the suite; cases opt in with a fixture.
export SAFE_PUSH_DENYLIST="$scratch/absent-denylist.toml"
export SAFE_PUSH_SOUTH="$scratch/absent-south"
export THREADS_DB="$scratch/threads.db"

case_output=''
case_status=0

fail() {
  printf 'FAIL: %s\n' "$1" >&2
  printf 'status: %s\noutput:\n%s\ntrace:\n' "$case_status" "$case_output" >&2
  sed 's/^/  /' "$trace" >&2
  exit 1
}

forget_run_measurements() {
  rm -rf -- "$xdg_state/safe-push/run-measurements"
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
real_north="$(cd "$REPO" && realpath "${CARGO_TARGET_DIR:-target}")/debug/north"
[ -x "$real_north" ] || { echo "safe-push.test.sh: build north from this tree first; $real_north is missing" >&2; exit 1; }
export SAFE_PUSH_TEST_REAL_NORTH="$real_north"
new_real_bin() {
  real_bin="$scratch/real-bin${1:-}"
  mkdir -p "$real_bin"
  ln -s "$(readlink -f "${BASH_SOURCE[0]}")" "$real_bin/gitleaks"
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

run_real_case() {
  printf "%s\n" run_real_case >>"$scratch/cases"
  local race="$1"
  shift
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
      SAFE_PUSH_TEST_DESTINATION_OID="$real_destination_oid" \
      SAFE_PUSH_TEST_REAL_RACE="$race" \
      SAFE_PUSH_TEST_OTHER_GIT_DIR="$real_other/.git" \
      XDG_STATE_HOME="$xdg_state" \
      PATH="${SAFE_PUSH_TEST_PATH:-$real_bin:$PATH}" \
      "$TARGET" "$@" 2>&1
  )"
  case_status=$?
  set -e

  # The worktree-race fixture deliberately swaps the gitfile while safe-push is
  # between scan and publication. Restore only the isolated scratch repository.
  if [ -d "$real_repo/.git.safe-push-original" ]; then
    [ ! -e "$real_repo/.git" ] || rm -f "${real_repo:?}/.git"
    mv "$real_repo/.git.safe-push-original" "$real_repo/.git"
  fi
}

make_real_fixture argument-boundary
{ xdg_state="$scratch/xdg-state-1"
argument_bin="$scratch/argument-bin"
mkdir -p "$argument_bin"
printf '#!/usr/bin/env bash\necho called >>"${SAFE_PUSH_TEST_TRACE:?}"\nexit 97\n' >"$argument_bin/git"
chmod +x "$argument_bin/git"
while IFS='|' read -r arguments; do
  read -r -a parsed_args <<<"$arguments"
  remote_before="$(real_remote_state)"
  PATH="$argument_bin:$PATH" run_real_case '' "${parsed_args[@]}"
  expect_status nonzero
  [ ! -s "$real_trace" ] || fail 'argument conflict reached git or scan'
  [ "$(real_remote_state)" = "$remote_before" ] || fail 'argument conflict changed origin'
done <<'CASES'
--to main --tag release-a
--dry-run --dry-run
--keep-lane
--to main --to other
--tag release-a --tag release-b
--help --dry-run
--to bad..name
CASES

make_real_fixture language-gate-boundary
language_remote_before=$(real_remote_state)
language_base=$("$real_git" --git-dir="$real_remote" rev-parse main)
language_head=$("$real_git" -C "$real_repo" rev-parse HEAD)
language_trace="$real_fixture/language-trace"
SAFE_PUSH_TEST_LANGUAGE_TRACE="$language_trace" SAFE_PUSH_TEST_LANGUAGE_DENY='new.py: Python files are forbidden' run_real_case '' --to main
expect_status nonzero
expect_output 'new.py: Python files are forbidden'
[ "$(real_remote_state)" = "$language_remote_before" ] || fail 'language refusal published'
! grep -q '^gitleaks ' "$real_trace" || fail 'language refusal reached heavy checks'
grep -Fxq "north <language-gate> <--repo> <$real_repo> <--base> <$language_base> <--head> <$language_head>" "$language_trace" || fail 'language gate lost repository or landing oids'
printf 'PASS: language gate refusal blocks every repository\n'

run_real_case '' --to main
expect_status zero
[ "$("$real_git" --git-dir="$real_remote" rev-parse main)" = "$language_head" ] || fail 'language pass did not publish'
printf 'PASS: language gate pass publishes\n'

missing_north_bin="$scratch/missing-north-bin"
mkdir -p "$missing_north_bin"
ln -s "$real_git" "$missing_north_bin/git"
ln -s "$(readlink -f "${BASH_SOURCE[0]}")" "$missing_north_bin/gitleaks"
missing_north_path="$missing_north_bin:/run/current-system/sw/bin:/usr/bin:/bin"
! PATH="$missing_north_path" command -v north >/dev/null || fail 'missing north fixture contains north'
language_gh_bin="$scratch/language-gh-bin"
mkdir -p "$language_gh_bin"
ln -s "$(readlink -f "${BASH_SOURCE[0]}")" "$language_gh_bin/gh"
for publication_mode in branch autoland; do
  make_real_fixture "language-$publication_mode"
  publication_args=(--to main)
  publication_path="$real_bin:$PATH"
  publication_missing_path="$missing_north_path"
  case "$publication_mode" in
    autoland)
      publication_path="$real_bin:$language_gh_bin:$PATH"
      publication_missing_path="$language_gh_bin:$missing_north_path"
      printf '[landing]\n\tqueue = autoland\n' >"$real_repo/.safe-push"
      "$real_git" -C "$real_repo" add .safe-push
      "$real_git" -C "$real_repo" commit -qm 'autoland queue'
      "$real_git" -C "$real_repo" push -q origin HEAD:main
      "$real_git" --git-dir="$real_remote" symbolic-ref HEAD refs/heads/main
      "$real_git" -C "$real_repo" commit --allow-empty -qm 'queued change'
      ;;
  esac
  remote_before=$(real_remote_state)
  if [ "$publication_mode" = autoland ]; then
    SAFE_PUSH_TEST_AUTOLAND_STATUS=$'failure\tLanguage fixture\tfixture' SAFE_PUSH_TEST_PATH="$publication_path" SAFE_PUSH_TEST_LANGUAGE_DENY='new.py: Python files are forbidden' run_real_case '' "${publication_args[@]}"
    expect_status nonzero
    expect_output 'new.py: Python files are forbidden'
    [ "$(real_remote_state)" = "$remote_before" ] || fail "$publication_mode language refusal published"
    ! grep -q '^gitleaks ' "$real_trace" || fail "$publication_mode refusal reached heavy checks"
  fi
  SAFE_PUSH_TEST_AUTOLAND_STATUS=$'failure\tLanguage fixture\tfixture' SAFE_PUSH_TEST_PATH="$publication_missing_path" run_real_case '' "${publication_args[@]}"
  expect_status nonzero
  expect_output 'north not on PATH; refusing to land without the language gate'
  [ "$(real_remote_state)" = "$remote_before" ] || fail "$publication_mode missing north published"
  ! grep -q '^gitleaks ' "$real_trace" || fail "$publication_mode missing north reached heavy checks"
done
printf 'PASS: branch and autoland refuse denial and missing north\n'

make_real_fixture language-initial
while read -r ref _; do
  "$real_git" --git-dir="$real_remote" update-ref -d "$ref"
done < <(real_remote_state)
language_head=$("$real_git" -C "$real_repo" rev-parse HEAD)
language_empty_tree=$("$real_git" -C "$real_repo" hash-object -t tree /dev/null)
language_trace="$real_fixture/language-trace"
SAFE_PUSH_TEST_PATH="$missing_north_path" run_real_case '' --to main
expect_status nonzero
expect_output 'north not on PATH; refusing to land without the language gate'
! grep -Fq 'agent-instruction-check:' <<<"$case_output" || fail 'initial missing north reached later checks'
[ -z "$(real_remote_state)" ] || fail 'initial push without north published'
SAFE_PUSH_TEST_LANGUAGE_TRACE="$language_trace" SAFE_PUSH_TEST_LANGUAGE_DENY='initial push denied by language gate' run_real_case '' --to main
expect_status nonzero
expect_output 'initial push denied by language gate'
grep -Fxq "north <language-gate> <--repo> <$real_repo> <--base> <$language_empty_tree> <--head> <$language_head>" "$language_trace" || fail 'initial push did not use the empty-tree base'
[ -z "$(real_remote_state)" ] || fail 'initial language refusal published'
printf 'PASS: initial push uses empty tree and fails closed without north\n'

make_real_fixture unreachable-fetch
printf '#!/bin/sh\ncat >/dev/null\n' >"$real_repo/.git/hooks/pre-push"
chmod +x "$real_repo/.git/hooks/pre-push"
"$real_git" -C "$real_repo" config remote.origin.pushurl "$real_remote"
"$real_git" -C "$real_repo" config remote.origin.url "$real_fixture/unreachable.git"
fetch_remote_before=$(real_remote_state)
fetch_language_trace="$real_fixture/language-trace"
SAFE_PUSH_TEST_LANGUAGE_TRACE="$fetch_language_trace" run_real_case '' --to main
expect_status nonzero
expect_output 'safe-push: could not fetch origin/main; NOT pushed.'
! grep -Fq 'R1' <<<"$case_output" || fail 'a failed fetch was reported as a language refusal'
! grep -Fq '<language-gate>' "$fetch_language_trace" || fail 'a failed fetch reached the language gate'
[ "$(real_remote_state)" = "$fetch_remote_before" ] || fail 'a failed fetch published'
printf 'PASS: a failed fetch of the destination is refused before any check runs\n'

eval "$(sed -n '/^sync_dependencies() {/,/^}$/p' "$TARGET")"
drop_lock_fd() { :; }
deps_root="$scratch/sync-deps"
deps_bin="$deps_root/bin"
mkdir -p "$deps_bin"
printf '#!/usr/bin/env bash\n[ "$*" = "install --frozen-lockfile" ] || exit 2\nprintf "%%s\\n" "$PWD" >>%q\nmkdir -p node_modules\n' "$deps_root/bun-runs" >"$deps_bin/bun"
chmod +x "$deps_bin/bun"
make_deps_repo() {
  local name="$1" origin="$2"
  deps_repo="$deps_root/$name"
  "$real_git" init -q -b main "$deps_repo"
  "$real_git" -C "$deps_repo" remote add origin "$deps_root/$origin.git"
  mkdir -p "$deps_repo/ts"
  printf '{}\n' >"$deps_repo/ts/package.json"
  printf 'pin 1\n' >"$deps_repo/ts/bun.lock"
  "$real_git" -C "$deps_repo" add ts
  "$real_git" -C "$deps_repo" -c user.name=t -c user.email=t@example.invalid commit -qm 'ts project'
}
: >"$deps_root/bun-runs"
make_deps_repo from-subdirectory smashcraft
deps_output="$(cd "$deps_repo/ts" && PATH="$deps_bin:$PATH" sync_dependencies 2>&1)" || fail "sync_dependencies from a subdirectory failed: $deps_output"
[ "$(cat "$deps_root/bun-runs")" = "$deps_repo/ts" ] || fail "sync_dependencies from ts/ did not install in ts/: $deps_output"
[ ! -e "$deps_repo/node_modules" ] || fail 'sync_dependencies from ts/ installed at the repository root'
printf 'PASS: sync_dependencies run from a subdirectory installs in the lockfile directory\n'

: >"$deps_root/bun-runs"
make_deps_repo symlinked-node-modules north
deps_checkout="$deps_root/checkout"
mkdir -p "$deps_checkout/node_modules"
printf 'dependency\n' >"$deps_checkout/node_modules/installed.txt"
printf 'stale stamp\n' >"$deps_checkout/node_modules/.safe-push-deps"
ln -s "$deps_checkout/node_modules" "$deps_repo/ts/node_modules"
deps_checkout_before="$(cd "$deps_checkout" && find . -type f -print0 | sort -z | xargs -0 sha256sum | sha256sum)"
deps_status=0
deps_output="$(cd "$deps_repo" && PATH="$deps_bin:$PATH" sync_dependencies 2>&1)" || deps_status=$?
[ "$deps_status" -ne 0 ] || fail "sync_dependencies accepted a symlinked node_modules whose lockfile differs: $deps_output"
grep -Fq "ts/node_modules is a symlink to $deps_checkout/node_modules" <<<"$deps_output" || fail "the refusal did not name the symlink: $deps_output"
[ ! -s "$deps_root/bun-runs" ] || fail 'bun install ran through the symlink'
[ "$(cd "$deps_checkout" && find . -type f -print0 | sort -z | xargs -0 sha256sum | sha256sum)" = "$deps_checkout_before" ] \
  || fail 'sync_dependencies changed the checkout behind the symlink'
printf 'PASS: sync_dependencies never installs through a symlinked node_modules\n'
unset -f sync_dependencies drop_lock_fd

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
reviewed_id="$(cd "$real_repo" && "$real_north" review-gate id refs/remotes/origin/main HEAD harness/ agent-machinery/ bin/ lib/)"
"$real_git" -C "$real_repo" commit -q --amend -m 'agent-machinery project' -m "Reviewed: approves; reviewer: fixture-reviewer; patch-id $reviewed_id"
cp "$deps_bin/bun" "$real_bin/bun"
HOME="$land_home" run_real_case '' --dry-run --to main
rm -f "$real_bin/bun"
expect_status zero
expect_output "safe-push: no node_modules — bun install --frozen-lockfile in $real_repo/agent-machinery"
expect_output 'safe-push: ✓ clean (--dry-run)'
printf 'PASS: a fresh north worktree installs agent-machinery before its land check\n'

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
HOME="$land_home" run_real_case '' --to main
expect_status nonzero
expect_output 'safe-push: ✗ this landing undoes recent commits on main'
[ ! -e "$real_repo/check-runs" ] || fail 'a stale-revert refusal still ran the land checks'
[ "$(real_remote_state)" = "$stale_remote_before" ] || fail 'a stale-revert refusal published'
printf 'PASS: a landing that undoes recent main is refused before its land checks start\n'

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
SAFE_PUSH_TEST_LANGUAGE_TRACE="$land_clean_trace" HOME="$land_home" run_real_case '' --to main
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
grep -Fxq "north <land-clean> <$real_repo>" "$land_clean_trace" || fail 'a failed land check left the lane build output'
[ "$(real_remote_state)" = "$land_remote_before" ] || fail 'failing land check published'
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
SAFE_PUSH_TEST_LANGUAGE_TRACE="$land_clean_trace" HOME="$land_home" run_real_case '' --dry-run --to main
expect_status zero
expect_output "safe-push: land check: $land_command"
expect_output 'PASS stdout'
expect_output 'PASS stderr'
expect_output 'safe-push: ✓ clean (--dry-run)'
! grep -Fq 'log:' <<<"$case_output" || fail 'successful check added visible logging output'
grep -Fxq "north <land-clean> <$real_repo>" "$land_clean_trace" || fail 'a passing land check left the lane build output'
: >"$land_clean_trace"
SAFE_PUSH_TEST_LANGUAGE_TRACE="$land_clean_trace" HOME="$land_home" run_real_case '' --dry-run --to main --keep-lane
expect_status zero
! grep -Fq '<land-clean>' "$land_clean_trace" || fail '--keep-lane released the lane build output'
: >"$land_clean_trace"
SAFE_PUSH_TEST_LANGUAGE_TRACE="$land_clean_trace" HOME="$land_home" run_real_case '' --dry-run --to other
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
HOME="$land_home" run_real_case '' --dry-run --to main
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
SAFE_PUSH_TEST_LANGUAGE_TRACE="$land_clean_trace" HOME="$land_home" run_real_case '' --dry-run --to main
expect_status zero
[ "$(wc -l <"$real_repo/check-runs")" -eq 1 ] || fail 'unmatched or uncommitted check ran'
! grep -Fq '<land-clean>' "$land_clean_trace" || fail 'a push that ran no land check released the lane build output'
"$real_git" -C "$real_repo" checkout -- .safe-push
"$real_git" -C "$real_repo" rm -q .safe-push
"$real_git" -C "$real_repo" commit -qm 'no land.check'
HOME="$land_home" run_real_case '' --to main
expect_status zero
[ "$("$real_git" --git-dir="$real_remote" rev-parse main)" = "$("$real_git" -C "$real_repo" rev-parse HEAD)" ] || fail 'unconfigured landing did not publish'

make_real_fixture land-publish
"$real_git" -C "$real_repo" config --file .safe-push land.publish farm/
"$real_git" -C "$real_repo" config --file .safe-push --add land.check "file.txt :: printf '%s\\n' \"\$SAFE_PUSH_LAND_REF\" > '$real_fixture/published-ref'; '$real_git' --git-dir='$real_remote' rev-parse \"refs/heads/\$SAFE_PUSH_LAND_REF\" > '$real_fixture/published-head'"
printf 'published\n' >"$real_repo/file.txt"
"$real_git" -C "$real_repo" add .safe-push file.txt
"$real_git" -C "$real_repo" commit -qm 'publish for the land check'
publish_head=$("$real_git" -C "$real_repo" rev-parse HEAD)
HOME="$real_fixture" run_real_case '' --to main
expect_status zero
[ "$(cat "$real_fixture/published-ref")" = "farm/${publish_head:0:12}" ] || fail 'land check did not get the published ref'
[ "$(cat "$real_fixture/published-head")" = "$publish_head" ] || fail 'published ref did not hold the lane head while the land check ran'
! "$real_git" --git-dir="$real_remote" rev-parse -q --verify "refs/heads/farm/${publish_head:0:12}" >/dev/null || fail 'published ref outlived the land checks'
printf 'PASS: land.publish pushes the scanned lane to a scratch ref for the land checks and deletes it afterwards\n'

make_real_fixture planning-outcomes
for args_text in '--dry-run' '--dry-run --to other'; do
  read -r -a parsed_args <<<"$args_text"
  remote_before="$(real_remote_state)"
  run_real_case '' "${parsed_args[@]}"
  expect_status zero
  [ "$(real_remote_state)" = "$remote_before" ] || fail 'dry-run changed origin'
done
remote_main_before=$("$real_git" --git-dir="$real_remote" rev-parse main)
run_real_case '' --to other
expect_status zero
[ "$("$real_git" --git-dir="$real_remote" rev-parse other)" = "$("$real_git" -C "$real_repo" rev-parse HEAD)" ] || fail 'explicit destination did not receive HEAD'
[ "$("$real_git" --git-dir="$real_remote" rev-parse main)" = "$remote_main_before" ] || fail 'explicit destination changed main'

# Real commit publication proves captured object refspecs still
# perform the normal operation against an actual bare remote.
make_real_fixture normal
run_real_case ''
expect_status zero
[ "$("$real_git" -C "$real_repo" rev-parse main)" \
  = "$("$real_git" --git-dir="$real_remote" rev-parse refs/heads/main)" ] \
  || fail 'normal branch publication did not update the remote to captured HEAD'

make_real_fixture lease-boundary
lease_oid=$("$real_git" -C "$real_repo" rev-parse HEAD)
"$real_git" -C "$real_repo" push -q origin "$lease_oid:refs/heads/lease-peer"
"$real_git" -C "$real_repo" commit --allow-empty -qm 'after intervening tip'
lease_bin="$scratch/lease-bin"
mkdir -p "$lease_bin"
cat >"$lease_bin/git" <<'GIT'
#!/usr/bin/env bash
set -euo pipefail
if [ "${1:-}" = push ]; then
  "$SAFE_PUSH_TEST_REAL_GIT" --git-dir="$SAFE_PUSH_TEST_REMOTE" update-ref refs/heads/main "$SAFE_PUSH_TEST_LEASE_OID"
  "$SAFE_PUSH_TEST_REAL_GIT" -C "$SAFE_PUSH_TEST_REPO" update-ref refs/remotes/origin/main "$SAFE_PUSH_TEST_LEASE_OID"
  printf 'raced\n' >"$SAFE_PUSH_TEST_STATE"
fi
exec "$SAFE_PUSH_TEST_REAL_GIT" "$@"
GIT
chmod +x "$lease_bin/git"
PATH="$lease_bin:$PATH" SAFE_PUSH_TEST_REAL_GIT="$real_git" SAFE_PUSH_TEST_LEASE_OID="$lease_oid" run_real_case ''
expect_status nonzero
[ -s "$real_state" ] || fail 'lease race was not injected'
[ "$("$real_git" --git-dir="$real_remote" rev-parse main)" = "$lease_oid" ] || fail 'publication overwrote a destination that moved after revalidation'

# A new branch with no upstream is off the default branch, so the one-branch-main
# guard refuses the bare default before any scan or push — no same-name litter,
# no tracking established.
make_real_fixture no-upstream
"$real_git" -C "$real_repo" switch -q -c topic
"$real_git" -C "$real_repo" commit --allow-empty -qm topic
remote_before="$(real_remote_state)"
run_real_case ''
expect_status nonzero
[ "$(real_remote_state)" = "$remote_before" ] \
  || fail 'non-main refusal mutated the remote'
[ ! -s "$real_trace" ] || fail 'non-main refusal reached scan'
if "$real_git" -C "$real_repo" rev-parse --verify '@{upstream}' >/dev/null 2>&1; then
  fail 'non-main refusal established local tracking'
fi

# A feature branch can inherit origin/main when it is created from that remote
# ref. The guard refuses the bare default regardless of inherited tracking —
# it never reaches the code that would decide which remote ref to touch.
make_real_fixture inherited-upstream
remote_main_before="$("$real_git" --git-dir="$real_remote" rev-parse refs/heads/main)"
"$real_git" -C "$real_repo" switch -q -c feature
"$real_git" -C "$real_repo" commit --allow-empty -qm feature
"$real_git" -C "$real_repo" branch --set-upstream-to=origin/main feature >/dev/null
run_real_case ''
expect_status nonzero
[ "$("$real_git" --git-dir="$real_remote" rev-parse refs/heads/main)" = "$remote_main_before" ] \
  || fail 'inherited origin/main upstream let the refused default push touch remote main'
if "$real_git" --git-dir="$real_remote" rev-parse --verify refs/heads/feature >/dev/null 2>&1; then
  fail 'non-main refusal advanced a remote feature ref'
fi

# ~/.local/bin links firn dotfiles/bin/ and north bin/ tools out-of-store, so a
# removed tool needs a same-name replacement in either repository.
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
HOME="$tool_home" run_real_case '' --dry-run
expect_status nonzero
"$real_git" -C "$real_repo" reset -q --hard HEAD~1
"$real_git" -C "$real_repo" rm -q dotfiles/bin/shared-tool
"$real_git" -C "$real_repo" commit -qm 'drop shared-tool'
HOME="$tool_home" run_real_case '' --dry-run
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
  HOME="$tool_home" run_real_case '' --dry-run
  case "$name" in
    retired-tool|helper.test.sh) expect_status zero ;;
    *) expect_status nonzero; [ ! -s "$real_trace" ] || fail 'core removal reached scan' ;;
  esac
  [ "$(real_remote_state)" = "$remote_before" ] || fail 'tool-removal dry-run changed origin'
done
"$real_git" -C "$real_repo" reset -q --hard "$tool_base"
"$real_git" -C "$real_repo" rm -q dotfiles/bin/helper.test.sh
"$real_git" -C "$real_repo" commit -qm 'remove test helper without trailer'
HOME="$tool_home" run_real_case '' --dry-run
expect_status zero

eval "$(sed -n '/^tracked_executable() {/,/^}$/p; /^tool_removal_scan() {/,/^}$/p; /^core_tools=/p' "$TARGET")"
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
printf "%s\n" tool_removal_scan >>"$scratch/cases"
(cd "$real_repo"; HOME="$tool_home" tool_removal_scan "$remote_tool_head..$local_tool_head") || fail 'tool scan compared with destination instead of merge base'

make_real_fixture test-kind
mkdir -p "$real_repo/src" "$real_repo/tests"
printf '#[test]\nfn deleted_boundary() {}\nfn rust_boundary() {}\n' >"$real_repo/src/lib.rs"
printf 'it("kept", () => {\n  check()\n})\n' >"$real_repo/tests/boundary.js"
"$real_git" -C "$real_repo" add src/lib.rs tests/boundary.js
"$real_git" -C "$real_repo" commit -qm 'existing tests'
"$real_git" -C "$real_repo" push -q origin HEAD:main
kind_base=$("$real_git" -C "$real_repo" rev-parse HEAD)
for kind_case in rust-unannotated js-unannotated rust-annotated js-annotated invalid-kind empty-mutant deleted edited-js moved moved-js-unannotated renamed retitled-js replaced-js; do
  "$real_git" -C "$real_repo" reset -q --hard "$kind_base"
  trailer=''
  case "$kind_case" in
    rust-annotated) trailer='Test-kind: rust_boundary 3 kills rust-trailer-pass' ;;
    js-annotated) trailer='Test-kind: js boundary 3 kills js-trailer-pass' ;;
    invalid-kind) trailer='Test-kind: rust_boundary 6 kills invalid-kind' ;;
    empty-mutant) trailer='Test-kind: rust_boundary 3 kills ' ;;
  esac
  if [ -n "$trailer" ]; then
    "$real_git" -C "$real_repo" commit --allow-empty -qm 'test evidence' -m "$trailer"
  fi
  case "$kind_case" in
    js-*)
      printf 'it("js boundary", () => {})\n' >"$real_repo/tests/boundary.js"
      case_path=tests/boundary.js; case_name='js boundary'
      ;;
    deleted)
      printf 'fn rust_boundary() {}\n' >"$real_repo/src/lib.rs"
      ;;
    edited-js)
      printf 'it("kept", async () => {})\n' >"$real_repo/tests/boundary.js"
      ;;
    retitled-js)
      printf 'it("kept, retitled", () => {\n  check()\n})\n' >"$real_repo/tests/boundary.js"
      ;;
    replaced-js)
      printf 'it("replaced", () => {\n  other()\n})\n' >"$real_repo/tests/boundary.js"
      case_path=tests/boundary.js; case_name=replaced
      ;;
    renamed)
      "$real_git" -C "$real_repo" mv src/lib.rs src/state.rs
      ;;
    moved*)
      mkdir -p "$real_repo/tests/moved"
      "$real_git" -C "$real_repo" mv tests/boundary.js tests/moved/boundary.js
      [ "$kind_case" = moved ] || printf '%s("js boundary", () => {})\n' it >>"$real_repo/tests/moved/boundary.js"
      case_path=tests/moved/boundary.js; case_name='js boundary'
      ;;
    *)
      printf '#[test]\nfn deleted_boundary() {}\n#[tokio::test]\nfn rust_boundary() {}\n' >"$real_repo/src/lib.rs"
      case_path=src/lib.rs; case_name=rust_boundary
      ;;
  esac
  "$real_git" -C "$real_repo" add -A src tests
  "$real_git" -C "$real_repo" commit -qm "$kind_case"
  case "$kind_case" in
    *-annotated)
      printf 'mutated\n' >"$real_repo/content.txt"
      (cd "$real_repo"; XDG_STATE_HOME="$xdg_state" "$real_north" mutant record "${trailer##* }")
      "$real_git" -C "$real_repo" restore content.txt
      ;;
  esac
  run_real_case '' --dry-run
  case "$kind_case" in
    *-annotated|deleted|edited-js|moved|renamed|retitled-js) expect_status zero ;;
    *)
      expect_status nonzero
      expect_output "safe-push: ✗ $case_path adds test $case_name without a Test-kind: trailer naming its kind (1-5) and a mutant only it kills; NOT pushed."
      ;;
  esac
done

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
    kit-tree) printf '%s("upstream boundary", () => {})\n' it >"$real_repo/repos/upstream/test/Dns.integration.test.ts" ;;
    repo-json-tree) printf '%s("library boundary", () => {})\n' it >"$real_repo/vendor/lib/test/Lib.test.ts" ;;
    both-trees)
      printf '%s("upstream boundary", () => {})\n' it >"$real_repo/repos/upstream/test/Dns.integration.test.ts"
      printf '%s("library boundary", () => {})\n' it >"$real_repo/vendor/lib/test/Lib.test.ts"
      printf '%s.skip("upstream skipped", () => {})\n' it >>"$real_repo/vendor/lib/test/Lib.test.ts"
      ;;
    first-party)
      printf '%s("upstream boundary", () => {})\n' it >"$real_repo/repos/upstream/test/Dns.integration.test.ts"
      printf '%s("first party boundary", () => {})\n' it >"$real_repo/tests/Own.test.ts"
      case_path=tests/Own.test.ts; case_name='first party boundary'
      ;;
    sibling-prefix)
      printf '%s("sibling boundary", () => {})\n' it >"$real_repo/repos/upstream-extra/test/Own.test.ts"
      case_path=repos/upstream-extra/test/Own.test.ts; case_name='sibling boundary'
      ;;
    self-declared)
      printf '{"vendor": "repos/upstream", "other": "own"}\n' >"$real_repo/effect-kit.json"
      printf '{"prefix": "own"}\n' >"$real_repo/repos/own.json"
      mkdir -p "$real_repo/own"
      printf '%s("own boundary", () => {})\n' it >"$real_repo/own/Own.test.ts"
      case_path=own/Own.test.ts; case_name='own boundary'
      ;;
  esac
  "$real_git" -C "$real_repo" add -A .
  "$real_git" -C "$real_repo" commit -qm "$vendored_case"
  run_real_case '' --dry-run
  case "$vendored_case" in
    *-tree*) expect_status zero ;;
    *)
      expect_status nonzero
      expect_output "safe-push: ✗ $case_path adds test $case_name without a Test-kind: trailer naming its kind (1-5) and a mutant only it kills; NOT pushed."
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
  run_real_case '' --dry-run
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
  run_real_case '' "${parsed_args[@]}"
  expect_status nonzero
  [ "$(real_remote_state)" = "$remote_before" ] || fail 'mutant refusal changed origin'
done
for trailer in '' 'Test-change: the retry rule was removed'; do
  "$real_git" -C "$real_repo" reset -q --hard "$integrity_base"
  printf '%s"retries", () => {})\n' "it.sk""ip(" >"$real_repo/retry.test.ts"
  "$real_git" -C "$real_repo" add retry.test.ts
  "$real_git" -C "$real_repo" commit -qm 'skip retry test' ${trailer:+-m "$trailer"}
  run_real_case '' --dry-run
  if [ -z "$trailer" ]; then expect_status nonzero
  else expect_status zero; fi
done
} >"$scratch/section-1.log" 2>&1 &
sections=($!)

{ xdg_state="$scratch/xdg-state-2"
new_real_bin -ci

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
  for args_text in '--dry-run' '--dry-run --to main'; do
    read -r -a parsed_args <<<"$args_text"
    run_real_case '' "${parsed_args[@]}"
    case "$journey" in
      churn|missing|removed-authority|untracked-input|added-input|removed-input|unchanged-missing|unchanged-removed|absent-output)
        expect_status nonzero
        expect_output 'NOT pushed.'
        if [ "$journey" = absent-output ]; then expect_output absent.tsv; else expect_output "$record_path"; fi
        ;;
      *) expect_status zero ;;
    esac
    [ "$(real_remote_state)" = "$remote_before" ] || fail 'generated dry-run changed origin'
  done
done
printf 'Generated rewrite recorded journey: PASS (both dry-run destinations, remote unchanged)\n'

make_real_fixture provenance
printf '| Box | Area | Done when | State |\n|---|---|---|---|\n| 9.10p | query view | the query view opens | open |\n' >"$real_repo/PLAN.md"
"$real_git" -C "$real_repo" add PLAN.md
"$real_git" -C "$real_repo" commit -qm 'galileo plan'
"$real_git" -C "$real_repo" push -q origin HEAD:main
target_base=$("$real_git" -C "$real_repo" rev-parse HEAD)
for target_case in unsourced unsourced-none unsourced-rationale sourced reworded; do
  "$real_git" -C "$real_repo" reset -q --hard "$target_base"
  printf '| 9.11p | query view | the 1,000-row query keystroke stays under 50 ms | open |\n' >>"$real_repo/PLAN.md"
  case "$target_case" in
    sourced) "$real_git" -C "$real_repo" commit -qam 'query keystroke target' \
      -m 'Provenance: keystroke under 50 ms; origin experiment; pointer galileo bench/keystroke.json plus just bench keystroke; rationale docs/rationale/targets.md#soft-targets; revisit 2026-11-01' ;;
    unsourced-rationale) "$real_git" -C "$real_repo" commit -qam 'query keystroke target' \
      -m 'Provenance: keystroke under 50 ms; origin experiment; pointer galileo bench/keystroke.json plus just bench keystroke; revisit 2026-11-01' ;;
    unsourced-none) "$real_git" -C "$real_repo" commit -qam 'query keystroke target' \
      -m 'Provenance: keystroke under 50 ms; origin none; pointer none; rationale not recorded; revisit 2026-11-01' ;;
    *) "$real_git" -C "$real_repo" commit -qam 'query keystroke target' ;;
  esac
  if [ "$target_case" = reworded ]; then
    "$real_git" -C "$real_repo" push -q origin HEAD:main
    sed -i 's/the 1,000-row query keystroke stays under 50 ms/a keystroke on the 1,000-row query renders under 50 ms/' "$real_repo/PLAN.md"
    "$real_git" -C "$real_repo" commit -qam 'reword query keystroke row'
  fi
  run_real_case '' --dry-run
  case "$target_case" in
    unsourced*)
      expect_status nonzero
      expect_output 'under 50 ms'
      expect_output "add 'Provenance: <claim with the bar>; origin <tom|experiment|prior-art|incident>; pointer"
      ;;
    *) expect_status zero ;;
  esac
done
"$real_git" -C "$real_repo" fetch -q origin
"$real_git" -C "$real_repo" reset -q --hard origin/main
printf '| 9.12p | query view | the 1,000-row query scroll stays under 40 ms | open |\n' >>"$real_repo/PLAN.md"
"$real_git" -C "$real_repo" commit -qam 'query scroll target for ops#17'
run_real_case '' --dry-run
expect_status nonzero
expect_output 'names a native NorthThread item'
expect_output "add 'Provenance: <claim with the bar>; origin <tom|experiment|prior-art|incident>; pointer"
printf 'Every refusing scan reports in one run: PASS (native ref and provenance together)\n'

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
  "$real_git" -C "$real_repo" commit -qm "policy sweep case $policy_case" \
    -m 'Provenance: agents/AGENTS.md: ask once on the policy fixture; origin experiment; pointer agents/replay/sweep-2026-10-28.jsonl plus the fixture sweep; rationale not recorded; revisit 2026-11-01'
  run_real_case '' --dry-run
  case "$policy_case" in
    row | no-policy-change) expect_status zero ;;
    *)
      expect_status nonzero
      expect_output 'makes agents/AGENTS.md'
      ;;
  esac
done
printf 'Policy sweep gate: PASS (no row, row, one trial, other sha, state-only row, no policy change)\n'

ci_fixture="$scratch/ci-fixture"
mkdir -p "$ci_fixture"
ln -s "$(readlink -f "${BASH_SOURCE[0]}")" "$real_bin/gh"
cat >"$ci_fixture/runs.json" <<'JSON'
[
  {"databaseId":106,"conclusion":"cancelled","createdAt":"2026-10-11T06:00:00Z"},
  {"databaseId":105,"conclusion":"skipped","createdAt":"2026-10-11T05:00:00Z"},
  {"databaseId":104,"conclusion":"failure","createdAt":"2026-10-11T04:00:00Z"},
  {"databaseId":103,"conclusion":"success","createdAt":"2026-10-11T03:00:00Z"},
  {"databaseId":102,"conclusion":"failure","createdAt":"2026-10-11T02:00:00Z"},
  {"databaseId":101,"conclusion":"success","createdAt":"2026-10-11T01:00:00Z"},
  {"databaseId":100,"conclusion":"success","createdAt":"2026-10-11T00:00:00Z"}
]
JSON
printf '[{"jobs":[]}]\n' >"$ci_fixture/104.json"
cat >"$ci_fixture/103.json" <<'JSON'
[
  {"jobs":[{"name":"bootstrap","started_at":"2026-10-11T00:00:00Z","completed_at":"2026-10-11T00:02:00Z"}]},
  {"jobs":[{"name":"integration","started_at":"2026-10-11T00:03:20Z","completed_at":"2026-10-11T00:08:00Z"}]}
]
JSON
printf '[{"jobs":[{"name":"older tests","started_at":"2026-10-11T00:00:00Z","completed_at":"2026-10-11T00:05:00Z"}]}]\n' >"$ci_fixture/102.json"
forget_run_measurements
printf '[{"jobs":[{"name":"oldest tests","started_at":"2026-10-11T00:00:00Z","completed_at":"2026-10-11T00:03:00Z"}]}]\n' >"$ci_fixture/101.json"
make_real_fixture ci-budget
printf '[ci]\n\tbudget = farm-test.yml 240\n' >"$real_repo/.safe-push"
"$real_git" -C "$real_repo" add .safe-push
"$real_git" -C "$real_repo" commit -qm 'main CI budget'
"$real_git" -C "$real_repo" push -q origin HEAD:main
ci_base=$("$real_git" -C "$real_repo" rev-parse HEAD)
printf 'new work\n' >>"$real_repo/content.txt"
"$real_git" -C "$real_repo" commit -qam 'new work'
remote_before="$(real_remote_state)"
SAFE_PUSH_TEST_CI_FIXTURE="$ci_fixture" run_real_case '' --dry-run --to main
expect_status nonzero
expect_output 'farm-test.yml: median 300s > budget 240s'
expect_output 'slowest job in most recent run: integration (280s); NOT pushed.'
[ "$(real_remote_state)" = "$remote_before" ] || fail 'CI budget refusal changed origin'
printf '%s\n' "$case_output" | grep 'CI budget .*NOT pushed\.'

"$real_git" -C "$real_repo" reset -q --hard "$ci_base"
printf 'prose\n' >"$real_repo/notes.md"
"$real_git" -C "$real_repo" add notes.md
"$real_git" -C "$real_repo" commit -qm 'markdown only'
SAFE_PUSH_TEST_CI_FIXTURE="$ci_fixture" run_real_case '' --dry-run --to main
expect_status zero
expect_output 'every changed path is Markdown'

"$real_git" -C "$real_repo" reset -q --hard "$ci_base"
printf 'new work\n' >>"$real_repo/content.txt"
"$real_git" -C "$real_repo" commit -qam 'new work'

printf '[{"jobs":[{"name":"older tests","started_at":"2026-10-11T00:00:00Z","completed_at":"2026-10-11T00:03:20Z"}]}]\n' >"$ci_fixture/102.json"
forget_run_measurements
SAFE_PUSH_TEST_CI_FIXTURE="$ci_fixture" run_real_case '' --dry-run
expect_status zero
expect_output 'clean (--dry-run)'

printf '[{"jobs":[{"name":"older tests","started_at":"2026-10-11T00:00:00Z","completed_at":"2026-10-11T00:05:00Z"}]}]\n' >"$ci_fixture/102.json"
forget_run_measurements
"$real_git" -C "$real_repo" commit --amend -qm 'speed up tests' -m 'CI-budget: farm-test.yml 300s -> 200s'
SAFE_PUSH_TEST_CI_FIXTURE="$ci_fixture" run_real_case '' --dry-run --to main
expect_status zero
expect_output 'CI budget farm-test.yml exemption'

"$real_git" -C "$real_repo" commit --amend -qm 'speed up tests and add a test' \
  -m $'CI-budget: farm-test.yml 300s -> 200s\nTest-kind: 3 boundary'
SAFE_PUSH_TEST_CI_FIXTURE="$ci_fixture" run_real_case '' --dry-run --to main
expect_status nonzero
expect_output 'farm-test.yml: median 300s > budget 240s'
expect_output 'NOT pushed.'

"$real_git" -C "$real_repo" commit --amend -qm 'fix red main' -m 'Main-red: 104'
SAFE_PUSH_TEST_CI_FIXTURE="$ci_fixture" run_real_case '' --dry-run --to main
expect_status zero
expect_output 'main is red at run 104'
cp "$ci_fixture/runs.json" "$ci_fixture/runs.red.json"
sed 's/"databaseId":104,"conclusion":"failure"/"databaseId":104,"conclusion":"success"/' \
  "$ci_fixture/runs.red.json" >"$ci_fixture/runs.json"
SAFE_PUSH_TEST_CI_FIXTURE="$ci_fixture" run_real_case '' --dry-run --to main
expect_status nonzero
expect_output 'farm-test.yml: median 300s > budget 240s'
mv "$ci_fixture/runs.red.json" "$ci_fixture/runs.json"

"$real_git" -C "$real_repo" reset -q --hard "$ci_base"
printf 'faster tests\n' >>"$real_repo/content.txt"
"$real_git" -C "$real_repo" commit -qam 'cut farm-test jobs' -m 'CI-budget: farm-test.yml 300s -> 150s' \
  -m 'Co-Authored-By: Claude <noreply@anthropic.com>'
"$real_git" -C "$real_repo" push -q origin HEAD:main
ci_speedup=$("$real_git" -C "$real_repo" rev-parse HEAD)
"$real_git" -C "$real_repo" fetch -q origin
cp "$ci_fixture/runs.json" "$ci_fixture/runs.before-speedup.json"
jq --arg old "$ci_base" --arg new "$ci_speedup" \
  '[{"databaseId":107,"conclusion":"success","createdAt":"2026-10-11T07:00:00Z","headSha":$new}] + map(. + {headSha: $old})' \
  "$ci_fixture/runs.before-speedup.json" >"$ci_fixture/runs.json"
printf '[{"jobs":[{"name":"fast tests","started_at":"2026-10-11T07:00:00Z","completed_at":"2026-10-11T07:02:30Z"}]}]\n' >"$ci_fixture/107.json"
printf 'new work\n' >>"$real_repo/content.txt"
"$real_git" -C "$real_repo" commit -qam 'new work after speed-up'
SAFE_PUSH_TEST_CI_FIXTURE="$ci_fixture" run_real_case '' --dry-run --to main
expect_status zero
expect_output 'measuring 1 main runs since CI-budget commit'
mv "$ci_fixture/runs.before-speedup.json" "$ci_fixture/runs.json"
"$real_git" -C "$real_repo" push -q origin "+$ci_base:refs/heads/main"
"$real_git" -C "$real_repo" fetch -q origin

ci_hold_dir="$xdg_state/safe-push"
"$real_git" -C "$real_repo" reset -q --hard "$ci_base"
printf 'held work\n' >>"$real_repo/content.txt"
"$real_git" -C "$real_repo" commit -qam 'work held by an over-budget history'
SAFE_PUSH_TEST_CI_FIXTURE="$ci_fixture" run_real_case '' --dry-run --to main
expect_status nonzero
expect_output 'farm-test.yml: median 300s > budget 240s'
expect_output 'a recorded override opens at'
ci_hold=$(ls "$ci_hold_dir"/ci-budget-hold-*farm-test.yml)
echo $(( $(date +%s) - 1900 )) >"$ci_hold"
SAFE_PUSH_TEST_CI_FIXTURE="$ci_fixture" run_real_case '' --dry-run --to main
expect_status zero
expect_output 'CI budget farm-test.yml override: median 300s > budget 240s'
grep -q $'\tfarm-test.yml\t300\t240\t' "$ci_hold_dir/ci-budget-overrides.tsv" || fail 'override was not recorded'
printf '[{"jobs":[{"name":"older tests","started_at":"2026-10-11T00:00:00Z","completed_at":"2026-10-11T00:03:20Z"}]}]\n' >"$ci_fixture/102.json"
forget_run_measurements
SAFE_PUSH_TEST_CI_FIXTURE="$ci_fixture" run_real_case '' --dry-run --to main
expect_status zero
[ ! -e "$ci_hold" ] || fail 'a passing budget left the hold in place'
mv "$ci_fixture/102.json" "$ci_fixture/102.json.gone"
SAFE_PUSH_TEST_CI_FIXTURE="$ci_fixture" run_real_case '' --dry-run --to main
expect_status zero
mv "$ci_fixture/102.json.gone" "$ci_fixture/102.json"
printf '[{"jobs":[{"name":"older tests","started_at":"2026-10-11T00:00:00Z","completed_at":"2026-10-11T00:05:00Z"}]}]\n' >"$ci_fixture/102.json"
forget_run_measurements
SAFE_PUSH_TEST_CI_FIXTURE="$ci_fixture" run_real_case '' --dry-run --to main
expect_status nonzero
expect_output 'a recorded override opens at'
mv "$ci_fixture/runs.json" "$ci_fixture/runs.offline.json"
echo $(( $(date +%s) - 1900 )) >"$ci_hold"
SAFE_PUSH_TEST_CI_FIXTURE="$ci_fixture" run_real_case '' --dry-run --to main
expect_status zero
expect_output 'CI budget farm-test.yml override: gh failed to list completed main runs'
mv "$ci_fixture/runs.offline.json" "$ci_fixture/runs.json"
rm -f -- "$ci_hold"

"$real_git" -C "$real_repo" reset -q --hard "$ci_base"
printf '[ci]\n\tbudget = local:timings.tsv 447\n' >"$real_repo/.safe-push"
printf '%s\t600\tpassed\tparser (580s)\n%s\t500\tfailed\tcompiler (480s)\n%s\t480\tpassed\ttypechecker (450s)\n' \
  "$ci_base" "$ci_base" "$ci_base" >"$real_repo/timings.tsv"
"$real_git" -C "$real_repo" add .safe-push timings.tsv
"$real_git" -C "$real_repo" commit -qm 'local CI budget'
"$real_git" -C "$real_repo" push -q origin HEAD:main
"$real_git" -C "$real_repo" commit --allow-empty -qm 'new work after local measurement'
remote_before="$(real_remote_state)"
SAFE_PUSH_TEST_CI_FIXTURE="$ci_fixture" run_real_case '' --dry-run
expect_status nonzero
expect_output 'local:timings.tsv: median 500s > budget 447s'
expect_output 'slowest job in most recent run: typechecker (450s); NOT pushed.'
[ "$(real_remote_state)" = "$remote_before" ] || fail 'local CI budget refusal changed origin'
printf '[ci]\n\tbudget = local:timings.tsv 447 600\n' >"$real_repo/.safe-push"
"$real_git" -C "$real_repo" commit -qam 'soft CI budget with a ceiling'
"$real_git" -C "$real_repo" push -q origin HEAD:main
"$real_git" -C "$real_repo" commit --allow-empty -qm 'work over a soft CI budget'
SAFE_PUSH_TEST_CI_FIXTURE="$ci_fixture" run_real_case '' --dry-run
expect_status zero
expect_output 'CI budget local:timings.tsv debt: median 500s over target 447s, under ceiling 600s'
grep -q $'\tlocal:timings.tsv\t500\t447\t600\t' "$ci_hold_dir/ci-budget-debt.tsv" || fail 'CI budget debt was not recorded'
printf '[ci]\n\tbudget = local:timings.tsv 447 480\n' >"$real_repo/.safe-push"
"$real_git" -C "$real_repo" commit -qam 'soft CI budget under the median'
"$real_git" -C "$real_repo" push -q origin HEAD:main
"$real_git" -C "$real_repo" commit --allow-empty -qm 'work over a CI budget ceiling'
SAFE_PUSH_TEST_CI_FIXTURE="$ci_fixture" run_real_case '' --dry-run
expect_status nonzero
expect_output 'local:timings.tsv: median 500s > budget 447s'

make_real_fixture ci-unconfigured
printf '[ci]\n\tbudget = farm-test.yml 240\n' >"$real_repo/.safe-push"
"$real_git" -C "$real_repo" add .safe-push
"$real_git" -C "$real_repo" commit -qm 'propose a budget for future landings'
SAFE_PUSH_TEST_CI_FIXTURE="$scratch/absent-ci-fixture" run_real_case '' --dry-run
expect_status zero
expect_output 'clean (--dry-run)'
! grep -Eq '^gh <(run|api)>' "$real_trace" || fail 'unconfigured main queried CI history'
make_real_fixture ci-filtered
mkdir -p "$real_repo/.github/workflows"
printf '[ci]\n\tbudget = farm-test.yml 240\n' >"$real_repo/.safe-push"
printf 'on:\n  push:\n    branches: [main]\n    paths:\n      - "src/**"\njobs: {}\n' >"$real_repo/.github/workflows/farm-test.yml"
"$real_git" -C "$real_repo" add .safe-push .github
"$real_git" -C "$real_repo" commit -qm 'filtered workflow'
"$real_git" -C "$real_repo" push -q origin HEAD:main
printf 'new work\n' >>"$real_repo/content.txt"
"$real_git" -C "$real_repo" commit -qam 'outside the filter'
SAFE_PUSH_TEST_CI_FIXTURE="$ci_fixture" run_real_case '' --dry-run --to main
expect_status zero
expect_output 'farm-test.yml exemption — no changed path matches its push paths filter'
mkdir -p "$real_repo/src/deep"
printf 'code\n' >"$real_repo/src/deep/main.ts"
"$real_git" -C "$real_repo" add src
"$real_git" -C "$real_repo" commit -qm 'inside the filter'
SAFE_PUSH_TEST_CI_FIXTURE="$ci_fixture" run_real_case '' --dry-run --to main
expect_status nonzero
expect_output 'farm-test.yml: median 300s > budget 240s'

rm -f "$real_bin/gh"
make_real_fixture autoland
ln -sf "$(readlink -f "${BASH_SOURCE[0]}")" "$real_bin/gh"
mkdir -p "$real_repo/.github/workflows"
printf 'on:\n  push:\n    branches: ["claude/**"]\n' >"$real_repo/.github/workflows/autoland.yml"
"$real_git" -C "$real_repo" add .github/workflows/autoland.yml
"$real_git" -C "$real_repo" commit -qm 'Autoland workflow'
"$real_git" -C "$real_repo" push -q origin HEAD:main
autoland_base=$("$real_git" -C "$real_repo" rev-parse HEAD)
printf 'queued work\n' >>"$real_repo/content.txt"
"$real_git" -C "$real_repo" commit -qam 'work for the train'
SAFE_PUSH_TEST_AUTOLAND_STATUS=$'error\tlocal refresh required: Classic effect record\thttps://example.test/refresh' run_real_case '' --to main
[ "$case_status" -eq 1 ] || fail 'a stale Classic record did not return as a refusal'
expect_output 'returned this lane for local Classic record refresh'
queue_tip=$("$real_git" -C "$real_repo" rev-parse HEAD)
handed_back_branch="claude/land-$("$real_git" -C "$real_repo" symbolic-ref --short HEAD)-${queue_tip:0:12}"
! "$real_git" --git-dir="$real_remote" rev-parse -q --verify "refs/heads/$handed_back_branch" >/dev/null || fail 'the handed-back Classic lane queue ref was kept on origin'
[ "$("$real_git" --git-dir="$real_remote" rev-parse main)" = "$autoland_base" ] || fail 'a stale Classic record was landed'
SAFE_PUSH_TEST_AUTOLAND_STATUS=$'failure\tTrain retested\thttps://example.test/run' run_real_case '' --to main
expect_status nonzero
expect_output "main lands through Autoland's one queue"
expect_output 'Autoland did not land this lane: Train retested'
expect_output 'safe-push: refused Autoland did not land this lane: Train retested https://example.test/run'
[ "$("$real_git" --git-dir="$real_remote" rev-parse main)" = "$autoland_base" ] || fail 'an Autoland repository pushed main directly'
autoland_lane_tip=$("$real_git" -C "$real_repo" rev-parse HEAD)
autoland_dropped=$autoland_lane_tip
autoland_other="$scratch/autoland-other"
"$real_git" clone -q "$real_remote" "$autoland_other"
"$real_git" -C "$autoland_other" config user.name safe-push-test
"$real_git" -C "$autoland_other" config user.email safe-push-test@example.invalid
printf 'queued work\n' >>"$autoland_other/content.txt"
printf 'elsewhere\n' >"$autoland_other/elsewhere.txt"
"$real_git" -C "$autoland_other" add content.txt elsewhere.txt
"$real_git" -C "$autoland_other" commit -qm 'queued work and more from elsewhere'
"$real_git" -C "$autoland_other" push -q origin HEAD:main
autoland_moved=$("$real_git" --git-dir="$real_remote" rev-parse main)
SAFE_PUSH_TEST_AUTOLAND_STATUS=$'success\tAutoland landed fixture\thttps://example.test/success' run_real_case '' --to main
expect_status nonzero
grep -Eq "lane commits missing from .*$autoland_dropped.*; NOT pushed\." <<<"$case_output" || fail "the dropped Autoland commit was not named: $case_output"
[ "$("$real_git" --git-dir="$real_remote" rev-parse main)" = "$autoland_moved" ] || fail 'a refused Autoland lane moved main'
[ -z "$("$real_git" --git-dir="$real_remote" for-each-ref 'refs/heads/claude/land-*')" ] || fail 'a refused Autoland lane pushed its queue branch'
"$real_git" -C "$real_repo" reset -q --hard "$autoland_lane_tip"
"$real_git" --git-dir="$real_remote" update-ref refs/heads/main "$autoland_base"
"$real_git" -C "$real_repo" fetch -q origin
printf 'PASS: autoland rebase that drops a lane commit is refused before the queue branch\n'
queue_hook_dir="$scratch/queue-hook"
mkdir -p "$queue_hook_dir/bin"
printf '#!/usr/bin/env bash\nif [ "${1:-}" = push ]; then printf "%%s | %%s\\n" "$(tr "\\n" , <"%s/hook-log" 2>/dev/null)" "$*" >>"%s/push-log"; fi\nexec %q "$@"\n' \
  "$queue_hook_dir" "$queue_hook_dir" "$real_git" >"$queue_hook_dir/bin/git"
chmod +x "$queue_hook_dir/bin/git"
for hook_verdict in 0 1; do
  : >"$queue_hook_dir/hook-log"
  : >"$queue_hook_dir/push-log"
  printf '#!/usr/bin/env bash\ncat >/dev/null\necho start >>"%s/hook-log"\nsleep 2\necho end >>"%s/hook-log"\nexit %s\n' \
    "$queue_hook_dir" "$queue_hook_dir" "$hook_verdict" >"$real_repo/.git/hooks/pre-push"
  chmod +x "$real_repo/.git/hooks/pre-push"
  SAFE_PUSH_TEST_PATH="$queue_hook_dir/bin:$real_bin:$PATH" \
    SAFE_PUSH_TEST_AUTOLAND_STATUS=$'failure\tTrain retested\thttps://example.test/run' run_real_case '' --to main
  expect_status nonzero
  [ "$(grep -c '^start' "$queue_hook_dir/hook-log" || true)" -eq 1 ] || fail "the Autoland pre-push hook did not run exactly once: $(cat "$queue_hook_dir/hook-log")"
  if [ "$hook_verdict" -eq 0 ]; then
    grep -q 'Autoland did not land this lane: Train retested' <<<"$case_output" || fail 'the Autoland lane did not reach the queue verdict after a passing hook'
    head -n 1 "$queue_hook_dir/push-log" | grep -Eq '^start,end, \| push .*--no-verify .*refs/heads/claude/land-' || fail "the queue push did not follow the finished hook with --no-verify: $(cat "$queue_hook_dir/push-log")"
  else
    expect_output 'the pre-push checks failed; NOT pushed.'
    [ ! -s "$queue_hook_dir/push-log" ] || fail 'a failing hook still opened a push'
    [ -z "$("$real_git" --git-dir="$real_remote" for-each-ref 'refs/heads/claude/land-*')" ] || fail 'a failing Autoland hook pushed its queue branch'
  fi
  rm -f "$real_repo/.git/hooks/pre-push"
done
printf 'PASS: an Autoland queue push runs the pre-push hook to completion before opening the push, then pushes with --no-verify\n'
printf 'landed by train\n' >>"$real_repo/content.txt"
"$real_git" -C "$real_repo" commit -qam 'landed by train'
test_landed_sha=$("$real_git" -C "$real_repo" rev-parse HEAD)
runtime_calls="$scratch/runtime-calls"
runtime_wrapper="$scratch/north-runtime-wrapper"
cat >"$runtime_wrapper" <<'SH'
#!/usr/bin/env bash
printf '%s\n' "$1" >>"${SAFE_PUSH_TEST_RUNTIME_CALLS:?}"
exec "${SAFE_PUSH_TEST_RUNTIME_BINARY:?}" "$@"
SH
chmod +x "$runtime_wrapper"
SAFE_PUSH_TEST_REAL_NORTH="$runtime_wrapper" SAFE_PUSH_TEST_RUNTIME_BINARY="$real_north" \
  SAFE_PUSH_TEST_RUNTIME_CALLS="$runtime_calls" SAFE_PUSH_TEST_REMOVE_NORTH_AFTER_PROVENANCE=1 \
  SAFE_PUSH_TEST_AUTOLAND_STATUS="$(printf 'success\tAutoland landed %s\thttps://example.test/success' "$test_landed_sha")" run_real_case '' --to main
expect_status zero
expect_output 'safe-push: landed '
grep -Eq 'safe-push: landed [0-9a-f]{40}$' <<<"$case_output" || fail 'successful landing omitted its terminal sha'
grep -Fxq follows-release "$runtime_calls" || fail 'follows-release was not invoked through the runtime after PATH lost north'
printf 'PASS: safe_push_follows_release_uses_runtime_without_path_north\n'
printf 'PASS: safe_push_landing_emits_terminal_sha\n'
ln -sf "$(readlink -f "${BASH_SOURCE[0]}")" "$real_bin/north"
printf 'timed out lane\n' >>"$real_repo/content.txt"
"$real_git" -C "$real_repo" commit -qam 'timed out lane'
SAFE_PUSH_TEST_AUTOLAND_STATUS=$'pending\tAutoland workflow queued\thttps://example.test/pending' \
  SAFE_PUSH_AUTOLAND_WAIT_SECONDS=1 SAFE_PUSH_AUTOLAND_POLL_SECONDS=1 run_real_case '' --to main
[ "$case_status" -eq 2 ] || fail 'Autoland timeout did not exit 2'
expect_output 'safe-push: timed out after 1m (run https://example.test/pending)'
printf 'stopped wait\n' >>"$real_repo/content.txt"
"$real_git" -C "$real_repo" commit -qam 'stopped while queued'
stopped_tip=$("$real_git" -C "$real_repo" rev-parse HEAD)
stopped_branch="claude/land-$("$real_git" -C "$real_repo" symbolic-ref --short HEAD)-${stopped_tip:0:12}"
stopped_pid="$scratch/stopped-safe-push.pid"
printf '#!/usr/bin/env bash\nread -r host _ </proc/self/stat\nprintf "%%s %%s\\n" "$$" "$host" >%q\nexec %q "$@"\n' "$stopped_pid" "$TARGET" >"$real_fixture/pid-target"
chmod +x "$real_fixture/pid-target"
stopped_target="$TARGET"
TARGET="$real_fixture/pid-target"
(
  SAFE_PUSH_TEST_AUTOLAND_STATUS=$'pending\tAutoland workflow queued\thttps://example.test/pending' \
    SAFE_PUSH_AUTOLAND_WAIT_SECONDS=600 SAFE_PUSH_AUTOLAND_POLL_SECONDS=1 run_real_case '' --to main
  printf '%s\n' "$case_status" >"$scratch/stopped.status"
  printf '%s\n' "$case_output" >"$scratch/stopped.out"
) &
stopped_case=$!
stopped_deadline=$((SECONDS + 60))
stopped_ns='' stopped_host=''
until { test -s "$stopped_pid" && read -r stopped_ns stopped_host <"$stopped_pid"; } && pgrep -P "$stopped_host" -f autoland-wait >/dev/null; do
  [ "$SECONDS" -lt "$stopped_deadline" ] || { kill -TERM ${stopped_ns:+"$stopped_ns"} "$stopped_case" 2>/dev/null; fail 'the stopped lane never reached its Autoland wait'; }
  sleep 0.05
done
stopped_started=$SECONDS
kill -TERM "$stopped_ns"
wait "$stopped_case" || true
TARGET="$stopped_target"
[ "$((SECONDS - stopped_started))" -lt 20 ] || fail 'a stopped Autoland wait did not end promptly'
[ "$(cat "$scratch/stopped.status")" = 130 ] || fail "a stopped Autoland wait exited $(cat "$scratch/stopped.status"): $(cat "$scratch/stopped.out")"
grep -Fq "safe-push: stopped; $stopped_branch stays queued for Autoland" "$scratch/stopped.out" || fail 'a stopped Autoland wait did not say its lane stays queued'
"$real_git" --git-dir="$real_remote" rev-parse -q --verify "refs/heads/$stopped_branch" >/dev/null || fail 'stopping a queued wait deleted its Autoland queue branch'
printf 'PASS: stopping safe-push after its lane is queued leaves the Autoland queue branch on origin\n'
resumed_main=$("$real_git" --git-dir="$real_remote" rev-parse main)
SAFE_PUSH_TEST_AUTOLAND_STATUS="$(printf 'success\tAutoland landed %s\thttps://example.test/resumed' "$stopped_tip")" run_real_case '' --to main
expect_status zero
expect_output "safe-push: Autoland already queues $stopped_tip as $stopped_branch — waiting on it without checking or pushing again"
! grep -Fq 'checking and pushing' <<<"$case_output" || fail "a rerun on a queued commit checked and pushed it again: $case_output"
[ "$("$real_git" --git-dir="$real_remote" rev-parse main)" = "$stopped_tip" ] || fail 'a resumed Autoland wait did not end on the landed commit'
"$real_git" --git-dir="$real_remote" update-ref -d "refs/heads/$stopped_branch" 2>/dev/null || true
"$real_git" --git-dir="$real_remote" update-ref refs/heads/main "$resumed_main"
printf 'PASS: rerunning safe-push on a commit Autoland already queues waits on it without re-checking or re-pushing\n'
run_real_case '' --to main --direct
expect_status zero
expect_output '--direct: bypassing'
[ "$("$real_git" --git-dir="$real_remote" rev-parse main)" = "$("$real_git" -C "$real_repo" rev-parse HEAD)" ] || fail '--direct did not push main'
grep -q $'\t' "$xdg_state/safe-push/direct-landings.tsv" || fail '--direct was not logged'
printf 'on: [push]\n' >"$real_repo/.github/workflows/autoland.yml"
"$real_git" -C "$real_repo" add .github/workflows/autoland.yml
"$real_git" -C "$real_repo" commit -qm 'workflow lane'
gap_lock="$xdg_state/safe-push/$(printf '%s' "$real_remote" | sha256sum | cut -c1-16)"
(
  sleep 3
  [ -e "$gap_lock.lock" ] || touch "$scratch/gap-probe-missed"
  if [ -n "$(find "$gap_lock.queue" -type f 2>/dev/null)" ] || ! flock -n "$gap_lock.lock" true; then
    touch "$scratch/gap-held-lock"
  fi
) &
gap_probe=$!
SAFE_PUSH_TEST_AUTOLAND_RUNS=2 SAFE_PUSH_AUTOLAND_GAP_SECONDS=6 SAFE_PUSH_AUTOLAND_POLL_SECONDS=1 \
  run_real_case '' --to main
wait "$gap_probe"
[ ! -e "$scratch/gap-probe-missed" ] || fail 'the gap probe looked at a lock this fixture never takes'
[ ! -e "$scratch/gap-held-lock" ] || fail 'a workflow lane held the landing lock or a queue ticket while it waited for Autoland trains'
expect_status zero
expect_output '2 Autoland train runs in flight; waiting for none before pushing main'
expect_output 'landing this workflow lane over them'
[ "$("$real_git" --git-dir="$real_remote" rev-parse main)" = "$("$real_git" -C "$real_repo" rev-parse HEAD)" ] || fail 'a workflow lane did not land after the wait'
printf 'on: [push, workflow_dispatch]\n' >"$real_repo/.github/workflows/autoland.yml"
"$real_git" -C "$real_repo" add .github/workflows/autoland.yml
"$real_git" -C "$real_repo" commit -qm 'workflow lane in a gap'
SAFE_PUSH_TEST_AUTOLAND_RUNS=0 run_real_case '' --to main
expect_status zero
! printf '%s\n' "$case_output" | grep -q 'waiting for none' || fail 'waited although no train was in flight'
[ "$("$real_git" --git-dir="$real_remote" rev-parse main)" = "$("$real_git" -C "$real_repo" rev-parse HEAD)" ] || fail 'a workflow lane did not land in the gap'
printf '[landing]\n\tqueue = lock\n' >"$real_repo/.safe-push"
"$real_git" -C "$real_repo" add .safe-push
"$real_git" -C "$real_repo" commit -qm 'land under the lock beside an Autoland workflow'
"$real_git" -C "$real_repo" push -q origin HEAD:main
printf 'locked work\n' >>"$real_repo/content.txt"
"$real_git" -C "$real_repo" commit -qam 'work for the lock'
run_real_case '' --to main
expect_status zero
! grep -Fq "lands through Autoland" <<<"$case_output" || fail "landing.queue = lock still queued for Autoland: $case_output"
[ "$("$real_git" --git-dir="$real_remote" rev-parse main)" = "$("$real_git" -C "$real_repo" rev-parse HEAD)" ] || fail 'landing.queue = lock did not push main'
printf 'PASS: landing.queue = lock lands under the landing lock although main has an Autoland workflow\n'
rm -f "$real_bin/gh"

make_real_fixture dep-gate
printf '{"dependencies":{"unowned-package":"1.0.0"}}\n' >"$real_repo/package.json"
"$real_git" -C "$real_repo" add package.json
"$real_git" -C "$real_repo" commit -qm 'add dependency without reason'
remote_before="$(real_remote_state)"
run_real_case ''
expect_status nonzero
[ ! -s "$real_trace" ] || fail 'dependency refusal reached the scan'
[ "$(real_remote_state)" = "$remote_before" ] || fail 'dependency refusal changed origin'

code_health_stub="$scratch/code-health-stub"
printf '#!/usr/bin/env bash\nprintf "code-health %%s\\n" "$*" >>"$CODE_HEALTH_TEST_TRACE"\nexit "${CODE_HEALTH_TEST_EXIT:-0}"\n' >"$code_health_stub"
chmod +x "$code_health_stub"
code_health_case() {
  local exit_code="$1"
  shift
  : >"$scratch/code-health-trace"
  SAFE_PUSH_CODE_HEALTH="$code_health_stub" CODE_HEALTH_TEST_EXIT="$exit_code" \
    CODE_HEALTH_TEST_TRACE="$scratch/code-health-trace" run_real_case '' "$@"
}
make_real_fixture code-health-pass
code_health_case 0 --dry-run --to main
expect_status zero
grep -Fxq "code-health gate $("$real_git" --git-dir="$real_remote" rev-parse main) $("$real_git" -C "$real_repo" rev-parse HEAD)" "$scratch/code-health-trace" \
  || fail 'code-health gate was not called with the destination and head'
make_real_fixture code-health-behind
behind_base="$("$real_git" -C "$real_repo" rev-parse HEAD)"
printf 'main moved\n' >"$real_repo/main-only.txt"
"$real_git" -C "$real_repo" add main-only.txt
"$real_git" -C "$real_repo" commit -qm 'main moves on'
main_moved="$("$real_git" -C "$real_repo" rev-parse HEAD)"
"$real_git" -C "$real_repo" push -q origin HEAD:main
"$real_git" -C "$real_repo" reset -q --hard "$behind_base"
printf 'lane\n' >"$real_repo/lane-only.txt"
"$real_git" -C "$real_repo" add lane-only.txt
"$real_git" -C "$real_repo" commit -qm 'lane work behind main'
behind_head="$("$real_git" -C "$real_repo" rev-parse HEAD)"
code_health_case 0 --to main
expect_status zero
! grep -Fxq "code-health gate $main_moved $behind_head" "$scratch/code-health-trace" \
  || fail 'code-health gate charged a lane behind main with the main commits it lacks'
grep -Fxq "code-health gate $behind_base $behind_head" "$scratch/code-health-trace" \
  || fail 'code-health gate did not measure a lane behind main from its merge base'
make_real_fixture code-health-debt
remote_before="$(real_remote_state)"
code_health_case 1 --to main
expect_status nonzero
expect_output 'safe-push: ✗ code-health gate refused — NOT pushed.'
[ ! -s "$real_trace" ] || fail 'code-health refusal reached the scan'
[ "$(real_remote_state)" = "$remote_before" ] || fail 'code-health refusal changed origin'
make_real_fixture code-health-before-land-checks
"$real_git" -C "$real_repo" config --file .safe-push --add land.check 'file.txt :: printf "ran\\n" >> land-check-runs'
"$real_git" -C "$real_repo" add .safe-push
"$real_git" -C "$real_repo" commit -qm 'land check'
printf 'changed\n' >"$real_repo/file.txt"
"$real_git" -C "$real_repo" add file.txt
"$real_git" -C "$real_repo" commit -qm 'changed input'
code_health_case 1 --to main
expect_status nonzero
expect_output 'safe-push: ✗ code-health gate refused — NOT pushed.'
[ ! -e "$real_repo/land-check-runs" ] || fail 'a code-health refusal ran the land checks first'
printf 'PASS: a code-health refusal stops the landing before its land checks run\n'
make_real_fixture code-health-error-unenrolled
code_health_case 2 --dry-run --to main
expect_status zero
expect_output 'no structure.toml — continuing'
make_real_fixture code-health-error-enrolled
printf '[thresholds]\n' >"$real_repo/structure.toml"
"$real_git" -C "$real_repo" add structure.toml
"$real_git" -C "$real_repo" commit -qm 'enroll in code-health'
remote_before="$(real_remote_state)"
code_health_case 2 --to main
expect_status nonzero
expect_output 'code-health gate failed (exit 2) — fix the tool; NOT pushed.'
[ "$(real_remote_state)" = "$remote_before" ] || fail 'code-health tool error changed origin'
SAFE_PUSH_CODE_HEALTH="$scratch/absent-code-health" run_real_case '' --to main
expect_status nonzero
expect_output 'code-health missing at'
[ "$(real_remote_state)" = "$remote_before" ] || fail 'missing code-health changed origin'

# Dry-run is not an escape hatch: a non-main branch with no --to is refused even
# under --dry-run, leaving the remote and local tracking configuration untouched.
make_real_fixture dry-run-destination
"$real_git" -C "$real_repo" switch -q -c feature
"$real_git" -C "$real_repo" commit --allow-empty -qm feature
remote_before="$(real_remote_state)"
run_real_case '' --dry-run
expect_status nonzero
[ "$(real_remote_state)" = "$remote_before" ] \
  || fail 'dry-run mutated the remote'
if "$real_git" -C "$real_repo" rev-parse --verify '@{upstream}' >/dev/null 2>&1; then
  fail 'dry-run established local tracking'
fi

# Cross-branch publication is available only through --to and scans the exact
# remote-destination..HEAD range before an object+lease-bound update.
make_real_fixture explicit-main
remote_main_before="$("$real_git" --git-dir="$real_remote" rev-parse refs/heads/main)"
"$real_git" -C "$real_repo" switch -q -c feature
"$real_git" -C "$real_repo" commit --allow-empty -qm feature
feature_oid="$("$real_git" -C "$real_repo" rev-parse feature)"
run_real_case '' --to main
expect_status zero
[ "$("$real_git" --git-dir="$real_remote" rev-parse refs/heads/main)" = "$feature_oid" ] \
  || fail '--to main did not advance the explicit destination'
if "$real_git" -C "$real_repo" rev-parse --verify '@{upstream}' >/dev/null 2>&1; then
  fail '--to unexpectedly changed local branch tracking'
fi

# A new destination branch scans only what origin's default branch lacks, not
# the whole reachable history.
make_real_fixture new-destination
remote_main_before="$("$real_git" --git-dir="$real_remote" rev-parse refs/heads/main)"
"$real_git" -C "$real_repo" switch -q -c scratch
"$real_git" -C "$real_repo" commit --allow-empty -qm scratch
scratch_oid="$("$real_git" -C "$real_repo" rev-parse scratch)"
run_real_case '' --to farm/scan-test
expect_status zero
[ "$("$real_git" --git-dir="$real_remote" rev-parse refs/heads/farm/scan-test)" = "$scratch_oid" ] \
  || fail 'new destination branch was not created at HEAD'

# Destination ancestry is checked before scanning or pushing. An unrelated
# remote main is rejected even when --to makes the target explicit.
make_real_fixture non-fast-forward
"$real_git" -C "$real_repo" fetch -q origin race-target:refs/remotes/origin/race-target
"$real_git" --git-dir="$real_remote" update-ref refs/heads/main "$real_destination_oid"
"$real_git" -C "$real_repo" switch -q -c feature "$real_base_oid"
"$real_git" -C "$real_repo" commit --allow-empty -qm feature
remote_before="$(real_remote_state)"
run_real_case '' --to main
expect_status nonzero
[ "$(real_remote_state)" = "$remote_before" ] \
  || fail 'non-fast-forward refusal mutated the remote'
[ ! -s "$real_trace" ] || fail 'non-fast-forward refusal reached gitleaks'

# A branch ref that resolves symbolically on the remote is not a concrete
# destination and must be rejected before publication.
make_real_fixture symbolic-destination
"$real_git" --git-dir="$real_remote" symbolic-ref refs/heads/alias refs/heads/main
remote_before="$(real_remote_state)"
run_real_case '' --to alias
expect_status nonzero
[ "$(real_remote_state)" = "$remote_before" ] \
  || fail 'symbolic destination refusal mutated the remote'

# A branch landing to a public destination runs the deny-list gate with real
# gitleaks before it pushes; the scanner and stage cases live in
# crates/north-landing/src/landing/denylist_publication_tests.rs and safe_push_guards_tests.rs.
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
real_gitleaks="$(command -v gitleaks)"
gh_bin="$scratch/gh-bin"
mkdir -p "$gh_bin"
ln -s "$(readlink -f "${BASH_SOURCE[0]}")" "$gh_bin/gh"
run_denylist_case() {
  PATH="$gh_bin:$PATH" SAFE_PUSH_TEST_VISIBILITY="$1" \
    SAFE_PUSH_TEST_REAL_GITLEAKS="$real_gitleaks" run_real_case '' "${@:2}"
}

make_real_fixture denylist-public-finding
printf 'owner: zebra-quokka-7731\n' >"$real_repo/identity.txt"
"$real_git" -C "$real_repo" add identity.txt
"$real_git" -C "$real_repo" commit -qm 'add identity'
remote_before="$(real_remote_state)"
SAFE_PUSH_DENYLIST="$fixture_denylist" run_denylist_case PUBLIC
expect_status nonzero
expect_output 'fake-private-term'
expect_output 'identity.txt'
if grep -Fq 'zebra-quokka-7731' <<<"$case_output"; then fail 'deny-list refusal printed the matched value'; fi
[ "$(real_remote_state)" = "$remote_before" ] || fail 'deny-list refusal mutated the remote'

# A claude/ push to a public Autoland repository asks north to attest its tip
# from south; a main push does not, a failed attest leaves the push standing,
# and a refusal says the tip needs a reworded push rather than a retry.
make_real_fixture denylist-attest
mkdir -p "$real_repo/.github/workflows" "$scratch/attest-south"
printf 'on: [workflow_dispatch]\n' >"$real_repo/.github/workflows/autoland.yml"
"$real_git" -C "$real_repo" add .github/workflows/autoland.yml
"$real_git" -C "$real_repo" commit -qm 'Autoland workflow'
"$real_git" -C "$real_repo" push -q origin HEAD:main
attest_trace="$scratch/attest-trace"
run_attest_case() {
  : >"$attest_trace"
  printf 'clean\n' >>"$real_repo/content.txt"
  "$real_git" -C "$real_repo" commit -qam 'attested work'
  SAFE_PUSH_SOUTH="$scratch/attest-south" SAFE_PUSH_DENYLIST="$fixture_denylist" \
    SAFE_PUSH_TEST_ATTEST_TRACE="$attest_trace" run_denylist_case PUBLIC "$@"
}
NORTH_FLEET_SESSION=attest-scratch run_attest_case --to claude/attest-work
expect_status zero
grep -Fxq "denylist attest --south $scratch/attest-south --repo real-denylist-attest/remote --branch claude/attest-work" "$attest_trace" \
  || fail "a claude/ push did not attest its tip: $(cat "$attest_trace")"
expect_output "safe-push: pushed $("$real_git" --git-dir="$real_remote" rev-parse refs/heads/claude/attest-work) to claude/attest-work"
! grep -q '^safe-push: landed ' <<<"$case_output" || fail 'a push to a scratch branch was reported as landed'
scratch_row=$(awk -F'\t' '$6 == "attest-scratch"' "$xdg_state/safe-push/pushes.tsv" | tail -n 1)
[ -n "$scratch_row" ] || fail 'a scratch-branch push recorded no push row'
[ -z "$(cut -f5 <<<"$scratch_row")" ] || fail "a scratch-branch push recorded a landed sha for fleet: $scratch_row"
SAFE_PUSH_TEST_ATTEST_STATUS=1 run_attest_case --to claude/attest-refused
expect_status zero
expect_output 'the deny-list attester refused claude/attest-refused, so Autoland will not test this tip; reword the deny-listed text and push again'
! grep -q 'timer retries' <<<"$case_output" || fail 'a refused attest promised a timer retry'
SAFE_PUSH_TEST_ATTEST_STATUS=3 run_attest_case --to claude/attest-deferred
expect_status zero
expect_output 'the nexus denylist-attest timer posts the status and dispatches Autoland within about five minutes, with nothing for you to run'
! grep -q 'failed (exit' <<<"$case_output" || fail 'a deferred attest was reported as a failure'
"$real_git" --git-dir="$real_remote" rev-parse -q --verify refs/heads/claude/attest-deferred >/dev/null || fail 'a deferred attest undid the push'
SAFE_PUSH_TEST_ATTEST_STATUS=2 run_attest_case --to claude/attest-retry
expect_status zero
expect_output 'the deny-list attest for claude/attest-retry failed (exit 2); the push stands'
"$real_git" --git-dir="$real_remote" rev-parse -q --verify refs/heads/claude/attest-retry >/dev/null || fail 'a failed attest undid the push'
printf 'reviewed\n' >>"$real_repo/content.txt"
"$real_git" -C "$real_repo" commit -qam 'reviewed work' -m "Reviewed: approves; reviewer: fixture-reviewer; patch-id $(printf '0%.0s' $(seq 40))"
NORTH_FLEET_SESSION=attest-main run_attest_case --to main --direct
expect_status zero
main_row=$(awk -F'\t' '$6 == "attest-main"' "$xdg_state/safe-push/pushes.tsv" | tail -n 1)
[ "$(cut -f5 <<<"$main_row")" = "$("$real_git" --git-dir="$real_remote" rev-parse refs/heads/main)" ] || fail "a main push did not record its landed sha: $main_row"
[ "$(cut -f7 <<<"$main_row")" = fixture-reviewer ] || fail "a main push did not record its reviewer: $main_row"
[ ! -s "$attest_trace" ] || fail "a main push attested: $(cat "$attest_trace")"
: >"$attest_trace"
printf 'clean\n' >>"$real_repo/content.txt"
"$real_git" -C "$real_repo" commit -qam 'unattested work'
SAFE_PUSH_DENYLIST="$fixture_denylist" SAFE_PUSH_TEST_ATTEST_TRACE="$attest_trace" run_denylist_case PUBLIC --to claude/attest-no-south
expect_status zero
[ ! -s "$attest_trace" ] || fail "a push without a south checkout attested: $(cat "$attest_trace")"
printf 'PASS: claude/ pushes to a public Autoland repository attest their tip\n'

# A branch landing runs the email scan before it pushes; its cases live in
# crates/north-landing/src/landing/safe_push_guards_tests.rs.
private_email=someone@private.example
expect_email_refused() {
  local role="$1" sha
  sha=$("$real_git" -C "$real_repo" rev-parse --short HEAD)
  expect_status nonzero
  expect_output "safe-push: commit $sha $role email $private_email is not safe-push-test@example.invalid"
  expect_output 'git commit --amend --reset-author (or rebase --exec) before pushing; NOT pushed.'
  [ "$(real_remote_state)" = "$remote_before" ] || fail "private $role email refusal mutated the remote"
}

make_real_fixture email-to-main
GIT_AUTHOR_EMAIL="$private_email" "$real_git" -C "$real_repo" commit --allow-empty -qm 'private author for main'
remote_before="$(real_remote_state)"
run_real_case '' --to main
expect_email_refused author

printf 'PASS: a branch landing refuses a private author email before push\n'

# Each race is planted by the gitleaks process after the scan begins. The exact
# real remote ref set must remain unchanged; a mocked "push was not called" is
# not sufficient evidence for this security boundary.
for race in branch commit destination worktree; do
  make_real_fixture "race-$race"
  remote_before="$(real_remote_state)"
  run_real_case "$race"
  expect_status nonzero
  remote_after="$(real_remote_state)"
  if [ "$race" = destination ]; then
    [ "$("$real_git" --git-dir="$real_remote" rev-parse refs/heads/main)" = "$real_destination_oid" ] \
      || fail 'destination race was not preserved as the sole remote mutation'
    [[ "$case_output" != *'pushing main -> origin'* ]] \
      || fail 'safe-push published after the destination changed during its scan'
  else
    [ "$remote_after" = "$remote_before" ] \
      || fail "$race race mutated the remote despite fail-closed verdict"
  fi
done
} >"$scratch/section-2.log" 2>&1 &
sections+=($!)

{ xdg_state="$scratch/xdg-state-3"
new_real_bin -landing

wait_for() {
  local producers=$1 producer live
  shift
  until "$@"; do
    live=''
    for producer in $producers; do kill -0 "$producer" 2>/dev/null && live=1; done
    [ -n "$live" ] || "$@" || fail "handshake never came and its producer exited: $*"
    sleep 0.01
  done
}

queued_lane() {
  local key
  key=$(printf '%s' "$landing_remote" | sha256sum | cut -c1-16)
  grep -Fqs -- "$1" "$xdg_state/safe-push/$key.queue/"* 2>/dev/null
}

hold_landing_lock() {
  local key
  key=$(printf '%s' "$landing_remote" | sha256sum | cut -c1-16)
  mkdir -p "$xdg_state/safe-push"
  mkfifo "$landing_root/lock-release"
  flock "$xdg_state/safe-push/$key.lock" bash -c \
    'touch "$1/lock-ready"; read -r _ <"$1/lock-release"' _ "$landing_root" & blocker=$!
  wait_for "$blocker" test -e "$landing_root/lock-ready"
}

release_landing_lock() {
  printf 'release\n' >"$landing_root/lock-release"
  wait "$blocker"
}

release_hook() {
  printf 'release\n' >"$1.hook-release"
}

finish_first_lane() {
  wait_for "$pid_a" test -s "$landing_root/a.hook-runs"
  wait_for "$pid_b" test -s "$landing_root/b.hook-runs"
  release_hook "$landing_root/a"
  status_a=0; wait "$pid_a" || status_a=$?
  release_hook "$landing_root/b"
  status_b=0; wait "$pid_b" || status_b=$?
}

make_landing_clone() {
  local dir="$1" file="$2"
  "$real_git" clone -q "$landing_remote" "$dir"
  "$real_git" -C "$dir" config user.name safe-push-test
  "$real_git" -C "$dir" config user.email safe-push-test@example.invalid
  "$real_git" -C "$dir" switch -q -c "lane-${dir##*/}"
  printf '%s\n' "${dir##*/}" >"$dir/$file"
  "$real_git" -C "$dir" add "$file"
  "$real_git" -C "$dir" commit -qm "change from ${dir##*/}"
}

run_landing() {
  printf "%s\n" run_landing >>"$scratch/cases"
  local dir="$1"
  (
    cd "$dir"
    SAFE_PUSH_TEST_SHIM=1 \
      SAFE_PUSH_TEST_TRACE="$dir.trace" \
      SAFE_PUSH_TEST_STATE="$dir.state" \
      SAFE_PUSH_TEST_SCAN_GATE="${landing_gate:+$dir}" \
      XDG_STATE_HOME="${landing_state:-${xdg_state:?}}" \
      PATH="${SAFE_PUSH_TEST_PATH:-$real_bin:$PATH}" \
      env --default-signal=INT "$TARGET" --to "${landing_to:-main}"
  ) >"$dir.out" 2>&1
}

new_landing_origin() {
  landing_root="$scratch/landing-$1"
  landing_remote="$landing_root/remote.git"
  if [ -d "$scratch/landing-template" ]; then
    cp -a "$scratch/landing-template" "$landing_root"
    return
  fi
  mkdir -p "$landing_root"
  "$real_git" init --bare -q -b main "$landing_remote"
  "$real_git" init -q -b main "$landing_root/seed"
  "$real_git" -C "$landing_root/seed" config user.name safe-push-test
  "$real_git" -C "$landing_root/seed" config user.email safe-push-test@example.invalid
  printf 'base\n' >"$landing_root/seed/shared.txt"
  "$real_git" -C "$landing_root/seed" add shared.txt
  "$real_git" -C "$landing_root/seed" commit -qm base
  "$real_git" -C "$landing_root/seed" push -q "$landing_remote" main
  cp -a "$landing_root" "$scratch/landing-template"
}

new_landing_origin deferred-capacity
landing_state="$landing_root/state"
printf '1\n2\n3\n4\n5\n' >"$landing_root/seed/shared.txt"
"$real_git" -C "$landing_root/seed" commit -qam 'five shared lines'
"$real_git" -C "$landing_root/seed" push -q "$landing_remote" main
make_landing_clone "$landing_root/a" a.txt
sed -i '1s/.*/a/' "$landing_root/a/shared.txt"
"$real_git" -C "$landing_root/a" commit -qam 'a edits the first shared line'
make_landing_clone "$landing_root/b" b.txt
sed -i '5s/.*/b/' "$landing_root/b/shared.txt"
"$real_git" -C "$landing_root/b" commit -qam 'b edits the last shared line'
mkfifo "$landing_root/admission-release" "$landing_root/recheck-release"
land_command="printf 'DEFER_NATIVE_QUIET\\n'; if [ -e '$landing_root/deferred' ]; then touch '$landing_root/recheck'; read -r _ <'$landing_root/recheck-release'; else touch '$landing_root/deferred'; read -r _ <'$landing_root/admission-release'; fi"
"$real_git" -C "$landing_root/a" config --file .safe-push --add land.check "a.txt :: $land_command"
"$real_git" -C "$landing_root/a" add .safe-push
"$real_git" -C "$landing_root/a" commit -qm 'record deferred admission'
run_landing "$landing_root/a" & pid_a=$!
wait_for "$pid_a" test -e "$landing_root/deferred"
lock_key=$(printf '%s' "$landing_remote" | sha256sum | cut -c1-16)
if ! flock -n "$landing_state/safe-push/$lock_key.lock" true; then
  printf 'admitted\n' >"$landing_root/admission-release"
  wait "$pid_a" || true
  fail 'deferred admission holds the landing lock'
fi
! queued_lane "$landing_root/a" || fail 'deferred admission retained a queue ticket'
run_landing "$landing_root/b" & pid_b=$!
status_b=0; wait "$pid_b" || status_b=$?
[ "$status_b" -eq 0 ] || fail "ready lane failed: $(cat "$landing_root/b.out")"
"$real_git" --git-dir="$landing_remote" cat-file -e main:b.txt || fail 'ready lane did not land first'
! "$real_git" --git-dir="$landing_remote" cat-file -e main:a.txt 2>/dev/null || fail 'deferred lane landed before admission'
printf 'admitted\n' >"$landing_root/admission-release"
wait_for "$pid_a" test -e "$landing_root/recheck"
printf 'admitted\n' >"$landing_root/recheck-release"
status_a=0; wait "$pid_a" || status_a=$?
[ "$status_a" -eq 0 ] || fail "deferred lane failed after requeue: $(cat "$landing_root/a.out")"
landing_state=''
printf 'PASS: recorded deferred admission holds no lock or ticket; ready lane lands first; deferred lane requeues\n'

new_landing_origin admitted-check-unlocked
make_landing_clone "$landing_root/a" a.txt
mkfifo "$landing_root/check-release"
land_command="printf 'DEFER_NATIVE_QUIET\\n'; printf 'run\\n' >>'$landing_root/runs'; read -r _ <'$landing_root/check-release'"
"$real_git" -C "$landing_root/a" config --file .safe-push landing.capacity env
"$real_git" -C "$landing_root/a" config --file .safe-push --add land.check "a.txt :: $land_command"
"$real_git" -C "$landing_root/a" add .safe-push
"$real_git" -C "$landing_root/a" commit -qm 'admitted land check'
run_landing "$landing_root/a" & pid_a=$!
wait_for "$pid_a" test -s "$landing_root/runs"
lock_key=$(printf '%s' "$landing_remote" | sha256sum | cut -c1-16)
if ! flock -n "${landing_state:-$xdg_state}/safe-push/$lock_key.lock" true; then
  printf 'go\n' >"$landing_root/check-release"
  wait "$pid_a" || true
  fail 'an admitted land check held the landing lock'
fi
printf 'other\n' >"$landing_root/seed/other.txt"
"$real_git" -C "$landing_root/seed" add other.txt
"$real_git" -C "$landing_root/seed" commit -qm 'disjoint main move'
"$real_git" -C "$landing_root/seed" push -q "$landing_remote" main
printf 'go\n' >"$landing_root/check-release"
status_a=0; wait "$pid_a" || status_a=$?
[ "$status_a" -eq 0 ] || fail "admitted lane failed: $(cat "$landing_root/a.out")"
[ "$(wc -l <"$landing_root/runs")" -eq 1 ] || fail 'a main move touching none of the lane files re-ran its land checks'
"$real_git" --git-dir="$landing_remote" cat-file -e main:a.txt || fail 'admitted lane did not land'
"$real_git" --git-dir="$landing_remote" cat-file -e main:other.txt || fail 'admitted lane dropped the disjoint main commit'
grep -q "keeping the land checks' verdict" "$landing_root/a.out" || fail "missing kept-verdict receipt: $(cat "$landing_root/a.out")"
printf 'PASS: recorded admitted land check runs unlocked; a disjoint main move keeps its verdict\n'

for second_move in disjoint touching; do
  new_landing_origin kept-verdict-second-move-$second_move
  printf '1\n2\n3\n4\n5\n' >"$landing_root/seed/shared.txt"
  "$real_git" -C "$landing_root/seed" commit -qam 'five shared lines'
  "$real_git" -C "$landing_root/seed" push -q "$landing_remote" main
  make_landing_clone "$landing_root/a" a.txt
  sed -i '1s/.*/a/' "$landing_root/a/shared.txt"
  "$real_git" -C "$landing_root/a" commit -qam 'a edits the first shared line'
  mkfifo "$landing_root/check-release"
  land_command="printf 'DEFER_NATIVE_QUIET\\n'; printf 'run\\n' >>'$landing_root/runs'; [ -e '$landing_root/second-move' ] || read -r _ <'$landing_root/check-release'"
  "$real_git" -C "$landing_root/a" config --file .safe-push landing.capacity env
  "$real_git" -C "$landing_root/a" config --file .safe-push --add land.check "a.txt,shared.txt :: $land_command"
  "$real_git" -C "$landing_root/a" add .safe-push
  "$real_git" -C "$landing_root/a" commit -qm 'admitted land check'
  if [ "$second_move" = disjoint ]; then second_edit="printf 'second\\n' >'$landing_root/seed/other2.txt'; '$real_git' -C '$landing_root/seed' add other2.txt"
  else second_edit="sed -i '5s/.*/s/' '$landing_root/seed/shared.txt'; '$real_git' -C '$landing_root/seed' add shared.txt"; fi
  cat >"$landing_root/a/.git/hooks/post-rewrite" <<HOOK
#!/bin/sh
[ ! -e '$landing_root/second-move' ] || exit 0
touch '$landing_root/second-move'
unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE
$second_edit
'$real_git' -C '$landing_root/seed' commit -qm 'second main move'
'$real_git' -C '$landing_root/seed' push -q '$landing_remote' main
HOOK
  chmod +x "$landing_root/a/.git/hooks/post-rewrite"
  run_landing "$landing_root/a" & pid_a=$!
  wait_for "$pid_a" test -s "$landing_root/runs"
  printf 'other\n' >"$landing_root/seed/other.txt"
  "$real_git" -C "$landing_root/seed" add other.txt
  "$real_git" -C "$landing_root/seed" commit -qm 'disjoint main move'
  "$real_git" -C "$landing_root/seed" push -q "$landing_remote" main
  printf 'go\n' >"$landing_root/check-release"
  status_a=0; wait "$pid_a" || status_a=$?
  [ "$status_a" -eq 0 ] || fail "lane failed after a second main move ($second_move): $(cat "$landing_root/a.out")"
  [ -e "$landing_root/second-move" ] || fail 'the second main move never happened'
  "$real_git" --git-dir="$landing_remote" cat-file -e main:a.txt || fail "lane did not land after a $second_move second main move"
  "$real_git" --git-dir="$landing_remote" cat-file -e main:other.txt || fail 'lane dropped the first main move'
  ! grep -q 'non-fast-forward' "$landing_root/a.out" || fail "second main move was refused: $(cat "$landing_root/a.out")"
  if [ "$second_move" = disjoint ]; then
    [ "$(wc -l <"$landing_root/runs")" -eq 1 ] || fail "a disjoint second main move re-ran the land checks: $(cat "$landing_root/a.out")"
    "$real_git" --git-dir="$landing_remote" cat-file -e main:other2.txt || fail 'lane dropped the second main move'
  else
    [ "$(wc -l <"$landing_root/runs")" -eq 2 ] || fail "a second main move touching a lane file did not rerun the land checks: $(cat "$landing_root/a.out")"
  fi
done
printf 'PASS: main moving again before the push keeps the verdict for a disjoint move and reruns the checks for a move touching the lane files\n'

new_landing_origin land-check-retry
printf '1\n2\n3\n4\n5\n' >"$landing_root/seed/shared.txt"
"$real_git" -C "$landing_root/seed" commit -qam 'five shared lines'
"$real_git" -C "$landing_root/seed" push -q "$landing_remote" main
make_landing_clone "$landing_root/a" a.txt
sed -i '1s/.*/a/' "$landing_root/a/shared.txt"
"$real_git" -C "$landing_root/a" commit -qam 'a edits the first shared line'
sed -i '5s/.*/s/' "$landing_root/seed/shared.txt"
"$real_git" -C "$landing_root/seed" commit -qam 'main edits the last shared line'
land_command="printf 'run\\n' >>'$landing_root/runs'; if [ -e '$landing_root/moved' ]; then ! grep -q '^north <land-clean>' '$landing_root/north-trace'; else touch '$landing_root/moved'; git -C '$landing_root/seed' push -q '$landing_remote' main; fi"
"$real_git" -C "$landing_root/a" config --file .safe-push landing.capacity env
"$real_git" -C "$landing_root/a" config --file .safe-push --add land.check "a.txt,shared.txt :: $land_command"
"$real_git" -C "$landing_root/a" add .safe-push
"$real_git" -C "$landing_root/a" commit -qm 'retried land check'
: >"$landing_root/north-trace"
SAFE_PUSH_TEST_LANGUAGE_TRACE="$landing_root/north-trace" run_landing "$landing_root/a" || fail "retried lane failed: $(cat "$landing_root/a.out")"
[ "$(wc -l <"$landing_root/runs")" -eq 2 ] || fail 'a main move touching a lane file did not rerun the land checks'
[ "$(grep -c '^north <land-clean>' "$landing_root/north-trace")" -eq 1 ] || fail 'the lane build output was not released exactly once, after the last land check'
printf 'PASS: a land check rerun keeps the lane build output until safe-push exits\n'

new_landing_origin main-red-marker
make_landing_clone "$landing_root/a" a.txt
make_landing_clone "$landing_root/b" b.txt
"$real_git" -C "$landing_root/b" commit --amend -qm 'repair red main' -m 'Main-red: 55'
hold_landing_lock
run_landing "$landing_root/a" & pid_a=$!
wait_for "$pid_a" bash -c 'compgen -G "$1/[1-9]*" >/dev/null' _ "$xdg_state/safe-push/$(printf '%s' "$landing_remote" | sha256sum | cut -c1-16).queue"
run_landing "$landing_root/b" & pid_b=$!
wait_for "$pid_b" bash -c 'compgen -G "$1/0*" >/dev/null' _ "$xdg_state/safe-push/$(printf '%s' "$landing_remote" | sha256sum | cut -c1-16).queue"
release_landing_lock
status_a=0; wait "$pid_a" || status_a=$?
status_b=0; wait "$pid_b" || status_b=$?
[ $((status_a + status_b)) -eq 0 ] || fail "main-red marker lane failed: $(cat "$landing_root/a.out" "$landing_root/b.out")"
[ "$("$real_git" --git-dir="$landing_remote" log --reverse --format=%s main | sed -n '2p')" = 'repair red main' ] || fail 'main-red marker did not take priority'
printf 'PASS: recorded Main-red marker takes the landing lock before an ordinary waiting lane\n'

new_landing_origin cancelled-admission
make_landing_clone "$landing_root/a" a.txt
mkfifo "$landing_root/cancel-wait"
"$real_git" -C "$landing_root/a" config --file .safe-push landing.capacity "bash -c 'printf \"%s\\n\" \"\$PPID\" >\"$landing_root/cancel-pid\"; read -r _ <\"$landing_root/cancel-wait\"'"
"$real_git" -C "$landing_root/a" add .safe-push
"$real_git" -C "$landing_root/a" commit -qm 'cancel during admission'
run_landing "$landing_root/a" & pid_a=$!
wait_for "$pid_a" test -s "$landing_root/cancel-pid"
kill -INT "$(cat "$landing_root/cancel-pid")"
status_a=0; wait "$pid_a" || status_a=$?
[ "$status_a" -eq 130 ] || fail "cancelled admission returned $status_a: $(cat "$landing_root/a.out")"
grep -qx 'safe-push: cancelled' "$landing_root/a.out" || fail 'missing cancelled receipt'
! grep -q '^safe-push: landed ' "$landing_root/a.out" || fail 'cancelled admission falsely reported the base as landed'
printf 'PASS: recorded SIGINT cancels with exit 130 and no false landed receipt\n'

new_landing_origin dead-check-orphans
make_landing_clone "$landing_root/a" a.txt
"$real_git" -C "$landing_root/a" config --file .safe-push --add land.check "a.txt :: sleep 40 & echo \$! >'$landing_root/orphan-pid'; exit 7"
"$real_git" -C "$landing_root/a" add .safe-push
"$real_git" -C "$landing_root/a" commit -qm 'check leaves an orphan holding its output'
run_landing "$landing_root/a" & pid_a=$!
wait_for "$pid_a" test -s "$landing_root/orphan-pid"
finished=''
for i in $(seq 150); do
  if ! kill -0 "$pid_a" 2>/dev/null; then finished="$i"; break; fi
  sleep 0.1
done
kill "$(cat "$landing_root/orphan-pid")" 2>/dev/null || true
status_a=0; wait "$pid_a" || status_a=$?
[ -n "$finished" ] || fail 'a dead land check left safe-push waiting on its orphaned log children'
[ "$status_a" -ne 0 ] || fail 'a failed land check let the push succeed'
grep -q 'exited 7' "$landing_root/a.out" || fail "missing land check failure: $(cat "$landing_root/a.out")"
lock_key=$(printf '%s' "$landing_remote" | sha256sum | cut -c1-16)
flock -n "${landing_state:-$xdg_state}/safe-push/$lock_key.lock" true || fail 'a dead land check left the landing lock held'
printf 'PASS: recorded dead land check with orphaned children releases safe-push and the lock\n'

new_landing_origin hung-check-timeout
make_landing_clone "$landing_root/a" a.txt
"$real_git" -C "$landing_root/a" config --file .safe-push --add land.check "a.txt :: sleep 40"
"$real_git" -C "$landing_root/a" add .safe-push
"$real_git" -C "$landing_root/a" commit -qm 'check hangs alive'
SAFE_PUSH_LAND_CHECK_TIMEOUT=2 run_landing "$landing_root/a" & pid_a=$!
finished=''
for i in $(seq 200); do
  if ! kill -0 "$pid_a" 2>/dev/null; then finished="$i"; break; fi
  sleep 0.1
done
[ -n "$finished" ] || { kill "$pid_a" 2>/dev/null || true; fail 'a hung land check held the landing queue past its timeout'; }
status_a=0; wait "$pid_a" || status_a=$?
[ "$status_a" -ne 0 ] || fail 'a timed out land check let the push succeed'
grep -q 'exited 124' "$landing_root/a.out" || fail "missing land check timeout: $(cat "$landing_root/a.out")"
lock_key=$(printf '%s' "$landing_remote" | sha256sum | cut -c1-16)
flock -n "${landing_state:-$xdg_state}/safe-push/$lock_key.lock" true || fail 'a timed out land check left the landing lock held'
printf 'PASS: recorded hung land check times out, fails the push and frees the lock\n'

new_landing_origin frozen-check-timeout
make_landing_clone "$landing_root/a" a.txt
"$real_git" -C "$landing_root/a" config --file .safe-push --add land.check "a.txt :: touch $landing_root/started; sleep 40"
"$real_git" -C "$landing_root/a" add .safe-push
"$real_git" -C "$landing_root/a" commit -qm 'check is frozen past its timeout'
lease_runtime="$landing_root/runtime"
mkdir -p "$lease_runtime/agent-capacity-v1/leases"
printf '{"expiresAt":1000,"frozenAt":1000}\n' >"$lease_runtime/agent-capacity-v1/leases/frozen.json"
XDG_RUNTIME_DIR="$lease_runtime" AGENT_CAPACITY_LEASE=frozen SAFE_PUSH_LAND_CHECK_TIMEOUT=2 run_landing "$landing_root/a" & pid_a=$!
for i in $(seq 200); do [ -e "$landing_root/started" ] && break; sleep 0.05; done
[ -e "$landing_root/started" ] || { kill "$pid_a" 2>/dev/null || true; fail 'the frozen land check never started'; }
sleep 4
kill -0 "$pid_a" 2>/dev/null || fail "a frozen land check was ended by its wall-clock timeout: $(cat "$landing_root/a.out")"
printf '{"expiresAt":5000}\n' >"$lease_runtime/agent-capacity-v1/leases/frozen.json"
sleep 1.6
kill -0 "$pid_a" 2>/dev/null || fail "a frozen land check ended before its timeout plus the time it was frozen: $(cat "$landing_root/a.out")"
finished=''
for i in $(seq 30); do
  if ! kill -0 "$pid_a" 2>/dev/null; then finished="$i"; break; fi
  sleep 0.1
done
[ -n "$finished" ] || { kill "$pid_a" 2>/dev/null || true; fail 'a thawed land check outlived its timeout plus the time it was frozen'; }
status_a=0; wait "$pid_a" || status_a=$?
[ "$status_a" -ne 0 ] || fail 'a timed out frozen land check let the push succeed'
grep -q 'exited 124' "$landing_root/a.out" || fail "missing frozen land check timeout: $(cat "$landing_root/a.out")"
printf 'PASS: recorded frozen land check times out only after its timeout plus the time it was frozen\n'

new_landing_origin push-without-movement
make_landing_clone "$landing_root/a" a.txt
printf '#!/bin/sh\nwhile read -r old new ref; do git update-ref "$ref" "$old"; done\n' >"$landing_remote/hooks/post-receive"
chmod +x "$landing_remote/hooks/post-receive"
main_before=$("$real_git" --git-dir="$landing_remote" rev-parse main)
status_a=0; run_landing "$landing_root/a" || status_a=$?
[ "$status_a" -ne 0 ] || fail "a push that left main unmoved exited 0: $(cat "$landing_root/a.out")"
[ "$("$real_git" --git-dir="$landing_remote" rev-parse main)" = "$main_before" ] || fail 'the fake push moved main'
grep -q 'NOT landed: 1 lane commits missing from origin/main' "$landing_root/a.out" || fail "missing NOT landed refusal: $(cat "$landing_root/a.out")"
! grep -q '^safe-push: landed ' "$landing_root/a.out" || fail 'an unmoved main was reported as landed'
printf 'PASS: recorded push that leaves main unmoved is refused as NOT landed\n'

new_landing_origin relanded-lane-rewritten-main
printf '[ci]\n\tbudget = farm-test.yml 240\n' >"$landing_root/seed/.safe-push"
"$real_git" -C "$landing_root/seed" add .safe-push
"$real_git" -C "$landing_root/seed" commit -qm 'main CI budget'
"$real_git" -C "$landing_root/seed" push -q "$landing_remote" main
make_landing_clone "$landing_root/a" a.txt
printf 'second\n' >"$landing_root/a/b.txt"
"$real_git" -C "$landing_root/a" add b.txt
"$real_git" -C "$landing_root/a" commit -qm 'second change from a'
"$real_git" -C "$landing_root/a" push -q origin HEAD:main
first_lane_commit=$("$real_git" -C "$landing_root/a" rev-parse HEAD~1)
cp "$landing_root/a/a.txt" "$landing_root/a/b.txt" "$landing_root/seed/"
"$real_git" -C "$landing_root/seed" add a.txt b.txt
"$real_git" -C "$landing_root/seed" commit -qm 'squash of the lane'
"$real_git" -C "$landing_root/seed" push -q -f "$landing_remote" main
main_before=$("$real_git" --git-dir="$landing_remote" rev-parse main)
status_a=0; run_landing "$landing_root/a" || status_a=$?
[ "$status_a" -ne 0 ] || fail "a relanded lane whose commits the rebase dropped exited 0: $(cat "$landing_root/a.out")"
! grep -q "ambiguous argument ''" "$landing_root/a.out" || fail "the empty lane range reached git as '': $(cat "$landing_root/a.out")"
grep -q "lane commits missing from .*: .*$first_lane_commit.*; NOT pushed\." "$landing_root/a.out" || fail "missing dropped-commit refusal: $(cat "$landing_root/a.out")"
! grep -q '^safe-push: landed ' "$landing_root/a.out" || fail 'a lane whose commits the rebase dropped was reported as landed'
[ "$("$real_git" --git-dir="$landing_remote" rev-parse main)" = "$main_before" ] || fail 'the refused relanding moved main'
printf 'PASS: recorded relanded lane over a rewritten main is refused, not landed\n'

new_landing_origin rebase-drops-commit
make_landing_clone "$landing_root/a" a.txt
printf 'second\n' >"$landing_root/a/b.txt"
"$real_git" -C "$landing_root/a" add b.txt
"$real_git" -C "$landing_root/a" commit -qm 'second change from a'
dropped_commit=$("$real_git" -C "$landing_root/a" rev-parse HEAD~1)
cp "$landing_root/a/a.txt" "$landing_root/seed/a.txt"
printf 'other\n' >"$landing_root/seed/other.txt"
"$real_git" -C "$landing_root/seed" add a.txt other.txt
"$real_git" -C "$landing_root/seed" commit -qm 'a.txt and more from elsewhere'
"$real_git" -C "$landing_root/seed" push -q "$landing_remote" main
main_before=$("$real_git" --git-dir="$landing_remote" rev-parse main)
status_a=0; run_landing "$landing_root/a" || status_a=$?
[ "$status_a" -ne 0 ] || fail "a lane whose rebase dropped a commit exited 0: $(cat "$landing_root/a.out")"
grep -q "lane commits missing from .*: .*$dropped_commit.*; NOT pushed\." "$landing_root/a.out" || fail "the dropped commit was not named: $(cat "$landing_root/a.out")"
! grep -q '^safe-push: landed ' "$landing_root/a.out" || fail 'a lane whose rebase dropped a commit was reported as landed'
[ "$("$real_git" --git-dir="$landing_remote" rev-parse main)" = "$main_before" ] || fail 'the refused lane moved main'
printf 'PASS: recorded rebase that drops a lane commit is refused naming it\n'

# Concurrent landings from lanes cut at the same base serialize on the
# per-origin lock; the later one rebases onto the earlier, so both land in
# order with no non-fast-forward rejection.
new_landing_origin concurrent
make_landing_clone "$landing_root/a" a.txt
make_landing_clone "$landing_root/b" b.txt
landing_gate=1
mkfifo "$landing_root/a.scan-release" "$landing_root/b.scan-release"
run_landing "$landing_root/a" & pid_a=$!
wait_for "$pid_a" test -d "$landing_root/a.scan-started"
run_landing "$landing_root/b" & pid_b=$!
wait_for "$pid_b" grep -Fq 'waiting for the landing lock' "$landing_root/b.out"
printf 'release\n' >"$landing_root/a.scan-release"
status_a=0; wait "$pid_a" || status_a=$?
wait_for "$pid_b" test -d "$landing_root/b.scan-started"
printf 'release\n' >"$landing_root/b.scan-release"
status_b=0; wait "$pid_b" || status_b=$?
landing_gate=''
case_output="$(cat "$landing_root/a.out" "$landing_root/b.out")"
case_status=$((status_a + status_b))
[ "$case_status" -eq 0 ] || fail 'concurrent landing: a run failed'
if grep -Eq 'non-fast-forward|rejected|state changed' <<<"$case_output"; then
  fail 'concurrent landing hit a non-fast-forward or race refusal'
fi
[ "$("$real_git" --git-dir="$landing_remote" rev-list --count main)" -eq 3 ] \
  || fail 'concurrent landing: origin main does not carry base plus both changes'
[ "$("$real_git" --git-dir="$landing_remote" rev-list --merges --count main)" -eq 0 ] \
  || fail 'concurrent landing: history is not linear'
for f in a.txt b.txt shared.txt; do
  "$real_git" --git-dir="$landing_remote" cat-file -e "main:$f" \
    || fail "concurrent landing: origin main lacks $f"
done

add_held_hook() {
  local dir="$1" verdict="${2:-0}"
  mkdir -p "$dir/.hooks"
  printf '#!/usr/bin/env bash\ncat >/dev/null\nprintf "%%s\\n" "$(git rev-parse HEAD)" >>"%s"\nif [ "$(wc -l <"%s")" -eq 1 ]; then read -r _ <"%s"; fi\nexit %s\n' \
    "$dir.hook-runs" "$dir.hook-runs" "$dir.hook-release" "$verdict" >"$dir/.hooks/pre-push"
  chmod +x "$dir/.hooks/pre-push"
  "$real_git" -C "$dir" config core.hooksPath .hooks
  : >"$dir.hook-runs"
  mkfifo "$dir.hook-release"
}


new_landing_origin hooked
make_landing_clone "$landing_root/a" a.txt
make_landing_clone "$landing_root/b" b.txt
add_held_hook "$landing_root/a"
add_held_hook "$landing_root/b"
run_landing "$landing_root/a" & pid_a=$!
run_landing "$landing_root/b" & pid_b=$!
wait_for "$pid_a" test -s "$landing_root/a.hook-runs"
wait_for "$pid_b" test -s "$landing_root/b.hook-runs"
lock_key=$(printf '%s' "$landing_remote" | sha256sum | cut -c1-16)
flock -n "$xdg_state/safe-push/$lock_key.lock" true || fail 'checks held the landing lock'
finish_first_lane
case_output="$(cat "$landing_root/a.out" "$landing_root/b.out")"
[ $((status_a + status_b)) -eq 0 ] || fail "hooked landing: a run failed: $case_output"
[ "$("$real_git" --git-dir="$landing_remote" rev-list --count main)" -eq 3 ] \
  || fail 'hooked landing: origin main does not carry base plus both changes'

# When origin changes the same file while the checks run, the later landing
# re-runs them on the rebased commit before pushing.
new_landing_origin hooked-overlap
printf 'top\nmiddle\nbottom\n' >"$landing_root/seed/shared.txt"
"$real_git" -C "$landing_root/seed" commit -qam 'three lines'
"$real_git" -C "$landing_root/seed" push -q "$landing_remote" main
for lane in a b; do
  "$real_git" clone -q "$landing_remote" "$landing_root/$lane"
  "$real_git" -C "$landing_root/$lane" config user.name safe-push-test
  "$real_git" -C "$landing_root/$lane" config user.email safe-push-test@example.invalid
  "$real_git" -C "$landing_root/$lane" switch -q -c "lane-$lane"
  add_held_hook "$landing_root/$lane"
done
sed -i 's/^top$/top from a/' "$landing_root/a/shared.txt"
sed -i 's/^bottom$/bottom from b/' "$landing_root/b/shared.txt"
"$real_git" -C "$landing_root/a" commit -qam 'a edits the top'
"$real_git" -C "$landing_root/b" commit -qam 'b edits the bottom'
run_landing "$landing_root/a" & pid_a=$!
wait_for "$pid_a" test -s "$landing_root/a.hook-runs"
run_landing "$landing_root/b" & pid_b=$!
finish_first_lane
case_output="$(cat "$landing_root/a.out" "$landing_root/b.out")"
[ $((status_a + status_b)) -eq 0 ] || fail "overlapping hooked landing: a run failed: $case_output"
[ "$("$real_git" --git-dir="$landing_remote" show main:shared.txt)" = $'top from a\nmiddle\nbottom from b' ] \
  || fail 'overlapping hooked landing: origin main lacks either change'

# A repository whose committed .safe-push sets landing.exact re-runs its
# checks on the exact rebased commit even when origin's new commits touch
# other files, including when the landing that moved origin introduced it.
new_landing_origin exact
make_landing_clone "$landing_root/a" a.txt
printf '[landing]\n\texact = true\n' >"$landing_root/a/.safe-push"
"$real_git" -C "$landing_root/a" add .safe-push
"$real_git" -C "$landing_root/a" commit -qm 'exact landings'
make_landing_clone "$landing_root/b" b.txt
add_held_hook "$landing_root/a"
add_held_hook "$landing_root/b"
run_landing "$landing_root/a" & pid_a=$!
wait_for "$pid_a" test -s "$landing_root/a.hook-runs"
run_landing "$landing_root/b" & pid_b=$!
finish_first_lane
case_output="$(cat "$landing_root/a.out" "$landing_root/b.out")"
[ $((status_a + status_b)) -eq 0 ] || fail "exact landing: a run failed: $case_output"
[[ "$case_output" != *'keeping the checks'* ]] || fail 'exact landing kept a verdict from another base'
[ "$(tail -n1 "$landing_root/b.hook-runs")" = "$("$real_git" --git-dir="$landing_remote" rev-parse main)" ] \
  || fail 'exact landing: the hook did not check the commit that landed'

# When both lanes already carry landing.exact they queue in arrival order
# before the checks, so the later one checks its rebased commit once and
# never restarts behind the peer it queued after.
new_landing_origin queued
printf '[landing]\n\texact = true\n' >"$landing_root/seed/.safe-push"
"$real_git" -C "$landing_root/seed" add .safe-push
"$real_git" -C "$landing_root/seed" commit -qm 'exact landings'
"$real_git" -C "$landing_root/seed" push -q "$landing_remote" main
make_landing_clone "$landing_root/a" a.txt
make_landing_clone "$landing_root/b" b.txt
add_held_hook "$landing_root/a"
add_held_hook "$landing_root/b"
run_landing "$landing_root/a" & pid_a=$!
wait_for "$pid_a" test -s "$landing_root/a.hook-runs"
run_landing "$landing_root/b" & pid_b=$!
wait_for "$pid_b" queued_lane "$landing_root/b"
release_hook "$landing_root/a"
status_a=0; wait "$pid_a" || status_a=$?
wait_for "$pid_b" test -s "$landing_root/b.hook-runs"
release_hook "$landing_root/b"
status_b=0; wait "$pid_b" || status_b=$?
case_output="$(cat "$landing_root/a.out" "$landing_root/b.out")"
[ $((status_a + status_b)) -eq 0 ] || fail "queued exact landing: a run failed: $case_output"
[ "$("$real_git" --git-dir="$landing_remote" log --format=%s -2 main | tr '\n' ' ')" = 'change from b change from a ' ] \
  || fail 'queued exact landing: lanes did not land in arrival order'
[ "$(tail -n1 "$landing_root/b.hook-runs")" = "$("$real_git" --git-dir="$landing_remote" rev-parse main)" ] \
  || fail 'queued exact landing: the hook did not check the commit that landed'

# Lanes queued behind an exact-landing lock holder land as one train: the
# holder cherry-picks each onto origin main in arrival order and checks the
# candidate once; a conflicting lane and a failing lane are each told why.
add_train_hook() {
  local dir="$1"
  mkdir -p "$dir/.hooks"
  printf '#!/usr/bin/env bash\ncat >/dev/null\nprintf "%%s\\n" "$(git rev-parse HEAD)" >>"%s"\nif [ -e fail.txt ]; then echo "farm red: https://github.com/o/r/actions/runs/4242"; exit 1; fi\n' \
    "$landing_root/hook-runs" >"$dir/.hooks/pre-push"
  chmod +x "$dir/.hooks/pre-push"
  "$real_git" -C "$dir" config core.hooksPath .hooks
}

landed_for() {
  awk -F'\t' -v session="$1" '$6 == session { landed = $5 } END { print landed }' "$xdg_state/safe-push/pushes.tsv"
}

run_train_case() {
  local lane
  : >"$landing_root/hook-runs"
  lock_key=$(printf '%s' "$landing_remote" | sha256sum | cut -c1-16)
  mkdir -p "$xdg_state/safe-push"
  hold_landing_lock
  train_pids=()
  for lane in "$@"; do
    add_train_hook "$landing_root/$lane"
    NORTH_FLEET_SESSION="${landing_root##*/}-$lane" run_landing "$landing_root/$lane" & train_pids+=($!)
    wait_for "${train_pids[-1]}" queued_lane "$landing_root/$lane"
  done
  release_landing_lock
  train_status=()
  for pid in "${train_pids[@]}"; do
    status=0; wait "$pid" || status=$?
    train_status+=("$status")
  done
  case_output="$(for lane in "$@"; do cat "$landing_root/$lane.out"; done)"
}

new_landing_origin train
printf '[landing]\n\texact = true\n' >"$landing_root/seed/.safe-push"
"$real_git" -C "$landing_root/seed" add .safe-push
"$real_git" -C "$landing_root/seed" commit -qm 'exact landings'
"$real_git" -C "$landing_root/seed" push -q "$landing_remote" main
for lane in a b c; do make_landing_clone "$landing_root/$lane" "$lane.txt"; done
run_train_case a b c
[ "${train_status[*]}" = '0 0 0' ] || fail "train: a lane failed (${train_status[*]}): $case_output"
[ "$("$real_git" --git-dir="$landing_remote" log --format=%s -3 main | tr '\n' ' ')" = 'change from c change from b change from a ' ] \
  || fail 'train: lanes did not land in arrival order'
[ "$(cat "$landing_root/hook-runs")" = "$("$real_git" --git-dir="$landing_remote" rev-parse main)" ] \
  || fail 'train: main is not the commit the checks passed'
for lane in a b c; do
  [ "$(landed_for "landing-train-$lane")" = "$("$real_git" --git-dir="$landing_remote" rev-parse main)" ] \
    || fail "train: the push log does not record landing-train-$lane landing on main"
done

missing_north_bin="$scratch/train-missing-north-bin"
mkdir -p "$missing_north_bin"
ln -s "$real_git" "$missing_north_bin/git"
ln -s "$(readlink -f "${BASH_SOURCE[0]}")" "$missing_north_bin/gitleaks"
missing_north_path="$missing_north_bin:/run/current-system/sw/bin:/usr/bin:/bin"
! PATH="$missing_north_path" command -v north >/dev/null || fail 'train missing north fixture contains north'
for language_failure in denied missing; do
  new_landing_origin "train-language-$language_failure"
  printf '[landing]\n\texact = true\n' >"$landing_root/seed/.safe-push"
  "$real_git" -C "$landing_root/seed" add .safe-push
  "$real_git" -C "$landing_root/seed" commit -qm 'exact landings'
  "$real_git" -C "$landing_root/seed" push -q "$landing_remote" main
  for lane in a b; do make_landing_clone "$landing_root/$lane" "$lane.txt"; done
  remote_before=$("$real_git" --git-dir="$landing_remote" rev-parse main)
  if [ "$language_failure" = denied ]; then
    SAFE_PUSH_TEST_LANGUAGE_DENY='new.py: Python files are forbidden' run_train_case a b
    expect_output 'new.py: Python files are forbidden'
  else
    SAFE_PUSH_TEST_PATH="$missing_north_path" run_train_case a b
    expect_output 'north not on PATH; refusing to land without the language gate'
  fi
  [ "${train_status[0]}" -ne 0 ] && [ "${train_status[1]}" -ne 0 ] || fail "train $language_failure published a lane"
  expect_output 'landing train of 2 lanes'
  [ "$("$real_git" --git-dir="$landing_remote" rev-parse main)" = "$remote_before" ] || fail "train $language_failure changed origin"
  [ ! -s "$landing_root/hook-runs" ] || fail "train $language_failure reached heavy checks"
done
printf 'PASS: train refuses denial and missing north before heavy checks\n'

new_landing_origin train-captured-candidate
printf '[landing]\n\texact = true\n' >"$landing_root/seed/.safe-push"
"$real_git" -C "$landing_root/seed" add .safe-push
"$real_git" -C "$landing_root/seed" commit -qm 'exact landings'
"$real_git" -C "$landing_root/seed" push -q "$landing_remote" main
make_landing_clone "$landing_root/a" new.py
make_landing_clone "$landing_root/b" clean.txt
candidate_base=$("$real_git" --git-dir="$landing_remote" rev-parse main)
candidate_oid_file="$landing_root/captured-oid"
candidate_head_trace="$landing_root/gated-heads"
printf '%s\n' "$candidate_base" >"$candidate_oid_file"
candidate_bin="$landing_root/candidate-bin"
mkdir -p "$candidate_bin"
cat >"$candidate_bin/git" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
real_git="${SAFE_PUSH_TEST_REAL_GIT:?}"
if [ "$*" = 'rev-parse HEAD' ]; then
  branch=$("$real_git" symbolic-ref --quiet --short HEAD) || branch=''
  if [[ "$branch" = safe-push-train-* ]]; then
    candidate=$("$real_git" rev-parse HEAD)
    if "$real_git" cat-file -e "$candidate:new.py" 2>/dev/null; then
      printf '%s\n' "$candidate" >"${SAFE_PUSH_TEST_LANGUAGE_DENY_OID_FILE:?}"
      "$real_git" reset -q --hard "${SAFE_PUSH_TEST_TRAIN_CLEAN_HEAD:?}"
      "$real_git" rev-parse HEAD >"$SAFE_PUSH_TEST_LANGUAGE_DENY_OID_FILE.clean"
      printf '%s\n' "$candidate"
      exit 0
    fi
  fi
fi
exec "$real_git" "$@"
SH
chmod +x "$candidate_bin/git"
SAFE_PUSH_TEST_REAL_GIT="$real_git" SAFE_PUSH_TEST_TRAIN_CLEAN_HEAD="$candidate_base" \
  SAFE_PUSH_TEST_LANGUAGE_DENY_OID_FILE="$candidate_oid_file" SAFE_PUSH_TEST_LANGUAGE_HEAD_TRACE="$candidate_head_trace" \
  SAFE_PUSH_TEST_PATH="$candidate_bin:$real_bin:$PATH" run_train_case a b
[ "${train_status[0]}" -ne 0 ] || fail 'train published the captured Python candidate after HEAD moved'
[ "${train_status[1]}" -eq 0 ] || fail 'train refused the clean lane'
expect_output 'train HEAD differs from captured candidate'
read -r candidate_oid <"$candidate_oid_file"
"$real_git" -C "$landing_root/a" cat-file -e "$candidate_oid:new.py" || fail 'captured candidate lacks new.py'
[ "$(cat "$candidate_oid_file.clean")" = "$candidate_base" ] || fail 'HEAD did not move to the clean commit after capture'
[ -s "$candidate_head_trace" ] || fail 'candidate-sensitive north stub was not exercised'
! "$real_git" --git-dir="$landing_remote" cat-file -e main:new.py 2>/dev/null || fail 'train published new.py after candidate capture race'
printf 'PASS: train refuses captured Python candidate when HEAD moves clean\n'

new_landing_origin train-mixed
printf '[landing]\n\texact = true\n' >"$landing_root/seed/.safe-push"
"$real_git" -C "$landing_root/seed" add .safe-push
"$real_git" -C "$landing_root/seed" commit -qm 'exact landings'
"$real_git" -C "$landing_root/seed" push -q "$landing_remote" main
make_landing_clone "$landing_root/a" a.txt
make_landing_clone "$landing_root/b" shared.txt
make_landing_clone "$landing_root/c" shared.txt
make_landing_clone "$landing_root/d" fail.txt
make_landing_clone "$landing_root/e" e.txt
"$real_git" -C "$landing_root/seed" pull -q --no-rebase "$landing_root/e" lane-e
"$real_git" -C "$landing_root/seed" commit -q --amend -m 'e landed earlier'
"$real_git" -C "$landing_root/seed" push -q "$landing_remote" main
run_train_case a b c d e
[ "${train_status[4]}" -eq 0 ] \
  || fail "mixed train: a lane already on origin was not counted as landed: $(cat "$landing_root/e.out")"
[ "${train_status[0]} ${train_status[1]}" = '0 0' ] || fail "mixed train: a good lane failed (${train_status[*]}): $case_output"
[ "${train_status[2]}" -ne 0 ] && [ "${train_status[3]}" -ne 0 ] || fail "mixed train: a bad lane landed (${train_status[*]})"
grep -Fq 'shared.txt' "$landing_root/c.out" || fail "mixed train: conflict path missing"
grep -Fq 'https://github.com/o/r/actions/runs/4242' "$landing_root/d.out" || fail "mixed train: failure receipt missing"
[ "$("$real_git" --git-dir="$landing_remote" show main:shared.txt)" = b ] || fail 'mixed train: main lacks b'
"$real_git" --git-dir="$landing_remote" cat-file -e main:a.txt || fail 'mixed train: main lacks a'
! "$real_git" --git-dir="$landing_remote" cat-file -e main:fail.txt 2>/dev/null || fail 'mixed train: the failing lane landed'
grep -qx "$("$real_git" --git-dir="$landing_remote" rev-parse main)" "$landing_root/hook-runs" \
  || fail 'mixed train: main is not a commit the checks passed'

# A lane that moves a Bun lockfile (a vendored pin bump) is checked against a
# fresh install in the holder's worktree, not the holder's old node_modules.
new_landing_origin train-deps
printf '[landing]\n\texact = true\n' >"$landing_root/seed/.safe-push"
printf '{}\n' >"$landing_root/seed/package.json"
printf 'pin 1\n' >"$landing_root/seed/bun.lock"
"$real_git" -C "$landing_root/seed" add .safe-push package.json bun.lock
"$real_git" -C "$landing_root/seed" commit -qm 'exact landings, pin 1'
"$real_git" -C "$landing_root/seed" push -q "$landing_remote" main
printf '#!/usr/bin/env bash\n[ "$*" = "install --frozen-lockfile" ] || exec %q "$@"\ncp bun.lock node_modules/installed.lock\n' "$(command -v bun)" >"$real_bin/bun"
chmod +x "$real_bin/bun"
make_landing_clone "$landing_root/a" a.txt
mkdir "$landing_root/a/node_modules" && cp "$landing_root/a/bun.lock" "$landing_root/a/node_modules/installed.lock"
"$real_git" -C "$landing_root/a" config core.excludesFile /dev/null
printf 'node_modules/\n' >"$landing_root/a/.git/info/exclude"
make_landing_clone "$landing_root/b" bun.lock
for lane in a b; do
  printf '#!/usr/bin/env bash\ncat >/dev/null\ncmp -s bun.lock node_modules/installed.lock || { echo stale dependencies; exit 1; }\n' >"$landing_root/$lane/.git/hooks/pre-push"
  chmod +x "$landing_root/$lane/.git/hooks/pre-push"
done
mkdir "$landing_root/b/node_modules" && cp "$landing_root/b/bun.lock" "$landing_root/b/node_modules/installed.lock"
printf 'node_modules/\n' >"$landing_root/b/.git/info/exclude"
lock_key=$(printf '%s' "$landing_remote" | sha256sum | cut -c1-16)
mkdir -p "$xdg_state/safe-push"
hold_landing_lock
run_landing "$landing_root/a" & pid_a=$!
wait_for "$pid_a" queued_lane "$landing_root/a"
run_landing "$landing_root/b" & pid_b=$!
wait_for "$pid_b" queued_lane "$landing_root/b"
release_landing_lock
status_a=0; wait "$pid_a" || status_a=$?
status_b=0; wait "$pid_b" || status_b=$?
rm -f "$real_bin/bun"
case_output="$(cat "$landing_root/a.out" "$landing_root/b.out")"
[ $((status_a + status_b)) -eq 0 ] || fail "train with a lockfile change: a lane failed: $case_output"
[ "$("$real_git" --git-dir="$landing_remote" show main:bun.lock)" = b ] || fail 'train with a lockfile change: main lacks the new lockfile'

new_landing_origin red-fix
printf '[landing]\n\texact = true\n' >"$landing_root/seed/.safe-push"
printf 'failing\n' >"$landing_root/seed/red.txt"
"$real_git" -C "$landing_root/seed" add .safe-push red.txt
"$real_git" -C "$landing_root/seed" commit -qm 'exact landings; main is red'
"$real_git" -C "$landing_root/seed" push -q "$landing_remote" main
red_base="$("$real_git" --git-dir="$landing_remote" rev-parse main)"
lock_key=$(printf '%s' "$landing_remote" | sha256sum | cut -c1-16)
red_queue="$xdg_state/safe-push/$lock_key.queue"
for lane in h w f; do
  make_landing_clone "$landing_root/$lane" "$lane.txt"
  mkdir -p "$landing_root/$lane/.hooks"
  printf '#!/usr/bin/env bash\ncat >/dev/null\nif [ -e red.txt ]; then\n  : >>%q\n  for _ in $(seq 100); do grep -qs %q %q/* && break; sleep 0.1; done\n  echo red >>%q; exit 1\nfi\necho green >>%q\n' \
    "$landing_root/red-check" "$landing_root/f" "$red_queue" "$landing_root/hook-runs" "$landing_root/hook-runs" >"$landing_root/$lane/.hooks/pre-push"
  chmod +x "$landing_root/$lane/.hooks/pre-push"
  "$real_git" -C "$landing_root/$lane" config core.hooksPath .hooks
done
"$real_git" -C "$landing_root/f" rm -q red.txt
"$real_git" -C "$landing_root/f" commit -qm 'fix #399: main is red'
cat >"$real_bin/gh" <<'GH'
#!/usr/bin/env bash
[ "$1 $2" = 'issue list' ] || exit 1
while [ "$#" -gt 0 ] && [ "$1" != --jq ]; do shift; done
jq -r "$2" <<<'[{"number":398,"title":"main is red-ish"},{"number":399,"title":"main is red"}]'
GH
chmod +x "$real_bin/gh"
: >"$landing_root/hook-runs"
mkdir -p "$xdg_state/safe-push"
hold_landing_lock
run_landing "$landing_root/h" & pid_h=$!
wait_for "$pid_h" queued_lane "$landing_root/h"
run_landing "$landing_root/w" & pid_w=$!
wait_for "$pid_w" queued_lane "$landing_root/w"
release_landing_lock
wait_for "$pid_h $pid_w" test -e "$landing_root/red-check"
run_landing "$landing_root/f" & pid_f=$!
red_status=()
for pid in "$pid_h" "$pid_w" "$pid_f"; do
  status=0; wait "$pid" || status=$?
  red_status+=("$status")
done
rm -f "$real_bin/gh"
case_output="$(cat "$landing_root/h.out" "$landing_root/w.out" "$landing_root/f.out")"
red_order="$("$real_git" --git-dir="$landing_remote" log --reverse --format=%s "$red_base..main" | paste -sd, -)"
red_runs="$(sort "$landing_root/hook-runs" | uniq -c | awk '{ printf "%s%s %s", sep, $1, $2; sep = ", " }')"
printf 'red-fix scenario: exits h w f = %s; landed %s; checks %s\n' "${red_status[*]}" "${red_order:-nothing}" "$red_runs"
[ "${red_status[*]}" = '0 0 0' ] || fail "red fix: a lane did not land (${red_status[*]}): $case_output"
[ "$red_order" = 'change from f,fix #399: main is red,change from h,change from w' ] \
  || fail "red fix: the fix did not land first with the held lanes behind it in arrival order ($red_order)"
[ "$(grep -c red "$landing_root/hook-runs")" -eq 1 ] \
  || fail "red fix: a check ran on red main after the fix queued ($red_runs): $case_output"

new_landing_origin killed-holder
printf '[landing]\n\texact = true\n' >"$landing_root/seed/.safe-push"
"$real_git" -C "$landing_root/seed" add .safe-push
"$real_git" -C "$landing_root/seed" commit -qm 'exact landings'
"$real_git" -C "$landing_root/seed" push -q "$landing_remote" main
make_landing_clone "$landing_root/lane" lane.txt
mkdir -p "$landing_root/lane/.hooks"
printf '#!/usr/bin/env bash\ncat >/dev/null\necho "$$" >%q\nexec sleep 30\n' "$landing_root/hook-pid" >"$landing_root/lane/.hooks/pre-push"
chmod +x "$landing_root/lane/.hooks/pre-push"
"$real_git" -C "$landing_root/lane" config core.hooksPath .hooks
lock_file="$xdg_state/safe-push/$(printf '%s' "$landing_remote" | sha256sum | cut -c1-16).lock"
run_landing "$landing_root/lane" & pid_lane=$!
wait_for "$pid_lane" test -s "$landing_root/hook-pid"
kill -TERM "$(awk '/^NSpid:/ { print $NF }' "/proc/$(sed -n 's/^pid \([0-9]*\) .*/\1/p' "$lock_file")/status")" 2>/dev/null || true
freed=''
for i in $(seq 30); do
  if flock -n "$lock_file" true; then freed="$i"; break; fi
  sleep 0.1
done
kill "$(cat "$landing_root/hook-pid")" 2>/dev/null || true
wait "$pid_lane" 2>/dev/null || true
[ -n "$freed" ] || fail 'killed holder: its hook kept the landing lock held for 3 s'

# A safe-push started by the checks of one that holds the landing lock (a
# farm run's scratch push) proceeds instead of waiting on its parent.
new_landing_origin reentry
make_landing_clone "$landing_root/lane" lane.txt
lock_key=$(printf '%s' "$landing_remote" | sha256sum | cut -c1-16)
mkdir -p "$xdg_state/safe-push"
hold_landing_lock
case_status=0
SAFE_PUSH_LOCK_HELD="$lock_key" timeout 15 bash -c "$(declare -f run_landing); xdg_state='$xdg_state' real_bin='$real_bin' scratch='$scratch' TARGET='$TARGET' run_landing '$landing_root/lane'" || case_status=$?
release_landing_lock
case_output="$(cat "$landing_root/lane.out")"
expect_status zero

# While a main landing holds the lock, a scratch push to farm/* goes straight through.
new_landing_origin farm-scratch
make_landing_clone "$landing_root/lane" lane.txt
lock_key=$(printf '%s' "$landing_remote" | sha256sum | cut -c1-16)
mkdir -p "$xdg_state/safe-push"
hold_landing_lock
case_status=0
timeout 15 bash -c "$(declare -f run_landing); landing_to=farm/x xdg_state='$xdg_state' real_bin='$real_bin' scratch='$scratch' TARGET='$TARGET' run_landing '$landing_root/lane'" || case_status=$?
release_landing_lock
case_output="$(cat "$landing_root/lane.out")"
expect_status zero
[[ "$case_output" != *'waiting for the landing lock'* ]] || fail 'farm scratch push waited for the landing lock'
[ "$("$real_git" --git-dir="$landing_remote" rev-parse refs/heads/farm/x)" = "$("$real_git" -C "$landing_root/lane" rev-parse HEAD)" ] \
  || fail 'farm scratch push did not publish farm/x'

# A failing hook refuses the landing before the lock, and origin is unchanged.
new_landing_origin hook-fails
make_landing_clone "$landing_root/lane" lane.txt
add_held_hook "$landing_root/lane" 1
remote_main_before="$("$real_git" --git-dir="$landing_remote" rev-parse main)"
case_status=0
run_landing "$landing_root/lane" & pid_lane=$!
wait_for "$pid_lane" test -s "$landing_root/lane.hook-runs"
release_hook "$landing_root/lane"
wait "$pid_lane" || case_status=$?
case_output="$(cat "$landing_root/lane.out")"
expect_status nonzero
[ "$("$real_git" --git-dir="$landing_remote" rev-parse main)" = "$remote_main_before" ] \
  || fail 'failing hook landing mutated origin main'

# A lane whose change conflicts with what landed meanwhile is refused with
# the file named, and the rebase is aborted so the worktree is left clean.
new_landing_origin conflict
make_landing_clone "$landing_root/lane" shared.txt
lane_head="$("$real_git" -C "$landing_root/lane" rev-parse HEAD)"
printf 'landed first\n' >"$landing_root/seed/shared.txt"
"$real_git" -C "$landing_root/seed" commit -qam 'conflicting landing'
"$real_git" -C "$landing_root/seed" push -q "$landing_remote" main
remote_main_before="$("$real_git" --git-dir="$landing_remote" rev-parse main)"
case_status=0
run_landing "$landing_root/lane" || case_status=$?
case_output="$(cat "$landing_root/lane.out")"
expect_status nonzero
expect_output 'shared.txt'
[ -z "$("$real_git" -C "$landing_root/lane" status --porcelain)" ] \
  || fail 'conflict refusal left the worktree dirty'
for d in rebase-merge rebase-apply; do
  [ ! -e "$("$real_git" -C "$landing_root/lane" rev-parse --absolute-git-dir)/$d" ] \
    || fail 'conflict refusal left a rebase in progress'
done
[ "$("$real_git" -C "$landing_root/lane" rev-parse HEAD)" = "$lane_head" ] \
  || fail 'conflict refusal moved the lane HEAD'
[ "$("$real_git" --git-dir="$landing_remote" rev-parse main)" = "$remote_main_before" ] \
  || fail 'conflict refusal mutated origin main'

# Lane cleanup: a landed lane under <container>/worktrees/<slug> is removed
# with its branch, judged against origin main even while the main checkout's
# local main is stale.
new_lane_container() {
  lane_container="$scratch/lanes-$1"
  lane_remote="$lane_container/remote.git"
  lane="$lane_container/worktrees/lane"
  mkdir -p "$lane_container/worktrees"
  "$real_git" init --bare -q -b main "$lane_remote"
  "$real_git" init -q -b main "$lane_container/main"
  "$real_git" -C "$lane_container/main" config user.name safe-push-test
  "$real_git" -C "$lane_container/main" config user.email safe-push-test@example.invalid
  "$real_git" -C "$lane_container/main" commit --allow-empty -qm base
  "$real_git" -C "$lane_container/main" remote add origin "$lane_remote"
  "$real_git" -C "$lane_container/main" push -q -u origin main
  "$real_git" -C "$lane_container/main" worktree add -q "$lane" -b lane
  "$real_git" -C "$lane" commit --allow-empty -qm lane-work
}

lane_runner=()

run_lane() {
  printf "%s\n" run_lane >>"$scratch/cases"
  set +e
  case_output="$(
    cd "$lane"
    SAFE_PUSH_TEST_SHIM=1 \
      SAFE_PUSH_TEST_TRACE="$lane_container/trace" \
      SAFE_PUSH_TEST_STATE="$lane_container/state" \
      XDG_STATE_HOME="$xdg_state" \
      PATH="$real_bin:$PATH" \
      "${lane_runner[@]}" "$TARGET" --to main "$@" 2>&1
    landed=$?
    pwd -P >/dev/null || exit 99
    exit "$landed"
  )"
  case_status=$?
  set -e
}

lane_removed() {
  local tries=0
  while [ -e "$lane" ] && [ "$tries" -lt 100 ]; do
    sleep 0.1
    tries=$((tries + 1))
  done
  [ ! -e "$lane" ]
}

lane_branch_exists() {
  "$real_git" -C "$lane_container/main" rev-parse -q --verify refs/heads/lane >/dev/null
}

lane_branch_removed() {
  local tries=0
  while lane_branch_exists && [ "$tries" -lt 100 ]; do
    sleep 0.1
    tries=$((tries + 1))
  done
  ! lane_branch_exists
}

remote_lane_exists() {
  "$real_git" --git-dir="$lane_remote" rev-parse -q --verify refs/heads/lane >/dev/null
}

new_lane_container clean
printf 'target/\n' >>"$lane_container/main/.git/info/exclude"
mkdir -p "$lane/target/debug/deps"
printf 'Signature: 8a477f597d28d172789f06886806bc55\n' >"$lane/target/CACHEDIR.TAG"
head -c 1048576 /dev/zero >"$lane/target/debug/deps/libbulk.rlib"
NORTH_FLEET_SESSION=lane-clean run_lane
expect_status zero
[ "$(landed_for lane-clean)" = "$("$real_git" --git-dir="$lane_remote" rev-parse main)" ] \
  || fail 'the push log does not record the session lane-clean landing on main'
lane_removed || fail 'landed clean lane was not removed once its caller left'
lane_branch_removed || fail 'landed clean lane branch was not deleted'

new_lane_container pid-namespace
lane_runner=(unshare --user --map-current-user --pid --fork --kill-child=KILL -- timeout 120)
run_lane
lane_runner=()
expect_status zero
[ -d "$lane" ] || fail 'a lane landed from inside it under a PID namespace with the host /proc, as run-bounded runs it, was removed under its caller'

new_lane_container keep
run_lane --keep-lane
expect_status zero
[ -d "$lane" ] || fail '--keep-lane removed the lane'
lane_branch_exists || fail '--keep-lane deleted the lane branch'

new_lane_container dirty
printf 'unsaved\n' >"$lane/notes.txt"
run_lane
expect_status zero
[ -f "$lane/notes.txt" ] || fail 'dirty lane was removed'
lane_branch_exists || fail 'dirty lane branch was deleted'

new_lane_container remote-merged
"$real_git" -C "$lane" push -q origin lane
run_lane
expect_status zero
! remote_lane_exists || fail 'merged remote lane branch was not deleted'
lane_removed || fail 'landed lane with a merged remote branch was not removed'

new_lane_container remote-unmerged
"$real_git" -C "$lane" commit --allow-empty -qm 'only on the remote branch'
"$real_git" -C "$lane" push -q origin lane
"$real_git" -C "$lane" reset -q --hard HEAD~1
run_lane
expect_status zero
remote_lane_exists || fail 'remote lane branch with unlanded work was deleted'

} >"$scratch/section-3.log" 2>&1 &
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
