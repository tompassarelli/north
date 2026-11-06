use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use super::{Fixture, executable, git};

const HOLD_TWO_AFTER_RUNS_ON_SPREAD_HOSTS: &str = r#"#!/usr/bin/env bash
if [ "$1 $2 $3 $4" = "capacity offload choose --any" ]; then
  case " $* " in *" --taken gex45 "*) echo ccx33 ;; *) echo gex45 ;; esac
  exit 0
fi
[ "$1 $2 $3" = "land checks --after" ] || exit 0
echo "$SAFE_PUSH_LAND_RUNNER" >>"$LAND_TRACE/runners"
if [ ! -e "$LAND_TRACE/release" ] && [ "$(wc -l <"$LAND_TRACE/runners")" -le 2 ]; then
  for _ in $(seq 1200); do [ -e "$LAND_TRACE/release" ] && exit 0; sleep 0.1; done
  exit 1
fi
"#;

const RED_WHEN_WIDE_SELECTED: &str = r#"#!/usr/bin/env bash
[ "$1 $2" = "land checks" ] || exit 0
while [ $# -gt 0 ]; do case $1 in --base) base=$2; shift ;; --head) head=$2; shift ;; --outcome) outcome=$2; shift ;; esac; shift; done
if [ -e broken.txt ] && git diff --name-only "$base" "$head" | grep -qx wide.txt; then echo "test wide_selection ... FAILED"; [ -z "${outcome:-}" ] || printf 'failed-step\twide_selection\t\n' >>"$outcome"; exit 1; fi
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
fn a_second_landing_runs_beside_the_first_on_another_host_and_later_landings_wait_as_one_run_that_reverts_only_the_breaking_lane() {
    let mut fixture = Fixture::new();
    git(&fixture.repo, &["config", "landing.after-runs", "2"]);
    fs::write(fixture.trace.join("post-land-red-if"), "broken.txt").unwrap();
    executable(&fixture.trace.join("north-hook"), HOLD_TWO_AFTER_RUNS_ON_SPREAD_HOSTS);
    let first = fixture.lane("first", "first.txt", "first\n");
    let first = fixture.enqueue(&first, None);
    let mut serve = fixture.command().spawn().unwrap();
    let first = landed(&fixture, &first);
    let trace = fixture.trace.join("runners");
    let runners = || fs::read_to_string(&trace).unwrap_or_default().lines().map(str::to_owned).collect::<Vec<_>>();
    until("the first after-landing run", || runners().len() == 1);
    let second = fixture.lane("second", "second.txt", "second\n");
    let second = fixture.enqueue(&second, None);
    let second = landed(&fixture, &second);
    until("the second after-landing run beside the held first", || runners().len() == 2);
    assert_eq!(runners(), ["gex45", "ccx33"], "the second run counts the host the first took");
    let mut heads = Vec::new();
    for (name, file) in [("breaker", "broken.txt"), ("last", "last.txt")] {
        let lane = fixture.lane(name, file, &format!("{name}\n"));
        let ticket = fixture.enqueue(&lane, None);
        heads.push(landed(&fixture, &ticket));
    }
    fs::write(fixture.trace.join("release"), "").unwrap();
    assert!(serve.wait().unwrap().success());
    assert_eq!(fixture.subjects(), ["change from first", "change from second", "change from breaker", "change from last", "Revert change from breaker"]);
    let runs = fixture.calls("land checks --after ");
    let newest = heads.last().unwrap();
    assert_eq!(runs.iter().filter(|run| run.contains(&format!("--base {first} --head {second} --tree {second} "))).count(), 1, "the second landing runs alone from the first: {runs:?}");
    assert_eq!(runs.iter().filter(|run| run.contains(&format!("--base {second} --head {newest} --tree {newest} "))).count(), 1, "one run covers the two later landings: {runs:?}");
    assert!(!runs.iter().any(|run| run.contains(&format!("--head {} ", heads[0]))), "no run of the covered head {}: {runs:?}", heads[0]);
    let told: Vec<String> = fixture.fleet().into_iter().filter(|call| call.contains("post-land checks failed")).collect();
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(told[0].starts_with("fleet send --from service:north-land session-lane-breaker "), "{told:?}");
    let ledger = fs::read_to_string(fixture.state.join("safe-push/post-land.tsv")).unwrap();
    for head in [&first, &second].into_iter().chain(&heads) {
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

const HOLD_FIRST_AFTER_RUN: &str = r#"#!/usr/bin/env bash
[ "$1 $2 $3" = "land checks --after" ] || exit 0
while [ $# -gt 0 ]; do case $1 in --head) head=$2; shift ;; esac; shift; done
echo "$head" >>"$LAND_TRACE/after-heads"
if [ ! -e "$LAND_TRACE/held" ]; then
  echo $$ >"$LAND_TRACE/held"
  sleep 600
  exit 1
fi
"#;

// KEEP(bug): on 2026-11-06 a 50-minute rerun of a firn check line that be08ad55 had replaced held firn's after-landing verdict (north#634)
#[test]
fn an_after_landing_run_whose_check_line_a_later_landing_changed_is_cancelled_with_no_verdict_and_its_landings_are_checked_on_the_newest_head() {
    let mut fixture = Fixture::new();
    fixture.commit(&fixture.repo, ".safe-push", "[land]\n\tafter = old-check\n", "check after landing with the old line");
    git(&fixture.repo, &["push", "-q", "origin", "main"]);
    fixture.base = fixture.main();
    executable(&fixture.trace.join("north-hook"), HOLD_FIRST_AFTER_RUN);
    let first = fixture.lane("first", "first.txt", "first\n");
    let first = fixture.enqueue(&first, None);
    let mut serve = fixture.command().spawn().unwrap();
    let first = landed(&fixture, &first);
    until("the first after-landing run", || fs::read_to_string(fixture.trace.join("held")).is_ok_and(|pid| pid.ends_with('\n')));
    let held: i32 = fs::read_to_string(fixture.trace.join("held")).unwrap().trim().parse().unwrap();
    let rewrite = fixture.lane("rewrite", ".safe-push", "[land]\n\tafter = new-check\n");
    let rewrite = fixture.enqueue(&rewrite, None);
    let rewrite = landed(&fixture, &rewrite);
    let ledger = fixture.state.join("safe-push/post-land.tsv");
    let rows = || fs::read_to_string(&ledger).unwrap_or_default().lines().map(|row| row.split('\t').map(str::to_owned).collect::<Vec<_>>()).collect::<Vec<_>>();
    until("the newest head's verdict", || rows().len() >= 2);
    assert!(!Path::new(&format!("/proc/{held}")).exists(), "the superseded run was killed");
    let rows = rows();
    assert_eq!(rows.iter().map(|row| (row[2].as_str(), row[3].as_str(), row[7].as_str())).collect::<Vec<_>>(), [(first.as_str(), "green", rewrite.as_str()), (rewrite.as_str(), "green", "")], "one verdict per landing, both from the newest head's run, none from the superseded run");
    let heads = fs::read_to_string(fixture.trace.join("after-heads")).unwrap();
    assert_eq!(heads.lines().collect::<Vec<_>>(), [first.as_str(), rewrite.as_str()], "the superseded run is not retried on its own head");
    assert!(fixture.fleet().iter().all(|call| !call.contains("main is red") && !call.contains("post-land checks failed")), "{:?}", fixture.fleet());
    serve.kill().unwrap();
    let _ = serve.wait();
}

const RED_THEN_HOLD_CULPRIT_PROBES: &str = r#"#!/usr/bin/env bash
[ "$1 $2 $3" = "land checks --after" ] || exit 0
while [ $# -gt 0 ]; do case $1 in --head) head=$2; shift ;; --tree) tree=$2; shift ;; --outcome) outcome=$2; shift ;; esac; shift; done
echo "$head $tree" >>"$LAND_TRACE/after-runs"
if [ "$head" != "$tree" ]; then
  for _ in $(seq 1200); do [ -e "$LAND_TRACE/release" ] && break; sleep 0.1; done
fi
if [ -e broken.txt ] && [ ! -e last.txt ]; then echo "test after_landing_probe ... FAILED"; printf 'failed-step\tafter_landing_probe\t\n' >>"$outcome"; exit 1; fi
"#;

// KEEP(bug): culprit searches of 87 to 650 s held every later landing's after-landing run and verdict on 2026-11-06 (north#634)
#[test]
fn a_red_runs_culprit_search_does_not_hold_the_next_landings_run() {
    let mut fixture = Fixture::new();
    executable(&fixture.trace.join("north-hook"), RED_THEN_HOLD_CULPRIT_PROBES);
    let breaker = fixture.lane("breaker", "broken.txt", "breaker\n");
    let breaker = fixture.enqueue(&breaker, None);
    let mut serve = fixture.command().spawn().unwrap();
    let breaker = landed(&fixture, &breaker);
    let ledger = fixture.state.join("safe-push/post-land.tsv");
    let trace = fixture.trace.join("after-runs");
    let runs = || fs::read_to_string(&trace).unwrap_or_default().lines().map(str::to_owned).collect::<Vec<_>>();
    until("the culprit search to hold on the base probe", || runs().iter().any(|run| run.split_once(' ').is_some_and(|(head, tree)| head != tree)));
    let last = fixture.lane("last", "last.txt", "last\n");
    let last = fixture.enqueue(&last, None);
    let last = landed(&fixture, &last);
    until("the next landing's run beside the held culprit search", || runs().contains(&format!("{last} {last}")));
    assert!(!fixture.trace.join("release").exists(), "the search was still held when the next run started");
    let rows = fs::read_to_string(&ledger).unwrap_or_default();
    assert!(rows.lines().any(|row| row.split('\t').nth(2) == Some(breaker.as_str()) && row.split('\t').nth(3) == Some("red-main")), "the red verdict row is in the ledger before the search ends: {rows}");
    fs::write(fixture.trace.join("release"), "").unwrap();
    assert!(serve.wait().unwrap().success());
    assert_eq!(fixture.subjects(), ["change from breaker", "change from last", "Revert change from breaker"]);
}
