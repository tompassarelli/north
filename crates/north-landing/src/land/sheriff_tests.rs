use super::*;

const CLIPPY: &str = "CARGO_TARGET_DIR=target ./bin/land-shell --dev -- cargo clippy --all-targets --workspace --locked --offline -- -D warnings";

fn request() -> Request {
    parse("head", "Main red at head\n\ntest suite::x ... FAILED\n\nGreen-parent: parent\nMain-red-run: https://example/runs/7")
}

fn clippy_request() -> Request {
    parse("red", &request_message("red", "parent", &clippy(), Path::new("/logs/post-land-red.log")))
}

fn lane(commit: &str, message: &str) -> Lane {
    Lane { commit: commit.into(), message: message.into() }
}

fn x() -> BTreeSet<Failure> {
    BTreeSet::from([Failure::Cargo("suite::x".into())])
}

fn clippy() -> BTreeSet<Failure> {
    BTreeSet::from([Failure::Step(CLIPPY.into())])
}

#[test]
fn bisects_a_range_to_the_lane_that_turns_the_case_red_and_verifies_only_that_one() {
    let lanes = [lane("head", "Add e"), lane("l4", "Add d"), lane("l3", "Break x"), lane("l2", "Add b"), lane("l1", "Add a")];
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
    assert_eq!(at_probed, ["head", "l3", "l2", "head"], "the tip reruns once more after the confirmation");
    let ledger = "1\tnorth\tg1\tgreen\t\t\t5\t\n2\tnorth\tg2\tred-main\tx\t\t5\t\n3\tother\tg3\tgreen\t\t\t5\t";
    assert_eq!(green_ancestor(ledger, "north", &["g3", "g2", "g1"]), Some("g1".to_owned()));
    assert_eq!(revert_probed, ["l3"]);
}

#[test]
fn never_reverts_a_revert_or_a_lane_twice_and_a_run_without_cases_reverts_nothing() {
    let lanes = [lane("head", "Revert \"Break x\""), lane("sheriff", "Fix\n\nSheriff-Reverts: abc"), lane("older", "Break x")];
    let green = |_: &Lane| Some(BTreeSet::new());
    let red = |_: &Lane| Some(x());
    assert_eq!(find(&request(), "head", &lanes, &BTreeSet::new(), red, green), Finding::Revert { lane: "older".into(), cases: x().into_iter().collect() });
    assert_eq!(find(&request(), "head", &lanes, &BTreeSet::from(["older".to_owned()]), red, green), Finding::Environment { cases: x().into_iter().collect() });
    infrastructure_reaches_no_verdict();
}

#[test]
fn a_failure_that_passes_at_the_tip_on_the_recheck_is_flaky_and_reverts_nothing() {
    let lanes = [lane("head", "Add e"), lane("l2", "Add b"), lane("l1", "Add a")];
    let mut tip_runs = 0;
    let finding = find(
        &request(),
        "head",
        &lanes,
        &BTreeSet::new(),
        |lane| {
            if lane.commit == "head" {
                tip_runs += 1;
            }
            let passes = (lane.commit == "head" && tip_runs > 1) || lane.commit == "l1";
            Some(if passes { BTreeSet::new() } else { x() })
        },
        |_| Some(BTreeSet::new()),
    );
    assert_eq!(finding, Finding::Flaky { lane: "l2".into(), cases: x().into_iter().collect() }, "a test that flips between runs at the same tip names no culprit");
    assert_eq!(tip_runs, 2, "the tip reruns once after the confirmation");
    let unreached = find(&request(), "head", &lanes, &BTreeSet::new(), |lane| {
        let at_tip = lane.commit == "head";
        tip_runs += usize::from(at_tip);
        (!at_tip || tip_runs < 4).then(|| if lane.commit == "l1" { BTreeSet::new() } else { x() })
    }, |_| Some(BTreeSet::new()));
    assert!(matches!(unreached, Finding::NoVerdict(_)), "a recheck with no verdict never reverts: {unreached:?}");
}

