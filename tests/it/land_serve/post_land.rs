use std::fs;

use super::{Fixture, executable, git};

const RED_WHEN_WIDE_SELECTED: &str = r#"#!/usr/bin/env bash
[ "$1 $2" = "land checks" ] || exit 0
while [ $# -gt 0 ]; do case $1 in --base) base=$2; shift ;; --head) head=$2; shift ;; esac; shift; done
if [ -e broken.txt ] && git diff --name-only "$base" "$head" | grep -qx wide.txt; then echo "test wide_selection ... FAILED"; exit 1; fi
"#;

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

#[test]
fn a_base_that_fails_only_the_wide_selection_of_the_red_run_blames_and_reverts_no_lane() {
    let mut fixture = Fixture::new();
    fixture.commit(&fixture.repo, "broken.txt", "red before the lane\n", "main already carries a break only a wide selection runs");
    fixture.commit(&fixture.repo, "narrow.txt", "narrow\n", "main's last change selects only a narrow set");
    git(&fixture.repo, &["push", "-q", "origin", "main"]);
    fixture.base = fixture.main();
    executable(&fixture.trace.join("north-hook"), RED_WHEN_WIDE_SELECTED);
    let lane = fixture.lane("wide", "wide.txt", "wide\n");
    fixture.enqueue(&lane, None);
    assert!(fixture.serve().status.success());
    assert_eq!(fixture.subjects(), ["change from wide"]);
    let runs = fixture.calls("land checks --after ");
    let head = git(&fixture.origin, &["rev-parse", "main"]);
    assert_eq!(runs.len(), 2, "the batch and its base: {runs:?}");
    assert!(runs[1].contains(&format!("--base {} --head {head} --tree {} ", fixture.base, fixture.base)), "the base runs with the red run's selection: {runs:?}");
}

#[test]
fn a_bisection_prefix_runs_with_the_red_run_selection_so_the_lane_that_carries_the_break_is_reverted() {
    let mut fixture = Fixture::new();
    executable(&fixture.trace.join("north-hook"), RED_WHEN_WIDE_SELECTED);
    let breaker = fixture.lane("breaker", "broken.txt", "breaks a test only a wide selection runs\n");
    let wide = fixture.lane("wide", "wide.txt", "wide\n");
    for lane in [breaker, wide] {
        fixture.enqueue(&lane, None);
    }
    assert!(fixture.serve().status.success());
    assert_eq!(fixture.subjects(), ["change from breaker", "change from wide", "Revert change from breaker"]);
    let tree = files(&fixture);
    assert!(tree.iter().any(|file| file == "wide.txt") && !tree.iter().any(|file| file == "broken.txt"), "{tree:?}");
}
