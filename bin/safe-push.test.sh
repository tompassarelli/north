#!/usr/bin/env bash
set -euo pipefail

shim_main() {
  local tool="${0##*/}"
  {
    printf '%s' "$tool"
    printf ' <%s>' "$@"
    printf '\n'
  } >>"${SAFE_PUSH_TEST_TRACE:?}"

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
trap 'rm -rf "${scratch:?}"' EXIT
trace="$scratch/trace"
: >"$trace"
# The real deny-list must not leak into the suite; cases opt in with a fixture.
export SAFE_PUSH_DENYLIST="$scratch/absent-denylist.toml"

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

eval "$(sed -n '/^usage() {/,/^}$/p; /^argument_error() {/,/^}$/p; /^plan_push() {/,/^}$/p' "$TARGET")"
git() { printf 'git\n' >>"$trace"; return 97; }
gitleaks() { printf 'scan\n' >>"$trace"; return 97; }
property_rows=0
while IFS='|' read -r verdict arguments; do
  read -r -a parsed_args <<<"$arguments"
  case_status=0
  case_output=$(plan_push arguments "${parsed_args[@]}" 2>&1) || case_status=$?
  expect_status "$verdict"
  property_rows=$((property_rows + 1))
done <<'CASES'
zero|-h
zero|--help
nonzero|--keep-lane
nonzero|--wat
nonzero|main
nonzero|--force
nonzero|-f
nonzero|--force-with-lease
nonzero|--mirror
nonzero|--delete
nonzero|--prune
nonzero|--tag
nonzero|--tag --dry-run
nonzero|--dry-run --dry-run
nonzero|--tag release-a --tag release-b
nonzero|--to
nonzero|--to main --to other
nonzero|--to main --tag release-a
nonzero|--help --dry-run
nonzero|--keep-lane --keep-lane --to main
zero|--dry-run
zero|--to other
zero|--dry-run --to other
zero|--tag release-a
zero|--dry-run --tag release-a
zero|--tag release-a --dry-run
CASES
for invalid in HEAD refs/heads/main origin/main ../main 'bad name' 'bad..name'; do
  case_status=0
  case_output=$(plan_push arguments --to "$invalid" 2>&1) || case_status=$?
  expect_status nonzero
  property_rows=$((property_rows + 1))
done
destination_ref=refs/heads/main
for shape in absent object symbolic ambiguous malformed wrong-ref; do
  oid=1111111111111111111111111111111111111111
  case "$shape" in
    absent) observed='' ;;
    object) observed=$(printf '%s\t%s' "$oid" "$destination_ref") ;;
    symbolic) observed=$(printf 'ref: refs/heads/other\t%s' "$destination_ref") ;;
    ambiguous) observed=$(printf '%s\t%s\n%s\t%s' "$oid" "$destination_ref" "$oid" "$destination_ref") ;;
    malformed) observed=$(printf 'not-an-oid\t%s' "$destination_ref") ;;
    wrong-ref) observed=$(printf '%s\trefs/heads/other' "$oid") ;;
  esac
  case_status=0
  case_output=$(plan_push snapshot "$observed" "$destination_ref" 2>&1) || case_status=$?
  case "$shape" in
    absent) expect_status zero; [ "$case_output" = absent ] || fail 'absent snapshot changed' ;;
    object) expect_status zero; [ "$case_output" = "object:$oid" ] || fail 'snapshot lost observed oid' ;;
    *) expect_status nonzero ;;
  esac
  property_rows=$((property_rows + 1))