#[test]
fn a_red_that_names_no_test_bisects_by_rerunning_its_failing_step_at_the_tip_and_reverts_the_lane_that_broke_it() {
    let request = clippy_request();
    assert_eq!(request.failing, clippy(), "the request carries the failing step");
    assert_eq!(request.log, "/logs/post-land-red.log");
    let lanes = [lane("red", "Add c"), lane("l2", "Warn in b"), lane("l1", "Add a")];
    let mut reran = Vec::new();
    let finding = find(
        &request,
        "red",
        &lanes,
        &BTreeSet::new(),
        |lane| {
            reran.push(lane.commit.clone());
            Some(if lane.commit == "l1" { BTreeSet::new() } else { clippy() })
        },
        |lane| Some(if lane.commit == "l2" { BTreeSet::new() } else { clippy() }),
    );
    assert_eq!(finding, Finding::Revert { lane: "l2".into(), cases: clippy().into_iter().collect() });
    assert_eq!(reran, ["red", "l2", "l1", "red"], "the step reruns at the red head, bisects, then rechecks the tip after the confirmation");
    main_moved_past_the_red_head();
}

fn main_moved_past_the_red_head() {
    let request = clippy_request();
    let lanes = [lane("tip", "Add e"), lane("later", "Add d"), lane("red", "Add c"), lane("culprit", "Warn in b"), lane("l1", "Add a")];
    let red_from = |commit: &str| ["tip", "later", "red", "culprit"].contains(&commit);
    let (mut reran, mut without) = (Vec::new(), Vec::new());
    let finding = find(
        &request,
        "tip",
        &lanes,
        &BTreeSet::new(),
        |lane| {
            reran.push(lane.commit.clone());
            Some(if red_from(&lane.commit) { clippy() } else { BTreeSet::new() })
        },
        |lane| {
            without.push(lane.commit.clone());
            Some(if lane.commit == "culprit" { BTreeSet::new() } else { clippy() })
        },
    );
    assert_eq!(finding, Finding::Revert { lane: "culprit".into(), cases: clippy().into_iter().collect() });
    assert_eq!(reran.first().map(String::as_str), Some("tip"), "the failing step reruns at the new tip first: {reran:?}");
    assert_eq!(without, ["culprit"], "main at the tip without the culprit is the confirmation");
    let fixed = find(&request, "tip", &lanes, &BTreeSet::new(), |_| Some(BTreeSet::new()), |_| Some(BTreeSet::new()));
    assert_eq!(fixed, Finding::Fixed { cases: clippy().into_iter().collect() }, "a later landing that fixed it leaves nothing to revert");
}

fn infrastructure_reaches_no_verdict() {
    let lanes = [lane("red", "Add c"), lane("l1", "Add a")];
    let mut confirmed = 0;
    let no_step = find(&parse("red", "Green-parent: parent"), "red", &lanes, &BTreeSet::new(), |_| Some(clippy()), |_| {
        confirmed += 1;
        Some(BTreeSet::new())
    });
    assert!(matches!(no_step, Finding::NoVerdict(_)), "a red run naming no failing step never reverts: {no_step:?}");
    let unreproduced = find(&clippy_request(), "red", &lanes, &BTreeSet::new(), |_| None, |_| {
        confirmed += 1;
        Some(BTreeSet::new())
    });
    assert!(matches!(unreproduced, Finding::NoVerdict(_)), "a rerun at the tip that reaches no verdict never reverts: {unreproduced:?}");
    let unconfirmed = find(&clippy_request(), "red", &lanes, &BTreeSet::new(), |lane| (lane.commit == "red").then(clippy), |_| None);
    assert_eq!(unconfirmed, Finding::Environment { cases: clippy().into_iter().collect() }, "a confirmation without a verdict never reverts");
    assert_eq!(confirmed, 0);
}
