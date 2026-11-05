use std::path::PathBuf;

use super::{Fixture, executable, git};

const CHOOSER: &str = r#"#!/usr/bin/env bash
if [ "$1 $2 $3" = "capacity offload choose" ]; then
  shift 4
  words=" $* "
  for host in stub-a stub-b; do
    case "$words" in *" --avoid $host "*) continue ;; esac
    echo "$host"
    exit 0
  done
  echo "north capacity offload: no host has room: every host is avoided" >&2
  exit 1
fi
exit 0
"#;

const UNFIT_ON_STUB_A: &str = r#"#!/usr/bin/env bash
lane=$(command git log -1 --format=%s); lane=${lane##* }
printf '%s %s\n' "$lane" "${SAFE_PUSH_LAND_RUNNER-unset}" >>"$LAND_TRACE/runners"
[ "${SAFE_PUSH_LAND_RUNNER:-}" = stub-a ] || exit 0
case "$lane" in
  crate) printf 'error: failed to download `ahash v0.8.12`\n\nCaused by:\n  attempting to make an HTTP request, but --offline was specified\nsafe-push: land check cargo clippy exited 101; NOT pushed.\n' ;;
  path) printf 'bash: line 1: north: command not found\nsafe-push: land check check-owners exited 127; NOT pushed.\n' ;;
  ship) printf 'north capacity offload: shipping the lander runtime 93b443a5742a to tom@cx53: rsync failed: mkdir failed: File exists (17)\nsafe-push: land check check-owners exited 255; NOT pushed.\n' ;;
esac
echo refused >"$LAND_TRACE/verdict-$1"
"#;

#[test]
fn a_check_that_fails_only_because_its_host_is_unfit_lands_on_the_next_host_with_no_refusal() {
    let mut fixture = Fixture::new();
    git(&fixture.repo, &["config", "landing.slots", "1"]);
    git(&fixture.repo, &["config", "land.runner", "build-host"]);
    executable(&fixture.trace.join("north-hook"), CHOOSER);
    executable(&fixture.trace.join("hook"), UNFIT_ON_STUB_A);
    let names = ["crate", "path", "ship"];
    let tickets: Vec<PathBuf> = names
        .iter()
        .map(|name| {
            let lane = fixture.lane(name, &format!("{name}.txt"), name);
            fixture.enqueue(&lane, None)
        })
        .collect();
    let output = fixture.serve();
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(output.status.success(), "{stdout}");
    for (ticket, name) in tickets.iter().zip(names) {
        assert!(fixture.verdict(ticket).is_some_and(|verdict| verdict.starts_with("landed ")), "lane {name} has no landed verdict: {stdout}");
        assert!(stdout.contains(&format!("north land: lane-{name}'s check host stub-a is unfit, so the lane is not refused and is checked again on another host: infrastructure: the check host is unfit: ")), "{stdout}");
    }
    assert_eq!(fixture.read("runners").lines().collect::<Vec<_>>(), ["crate stub-a", "crate stub-b", "path stub-a", "path stub-b", "ship stub-a", "ship stub-b"], "each lane's check runs again on the host it did not fail on");
    assert_eq!(fixture.calls("capacity offload choose").iter().filter(|call| call.ends_with("--avoid stub-a")).count(), 3, "each retry leaves the unfit host out: {:?}", fixture.calls("capacity offload choose"));
    assert!(!stdout.contains("refused alone"), "{stdout}");
    assert!(!fixture.fleet().iter().any(|call| call.contains("did not land")), "{:?}", fixture.fleet());
    assert_eq!(fixture.subjects(), ["change from crate", "change from path", "change from ship"]);
}