done
for current in main feature farm/topic; do
  for destination in '' main farm/output; do
    for dry_arg in '' --dry-run; do
      args=()
      [ -z "$dry_arg" ] || args+=("$dry_arg")
      [ -z "$destination" ] || args+=(--to "$destination")
      plan_push arguments "${args[@]}"
      case_status=0
      if [ "$to_seen" -eq 0 ]; then
        case_output=$(plan_push source "$current" main 2>&1) || case_status=$?
      fi
      if [ "$current" != main ] && [ -z "$destination" ]; then
        expect_status nonzero
      else
        expect_status zero
        for observed in '' 1111111111111111111111111111111111111111 2222222222222222222222222222222222222222; do
          destination_oid="$observed"
          head_oid=3333333333333333333333333333333333333333
          destination_ref="refs/heads/${destination:-$current}"
          plan_push publication "$head_oid" "$destination_ref" "$destination_oid"
          [ "$publication" = "$head_oid:$destination_ref" ] || fail 'plan lost the captured source or destination'
          [ "${publication_flags[*]}" = "--force-with-lease=$destination_ref:$observed" ] || fail 'plan lost the observed destination lease'
        done
      fi
      property_rows=$((property_rows + 1))
    done
  done
done
printf 'planning property: %s rows\n' "$property_rows"

[ ! -s "$trace" ] || fail 'pure planning invoked git or a scanner'
unset -f git gitleaks
real_git="$(command -v git)"
real_bin="$scratch/real-bin"
mkdir -p "$real_bin"
ln -s "$(readlink -f "${BASH_SOURCE[0]}")" "$real_bin/gitleaks"

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
      XDG_STATE_HOME="$scratch/xdg-state" \
      PATH="$real_bin:$PATH" \
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

make_real_fixture land-check-boundary
land_home="$real_fixture/home"
mkdir -p "$land_home"
land_remote_before=$(real_remote_state)
"$real_git" -C "$real_repo" config --file .safe-push --add land.check 'file.txt,other.txt :: printf "ran\\n" >> check-runs; exit 23'
"$real_git" -C "$real_repo" config --file .safe-push --add land.check 'unmatched/ :: exit 71'
"$real_git" -C "$real_repo" add .safe-push
"$real_git" -C "$real_repo" commit -qm 'affected checks'
printf 'changed\n' >"$real_repo/file.txt"
"$real_git" -C "$real_repo" add file.txt
"$real_git" -C "$real_repo" commit -qm 'changed input'
HOME="$land_home" run_real_case '' --to main
expect_status nonzero
expect_output 'exited 23; NOT pushed.'
[ "$(wc -l <"$real_repo/check-runs")" -eq 1 ] || fail 'matching check did not run exactly once'
[ "$(real_remote_state)" = "$land_remote_before" ] || fail 'failing land check published'
"$real_git" -C "$real_repo" config --file .safe-push --unset-all land.check
"$real_git" -C "$real_repo" config --file .safe-push --add land.check 'unmatched/ :: exit 71'
"$real_git" -C "$real_repo" add .safe-push
"$real_git" -C "$real_repo" commit -qm 'unmatched check'
printf 'uncommitted land.check must not run\n' >>"$real_repo/.safe-push"
HOME="$land_home" run_real_case '' --dry-run --to main
expect_status zero
[ "$(wc -l <"$real_repo/check-runs")" -eq 1 ] || fail 'unmatched or uncommitted check ran'
"$real_git" -C "$real_repo" checkout -- .safe-push
"$real_git" -C "$real_repo" rm -q .safe-push
"$real_git" -C "$real_repo" commit -qm 'no land.check'
HOME="$land_home" run_real_case '' --to main
expect_status zero
[ "$("$real_git" --git-dir="$real_remote" rev-parse main)" = "$("$real_git" -C "$real_repo" rev-parse HEAD)" ] || fail 'unconfigured landing did not publish'

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

# Real commit + annotated-tag publication proves captured object refspecs still
# perform the normal operation against an actual bare remote.
make_real_fixture normal
run_real_case ''
expect_status zero
[ "$("$real_git" -C "$real_repo" rev-parse main)" \
  = "$("$real_git" --git-dir="$real_remote" rev-parse refs/heads/main)" ] \
  || fail 'normal branch publication did not update the remote to captured HEAD'

