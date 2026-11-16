//! The scripts of the two canary probes that make a scratch project and ask its own broker for a verb (broker_start.rs): the runtime's canary runs them (north-runtime runtime/canary/probes.rs), and the broker's test replays them (offload_request_tests.rs).

use north_base::platform::session_scope::REMOVAL_ROOTS;

/// `script`, a probe that makes a scratch project under `$root` and asks for a verb that leaves a command's domain, with a broker of the judged runtime started for that project on a socket in `$root` when the probe runs inside a domain (an order issued from a session's window). The host's own broker serves only the projects the host keeps, so it refuses a scratch project, as it refused both probes of the order of 2026-11-12, and it runs the build being replaced. Outside a domain the verbs run in the probe's own process and no broker is asked.
pub fn own_broker(script: &str) -> String {
    let serving = super::broker_for("\"$root\"").join(" ");
    let started = OWN_BROKER.replace("__MARK__", north_base::platform::session_scope::SANDBOX_MARK).replace("__HIDDEN__", north_base::platform::session_scope::SOCKETS_HIDDEN).replace("__BROKER__", super::BROKER_SOCKET_ENV).replace("__SERVING__", &serving);
    script.replace("__OWN_BROKER__", &started)
}

const OWN_BROKER: &str = r#"if [ -n "${__MARK__:-}${__HIDDEN__:-}" ]; then
  export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
  export __BROKER__="$root/broker.sock"
  "$north" __SERVING__
fi
"#;

pub fn depbox_run(north: &str) -> String {
    own_broker(DEPBOX_RUN).replace("__NORTH__", north).replace("__ROOTS__", REMOVAL_ROOTS)
}

const DEPBOX_RUN: &str = r#"set -euo pipefail
north=__NORTH__
root=$(mktemp -d "${TMPDIR:-/tmp}/north-canary-depbox.XXXXXX")
trap 'rm -rf "$root"' EXIT
__OWN_BROKER__git init -q -b main "$root/canary/main"
cd "$root/canary/main"
checkout=$(pwd -P)
__ROOTS__="[\"$checkout\"]" "$north" depbox run -- true
"#;

/// A scratch repository whose lane queues one commit with the judged runtime and lands it through
/// that runtime's `land serve`, so a runtime that cannot walk the landing half of the spine fails
/// here instead of shipping to the other hosts.
pub fn scratch_landing(north: &str) -> String {
    own_broker(SCRATCH_LANDING).replace("__NORTH__", north)
}

const SCRATCH_LANDING: &str = r#"set -euo pipefail
north=__NORTH__
root=$(mktemp -d "${TMPDIR:-/tmp}/north-canary-landing.XXXXXX")
trap 'rm -rf "$root"' EXIT
__OWN_BROKER__state="$root/state"
mkdir -p "$state"
export XDG_STATE_HOME="$state"
export NORTH_TEST_ROOT="$state"
export THREADS_DB="$state/threads.db"
export NORTH_PUSH_LOG="$root/pushes.tsv"
export NORTH_FLEET_TEST=1
export GIT_CONFIG_GLOBAL="$root/gitconfig"
export GIT_CONFIG_NOSYSTEM=1
: > "$root/gitconfig"
export PATH="$(dirname "$north"):$PATH"
origin="$root/origin.git"
main="$root/project/main"
lane="$root/project/worktrees/canary-landing"
mkdir -p "$main"
git -C "$main" init -q -b main
git -C "$main" config user.name canary
git -C "$main" config user.email canary@example.invalid
git -C "$main" config commit.gpgsign false
printf 'the scratch repository the runtime canary lands into\n' > "$main/README.md"
printf '[landing]\n\tqueue = lander\n\tlander-host = %s\n\tremote-hosts =\n[land]\n\tlocal-only = the canary scratch check builds nothing and reads nothing of this host :: true\n\tcheck = ** :: true\n' "$(hostname -s 2>/dev/null || hostname)" > "$main/.safe-push"
git -C "$main" add .
git -C "$main" commit -q -m 'Start the scratch repository'
git clone -q --bare "$main" "$origin"
git -C "$main" remote add origin "$origin"
git -C "$main" fetch -q origin
git -C "$main" worktree add -q -b canary-landing "$lane" origin/main
base=$(git -C "$origin" rev-parse refs/heads/main)
printf 'the canary change\n' > "$lane/canary.txt"
git -C "$lane" add canary.txt
git -C "$lane" commit -q -m 'Land the canary change'
checker="$root/checker"
printf '#!/bin/sh\nprintf "safe-push: verdict green\\n"\nexit 0\n' > "$checker"
chmod +x "$checker"
token="$root/token"
printf '#!/bin/sh\nprintf "ghs_canary_landing\\n"\n' > "$token"
chmod +x "$token"
(cd "$lane" && "$north" safe-push --to main >/dev/null)
(cd "$main" && "$north" land serve --once --repo "$main" --checker "$checker" --app-token "$token")
landed=$(git -C "$origin" rev-parse refs/heads/main)
if [ "$landed" = "$base" ]; then
  echo "the scratch landing did not move origin main past $base" >&2
  exit 1
fi
changed=$(git -C "$origin" show --format= --name-only "$landed")
if ! grep -qx canary.txt <<<"$changed"; then
  echo "origin main $landed does not change canary.txt" >&2
  exit 1
fi
echo "landed $landed"
"#;
