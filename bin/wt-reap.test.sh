#!/usr/bin/env bash
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="$REPO/bin/wt-reap"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/wt-reap-test.XXXXXX")"
live_pid=''
trap 'if [ -n "${live_pid:-}" ]; then kill "$live_pid" 2>/dev/null || true; fi; rm -rf "${scratch:?}"' EXIT

export GIT_AUTHOR_NAME=wt-reap-test
export GIT_AUTHOR_EMAIL=wt-reap-test@example.invalid
export GIT_COMMITTER_NAME=wt-reap-test
export GIT_COMMITTER_EMAIL=wt-reap-test@example.invalid

pass=0
fail_count=0

check() { # check <description> <0|1 truth>
  if [ "$2" -eq 0 ]; then
    printf 'PASS: %s\n' "$1"
    pass=$((pass + 1))
  else
    printf 'FAIL: %s\n' "$1" >&2
    fail_count=$((fail_count + 1))
  fi
}

assert_ok() { # assert_ok <description> <command...>
  local desc="$1"; shift
  if "$@" >/dev/null 2>&1; then check "$desc" 0; else check "$desc" 1; fi
}

assert_not_ok() {
  local desc="$1"; shift
  if "$@" >/dev/null 2>&1; then check "$desc" 1; else check "$desc" 0; fi
}

aged() { # a worktree older than wt-reap's 2h freshness gate
  touch -h -d '3 hours ago' "$1/.git"
}

# --- fixture: one container, one lane per case -----------------------------
# Layout under test: <container>/main, <container>/worktrees/<slug> (sweepable),
# <container>/pins/<full-object-id> (externally consumed, never swept).
container="$scratch/proj"
mkdir -p "$container"
# git records resolved paths in `worktree list`; compare against the same form.
container="$(cd "$container" && pwd -P)"
git init -q -b main "$container/main"
printf 'base\n' >"$container/main/f.txt"
# every lane inherits these ignores; the ignored-file gate is tested through them
printf 'docs/private/\n.direnv/\nresult\nresult-*\n' >"$container/main/.gitignore"
git -C "$container/main" add f.txt .gitignore
git -C "$container/main" commit -qm base

# 1. fresh + clean + merged -> kept. This is the race guard for a just-created
# worktree whose HEAD starts at main.
git -C "$container/main" worktree add -q -b merged-fresh "$container/worktrees/merged-fresh" main

# 2. aged + clean + merged -> reaped (the ordinary ancestor path).
git -C "$container/main" worktree add -q -b merged-aged "$container/worktrees/merged-aged" main
aged "$container/worktrees/merged-aged"

# 3-5. Aged ordinary-merged lanes are still protected by rescue, lock, and a
# live CWD, exactly as their patch-equivalent counterparts are.
git -C "$container/main" worktree add -q -b rescue-merged "$container/worktrees/rescue-merged" main
aged "$container/worktrees/rescue-merged"
git -C "$container/main" worktree add -q -b merged-locked "$container/worktrees/merged-locked" main
aged "$container/worktrees/merged-locked"
git -C "$container/main" worktree lock "$container/worktrees/merged-locked"
git -C "$container/main" worktree add -q -b merged-live "$container/worktrees/merged-live" main
aged "$container/worktrees/merged-live"
(cd "$container/worktrees/merged-live" && exec sleep 60) >/dev/null 2>&1 &
live_pid=$!

# The intervening commit is load-bearing: cherry-picking straight onto the
# branch's own parent reproduces a byte-identical sha, i.e. a plain ancestor.
land_equivalent() { # land <branch>'s commit onto main under a different sha
  local branch="$1" sha
  sha=$(git -C "$container/main" rev-parse "$branch")
  printf '%s landed\n' "$branch" >>"$container/main/log.txt"
  git -C "$container/main" add log.txt
  git -C "$container/main" commit -qm "main advances before $branch lands"
  git -C "$container/main" cherry-pick "$sha" >/dev/null
}

