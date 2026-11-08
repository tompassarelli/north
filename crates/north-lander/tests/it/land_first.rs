use std::fs;
use std::path::Path;

use north_lander::land::queue;

use super::serve::{Repository, World, git, report};

const COUNTED: &str = "#!/bin/sh\npwd >> \"$0.calls\"\necho 'safe-push: verdict green'\n";
const AFTER_RUNNER: &str = r#"#!/bin/sh
[ "$1" = land ] || exit 0
eval outcome=\${$#}
if [ -e bad.txt ]; then printf "event\tafter\tfailed\tbad.txt check\nfailed-step\tbad.txt check\t\n" > "$outcome"; exit 1; fi
"#;

fn checker(world: &World) -> std::path::PathBuf {
    let path = world.path().join("bin/checker");
    north_base::foundation::shim::write_executable(&path, COUNTED);
    world.path().join("bin/checker.calls")
}

fn calls(path: &Path) -> usize {
    fs::read_to_string(path).map_or(0, |text| text.lines().count())
}

fn serve(world: &World, north: &Repository) -> String {
    let output = world.lander(north, None).wait_with_output().unwrap();
    let text = report(&output);
    assert_eq!(output.status.code(), Some(0), "{text}");
    text
}

fn verdict(north: &Repository, branch: &str) -> String {
    let done = north.queue.join("done");
    fs::read_dir(&done).unwrap().flatten().map(|entry| queue::read(&entry.path()).unwrap()).find(|ticket| ticket.branch == branch).and_then(|ticket| ticket.verdict).unwrap_or_else(|| panic!("{branch} has no verdict in {}", done.display()))
}

fn subjects(north: &Repository) -> Vec<String> {
    git(&north.origin, &["log", "--format=%s", "main"]).lines().map(str::to_owned).collect()
}

// KEEP(bug): on 2026-11-08 the lander checked cumulative prefixes of the queue before every push, the queue p90 reached 38 min and main stood still for 40 min (north#902)
#[test]
fn three_queued_lanes_land_in_one_push_with_no_check_of_their_merged_tree_before_it() {
    let world = World::new();
    let north = world.repository("north");
    let calls_file = checker(&world);
    let start = git(&north.origin, &["rev-parse", "main"]);
    for branch in ["a", "b", "c"] {
        north.lane(branch);
    }
    let journal = serve(&world, &north);
    assert_eq!(git(&north.origin, &["rev-list", "--count", &format!("{start}..main")]), "3", "{journal}");
    assert_eq!(journal.matches("lanes that passed their checks before queueing").count(), 1, "{journal}");
    assert!(journal.contains("carries 3 lanes that passed their checks before queueing"), "{journal}");
    assert_eq!(calls(&calls_file), 0, "the lander checked a merged tree before the push: {journal}");
    for branch in ["a", "b", "c"] {
        assert!(verdict(&north, branch).starts_with("landed "), "{journal}");
    }
}

// KEEP(bug): with no merged-tree check before the push a lane that breaks the batch is found only by the after-landing run, which must still bisect the batch and revert exactly that lane (north#621, north#902)
#[test]
fn a_red_after_landing_run_of_a_batch_landed_without_a_merged_tree_check_reverts_exactly_the_culprit() {
    let world = World::new();
    let north = world.repository("north");
    north_base::foundation::shim::write_executable(&world.path().join("bin/north"), AFTER_RUNNER);
    north.lane("a");
    north.lane_with("bad", "origin/main", ("bad.txt", "bad\n"), ("bad (north#360)", None));
    north.lane("c");
    let journal = serve(&world, &north);
    assert!(journal.contains("carries 3 lanes that passed their checks before queueing"), "{journal}");
    let subjects = subjects(&north);
    assert_eq!(subjects.iter().filter(|subject| subject.starts_with("Revert ")).count(), 1, "{subjects:?}\n{journal}");
    assert_eq!(subjects[0], "Revert bad (north#360)", "{subjects:?}\n{journal}");
    let culprit = git(&north.origin, &["log", "--format=%H", "--grep=^bad (north#360)$", "main"]);
    assert!(git(&north.origin, &["log", "-1", "--format=%B", "main"]).contains(&format!("Reverts: {culprit}")), "{journal}");
    let names = git(&north.origin, &["ls-tree", "--name-only", "main"]);
    assert!(!names.lines().any(|name| name == "bad.txt") && names.lines().any(|name| name == "a.txt") && names.lines().any(|name| name == "c.txt"), "{names}\n{journal}");
}

const HELD_AFTER_RUNNER: &str = r#"#!/bin/sh
[ "$1" = land ] || exit 0
: > "$HOME/after-started"
while [ ! -e "$HOME/release" ]; do sleep 0.05; done
"#;

fn wait_for(what: &str, ready: impl Fn() -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while !ready() {
        assert!(std::time::Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

// KEEP(bug): on 2026-11-08 a lander round held main for 40 min while it checked merged trees; a push must wait for no earlier batch's checks, only for the lease on main (north#902)
#[test]
fn a_batch_queued_while_the_previous_batch_is_checked_after_landing_is_pushed_before_that_check_ends() {
    let world = World::new();
    let north = world.repository("north");
    north_base::foundation::shim::write_executable(&world.path().join("bin/north"), HELD_AFTER_RUNNER);
    let calls_file = checker(&world);
    north.lane("first");
    let mut lander = world.lander(&north, None);
    let landed = |file: &str| git(&north.origin, &["ls-tree", "--name-only", "main"]).lines().any(|name| name == file);
    wait_for("the first batch's after-landing run to start", || world.path().join("after-started").exists() && landed("first.txt"));
    north.lane("second");
    wait_for("the second batch to be pushed while the first batch's after-landing run is held", || landed("second.txt"));
    assert!(lander.try_wait().unwrap().is_none(), "the lander ended before the held run did");
    assert_eq!(calls(&calls_file), 0, "the second batch waited for a check of a merged tree before its push");
    fs::write(world.path().join("release"), "").unwrap();
    let output = lander.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(0), "{}", report(&output));
}
