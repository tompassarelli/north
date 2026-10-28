#!/usr/bin/env bash
# Passes only when the family's own check, read from its workflow step or .safe-push line, fails on the planted fault.
set -euo pipefail
family=${1:?usage: canary.sh FAMILY}
root=$(git rev-parse --show-toplevel)
conf="$root/harness/canaries/canaries.conf"
patch="$root/harness/canaries/$family.patch"
source=$(git config -f "$conf" --get "canary.$family.check") || { echo "canary: no canary.$family.check in $conf" >&2; exit 2; }
expect=$(git config -f "$conf" --get "canary.$family.expect") || { echo "canary: no canary.$family.expect in $conf" >&2; exit 2; }
[ -s "$patch" ] || { echo "canary: missing $patch" >&2; exit 2; }
read -r kind rest <<<"$source"
dir=. step_env=() shell=(bash -c)
case "$kind" in
  workflow)
    read -r workflow job step <<<"$rest"
    query=".jobs.\"$job\".steps[] | select(.name == \"$step\")"
    command=$(yq -r "$query | .run // \"\"" "$root/.github/workflows/$workflow")
    dir=$(yq -r "$query | .\"working-directory\" // \".\"" "$root/.github/workflows/$workflow")
    mapfile -t step_env < <(yq -r "$query | .env // {} | to_entries[] | .key + \"=\" + .value" "$root/.github/workflows/$workflow")
    shell=(bash -e -c)
    ;;
  land)
    command=$(git config -f "$root/.safe-push" --get-all land.check | grep -F -- "$rest" | head -n 1 || true)
    command=${command#* :: }
    command=$(sed -E 's#(\./bin/)?depbox run -- ##g; s#nix develop -c ##g' <<<"$command")
    ;;
  *) echo "canary: unknown check source '$source'" >&2; exit 2 ;;
esac
[ -n "$command" ] || { echo "canary $family: check '$source' not found; the family no longer runs" >&2; exit 1; }
tree=$root
if [ "${GITHUB_ACTIONS:-}" = true ]; then
  trap 'git -C "$root" apply -R "$patch"' EXIT
else
  tree=$(mktemp -d "${TMPDIR:-/tmp}/canary-$family.XXXXXX")
  trap 'git -C "$root" worktree remove --force "$tree" >/dev/null 2>&1 || true' EXIT
  git -C "$root" worktree add --detach --quiet "$tree" HEAD
  while IFS= read -r dep; do ln -s "$root/$dep" "$tree/$dep"; done < <(fd -t d -H --no-ignore --prune -d 3 "^(target|node_modules)\$" --base-directory "$root" | sed "s#/\$##")
fi
git -C "$tree" apply "$patch"
log=$(mktemp)
export RUNNER_TEMP=${RUNNER_TEMP:-$(mktemp -d)}
printf 'canary %s: %s (in %s)\n' "$family" "$command" "$dir"
status=0
(cd "$tree/$dir" && env "${step_env[@]//\$\{\{ github.workspace \}\}/$tree}" "${shell[@]}" "$command") >"$log" 2>&1 || status=$?
if [ "$status" = 0 ]; then
  tail -n 40 "$log"
  echo "::error::canary $family: the planted fault passed '$source'; that check no longer checks"
  exit 1
fi
if ! grep -qF -- "$expect" "$log"; then
  tail -n 80 "$log"
  echo "::error::canary $family: '$source' failed (exit $status) without '$expect'; it failed for another reason"
  exit 1
fi
printf 'canary %s: caught (exit %s): %s\n' "$family" "$status" "$expect"
