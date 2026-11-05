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
fn never_reverts_a_revert_or_a_lane_twice_and_a_run_without_cases_reverts_nothing() {
    let lanes = [lane("revert", "Revert \"Break x\""), lane("sheriff", "Fix\n\nSheriff-Reverts: abc"), lane("older", "Break x")];
    let green = |_: &Lane| Some(BTreeSet::new());
    let red = |_: &Lane| Some(x());
    assert_eq!(find(&request(), "head", &lanes, &BTreeSet::new(), red, green), Finding::Revert { lane: "older".into(), cases: x().into_iter().collect() });
    assert_eq!(find(&request(), "head", &lanes, &BTreeSet::from(["older".to_owned()]), red, green), Finding::Environment { cases: x().into_iter().collect() });
    assert_eq!(find(&parse("head", "Green-parent: parent"), "head", &lanes, &BTreeSet::new(), green, green), Finding::Unnamed);
}
