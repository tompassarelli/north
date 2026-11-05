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
fn bisects_a_range_to_the_lane_that_turns_the_case_red_and_verifies_only_that_one() {
    let lanes = [lane("l5", "Add e"), lane("l4", "Add d"), lane("l3", "Break x"), lane("l2", "Add b"), lane("l1", "Add a")];
    let (mut at_probed, mut revert_probed) = (Vec::new(), Vec::new());
    let finding = find(
        &request(),
        "head",
        &lanes,
        &BTreeSet::new(),
        |lane| {
            at_probed.push(lane.commit.clone());
            Some(if ["l1", "l2"].contains(&lane.commit.as_str()) { BTreeSet::new() } else { x() })
        },
        |lane| {
            revert_probed.push(lane.commit.clone());
            Some(if lane.commit == "l3" { BTreeSet::new() } else { x() })
        },
    );
    assert_eq!(finding, Finding::Revert { lane: "l3".into(), cases: x().into_iter().collect() });
    assert_eq!(at_probed, ["l3", "l2"]);
    assert_eq!(revert_probed, ["l3"]);
}

#[test]
fn a_range_of_one_lane_needs_no_bisection() {
    let finding = find(&request(), "head", &[lane("only", "Break x")], &BTreeSet::new(), |_| panic!("one lane has nothing to bisect"), |_| Some(BTreeSet::new()));
    assert_eq!(finding, Finding::Revert { lane: "only".into(), cases: x().into_iter().collect() });
}

#[test]
fn an_unreadable_bisection_probe_falls_back_to_trying_each_lane_newest_first() {
    let lanes = [lane("newer", "Add b"), lane("older", "Break x")];
    let mut probed = Vec::new();
    let finding = find(&request(), "head", &lanes, &BTreeSet::new(), |_| None, |lane| {
        probed.push(lane.commit.clone());
        Some(if lane.commit == "older" { BTreeSet::new() } else { x() })
    });
    assert_eq!(finding, Finding::Revert { lane: "older".into(), cases: x().into_iter().collect() });
    assert_eq!(probed, ["newer", "older"]);
}

#[test]
fn a_failure_that_persists_without_the_bisected_lane_reverts_nothing() {
    let lanes = [lane("newer", "Add b"), lane("older", "Add c")];
    let culprit_is_newer = |lane: &Lane| Some(if lane.commit == "older" { BTreeSet::new() } else { x() });
    assert_eq!(find(&request(), "head", &lanes, &BTreeSet::new(), culprit_is_newer, |_| Some(x())), Finding::Environment { cases: x().into_iter().collect() });
    assert_eq!(find(&request(), "head", &lanes, &BTreeSet::new(), culprit_is_newer, |_| None), Finding::Environment { cases: x().into_iter().collect() });
}

#[test]
fn main_moved_past_the_failing_commit_reverts_nothing_and_probes_nothing() {
    let finding = find(&request(), "fix-forward", &[lane("older", "Break x")], &BTreeSet::new(), |_| panic!("a stale request must not probe"), |_| panic!("a stale request must not probe"));
    assert!(matches!(finding, Finding::Stale(reason) if reason.contains("fix-forward")));
}

#[test]
fn never_reverts_a_revert_or_a_lane_twice_and_a_run_without_cases_reverts_nothing() {
    let lanes = [lane("revert", "Revert \"Break x\""), lane("sheriff", "Fix\n\nSheriff-Reverts: abc"), lane("older", "Break x")];
    let green = |_: &Lane| Some(BTreeSet::new());
    let red = |_: &Lane| Some(x());
    assert_eq!(find(&request(), "head", &lanes, &BTreeSet::new(), red, green), Finding::Revert { lane: "older".into(), cases: x().into_iter().collect() });
    assert_eq!(find(&request(), "head", &lanes, &BTreeSet::from(["older".to_owned()]), red, green), Finding::Environment { cases: x().into_iter().collect() });
    assert_eq!(find(&parse("head", "Green-parent: parent"), "head", &lanes, &BTreeSet::new(), green, green), Finding::Unnamed);
}
