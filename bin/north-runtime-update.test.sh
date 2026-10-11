#!/usr/bin/env bash
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
updater="$repo/bin/north-runtime-update"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/north-runtime-update-test.XXXXXX")"
trap 'rm -rf -- "${scratch:?}"' EXIT

export HOME="$scratch/home"
unset XDG_STATE_HOME CARGO_TARGET_DIR
stubs="$scratch/bin"
state="$HOME/.local/state/north"
mkdir -p "$stubs" "$HOME/code/north" "$state/runtimes"
export PATH="$stubs:$PATH"
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@example.invalid GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@example.invalid

cat >"$stubs/nix" <<'EOF'
#!/usr/bin/env bash
[[ "$1 $2" == "develop -c" ]] || exit 90
shift 2
exec "$@"
EOF
cat >"$stubs/cargo" <<'EOF'
#!/usr/bin/env bash
[[ "$*" == "build --release --locked" ]] || exit 91
mkdir -p "$CARGO_TARGET_DIR/release"
printf '#!/bin/sh\necho %s\n' "$(git rev-parse HEAD)" >"$CARGO_TARGET_DIR/release/north"
chmod +x "$CARGO_TARGET_DIR/release/north"
echo build >>"$HOME/builds"
EOF
cat >"$stubs/agents" <<'EOF'
#!/usr/bin/env bash
case "$1" in
  sync) echo sync >>"$HOME/syncs"; [[ ! -e "$HOME/fail-sync" ]] ;;
  *) exit 1 ;;
esac
EOF
chmod +x "$stubs"/*

fail() { printf 'FAIL: %s\n' "$*" >&2; exit 1; }
current() { readlink "$state/current"; }
fake() { printf 'a%039d' "$1"; }

git init -q --bare "$scratch/origin.git"
git clone -q "$scratch/origin.git" "$HOME/code/north/main" 2>/dev/null
cd "$HOME/code/north/main"
commit() { echo "$1" >f; git add f; git commit -qm "$1"; git push -q origin HEAD:main; git rev-parse HEAD; }
c1="$(commit one)"

mkdir -p "$state/runtimes/$c1"
ln -s "runtimes/$c1" "$state/current"
out="$("$updater")"
[[ "$out" == "north runtime current: ${c1:0:8}" ]] || fail "no-op printed: $out"
[[ ! -e "$HOME/builds" && ! -e "$HOME/syncs" ]] || fail "no-op built or synced"

for i in 1 2 3 4 5; do
  old="$(fake "$i")"
  mkdir -p "$state/runtimes/$old"
  touch -d "2026-01-0$i" "$state/runtimes/$old"
done
touch -d 2025-01-01 "$state/runtimes/$c1"
c2="$(commit two)"
"$updater" >/dev/null
[[ "$(current)" == "runtimes/$c2" ]] || fail "not promoted: $(current)"
[[ "$("$state/current/north")" == "$c2" ]] || fail "installed binary is not $c2"
[[ ! -e "$HOME/code/north/worktrees/runtime-${c2:0:8}" ]] || fail "temporary worktree left behind"
[[ -z "$(git -C "$HOME/code/north/main" worktree list | sed 1d)" ]] || fail "worktree still registered"
[[ "$(git -C "$HOME/code/north/main" rev-parse HEAD)" == "$c2" ]] || fail "main checkout moved"
left="$(find "$state/runtimes" -mindepth 1 -maxdepth 1 -type d -printf '%f\n' | sort)"
expected="$(printf '%s\n' "$c2" "$c1" "$(fake 3)" "$(fake 4)" "$(fake 5)" | sort)"
[[ "$left" == "$expected" ]] || fail "pruning kept: $left"

c3="$(commit three)"
touch "$HOME/fail-sync"
if "$updater" >/dev/null 2>"$scratch/err"; then fail "sync failure exited 0"; fi
[[ "$(current)" == "runtimes/$c2" ]] || fail "not rolled back: $(current)"
grep -q 'agents sync FAILED' "$scratch/err" || fail "failure not reported"
rm "$HOME/fail-sync"

exec 8>"$state/runtime-update.lock"
flock 8
builds_before="$(wc -l <"$HOME/builds")"
out="$("$updater")"
flock -u 8
[[ "$out" == "north runtime update already running" ]] || fail "lock not honored: $out"
[[ "$(current)" == "runtimes/$c2" && "$(wc -l <"$HOME/builds")" == "$builds_before" ]] || fail "ran under a held lock"

"$updater" >/dev/null
[[ "$(current)" == "runtimes/$c3" ]] || fail "retry after rollback did not promote"

echo "north-runtime-update tests passed"
