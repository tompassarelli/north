git() { command git -c core.hooksPath=/dev/null -c core.fsmonitor=false -c gc.auto=0 "$@"; }
fill() {
  if ! git -C "$repo" bundle verify "$bundle" >/dev/null 2>&1; then
    git -C "$repo" fetch -q --no-auto-maintenance origin 2>/dev/null || true
    git -C "$repo" bundle verify "$bundle" >/dev/null 2>&1 || { echo "offload: $repo lacks the commits under $base" >&2; exit 97; }
  fi
  git -C "$repo" fetch -q --no-auto-maintenance "$bundle" "+$ref:$ref"
  git -C "$repo" update-ref "refs/offload-held/$name" "$head"
}
place() {
  if [ -e "$slot/.git" ]; then git -C "$slot" checkout -q --detach -f "$head"; else git -C "$repo" worktree remove --force "$slot" 2>/dev/null || true; git -C "$repo" worktree add -q --detach "$slot" "$head"; fi
  git -C "$slot" clean -ffdxq -e /target
}
confine() {
  common=$(git rev-parse --path-format=absolute --git-common-dir) && own=$(git rev-parse --absolute-git-dir) || return 1
  command -v bwrap >/dev/null 2>&1 || { echo "offload: refusing to run: bwrap is missing, so the check would run with the shared git directory writable" >&2; return 1; }
  touch "$HOME/.gitconfig" && mkdir -p "$HOME/.config/git" || return 1
  bwrap --dev-bind / / --ro-bind "$common" "$common" --bind "$own" "$own" --ro-bind "$HOME/.gitconfig" "$HOME/.gitconfig" --ro-bind "$HOME/.config/git" "$HOME/.config/git" --die-with-parent -- "$@"
}
