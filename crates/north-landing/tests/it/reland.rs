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

#[test]
fn a_lane_the_lander_rebased_over_a_neighbouring_edit_is_told_it_landed() {
    let world = World::new();
    let north = world.repository("north");
    north.lane_with("seed", "origin/main", ("f.txt", "a\nb\nc\nd\ne\n"), ("seed", None));
    let seeded = world.lander(&north, None).wait_with_output().unwrap();
    assert!(seeded.status.success(), "{}", report(&seeded));
    git(&north.checkout, &["fetch", "-q", "origin"]);
    git(&north.checkout, &["merge", "-q", "--ff-only", "origin/main"]);

    north.lane_with("upper", "origin/main", ("f.txt", "a\nB\nc\nd\ne\n"), ("upper", Some("session-upper")));
    north.lane_with("lower", "origin/main", ("f.txt", "a\nb\nc\nD\ne\n"), ("lower", Some("session-lower")));
    let landed = world.lander(&north, None).wait_with_output().unwrap();
    assert!(landed.status.success(), "{}", report(&landed));
    let calls = std::fs::read_to_string(world.path().join("bin/north.calls")).unwrap_or_default();
    for session in ["session-upper", "session-lower"] {
        let told: Vec<&str> = calls.lines().filter(|call| call.contains(session) && call.contains(" landed ")).collect();
        assert!(told.len() == 1, "{session} was told {told:?}\n{calls}\n{}", report(&landed));
    }
}