"$real_git" -C "$real_repo" tag -a release-real -m 'release annotation'
remote_before="$(real_remote_state)"
run_real_case '' --dry-run --tag release-real
expect_status zero
[ "$(real_remote_state)" = "$remote_before" ] || fail 'tag dry-run changed origin'
run_real_case '' --tag release-real
expect_status zero
[ "$("$real_git" -C "$real_repo" rev-parse refs/tags/release-real)" \
  = "$("$real_git" --git-dir="$real_remote" rev-parse refs/tags/release-real)" ] \
  || fail 'normal tag publication did not update the remote to the captured tag object'

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

# A support-only commit may be published by tag without adding a remote branch;
# its commit history and annotation must both pass the secret scan first.
make_real_fixture support-tag
"$real_git" -C "$real_repo" tag -a release-support -m 'support release annotation'
remote_main_before="$("$real_git" --git-dir="$real_remote" rev-parse refs/heads/main)"
run_real_case '' --tag release-support
expect_status zero
[ "$("$real_git" --git-dir="$real_remote" rev-parse refs/heads/main)" = "$remote_main_before" ] \
  || fail 'support-only tag publication changed remote main'
if "$real_git" --git-dir="$real_remote" rev-parse --verify refs/heads/support-tag >/dev/null 2>&1; then
  fail 'support-only tag publication created a remote support branch'
fi
[ "$("$real_git" -C "$real_repo" rev-parse refs/tags/release-support)" \
  = "$("$real_git" --git-dir="$real_remote" rev-parse refs/tags/release-support)" ] \
  || fail 'support-only tag publication did not publish the captured tag object'

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
for name in agents threads safe-push worker-sweep capacity-watchdog codex-lead retired-tool helper.test.sh; do
  printf '#!/bin/sh\n' >"$real_repo/dotfiles/bin/$name"
  chmod +x "$real_repo/dotfiles/bin/$name"