# 6. patch-equivalent + aged -> reaped (equiv)
git -C "$container/main" worktree add -q -b equiv-aged "$container/worktrees/equiv-aged" main
printf 'equiv-aged payload\n' >"$container/worktrees/equiv-aged/equiv-aged.txt"
git -C "$container/worktrees/equiv-aged" add equiv-aged.txt
git -C "$container/worktrees/equiv-aged" commit -qm 'equiv-aged work'
equiv_aged_sha=$(git -C "$container/main" rev-parse equiv-aged)
land_equivalent equiv-aged
aged "$container/worktrees/equiv-aged"

# --- case 9 first: --dry-run must not disturb the fixture ------------------
dry_output="$(cd "$container" && "$TARGET" --dry-run 2>&1)"
dry_status=$?
check "--dry-run exits 0" "$dry_status"
[ -d "$container/worktrees/equiv-aged" ] \
  && check "--dry-run left the worktree in place" 0 \
  || check "--dry-run left the worktree in place" 1
assert_ok "--dry-run left the branch in place" \
  git -C "$container/main" rev-parse --verify equiv-aged
[ -d "$container/worktrees/merged-aged" ] \
  && check "--dry-run left the aged merged worktree in place" 0 \
  || check "--dry-run left the aged merged worktree in place" 1

# 8. dirty tracked change -> kept
git -C "$container/main" worktree add -q -b dirty-branch "$container/worktrees/dirty" main
printf 'uncommitted\n' >>"$container/worktrees/dirty/f.txt"
aged "$container/worktrees/dirty"

# 9. clean tracked, but an untracked file present -> kept
git -C "$container/main" worktree add -q -b untracked-branch "$container/worktrees/untracked" main
printf 'untracked payload\n' >"$container/worktrees/untracked/scratch.txt"
aged "$container/worktrees/untracked"

# 11. genuinely unique commit -> kept
git -C "$container/main" worktree add -q -b unmerged-branch "$container/worktrees/unmerged" main
printf 'unique\n' >"$container/worktrees/unmerged/unique.txt"
git -C "$container/worktrees/unmerged" add unique.txt
git -C "$container/worktrees/unmerged" commit -qm 'unique work'
aged "$container/worktrees/unmerged"

# 14. equiv + aged, but carrying ignored files `git status` never shows.
#     Disposable caches alongside them must not be what holds the tree.
git -C "$container/main" worktree add -q -b equiv-ignored "$container/worktrees/equiv-ignored" main
printf 'equiv-ignored payload\n' >"$container/worktrees/equiv-ignored/equiv-ignored.txt"
git -C "$container/worktrees/equiv-ignored" add equiv-ignored.txt
git -C "$container/worktrees/equiv-ignored" commit -qm 'equiv-ignored work'
equiv_ignored_sha=$(git -C "$container/main" rev-parse equiv-ignored)
land_equivalent equiv-ignored
mkdir -p "$container/worktrees/equiv-ignored/docs/private" \
         "$container/worktrees/equiv-ignored/.direnv/bin" \
         "$container/worktrees/equiv-ignored/sub/.direnv"
printf 'handoff notes\n' >"$container/worktrees/equiv-ignored/docs/private/note.md"
printf 'cache\n' >"$container/worktrees/equiv-ignored/.direnv/bin/x"
printf 'cache\n' >"$container/worktrees/equiv-ignored/sub/.direnv/y"
ln -s /nix/store/nonexistent "$container/worktrees/equiv-ignored/result"
ln -s /nix/store/nonexistent "$container/worktrees/equiv-ignored/result-2"
aged "$container/worktrees/equiv-ignored"

