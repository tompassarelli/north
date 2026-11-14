use super::*;
use crate::land::verdict_tree;

const CLIPPY: &str = "CARGO_TARGET_DIR=target ./bin/land-shell --dev -- cargo clippy --all-targets --workspace --locked --offline -- -D warnings";

fn request() -> Request {
    parse("head", "Main red at head\n\ntest suite::x ... FAILED\n\nGreen-parent: parent\nMain-red-run: https://example/runs/7")
}

fn clippy_request() -> Request {
    parse("red", &request_message("red", "parent", &clippy(), Path::new("/logs/post-land-red.log"), "tree"))
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

/// `find` over a bench whose every run lands on one host, and whose run elsewhere lands on another that answers alike.
fn find_here(request: &Request, tip: &str, lanes: &[Lane], reverted: &BTreeSet<String>, mut at: impl FnMut(&Lane) -> Option<BTreeSet<Failure>>, mut probe: impl FnMut(&Lane) -> Option<BTreeSet<Failure>>) -> Finding {
    let host = |on: On| (if matches!(on, On::Besides(_)) { "cpx62" } else { "gex45" }).to_owned();
    find(request, tip, lanes, reverted, None, |lane, on| at(lane).map(|failures| Probed { host: host(on), failures }), |lane, on| probe(lane).map(|failures| Probed { host: host(on), failures }))
}

#[test]
fn bisects_a_range_to_the_lane_that_turns_the_case_red_and_verifies_only_that_one() {
    let lanes = [lane("head", "Add e"), lane("l4", "Add d"), lane("l3", "Break x"), lane("l2", "Add b"), lane("l1", "Add a")];
    let (mut at_probed, mut revert_probed) = (Vec::new(), Vec::new());
    let finding = find_here(
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
    assert_eq!(at_probed, ["head", "l3", "l2", "head", "head"], "the tip reruns twice more, each after a confirmation");
    let ledger = "1\tnorth\tg1\tgreen\t\t\t5\t\n2\tnorth\tg2\tred-main\tx\t\t5\t\n3\tother\tg3\tgreen\t\t\t5\t";
    assert_eq!(green_ancestor(ledger, "north", &["g3", "g2", "g1"]), Some("g1".to_owned()));
    assert_eq!(revert_probed, ["l3", "l3"], "main without the culprit passes twice before its revert");
}

#[test]
fn never_reverts_a_revert_or_a_lane_twice_and_a_run_without_cases_reverts_nothing() {
    let lanes = [lane("head", "Revert \"Break x\""), lane("sheriff", "Fix\n\nSheriff-Reverts: abc"), lane("older", "Break x")];
    let green = |_: &Lane| Some(BTreeSet::new());
    let red = |_: &Lane| Some(x());
    assert_eq!(find_here(&request(), "head", &lanes, &BTreeSet::new(), red, green), Finding::Revert { lane: "older".into(), cases: x().into_iter().collect() });
    assert_eq!(find_here(&request(), "head", &lanes, &BTreeSet::from(["older".to_owned()]), red, green), Finding::Environment { cases: x().into_iter().collect() });
    let walked_back = |message: &str| find(&request(), "head", &lanes, &BTreeSet::from(["older".to_owned()]), Some(&lane("early", message)), |_, on| Some(Probed { host: (if matches!(on, On::Besides(_)) { "cpx62" } else { "gex45" }).into(), failures: x() }), |lane, _| Some(Probed { host: "gex45".into(), failures: if lane.commit == "early" { BTreeSet::new() } else { x() } }));
    assert_eq!(walked_back("Break x"), Finding::Revert { lane: "early".into(), cases: x().into_iter().collect() }, "a main broken before its green parent reverts the walked-back landing once main without it passes twice");
    assert_eq!(walked_back("Revert \"Add x\""), Finding::Environment { cases: x().into_iter().collect() }, "a walked-back revert is never reverted");
    let mut told = Vec::new();
    let report = main_breakage(&request(), "head", &x().into_iter().collect::<Vec<_>>(), |session, text| {
        told.push((session.to_owned(), text.to_owned()));
        true
    });
    assert!(report.contains("main breakage: suite::x") && report.contains("nothing is reverted"), "{report}");
    assert_eq!(told.len(), 1, "the proxy is told once: {told:?}");
    assert!(told[0].0 == "proxy" && told[0].1.contains("main is broken, not a lane: suite::x"), "{told:?}");
    infrastructure_reaches_no_verdict();
    let (commit, other) = ("c".repeat(40), "f".repeat(40));
    let run = tempfile::tempdir().unwrap();
    let log = run.path().join("post-land-red.log");
    fs::write(&log, format!("offload: running {} in /slots/code-north-main\n{}{other} of red\n", "a".repeat(40), north_base::foundation::offload::PROVED_TREE)).unwrap();
    let named = |record: Option<String>| {
        let _ = fs::remove_file(verdict_tree::record(&log));
        if let Some(record) = record {
            fs::write(verdict_tree::record(&log), record).unwrap();
        }
        parse("red", &request_message("red", "parent", &clippy(), &log, &verdict_tree::named(&log, "red", &commit))).tree
    };
    let offloaded = verdict_tree::header("red", true);
    assert_eq!(verdict_tree::misjudged(&named(Some(format!("{offloaded}{commit} red\n"))), &commit), None, "a red its record proves was computed on the commit's own tree is judged, whatever tree a line of its log says was proved");
    assert!(verdict_tree::misjudged(&named(Some(format!("{offloaded}{other} red\n"))), &commit).is_some_and(|why| why.contains(&other)), "a red computed on another tree reverts nothing and names that tree");
    assert!(verdict_tree::misjudged(&named(Some(format!("{offloaded}unproved red\n"))), &commit).is_some(), "a red from a shipped run that proved no tree reverts nothing");
    assert!(verdict_tree::misjudged(&named(Some(offloaded)), &commit).is_some(), "an offloaded run that recorded no shipped run reverts nothing");
    assert!(verdict_tree::misjudged(&named(Some(format!("{}{commit} red\n", verdict_tree::header("blue", true)))), &commit).is_some(), "another run's record reverts nothing");
    assert!(verdict_tree::misjudged(&named(None), &commit).is_some(), "a run with no record reverts nothing");
    assert!(verdict_tree::misjudged(&request().tree, &commit).is_some(), "a request naming no tree reverts nothing");
}

#[test]
fn a_failure_that_passes_at_the_tip_on_the_recheck_is_flaky_and_reverts_nothing() {
    let lanes = [lane("head", "Add e"), lane("l2", "Add b"), lane("l1", "Add a")];
    let mut tip_runs = 0;
    let finding = find_here(
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
    let unreached = find_here(&request(), "head", &lanes, &BTreeSet::new(), |lane| {
        let at_tip = lane.commit == "head";
        tip_runs += usize::from(at_tip);
        (!at_tip || tip_runs < 4).then(|| if lane.commit == "l1" { BTreeSet::new() } else { x() })
    }, |_| Some(BTreeSet::new()));
    assert!(matches!(unreached, Finding::NoVerdict(_)), "a recheck with no verdict never reverts: {unreached:?}");
    let mut base_runs = 0;
    let host_state = find_here(&request(), "head", &lanes, &BTreeSet::new(), |lane| Some(if lane.commit == "l1" { BTreeSet::new() } else { x() }), |_| {
        base_runs += 1;
        Some(if base_runs == 1 { BTreeSet::new() } else { x() })
    });
    assert_eq!(host_state, Finding::Flaky { lane: "l2".into(), cases: x().into_iter().collect() }, "a case that fails once in two runs on the base names no culprit");
    assert_eq!(base_runs, 2, "the base runs a second time before any revert");
}

#[test]
fn a_red_that_names_no_test_bisects_by_rerunning_its_failing_step_at_the_tip_and_reverts_the_lane_that_broke_it() {
    let request = clippy_request();
    assert_eq!(request.failing, clippy(), "the request carries the failing step");
    assert_eq!(request.log, "/logs/post-land-red.log");
    let lanes = [lane("red", "Add c"), lane("l2", "Warn in b"), lane("l1", "Add a")];
    let mut reran = Vec::new();
    let finding = find_here(
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
    assert_eq!(reran, ["red", "l2", "l1", "red", "red"], "the step reruns at the red head, bisects, then rechecks the tip after each confirmation");
    main_moved_past_the_red_head();
}

fn main_moved_past_the_red_head() {
    let request = clippy_request();
    let lanes = [lane("tip", "Add e"), lane("later", "Add d"), lane("red", "Add c"), lane("culprit", "Warn in b"), lane("l1", "Add a")];
    let red_from = |commit: &str| ["tip", "later", "red", "culprit"].contains(&commit);
    let (mut reran, mut without) = (Vec::new(), Vec::new());
    let finding = find_here(
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
    assert_eq!(without, ["culprit", "culprit"], "main at the tip without the culprit, twice, is the confirmation");
    let fixed = find_here(&request, "tip", &lanes, &BTreeSet::new(), |_| Some(BTreeSet::new()), |_| Some(BTreeSet::new()));
    assert_eq!(fixed, Finding::Fixed { cases: clippy().into_iter().collect() }, "a later landing that fixed it leaves nothing to revert");
}

fn infrastructure_reaches_no_verdict() {
    let lanes = [lane("red", "Add c"), lane("l1", "Add a")];
    let mut confirmed = 0;
    let no_step = find_here(&parse("red", "Green-parent: parent"), "red", &lanes, &BTreeSet::new(), |_| Some(clippy()), |_| {
        confirmed += 1;
        Some(BTreeSet::new())
    });
    assert!(matches!(no_step, Finding::NoVerdict(_)), "a red run naming no failing step never reverts: {no_step:?}");
    let unreproduced = find_here(&clippy_request(), "red", &lanes, &BTreeSet::new(), |_| None, |_| {
        confirmed += 1;
        Some(BTreeSet::new())
    });
    assert!(matches!(unreproduced, Finding::NoVerdict(_)), "a rerun at the tip that reaches no verdict never reverts: {unreproduced:?}");
    let unconfirmed = find_here(&clippy_request(), "red", &lanes, &BTreeSet::new(), |lane| (lane.commit == "red").then(clippy), |_| None);
    assert_eq!(unconfirmed, Finding::Environment { cases: clippy().into_iter().collect() }, "a confirmation without a verdict never reverts");
    assert_eq!(confirmed, 0);
}

const NIX_CHECK: &str = "nix flake check --offline --no-update-lock-file --no-build";

#[test]
fn a_check_that_fails_only_on_one_host_is_that_hosts_fault_and_reverts_nothing() {
    let request = parse("9b38429b23", &request_message("9b38429b23", "parent", &BTreeSet::from([Failure::Step(NIX_CHECK.into())]), Path::new("/logs/post-land-9b38429b23b7.log"), "tree"));
    let lanes = [lane("9b38429b23", "Add land.after-only reasons to .safe-push"), lane("older", "Add a")];
    let ran = std::cell::RefCell::new(Vec::<String>::new());
    let bench = |_: &Lane, on: On| {
        let host = match on {
            On::Any if ran.borrow().is_empty() => "cpx62b",
            On::Any => "cpx62",
            On::Host("cpx62b") => "cpx62b",
            On::Host(other) => other,
            On::Besides(_) => "cpx62",
        };
        ran.borrow_mut().push(host.to_owned());
        let missing_drv = host == "cpx62b";
        Some(Probed { host: host.to_owned(), failures: if missing_drv { BTreeSet::from([Failure::Step(NIX_CHECK.into())]) } else { BTreeSet::new() } })
    };
    let finding = find(&request, "9b38429b23", &lanes, &BTreeSet::new(), None, bench, bench);
    assert_eq!(finding, Finding::HostFault { host: "cpx62b".into(), cases: vec![Failure::Step(NIX_CHECK.into())], elsewhere: Some("cpx62".into()) });
    let probed = ran.borrow().clone();
    ran.borrow_mut().clear();
    assert_eq!(probed.last().map(String::as_str), Some("cpx62"), "the tip reruns on another host before anything is judged");
    assert!(probed[..probed.len() - 1].iter().all(|host| host == "cpx62b"), "every probe before that runs on the host the first ran on: {probed:?}");
    let unanswered = find(&request, "9b38429b23", &lanes, &BTreeSet::new(), None, |lane: &Lane, on: On| (!matches!(on, On::Besides(_))).then(|| bench(lane, on)).flatten(), bench);
    assert!(matches!(unanswered, Finding::HostFault { elsewhere: None, .. }), "no other host reaching a verdict reverts nothing: {unanswered:?}");
}

