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
confined_here() {
  [ -z "${confined:-}" ] || return 0
  [ -e /etc/north/operator ] || return 1
  [ -r /etc/north/operator ] || return 0
  [ "$(cat /etc/north/operator)" != "$(id -un)" ]
}
confine() {
  slot_top=$(git rev-parse --show-toplevel) && common=$(git rev-parse --path-format=absolute --git-common-dir) || return 1
  command -v bwrap >/dev/null 2>&1 || { echo "offload: refusing to run: bwrap is missing, so the check would run with the home and the shared git directory writable" >&2; return 1; }
  own=$slot_top.own runtime=/run/user/$(id -u)
  chmod -R u+rwx "$own" 2>/dev/null; rm -rf "$own" && mkdir -p "$own/tmp" "$own/run" || return 1
  set -- -- "$@"
  [ ! -d "$runtime/agent-capacity-v1" ] || set -- --ro-bind "$runtime/agent-capacity-v1" "$runtime/agent-capacity-v1" "$@"
  for cache in .cargo .cache/sccache .cache/nix .local/state/nix; do
    mkdir -p "$HOME/$cache" "$own/upper/$cache" "$own/work/$cache" || return 1
    set -- --overlay-src "$HOME/$cache" --overlay "$own/upper/$cache" "$own/work/$cache" "$HOME/$cache" "$@"
  done
  status=0
  bwrap --dev-bind / / --unshare-pid --proc /proc --die-with-parent --ro-bind "$HOME" "$HOME" --ro-bind "$common" "$common" --bind "$slot_top" "$slot_top" --ro-bind "$slot_top/.git" "$slot_top/.git" --bind "$own/tmp" /tmp --bind "$own/run" "$runtime" "$@" || status=$?
  chmod -R u+rwx "$own" 2>/dev/null; rm -rf "$own"
  return "$status"
}