# 15. every non-merge commit is cherry-equivalent, but the lane also carries a
#     merge whose conflict resolution exists nowhere else. `git cherry` is blind
#     to merges, so only the merge gate can keep this tree.
merge_lane_base=$(git -C "$container/main" rev-parse main)
git -C "$container/main" worktree add -q -b merge-lane "$container/worktrees/merge-lane" main
# merge-side edits the same file differently, so merging it conflicts
git -C "$container/worktrees/merge-lane" checkout -q -b merge-side
printf 'side version\n' >"$container/worktrees/merge-lane/f.txt"
git -C "$container/worktrees/merge-lane" commit -qam 'side work'
git -C "$container/worktrees/merge-lane" checkout -q merge-lane
printf 'merge-lane payload\n' >"$container/worktrees/merge-lane/merge-lane.txt"
printf 'lane version\n' >"$container/worktrees/merge-lane/f.txt"
git -C "$container/worktrees/merge-lane" add merge-lane.txt f.txt
git -C "$container/worktrees/merge-lane" commit -qm 'merge-lane work'
# land both patches on main, restoring f.txt between them so each applies clean
land_equivalent merge-lane
git -C "$container/main" show "$merge_lane_base:f.txt" >"$container/main/f.txt"
git -C "$container/main" commit -qam 'main restores f.txt'
git -C "$container/main" cherry-pick "$(git -C "$container/main" rev-parse merge-side)" >/dev/null
git -C "$container/worktrees/merge-lane" merge --no-ff --no-edit merge-side >/dev/null 2>&1 || true
printf 'evil resolution found nowhere else\n' >"$container/worktrees/merge-lane/f.txt"
git -C "$container/worktrees/merge-lane" add f.txt
git -C "$container/worktrees/merge-lane" commit -qm 'merge merge-side (evil resolution)'
aged "$container/worktrees/merge-lane"

# 16. a tag whose name equals the lane's branch, pointing at main's tip: bare
#     names resolve tags first, so an unqualified cherry would read "landed".
git -C "$container/main" worktree add -q -b tag-shadow "$container/worktrees/tag-shadow" main
printf 'tag-shadow unique\n' >"$container/worktrees/tag-shadow/tag-shadow.txt"
git -C "$container/worktrees/tag-shadow" add tag-shadow.txt
git -C "$container/worktrees/tag-shadow" commit -qm 'tag-shadow work'
git -C "$container/main" tag tag-shadow main
aged "$container/worktrees/tag-shadow"

# 17. a pin: clean, merged, detached, and aged — the exact shape the live pins
#     have. Only its parent directory keeps it alive.
pin_oid="$(git -C "$container/main" rev-parse HEAD)"
git -C "$container/main" worktree add -q --detach "$container/pins/$pin_oid" "$pin_oid"
aged "$container/pins/$pin_oid"
printf 'Consumers: this test.\n' >"$container/pins/$pin_oid.pin"

# 18. the identical shape one directory over, under worktrees/ -> reaped.
#     Together with 17 this isolates the parent directory as the whole rule.
git -C "$container/main" worktree add -q --detach "$container/worktrees/detached-aged" main
aged "$container/worktrees/detached-aged"

# --- the real sweep --------------------------------------------------------
set +e
output="$(cd "$container" && "$TARGET" 2>&1)"
status=$?
set -e
kill "$live_pid" 2>/dev/null || true
wait "$live_pid" 2>/dev/null || true
live_pid=''
check "wt-reap exits 0" "$status"

# 1. fresh merged tree is held before the ancestor reap path
if [ -d "$container/worktrees/merged-fresh" ]; then
  check "fresh merged worktree kept" 0
else
  check "fresh merged worktree kept" 1
fi

# 2. aged merged + clean
[ ! -d "$container/worktrees/merged-aged" ] \
  && check "aged merged+clean worktree reaped" 0 \
  || check "aged merged+clean worktree reaped" 1
assert_not_ok "merged-aged branch deleted" \
  git -C "$container/main" rev-parse --verify merged-aged

# 3-5. ordinary merged protections
for protected in rescue-merged merged-locked merged-live; do
  if [ -d "$container/worktrees/$protected" ]; then
    check "aged $protected worktree kept" 0
  else
    check "aged $protected worktree kept" 1
  fi
done
# 6. patch-equivalent + aged
[ ! -d "$container/worktrees/equiv-aged" ] \
  && check "equivalent+aged worktree reaped" 0 \
  || check "equivalent+aged worktree reaped" 1
assert_not_ok "equiv-aged branch deleted" \
  git -C "$container/main" rev-parse --verify equiv-aged
assert_ok "reaped equiv tip sha still resolvable (recoverable)" \
  git -C "$container/main" cat-file -e "${equiv_aged_sha}^{commit}"

# 8. dirty tracked change
[ -d "$container/worktrees/dirty" ] \
  && check "dirty worktree kept" 0 \
  || check "dirty worktree kept" 1
assert_ok "dirty-branch not deleted" \
  git -C "$container/main" rev-parse --verify dirty-branch