done
"$real_git" -C "$real_repo" add dotfiles/bin
"$real_git" -C "$real_repo" commit -qm 'all core tools'
"$real_git" -C "$real_repo" push -q origin HEAD:main
tool_base=$("$real_git" -C "$real_repo" rev-parse HEAD)
remote_before="$(real_remote_state)"
for name in agents threads safe-push worker-sweep capacity-watchdog codex-lead retired-tool helper.test.sh; do
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
printf 'it("kept", () => {})\n' >"$real_repo/tests/boundary.js"
"$real_git" -C "$real_repo" add src/lib.rs tests/boundary.js
"$real_git" -C "$real_repo" commit -qm 'existing tests'
"$real_git" -C "$real_repo" push -q origin HEAD:main
kind_base=$("$real_git" -C "$real_repo" rev-parse HEAD)
for kind_case in rust-unannotated js-unannotated rust-annotated js-annotated invalid-kind empty-mutant deleted edited-js; do
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
    *)
      printf '#[test]\nfn deleted_boundary() {}\n#[tokio::test]\nfn rust_boundary() {}\n' >"$real_repo/src/lib.rs"
      case_path=src/lib.rs; case_name=rust_boundary
      ;;
  esac
  "$real_git" -C "$real_repo" add src/lib.rs tests/boundary.js
  "$real_git" -C "$real_repo" commit -qm "$kind_case"
  case "$kind_case" in
    *-annotated)
      printf 'mutated\n' >"$real_repo/content.txt"
      (cd "$real_repo"; XDG_STATE_HOME="$scratch/xdg-state" "$REPO/bin/mutant" record "${trailer##* }")
      "$real_git" -C "$real_repo" restore content.txt
      ;;
  esac
  run_real_case '' --dry-run
  case "$kind_case" in
    *-annotated|deleted|edited-js) expect_status zero ;;
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
for mutant_case in empty missing real stale new-code shared-clone; do
  "$real_git" -C "$real_repo" reset -q --hard "$mutant_base"
  patch="$scratch/xdg-state/agents/mutants/remote/$mutant_case.patch"
  if [ "$mutant_case" = stale ]; then
    printf 'different\n' >"$real_repo/content.txt"
    "$real_git" -C "$real_repo" commit -qam 'different code'
    printf 'mutated\n' >"$real_repo/content.txt"
    (cd "$real_repo"; XDG_STATE_HOME="$scratch/xdg-state" "$REPO/bin/mutant" record "$mutant_case")
    "$real_git" -C "$real_repo" reset -q --hard "$mutant_base"
  elif [ "$mutant_case" = new-code ]; then
    printf 'new code\n' >"$real_repo/new.txt"
    "$real_git" -C "$real_repo" add new.txt
    "$real_git" -C "$real_repo" commit -qm 'new code'
  fi
  case "$mutant_case" in
    empty)
      sed -i 's/does-not-match/mutated/' "$real_repo/content.txt"
      case_status=0
      case_output=$(cd "$real_repo"; XDG_STATE_HOME="$scratch/xdg-state" "$REPO/bin/mutant" record "$mutant_case" 2>&1) || case_status=$?
      expect_status nonzero
      [ ! -e "$patch" ] || fail 'no-op sed recorded a patch'
      : >"$patch"
      ;;
    real|new-code)
      mutant_path=content.txt
      [ "$mutant_case" != new-code ] || mutant_path=new.txt
      printf 'mutated\n' >"$real_repo/$mutant_path"
      (cd "$real_repo"; XDG_STATE_HOME="$scratch/xdg-state" "$REPO/bin/mutant" record "$mutant_case")
      "$real_git" -C "$real_repo" restore "$mutant_path"
      ;;
    shared-clone)
      "$real_git" clone -q --shared "$real_repo" "$real_fixture/shared-clone"
      printf 'mutated\n' >"$real_fixture/shared-clone/content.txt"
      (cd "$real_fixture/shared-clone"; XDG_STATE_HOME="$scratch/xdg-state" "$REPO/bin/mutant" record "$mutant_case")
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
printf 'mutant evidence: PASS (6 cases; historical main claim accepted)\n'

# A planted product mutant and an unexplained new skip are the faults the landing scan must trip.
make_real_fixture lead-lands
printf 'lane work\n' >>"$real_repo/content.txt"
"$real_git" -C "$real_repo" commit -qam 'lane work'
touch "$("$real_git" -C "$real_repo" rev-parse --path-format=absolute --git-path lead-lands)"
CODEX_THREAD_ID=fixture-thread run_real_case '' --dry-run --to main
expect_status nonzero
expect_output 'carries a lead-lands marker'
CODEX_THREAD_ID='' run_real_case '' --dry-run --to main
expect_status zero

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
SAFE_PUSH_TEST_CI_FIXTURE="$ci_fixture" run_real_case '' --dry-run
expect_status zero
expect_output 'clean (--dry-run)'

printf '[{"jobs":[{"name":"older tests","started_at":"2026-10-11T00:00:00Z","completed_at":"2026-10-11T00:05:00Z"}]}]\n' >"$ci_fixture/102.json"
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

