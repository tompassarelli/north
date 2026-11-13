use super::{Outcome, verdict};
use crate::gates::plan::{Destination, Gate, GateFacts, plan};

fn every_gate() -> Vec<Gate> {
    let facts = GateFacts { destination: Destination::Default, land_checks_present: true, betterleaks_on_path: true };
    let mut gates = plan(&facts).expect("planned");
    gates.push(Gate::ShellHook);
    let mut seen = std::collections::HashSet::new();
    gates.retain(|gate| seen.insert(*gate));
    gates
}

#[test]
fn the_refusal_line_names_each_failed_gate_once_in_run_order() {
    assert_eq!(verdict(&[]), Ok(()));
    assert_eq!(verdict(&[(Gate::Language, Outcome::Passed), (Gate::Secrets, Outcome::Passed)]), Ok(()));

    let refusal = verdict(&[(Gate::Language, Outcome::Passed), (Gate::LandChecks, Outcome::failed()), (Gate::Secrets, Outcome::failed())])
        .expect_err("two checks failed");
    assert_eq!(refusal.line(), "safe-push: ✗ 2 blocking check(s) failed: land checks; secret scan; NOT pushed.");

    let refusal = verdict(&[(Gate::Language, Outcome::failed())]).expect_err("one check failed");
    assert_eq!(refusal.reason(), "1 blocking check(s) failed: language gate");

    let refusal = verdict(&[
        (Gate::LandChecks, Outcome::failed_with("main's check environment is broken: bun install failed")),
        (Gate::DependencyGate, Outcome::failed_with("(no north on PATH)")),
    ])
    .expect_err("two checks failed");
    assert_eq!(
        refusal.reason(),
        "2 blocking check(s) failed: land checks main's check environment is broken: bun install failed; dependency gate (no north on PATH)"
    );

    let refusal = verdict(&[
        (Gate::StaleRevert, Outcome::Passed),
        (Gate::LandChecks, Outcome::failed()),
        (Gate::StaleRevert, Outcome::failed()),
        (Gate::StaleRevert, Outcome::failed()),
    ])
    .expect_err("two gates failed");
    assert_eq!(refusal.reason(), "2 blocking check(s) failed: land checks; stale revert of newer main");
}

#[test]
fn the_refusal_line_counts_and_names_the_failed_gates_for_every_subset_of_gates() {
    let gates = every_gate();
    for mask in 0..(1u32 << gates.len()) {
        let results: Vec<_> = gates
            .iter()
            .enumerate()
            .map(|(index, &gate)| (gate, if mask >> index & 1 == 1 { Outcome::failed() } else { Outcome::Passed }))
            .collect();
        let failed: Vec<_> = results.iter().filter(|(_, outcome)| *outcome != Outcome::Passed).map(|(gate, _)| gate.label()).collect();
        match verdict(&results) {
            Ok(()) => assert!(failed.is_empty(), "mask {mask}"),
            Err(refusal) => {
                assert_eq!(refusal.count(), failed.len(), "mask {mask}");
                assert_eq!(refusal.reason(), format!("{} blocking check(s) failed: {}", failed.len(), failed.join("; ")), "mask {mask}");
            }
        }
    }
}

#[test]
fn an_over_budget_added_test_is_refused_the_same_whether_finish_or_safe_push_hands_the_lane_in() {
    use super::standing;
    use north_landing::land::landing_refusals::Lane;

    let detail = "safe-push: ✗ test admission: origin's verification takes 100000 s, over its 20 s budget, and the lane adds 1 test in src/tests.rs but deletes 0".to_owned();
    let state = tempfile::tempdir().unwrap();
    let lane = Lane { at: "enqueue", root: "root", branch: "lane", head: "head" };

    let through_finish = lane.judged(state.path(), "test admission", Err(detail.clone()));
    let through_finish_beside_another = lane.judged(state.path(), "policy sweep, test admission", Err(detail.clone()));
    let through_safe_push = verdict(&standing(vec![(Gate::TestAdmission, Outcome::failed_with(detail.clone()))], false));

    assert_eq!(through_finish, Err(detail.clone()));
    assert_eq!(through_finish_beside_another, Err(detail));
    assert_eq!(through_safe_push.expect_err("refused").reason(), "1 blocking check(s) failed: test admission safe-push: ✗ test admission: origin's verification takes 100000 s, over its 20 s budget, and the lane adds 1 test in src/tests.rs but deletes 0");

    assert_eq!(lane.judged(state.path(), "policy sweep", Err("shadowed".to_owned())), Ok(()));
    assert_eq!(verdict(&standing(vec![(Gate::PolicySweep, Outcome::failed())], false)), Ok(()));
}

#[test]
fn a_bug_lane_with_no_test_is_refused_in_shadow_through_finish_and_safe_push() {
    use super::standing;
    use north_landing::land::landing_refusals::Lane;

    let detail = "safe-push: ✗ this lane fixes bug north#1041 but adds or changes no test".to_owned();
    let state = tempfile::tempdir().unwrap();
    let lane = Lane { at: "enqueue", root: "root", branch: "lane", head: "head" };

    assert_eq!(lane.judged(state.path(), "incident test", Err(detail.clone())), Err(detail.clone()));
    let through_safe_push = verdict(&standing(vec![(Gate::IncidentTest, Outcome::failed_with(detail.clone()))], false));
    assert_eq!(through_safe_push.expect_err("refused").reason(), format!("1 blocking check(s) failed: incident test {detail}"));
}
