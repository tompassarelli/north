use std::fs;

use super::{Fixture, git};

const CHECKS_BEFORE: &str = "[land]\n\tcheck = src/ :: test -d src\n\tcheck = docs/ :: test -d docs\n";

fn covered_lane(checks: &str, message: &str) -> Fixture {
    let fixture = Fixture::seeded(&[("src/a.rs", "pub fn a() {}\n"), ("src/b.rs", "pub fn b() {}\n"), ("docs/guide.md", "# Guide\n"), (".safe-push", CHECKS_BEFORE)]);
    fs::write(fixture.repo.join(".safe-push"), checks).unwrap();
    git(&fixture.repo, &["commit", "-qam", message]);
    fixture
}

#[test]
fn enqueue_refuses_a_lane_deleting_a_landing_check_and_names_the_uncovered_paths() {
    let fixture = covered_lane("[land]\n\tcheck = docs/ :: test -d docs\n", "Drop the src check");
    let refusal = fixture.try_enqueue().expect_err("deleting a check must not queue");
    println!("{refusal}");
    assert!(refusal.contains("land check `test -d src` no longer covers src/a.rs, src/b.rs"), "{refusal}");
    assert!(refusal.contains("'Removes-check: src because REASON'"), "{refusal}");
    assert!(fixture.pending().is_empty());
}

#[test]
fn enqueue_refuses_a_lane_narrowing_a_check_path_filter_for_the_paths_it_drops() {
    let fixture = covered_lane("[land]\n\tcheck = src/a.rs :: test -d src\n\tcheck = docs/ :: test -d docs\n", "Narrow the src check");
    let refusal = fixture.try_enqueue().expect_err("narrowing a check must not queue");
    assert!(refusal.contains("no longer covers src/b.rs;"), "{refusal}");
}

#[test]
fn enqueue_accepts_a_lane_replacing_a_check_with_one_covering_the_same_paths() {
    let fixture = covered_lane("[land]\n\tcheck = src/a.rs,src/b.rs :: test -e src\n\tcheck = docs/ :: test -d docs\n", "Replace the src check");
    let queued = fixture.try_enqueue().expect("an equivalent check keeps coverage");
    println!("Done: queued {} {}", queued.ticket.branch, queued.ticket.head);
    assert_eq!(fixture.pending(), vec![queued.ticket]);
}

#[test]
fn enqueue_accepts_a_lane_removing_a_check_under_a_removes_check_trailer() {
    let fixture = covered_lane("[land]\n\tcheck = docs/ :: test -d docs\n", "Drop the src check\n\nRemoves-check: src because the src tree is generated");
    assert!(fixture.try_enqueue().is_ok());
}

#[test]
fn enqueue_accepts_a_lane_behind_main_when_main_added_a_check_since_the_lane_branched() {
    let fixture = Fixture::seeded(&[("src/a.rs", "pub fn a() {}\n"), ("docs/guide.md", "# Guide\n"), (".safe-push", CHECKS_BEFORE)]);
    fixture.commit(&fixture.repo, "src/c.rs", "Add c");
    let upstream = fixture.root.join("upstream");
    git(&fixture.repo, &["worktree", "add", "-q", "-b", "upstream", upstream.to_str().unwrap(), "origin/main"]);
    fs::write(upstream.join(".safe-push"), format!("{CHECKS_BEFORE}\tcheck = src/a.rs :: test -f src/a.rs\n")).unwrap();
    git(&upstream, &["commit", "-qam", "Check a on main"]);
    git(&upstream, &["push", "-q", "origin", "HEAD:main"]);
    let queued = fixture.try_enqueue().expect("a check main added after the lane branched is not one the lane removed");
    assert_eq!(fixture.pending(), vec![queued.ticket]);
}