ci_hold_dir="$scratch/xdg-state/safe-push"
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
SAFE_PUSH_TEST_CI_FIXTURE="$ci_fixture" run_real_case '' --dry-run --to main
expect_status zero
[ ! -e "$ci_hold" ] || fail 'a passing budget left the hold in place'
printf '[{"jobs":[{"name":"older tests","started_at":"2026-10-11T00:00:00Z","completed_at":"2026-10-11T00:05:00Z"}]}]\n' >"$ci_fixture/102.json"
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
SAFE_PUSH_TEST_AUTOLAND_STATUS=$'failure\tTrain retested\thttps://example.test/run' run_real_case '' --to main
expect_status nonzero
expect_output "main lands through Autoland's one queue"
expect_output 'Autoland did not land this lane: Train retested'
[ "$("$real_git" --git-dir="$real_remote" rev-parse main)" = "$autoland_base" ] || fail 'an Autoland repository pushed main directly'
run_real_case '' --to main --direct
expect_status zero
expect_output '--direct: bypassing'
[ "$("$real_git" --git-dir="$real_remote" rev-parse main)" = "$("$real_git" -C "$real_repo" rev-parse HEAD)" ] || fail '--direct did not push main'
grep -q $'\t' "$scratch/xdg-state/safe-push/direct-landings.tsv" || fail '--direct was not logged'
printf 'on: [push]\n' >"$real_repo/.github/workflows/autoland.yml"
"$real_git" -C "$real_repo" add .github/workflows/autoland.yml
"$real_git" -C "$real_repo" commit -qm 'workflow lane'
gap_lock="$scratch/xdg-state/safe-push/$(printf '%s' "$real_remote" | sha256sum | cut -c1-16)"
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
rm -f "$real_bin/gh"
printf 'Autoland routing: 4 cases\n'

printf 'CI budget boundary: 19 cases\n'

make_real_fixture dep-gate
printf '{"dependencies":{"unowned-package":"1.0.0"}}\n' >"$real_repo/package.json"
"$real_git" -C "$real_repo" add package.json
"$real_git" -C "$real_repo" commit -qm 'add dependency without reason'
remote_before="$(real_remote_state)"
run_real_case ''
expect_status nonzero
[ ! -s "$real_trace" ] || fail 'dependency refusal reached the scan'
[ "$(real_remote_state)" = "$remote_before" ] || fail 'dependency refusal changed origin'

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

# The public deny-list gate runs real gitleaks on a fake private term: a public
# destination with a match is refused with redacted findings and nothing pushed;
# a clean public push, a PRIVATE destination and an absent deny-list publish.
fixture_denylist="$scratch/denylist.toml"
cat >"$fixture_denylist" <<'TOML'
title = "safe-push test deny-list"
[[rules]]
id = "fake-private-term"
description = "fake private identity term"
regex = '''zebra-quokka-7731'''
TOML
real_gitleaks="$(command -v gitleaks)"
gh_bin="$scratch/gh-bin"
mkdir -p "$gh_bin"
ln -s "$(readlink -f "${BASH_SOURCE[0]}")" "$gh_bin/gh"
commit_private_term() {
  printf 'owner: zebra-quokka-7731\n' >"$real_repo/identity.txt"
  "$real_git" -C "$real_repo" add identity.txt
  "$real_git" -C "$real_repo" commit -qm 'add identity'
}
run_denylist_case() {
  PATH="$gh_bin:$PATH" SAFE_PUSH_TEST_VISIBILITY="$1" \
    SAFE_PUSH_TEST_REAL_GITLEAKS="$real_gitleaks" run_real_case ''
}
expect_published() {
  [ "$("$real_git" -C "$real_repo" rev-parse main)" = "$("$real_git" --git-dir="$real_remote" rev-parse refs/heads/main)" ] \
    || fail "$1"
}

make_real_fixture denylist-public-finding
commit_private_term
remote_before="$(real_remote_state)"
SAFE_PUSH_DENYLIST="$fixture_denylist" run_denylist_case PUBLIC
expect_status nonzero
expect_output 'fake-private-term'
expect_output 'identity.txt'
if grep -Fq 'zebra-quokka-7731' <<<"$case_output"; then fail 'deny-list refusal printed the matched value'; fi
[ "$(real_remote_state)" = "$remote_before" ] || fail 'deny-list refusal mutated the remote'

make_real_fixture denylist-public-clean
SAFE_PUSH_DENYLIST="$fixture_denylist" run_denylist_case PUBLIC
expect_status zero
expect_published 'clean public push did not publish'

make_real_fixture denylist-private
commit_private_term
SAFE_PUSH_DENYLIST="$fixture_denylist" run_denylist_case PRIVATE
expect_status zero
expect_published 'PRIVATE destination push did not publish'