# 9. untracked file present
[ -d "$container/worktrees/untracked" ] \
  && check "untracked-only worktree kept" 0 \
  || check "untracked-only worktree kept" 1

# 11. genuinely unique commit
[ -d "$container/worktrees/unmerged" ] \
  && check "unmerged worktree kept" 0 \
  || check "unmerged worktree kept" 1
assert_ok "unmerged-branch not deleted" \
  git -C "$container/main" rev-parse --verify unmerged-branch

# 14. equiv + aged, holding an ignored file
[ -d "$container/worktrees/equiv-ignored" ] \
  && check "equivalent lane with ignored files kept" 0 \
  || check "equivalent lane with ignored files kept" 1
assert_ok "equiv-ignored branch not deleted" \
  git -C "$container/main" rev-parse --verify equiv-ignored

# 15. cherry-equivalent commits plus an evil merge
merge_lane_cherry_plus() {
  git -C "$container/main" cherry refs/heads/main refs/heads/merge-lane | grep -q '^+'
}
assert_not_ok "merge-lane's non-merge commits are all cherry-equivalent (isolates the merge gate)" \
  merge_lane_cherry_plus
merge_lane_merges=$(git -C "$container/main" rev-list --merges refs/heads/main..refs/heads/merge-lane 2>/dev/null) \
  || merge_lane_merges=""
[ -n "$merge_lane_merges" ] \
  && check "merge-lane is ahead by a merge commit" 0 \
  || check "merge-lane is ahead by a merge commit" 1
[ -d "$container/worktrees/merge-lane" ] \
  && check "lane carrying a merge commit kept" 0 \
  || check "lane carrying a merge commit kept" 1
assert_ok "merge-lane branch not deleted" \
  git -C "$container/main" rev-parse --verify refs/heads/merge-lane

# 16. tag shadowing the lane's branch name
[ -d "$container/worktrees/tag-shadow" ] \
  && check "tag-shadowed lane kept" 0 \
  || check "tag-shadowed lane kept" 1
assert_ok "tag-shadow branch not deleted" \
  git -C "$container/main" rev-parse --verify refs/heads/tag-shadow
assert_ok "tag-shadow tag still present" \
  git -C "$container/main" rev-parse --verify refs/tags/tag-shadow

# 17. the pin is not reaped, and is never even enumerated as a candidate
[ -d "$container/pins/$pin_oid" ] \
  && check "clean+merged+detached+aged tree under pins/ not reaped" 0 \
  || check "clean+merged+detached+aged tree under pins/ not reaped" 1
if printf '%s\n%s\n' "$output" "$dry_output" | grep -Fq "$container/pins/$pin_oid"; then
  printf '%s\n' "$output" >&2
  check "pin never appears in wt-reap output, not even as a keep" 1
else
  check "pin never appears in wt-reap output, not even as a keep" 0
fi
[ -f "$container/pins/$pin_oid.pin" ] \
  && check "pin manifest untouched" 0 \
  || check "pin manifest untouched" 1

# 18. the same shape under worktrees/ is reaped — the parent is the rule
[ ! -d "$container/worktrees/detached-aged" ] \
  && check "clean+merged+detached+aged lane under worktrees/ reaped" 0 \
  || check "clean+merged+detached+aged lane under worktrees/ reaped" 1

# --- second sweep: only the irreplaceable ignored file was holding case 10 --
rm "$container/worktrees/equiv-ignored/docs/private/note.md"
rmdir "$container/worktrees/equiv-ignored/docs/private" "$container/worktrees/equiv-ignored/docs"
second="$(cd "$container" && "$TARGET" 2>&1)"
[ ! -d "$container/worktrees/equiv-ignored" ] \
  && check "disposable ignored files alone do not block the reap" 0 \
  || check "disposable ignored files alone do not block the reap" 1
assert_not_ok "equiv-ignored branch deleted once the notes were gone" \
  git -C "$container/main" rev-parse --verify equiv-ignored

