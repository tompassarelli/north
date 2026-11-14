use super::*;

const ACTIONS_LOG: &str = "\
check (cargo tests)\tLand check\t2026-11-04T06:33:01.1234567Z land check: target/debug/deps/land_offload-0abc exited 101
check (cargo tests)\tLand check\t2026-11-04T06:33:01.1234568Z ---- the_remote_checks_exit_status_is_the_verdict stdout ----
check (cargo tests)\tLand check\t2026-11-04T06:33:01.1234569Z test landing::red_check::tests::x ... FAILED
check (cargo tests)\tLand check\t2026-11-04T06:33:01.1234570Z test landing::red_check::tests::y ... ok
check (harness)\tLand check\t2026-11-04T06:33:02.0000000Z FAIL grounding hook names the file
check (harness)\tLand check\t2026-11-04T06:33:02.0000001Z land check: harness/hooks/prompt-grounding.test.sh exited 1
check (cargo tests)\tLand check\t2026-11-07T05:24:50.0000000Z FAIL [   0.085s] (  2/788) north::it agents_plan::every_recorded_scenario_replays_to_its_expected_decisions
check (harness)\tLand check\t2026-11-07T05:13:40.0000000Z land check: held: harness/hooks/quarantined.test.sh has a case quarantined on main for over 24 h
[2026-11-14T07:19:53.314+00:00]         FAIL [   0.694s] north-guard guard::launch_critical::tests::launch_guard_preserves_recorded_writes_and_allows_lanes
";

#[test]
fn an_actions_failed_log_names_cargo_and_harness_failures() {
    let found = failures(ACTIONS_LOG);
    assert_eq!(
        found.into_iter().collect::<Vec<_>>(),
        vec![
            Failure::Cargo("agents_plan::every_recorded_scenario_replays_to_its_expected_decisions".into()),
            Failure::Cargo("guard::launch_critical::tests::launch_guard_preserves_recorded_writes_and_allows_lanes".into()),
            Failure::Cargo("landing::red_check::tests::x".into()),
            Failure::Cargo("the_remote_checks_exit_status_is_the_verdict".into()),
            Failure::Harness("harness/hooks/prompt-grounding.test.sh".into()),
            Failure::Harness("harness/hooks/quarantined.test.sh".into()),
            Failure::Line("grounding hook names the file".into()),
        ]
    );
}