make_real_fixture denylist-absent
commit_private_term
run_denylist_case PUBLIC
expect_status zero
expect_published 'absent deny-list push did not publish'

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

wait_for() {
  local deadline=$((SECONDS + 15))
  until "$@"; do
    [ "$SECONDS" -lt "$deadline" ] || fail "handshake timed out: $*"
    sleep 0.01
  done
}

queued_lane() {
  local key
  key=$(printf '%s' "$landing_remote" | sha256sum | cut -c1-16)
  grep -Fqs -- "$1" "$scratch/xdg-state/safe-push/$key.queue/"* 2>/dev/null
}

hold_landing_lock() {
  local key
  key=$(printf '%s' "$landing_remote" | sha256sum | cut -c1-16)
  mkdir -p "$scratch/xdg-state/safe-push"
  mkfifo "$landing_root/lock-release"
  flock "$scratch/xdg-state/safe-push/$key.lock" bash -c \
    'touch "$1/lock-ready"; read -r _ <"$1/lock-release"' _ "$landing_root" & blocker=$!
  wait_for test -e "$landing_root/lock-ready"
}

release_landing_lock() {
  printf 'release\n' >"$landing_root/lock-release"
  wait "$blocker"
}

release_hook() {
  printf 'release\n' >"$1.hook-release"
}

