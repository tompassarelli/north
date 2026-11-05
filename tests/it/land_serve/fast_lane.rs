use std::fs;
use std::path::PathBuf;

use super::{Fixture, git};

fn stdout(fixture: &Fixture) -> String {
    let output = fixture.serve();
    assert!(output.status.success());
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn post_land_runs(fixture: &Fixture) -> usize {
    fixture.calls("land checks --after ").len()
}

#[test]
fn a_docs_lane_with_no_overlap_lands_without_the_full_precheck_and_is_checked_after_landing() {
    let mut fixture = Fixture::new();
    git(&fixture.repo, &["config", "landing.slots", "1"]);
    let docs = fixture.lane("docs", "notes.md", "notes\n");
    let ticket = fixture.enqueue(&docs, None);
    let stdout = stdout(&fixture);
    assert!(stdout.contains("lane-docs takes the fast lane: it changes no crate"), "{stdout}");
    assert!(fixture.checks().is_empty(), "the full precheck ran on a docs-only lane: {:?}", fixture.checks());
    assert_eq!(fixture.subjects(), ["change from docs"]);
    assert_eq!(fixture.verdict(&ticket), Some(format!("landed {}", fixture.main())));
    assert_eq!(post_land_runs(&fixture), 1, "the landed fast lane gets its full checks after landing");
}

#[test]
fn a_fast_lane_whose_post_land_checks_fail_is_reverted_through_the_queue_and_its_session_is_told() {
    let mut fixture = Fixture::new();
    git(&fixture.repo, &["config", "landing.slots", "1"]);
    fs::write(fixture.trace.join("post-land-fail"), "").unwrap();
    let docs = fixture.lane("docs", "notes.md", "notes\n");
    fixture.enqueue(&docs, None);
    let stdout = stdout(&fixture);
    assert!(stdout.contains("fast lane: revert of lane-docs"), "{stdout}");
    assert_eq!(fixture.subjects(), ["change from docs", "Revert change from docs"]);
    assert!(git(&fixture.origin, &["ls-tree", "--name-only", "main"]).lines().all(|file| file != "notes.md"), "the revert removes the fast lane's change");
    let body = git(&fixture.origin, &["log", "-1", "--format=%B", "main"]);
    let landed = git(&fixture.origin, &["rev-parse", "main~1"]);
    assert!(body.contains(&format!("Reverts: {landed}")), "{body}");
    assert_eq!(fixture.checks().len(), 1, "the revert, a red fix, takes the full precheck: {:?}", fixture.checks());
    let told: Vec<String> = fixture.fleet().into_iter().filter(|call| call.starts_with("fleet send --from service:north-land session-lane-docs ") && call.contains("post-land checks failed") && call.contains("fast-revert-")).collect();
    assert_eq!(told.len(), 1, "{:?}", fixture.fleet());
}

#[test]
fn a_lane_that_overlaps_a_queued_lane_or_touches_lander_code_takes_the_full_precheck() {
    let mut fixture = Fixture::new();
    git(&fixture.repo, &["config", "landing.slots", "1"]);
    let first = fixture.lane("first", "notes.md", "first\n");
    let second = fixture.lane("second", "notes.md", "second\n");
    let lander: PathBuf = fixture.lane("lander", "lander.md", "lander\n");
    fs::create_dir_all(lander.join("crates/north-landing/src/land")).unwrap();
    fixture.commit(&lander, "crates/north-landing/src/land/x.rs", "x\n", "lander change from lander");
    let first_ticket = fixture.enqueue(&first, None);
    fixture.enqueue(&second, None);
    let lander_ticket = fixture.enqueue(&lander, None);
    let stdout = stdout(&fixture);
    assert!(stdout.contains("lane-first takes the full precheck: notes.md is also changed by another queued lane"), "{stdout}");
    assert!(stdout.contains("lane-lander takes the full precheck: crates/north-landing/src/land/x.rs is lander code"), "{stdout}");
    assert!(!stdout.contains("lane-first takes the fast lane") && !stdout.contains("lane-lander takes the fast lane"), "{stdout}");
    let checked: Vec<String> = fixture.checks().into_iter().map(|(head, _)| git(&fixture.repo, &["log", "-1", "--format=%s", &head])).collect();
    assert!(checked.iter().any(|subject| subject == "change from first") && checked.iter().any(|subject| subject == "lander change from lander"), "{checked:?}");
    assert!(fixture.verdict(&first_ticket).is_some_and(|verdict| verdict.starts_with("landed ")));
    assert!(fixture.verdict(&lander_ticket).is_some_and(|verdict| verdict.starts_with("landed ")));
    assert_eq!(post_land_runs(&fixture), 0, "lanes that took the full precheck get no fast-lane follow-up");
}
