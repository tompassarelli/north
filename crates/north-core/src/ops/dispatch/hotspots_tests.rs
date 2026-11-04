use super::plan::Cloud;
use super::tests::{fleet, issue, survey};
use super::decide;

fn outcome(bodies: [&str; 2]) -> super::Outcome {
    let (hosts, accounts) = fleet();
    let boxes = bodies.iter().zip(20..).map(|(body, number)| issue("makgora", number, body, &[])).collect();
    decide(&survey(boxes, Cloud::Unreachable("the credit login is on nexus".into()), hosts, accounts), "test")
}

#[test]
fn two_ready_briefs_naming_one_file_staff_one_and_hold_the_other_with_the_file_named() {
    let outcome = outcome(["- [ ] a\nFiles: src/cli/fleet/placement.rs, src/a.rs", "- [ ] b\nFiles: `src/b.rs`, src/cli/fleet/placement.rs"]);
    let launched: Vec<String> = outcome.launches.iter().map(|launch| launch.item.to_string()).collect();
    assert_eq!(launched, ["makgora#20"]);
    let (held, why) = &outcome.skipped[0];
    assert_eq!(held.to_string(), "makgora#21");
    assert!(why.starts_with("ready, not placed: held for makgora#20") && why.contains("src/cli/fleet/placement.rs"), "{why}");
}

#[test]
fn briefs_with_disjoint_files_both_staff() {
    let outcome = outcome(["- [ ] a\nFiles: src/a.rs", "- [ ] b\nFiles: src/b.rs"]);
    assert_eq!(outcome.launches.len(), 2, "{:?}", outcome.skipped);
}

#[test]
fn briefs_naming_no_files_never_hold_each_other() {
    let outcome = outcome(["- [ ] a", "- [ ] b"]);
    assert_eq!(outcome.launches.len(), 2, "{:?}", outcome.skipped);
}
