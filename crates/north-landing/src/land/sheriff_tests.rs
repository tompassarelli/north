use super::*;

fn request() -> Request {
    parse("head", "Main red at head\n\ntest suite::x ... FAILED\n\nGreen-parent: parent\nMain-red-run: https://example/runs/7")
}

fn lane(commit: &str, message: &str) -> Lane {
    Lane { commit: commit.into(), message: message.into() }
}

fn x() -> BTreeSet<Failure> {
    BTreeSet::from([Failure::Cargo("suite::x".into())])
}

#[test]
fn a_request_names_its_failing_cases_green_parent_and_run() {
    let request = request();
    assert_eq!((request.parent.as_str(), request.run.as_str()), ("parent", "https://example/runs/7"));
    assert_eq!(request.failing, x());
}

#[test]
fn reverts_only_the_lane_whose_removal_turns_the_case_green() {
    let lanes = [lane("newer", "Add b"), lane("older", "Break x")];
    let mut probed = Vec::new();
    let finding = find(&request(), "head", &lanes, &BTreeSet::new(), |lane| {
        probed.push(lane.commit.clone());
        Some(if lane.commit == "older" { BTreeSet::new() } else { x() })
    });
    assert_eq!(finding, Finding::Revert { lane: "older".into(), cases: x().into_iter().collect() });
    assert_eq!(probed, ["newer", "older"]);
}

#[test]
fn a_failure_that_persists_without_each_lane_reverts_nothing() {
    let lanes = [lane("newer", "Add b"), lane("older", "Add c")];
    assert_eq!(find(&request(), "head", &lanes, &BTreeSet::new(), |_| Some(x())), Finding::Environment { cases: x().into_iter().collect() });
    assert_eq!(find(&request(), "head", &lanes, &BTreeSet::new(), |_| None), Finding::Environment { cases: x().into_iter().collect() });
}

#[test]
fn main_moved_past_the_failing_commit_reverts_nothing_and_probes_nothing() {
    let finding = find(&request(), "fix-forward", &[lane("older", "Break x")], &BTreeSet::new(), |_| panic!("a stale request must not probe"));
    assert!(matches!(finding, Finding::Stale(reason) if reason.contains("fix-forward")));
}

#[test]
fn never_reverts_a_revert_or_a_lane_twice_and_a_run_without_cases_reverts_nothing() {
    let lanes = [lane("revert", "Revert \"Break x\""), lane("sheriff", "Fix\n\nSheriff-Reverts: abc"), lane("older", "Break x")];
    let green = |_: &Lane| Some(BTreeSet::new());
    assert_eq!(find(&request(), "head", &lanes, &BTreeSet::new(), green), Finding::Revert { lane: "older".into(), cases: x().into_iter().collect() });
    assert_eq!(find(&request(), "head", &lanes, &BTreeSet::from(["older".to_owned()]), green), Finding::Environment { cases: x().into_iter().collect() });
    assert_eq!(find(&parse("head", "Green-parent: parent"), "head", &lanes, &BTreeSet::new(), green), Finding::Unnamed);
}