finish_first_lane() {
  wait_for test -s "$landing_root/a.hook-runs"
  wait_for test -s "$landing_root/b.hook-runs"
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
      XDG_STATE_HOME="$scratch/xdg-state" \
      PATH="$real_bin:$PATH" \
      "$TARGET" --to "${landing_to:-main}"
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

# Concurrent landings from lanes cut at the same base serialize on the
# per-origin lock; the later one rebases onto the earlier, so both land in
# order with no non-fast-forward rejection.
new_landing_origin concurrent
make_landing_clone "$landing_root/a" a.txt
make_landing_clone "$landing_root/b" b.txt
landing_gate=1
mkfifo "$landing_root/a.scan-release" "$landing_root/b.scan-release"
run_landing "$landing_root/a" & pid_a=$!
wait_for test -d "$landing_root/a.scan-started"
run_landing "$landing_root/b" & pid_b=$!
wait_for grep -Fq 'waiting for the landing lock' "$landing_root/b.out"
printf 'release\n' >"$landing_root/a.scan-release"
status_a=0; wait "$pid_a" || status_a=$?
wait_for test -d "$landing_root/b.scan-started"
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
wait_for test -s "$landing_root/a.hook-runs"
wait_for test -s "$landing_root/b.hook-runs"
lock_key=$(printf '%s' "$landing_remote" | sha256sum | cut -c1-16)
flock -n "$scratch/xdg-state/safe-push/$lock_key.lock" true || fail 'checks held the landing lock'
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
wait_for test -s "$landing_root/a.hook-runs"
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
wait_for test -s "$landing_root/a.hook-runs"
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
wait_for test -s "$landing_root/a.hook-runs"
run_landing "$landing_root/b" & pid_b=$!
wait_for queued_lane "$landing_root/b"
release_hook "$landing_root/a"
status_a=0; wait "$pid_a" || status_a=$?
wait_for test -s "$landing_root/b.hook-runs"
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

run_train_case() {
  local lane
  : >"$landing_root/hook-runs"
  lock_key=$(printf '%s' "$landing_remote" | sha256sum | cut -c1-16)
  mkdir -p "$scratch/xdg-state/safe-push"
  hold_landing_lock
  train_pids=()
  for lane in "$@"; do
    add_train_hook "$landing_root/$lane"
    run_landing "$landing_root/$lane" & train_pids+=($!)
    wait_for queued_lane "$landing_root/$lane"
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
printf '#!/usr/bin/env bash\n[ "$*" = "install --frozen-lockfile" ] && cp bun.lock node_modules/installed.lock\n' >"$real_bin/bun"
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
mkdir -p "$scratch/xdg-state/safe-push"
hold_landing_lock
run_landing "$landing_root/a" & pid_a=$!
wait_for queued_lane "$landing_root/a"
run_landing "$landing_root/b" & pid_b=$!
wait_for queued_lane "$landing_root/b"
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
red_queue="$scratch/xdg-state/safe-push/$lock_key.queue"
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
mkdir -p "$scratch/xdg-state/safe-push"
hold_landing_lock
run_landing "$landing_root/h" & pid_h=$!
wait_for queued_lane "$landing_root/h"
run_landing "$landing_root/w" & pid_w=$!
wait_for queued_lane "$landing_root/w"
release_landing_lock
wait_for test -e "$landing_root/red-check"
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
lock_file="$scratch/xdg-state/safe-push/$(printf '%s' "$landing_remote" | sha256sum | cut -c1-16).lock"
run_landing "$landing_root/lane" & pid_lane=$!
wait_for test -s "$landing_root/hook-pid"
kill -TERM "$(sed -n 's/^pid \([0-9]*\) .*/\1/p' "$lock_file")"
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
mkdir -p "$scratch/xdg-state/safe-push"
hold_landing_lock
case_status=0
SAFE_PUSH_LOCK_HELD="$lock_key" timeout 15 bash -c "$(declare -f run_landing); real_bin='$real_bin' scratch='$scratch' TARGET='$TARGET' run_landing '$landing_root/lane'" || case_status=$?
release_landing_lock
case_output="$(cat "$landing_root/lane.out")"
expect_status zero

# While a main landing holds the lock, a scratch push to farm/* goes straight through.
new_landing_origin farm-scratch
make_landing_clone "$landing_root/lane" lane.txt
lock_key=$(printf '%s' "$landing_remote" | sha256sum | cut -c1-16)
mkdir -p "$scratch/xdg-state/safe-push"
hold_landing_lock
case_status=0
timeout 15 bash -c "$(declare -f run_landing); landing_to=farm/x real_bin='$real_bin' scratch='$scratch' TARGET='$TARGET' run_landing '$landing_root/lane'" || case_status=$?
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
wait_for test -s "$landing_root/lane.hook-runs"
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

run_lane() {
  printf "%s\n" run_lane >>"$scratch/cases"
  set +e
  case_output="$(
    cd "$lane"
    SAFE_PUSH_TEST_SHIM=1 \
      SAFE_PUSH_TEST_TRACE="$lane_container/trace" \
      SAFE_PUSH_TEST_STATE="$lane_container/state" \
      XDG_STATE_HOME="$scratch/xdg-state" \
      PATH="$real_bin:$PATH" \
      "$TARGET" --to main "$@" 2>&1
  )"
  case_status=$?
  set -e
}

lane_branch_exists() {
  "$real_git" -C "$lane_container/main" rev-parse -q --verify refs/heads/lane >/dev/null
}

remote_lane_exists() {
  "$real_git" --git-dir="$lane_remote" rev-parse -q --verify refs/heads/lane >/dev/null
}

new_lane_container clean
run_lane
expect_status zero
[ ! -e "$lane" ] || fail 'landed clean lane was not removed'
! lane_branch_exists || fail 'landed clean lane branch was not deleted'

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
[ ! -e "$lane" ] || fail 'landed lane with a merged remote branch was not removed'

new_lane_container remote-unmerged
"$real_git" -C "$lane" commit --allow-empty -qm 'only on the remote branch'
"$real_git" -C "$lane" push -q origin lane
"$real_git" -C "$lane" reset -q --hard HEAD~1
run_lane
expect_status zero
remote_lane_exists || fail 'remote lane branch with unlanded work was deleted'

printf 'safe-push tests: PASS (%s planning rows, %s boundary invocations)\n' "$property_rows" "$(wc -l <"$scratch/cases")"