property_target="$TARGET"
(
  source "$property_target"
  seed=317
  for (( trial=0; trial<64; trial++ )); do
    seed=$(( (1103515245 * seed + 12345) & 2147483647 ))
    minima=(0 2 24 72)
    archives=(-1 24 72)
    minimum=${minima[$((seed % 4))]}
    archive=${archives[$(((seed >> 3) % 3))]}
    ages=(-1 0 "$((minimum - 1))" "$minimum" "$((minimum + 1))" "$((archive - 1))" "$archive" "$((archive + 1))" 100)
    age=${ages[$((trial % 9))]}
    merged=$(((seed >> 7) & 1))
    equivalent=$(((seed >> 8) & 1))
    evil=$(((seed >> 9) & 1))
    protections=$(((seed >> 10) & 3))
    retirement_verdict actual "$age" "$minimum" "$merged" "$equivalent" "$evil" "$archive" "$protections"
    admissible=0
    if (( protections == 0 && age >= minimum )); then
      if (( merged || (equivalent && !evil) || (archive >= 0 && age >= archive) )); then admissible=1; fi
    fi
    if [[ "$actual" == keep ]]; then
      (( admissible == 0 )) || { echo "FAIL completeness seed=$seed trial=$trial" >&2; exit 1; }
    else
      (( admissible == 1 )) || { echo "FAIL retirement gates seed=$seed trial=$trial" >&2; exit 1; }
    fi
    case "$actual" in
      keep) ;;
      retire) (( merged )) || exit 1 ;;
      retire-equiv) (( equivalent && !evil && !merged )) || { echo "FAIL force-delete seed=$seed trial=$trial" >&2; exit 1; } ;;
      archive-then-delete) (( !merged && !(equivalent && !evil) && archive >= 0 && age >= archive )) || exit 1 ;;
      *) echo "FAIL unknown verdict $actual" >&2; exit 1 ;;
    esac
    for (( protection=0; protection<17; protection++ )); do
      retirement_verdict protected "$age" "$minimum" "$merged" "$equivalent" "$evil" "$archive" "$((protections | (1 << protection)))"
      [[ "$protected" == keep ]] || { echo "FAIL protection=$protection seed=$seed trial=$trial from=$actual to=$protected" >&2; exit 1; }
    done
  done
)
check "retirement gates, protection monotonicity and force-delete proof (seed 317; 64 facts; 17 protections)" "$?"

# --- --all sweeps every ~/code/*/main-shaped container ----------------------
all_root="$scratch/codehome"
mkdir -p "$all_root/code/projA"
git init -q -b main "$all_root/code/projA/main"
printf 'a\n' >"$all_root/code/projA/main/f.txt"
git -C "$all_root/code/projA/main" add f.txt
git -C "$all_root/code/projA/main" commit -qm base
git -C "$all_root/code/projA/main" worktree add -q -b done-branch "$all_root/code/projA/worktrees/done" main
git -C "$all_root/code/projA/main" merge -q done-branch >/dev/null
aged "$all_root/code/projA/worktrees/done"

# --- the container walk-up still works from deep inside a lane -------------
mkdir -p "$all_root/code/projA/worktrees/done/sub/dir"
set +e
deep_output="$(cd "$all_root/code/projA/worktrees/done/sub/dir" && "$TARGET" --dry-run 2>&1)"
deep_status=$?
set -e
check "wt-reap run from worktrees/<slug>/sub/dir exits 0" "$deep_status"
if grep -Fq 'no container found' <<<"$deep_output"; then
  printf '%s\n' "$deep_output" >&2
  check "wt-reap finds the container from inside a lane subdirectory" 1
else
  check "wt-reap finds the container from inside a lane subdirectory" 0
fi

all_output="$(HOME="$all_root" bash -c 'cd "$1" && exec "$2" --all' _ "$all_root" "$TARGET" 2>&1)"
if grep -Fq 'reaped: ' <<<"$all_output"; then
  check "--all reaped the fixture" 0
else
  printf '%s\n' "$all_output" >&2
  check "--all reaped the fixture" 1
fi
[ ! -d "$all_root/code/projA/worktrees/done" ] \
  && check "--all removed the merged worktree" 0 \
  || check "--all removed the merged worktree" 1

if [ "$fail_count" -eq 0 ]; then
  printf 'wt-reap tests: PASS (%d checks)\n' "$pass"
else
  printf 'wt-reap tests: FAIL (%d passed, %d failed)\n' "$pass" "$fail_count" >&2
  exit 1
fi
