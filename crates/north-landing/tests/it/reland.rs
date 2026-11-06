use super::serve::{World, git, report};

#[test]
fn a_lane_that_carries_a_landed_lane_is_rebased_by_the_lander_and_only_a_conflict_in_its_own_lines_reaches_its_session() {
    let world = World::new();
    let north = world.repository("north");
    north.lane_with("first", "origin/main", ("a.txt", "a\n"), ("first", Some("session-first")));
    north.lane_with("second", "first", ("b.txt", "b\n"), ("second", Some("session-second")));
    let landed = world.lander(&north, None).wait_with_output().unwrap();
    assert!(landed.status.success(), "{}", report(&landed));
    git(&north.checkout, &["fetch", "-q", "origin"]);
    let tree = git(&north.checkout, &["ls-tree", "-r", "--name-only", "origin/main"]);
    assert!(tree.lines().any(|path| path == "a.txt") && tree.lines().any(|path| path == "b.txt"), "{tree}\n{}", report(&landed));
    let calls = std::fs::read_to_string(world.path().join("bin/north.calls")).unwrap_or_default();
    assert!(!calls.contains("did not land"), "{calls}\n{}", report(&landed));

    git(&north.checkout, &["merge", "-q", "--ff-only", "origin/main"]);
    north.lane_with("third", "origin/main", ("README", "third\n"), ("third", Some("session-third")));
    north.lane_with("fourth", "origin/main", ("README", "fourth\n"), ("fourth", Some("session-fourth")));
    let landed = world.lander(&north, None).wait_with_output().unwrap();
    assert!(landed.status.success(), "{}", report(&landed));
    let calls = std::fs::read_to_string(world.path().join("bin/north.calls")).unwrap_or_default();
    let refused: Vec<&str> = calls.lines().filter(|call| call.contains("did not land")).collect();
    assert!(refused.len() == 1 && refused[0].contains("session-fourth") && refused[0].contains("conflict README"), "{calls}");
}
