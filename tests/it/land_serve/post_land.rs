use std::fs;

use super::{Fixture, git};

fn post_land_runs(fixture: &Fixture) -> usize {
    fixture.calls("land checks --after ").len()
}

fn files(fixture: &Fixture) -> Vec<String> {
    git(&fixture.origin, &["ls-tree", "--name-only", "main"]).lines().map(str::to_owned).collect()
}

#[test]
fn a_lane_whose_change_breaks_a_test_is_reverted_alone_after_landing_and_the_others_stay() {
    let mut fixture = Fixture::new();
    fs::write(fixture.trace.join("post-land-red-if"), "broken.txt").unwrap();
    let first = fixture.lane("first", "first.txt", "first\n");
    let breaker = fixture.lane("breaker", "breaker.txt", "breaker\n");
    fixture.commit(&breaker, "broken.txt", "breaks the test\n", "second change from breaker");
    let last = fixture.lane("last", "last.txt", "last\n");
    let tickets = [first, breaker, last].map(|lane| fixture.enqueue(&lane, None));
    assert!(fixture.serve().status.success());
    assert_eq!(fixture.subjects(), ["change from first", "change from breaker", "second change from breaker", "change from last", "Revert second change from breaker"]);
    let tree = files(&fixture);
    assert!(tree.iter().any(|file| file == "first.txt") && tree.iter().any(|file| file == "last.txt"), "{tree:?}");
    assert!(!tree.iter().any(|file| file == "breaker.txt" || file == "broken.txt"), "{tree:?}");
    let body = git(&fixture.origin, &["log", "-1", "--format=%B", "main"]);
    for commit in git(&fixture.origin, &["rev-list", "main~2", &format!("^{}", git(&fixture.origin, &["rev-parse", "main~4"]))]).lines() {
        assert!(body.contains(&format!("Reverts: {commit}")), "{body}");
    }
    assert!(tickets.iter().all(|ticket| fixture.verdict(ticket).is_some_and(|verdict| verdict.starts_with("landed "))), "every lane landed first");
    assert_eq!(post_land_runs(&fixture), 4, "the batch, two bisection probes, and the revert");
    let told: Vec<String> = fixture.fleet().into_iter().filter(|call| call.starts_with("fleet send --from service:north-land session-lane-breaker ") && call.contains("post-land checks failed") && call.contains("Follows:")).collect();
    assert_eq!(told.len(), 1, "{:?}", fixture.fleet());
    assert!(fixture.fleet().iter().all(|call| !call.contains("session-lane-first ") && !call.contains("session-lane-last ") || call.contains("landed ")), "{:?}", fixture.fleet());
}

#[test]
fn a_main_that_was_already_red_before_the_batch_blames_and_reverts_no_lane() {
    let mut fixture = Fixture::new();
    fixture.commit(&fixture.repo, "broken.txt", "red before the lane\n", "main already carries the break");
    git(&fixture.repo, &["push", "-q", "origin", "main"]);
    fixture.base = fixture.main();
    fs::write(fixture.trace.join("post-land-red-if"), "broken.txt").unwrap();
    let lane = fixture.lane("innocent", "innocent.txt", "innocent\n");
    fixture.enqueue(&lane, None);
    assert!(fixture.serve().status.success());
    assert_eq!(fixture.subjects(), ["change from innocent"]);
    assert_eq!(post_land_runs(&fixture), 2, "the batch and its base");
}
