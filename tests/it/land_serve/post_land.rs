use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use super::{Fixture, executable, git};

const HOLD_FIRST_AFTER_RUN: &str = r#"#!/usr/bin/env bash
if [ "$1 $2 $3" = "land checks --after" ] && [ ! -e "$LAND_TRACE/held" ]; then
  touch "$LAND_TRACE/held"
  for _ in $(seq 1200); do [ -e "$LAND_TRACE/release" ] && exit 0; sleep 0.1; done
  exit 1
fi
"#;

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

fn until(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(120);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn landed(fixture: &Fixture, ticket: &Path) -> String {
    until("a landing", || fixture.verdict(ticket).is_some_and(|verdict| verdict.starts_with("landed ")));
    fixture.verdict(ticket).unwrap().trim_start_matches("landed ").trim().to_owned()
}

#[test]
fn landings_during_an_after_landing_run_wait_as_one_run_of_the_newest_head_that_reverts_only_the_breaking_lane() {
    let mut fixture = Fixture::new();
    fs::write(fixture.trace.join("post-land-red-if"), "broken.txt").unwrap();
    executable(&fixture.trace.join("north-hook"), HOLD_FIRST_AFTER_RUN);
    let first = fixture.lane("first", "first.txt", "first\n");
    let first = fixture.enqueue(&first, None);
    let mut serve = fixture.command().spawn().unwrap();
    let first = landed(&fixture, &first);
    until("the first after-landing run", || fixture.trace.join("held").exists());
    let mut heads = Vec::new();
    for (name, file) in [("second", "second.txt"), ("breaker", "broken.txt"), ("last", "last.txt")] {
        let lane = fixture.lane(name, file, &format!("{name}\n"));
        let ticket = fixture.enqueue(&lane, None);
        heads.push(landed(&fixture, &ticket));
    }
    fs::write(fixture.trace.join("release"), "").unwrap();
    assert!(serve.wait().unwrap().success());
    assert_eq!(fixture.subjects(), ["change from first", "change from second", "change from breaker", "change from last", "Revert change from breaker"]);
    let runs = fixture.calls("land checks --after ");
    let newest = heads.last().unwrap();
    assert_eq!(runs.iter().filter(|run| run.contains(&format!("--base {first} --head {newest} --tree {newest} "))).count(), 1, "one run covers the three landings: {runs:?}");
    for head in &heads[..2] {
        assert!(!runs.iter().any(|run| run.contains(&format!("--head {head} "))), "no run of the covered head {head}: {runs:?}");
    }
    let told: Vec<String> = fixture.fleet().into_iter().filter(|call| call.contains("post-land checks failed")).collect();
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(told[0].starts_with("fleet send --from service:north-land session-lane-breaker "), "{told:?}");
    let ledger = fs::read_to_string(fixture.state.join("safe-push/post-land.tsv")).unwrap();
    for head in std::iter::once(&first).chain(&heads) {
        let rows: Vec<&str> = ledger.lines().filter(|row| row.split('\t').nth(2) == Some(head.as_str())).collect();
        assert_eq!(rows.len(), 1, "one verdict row for landed head {head}: {ledger}");
    }
}

const HOLD_THEN_HANG_NEXT_AFTER_RUN: &str = r#"#!/usr/bin/env bash
if [ "$1 $2 $3" = "land checks --after" ]; then
  if [ ! -e "$LAND_TRACE/held" ]; then
    touch "$LAND_TRACE/held"
    for _ in $(seq 1200); do [ -e "$LAND_TRACE/release" ] && exit 0; sleep 0.1; done
    exit 1
  fi
  echo $$ >>"$LAND_TRACE/hung"
  sleep 600
fi
"#;

#[test]
fn an_after_landing_run_past_its_deadline_reaches_no_verdict_stays_journaled_and_counts_every_covered_head() {
    let mut fixture = Fixture::new();
    executable(&fixture.trace.join("north-hook"), HOLD_THEN_HANG_NEXT_AFTER_RUN);
    let first = fixture.lane("first", "first.txt", "first\n");
    let first = fixture.enqueue(&first, None);
    let mut serve = fixture.command().spawn().unwrap();
    landed(&fixture, &first);
    until("the first after-landing run", || fixture.trace.join("held").exists());
    let mut heads = Vec::new();
    for (name, file) in [("second", "second.txt"), ("third", "third.txt")] {
        let lane = fixture.lane(name, file, &format!("{name}\n"));
        let ticket = fixture.enqueue(&lane, None);
        heads.push(landed(&fixture, &ticket));
    }
    git(&fixture.repo, &["config", "landing.after-deadline-sec", "1"]);
    fs::write(fixture.trace.join("release"), "").unwrap();
    let ledger = fixture.state.join("safe-push/post-land.tsv");
    let expired = || fs::read_to_string(&ledger).unwrap_or_default().lines().filter(|row| row.split('\t').nth(3) == Some("transient")).map(|row| row.split('\t').nth(2).unwrap_or_default().to_owned()).collect::<Vec<_>>();
    until("the stuck run's deadline", || expired().len() >= heads.len());
    let mut rows = expired();
    rows.sort();
    let mut covered = heads.clone();
    covered.sort();
    assert_eq!(rows, covered, "one row per covered head");
    let journal = fixture.queue().join("following");
    let mut journaled: Vec<String> = fs::read_dir(&journal).unwrap().map(|entry| entry.unwrap().file_name().to_string_lossy().trim_end_matches(".json").to_owned()).collect();
    journaled.sort();
    assert_eq!(journaled, covered, "the covered landings stay journaled for the rerun");
    assert_eq!(fixture.subjects(), ["change from first", "change from second", "change from third"], "no verdict, so nothing is reverted");
    assert!(fixture.fleet().iter().all(|call| !call.contains("post-land checks failed") && !call.contains("red main")), "{:?}", fixture.fleet());
    let hung = fs::read_to_string(fixture.trace.join("hung")).unwrap();
    let pid: i32 = hung.lines().next().unwrap().trim().parse().unwrap();
    assert!(!Path::new(&format!("/proc/{pid}")).exists(), "the stuck run was killed");
    serve.kill().unwrap();
    let _ = serve.wait();
}
