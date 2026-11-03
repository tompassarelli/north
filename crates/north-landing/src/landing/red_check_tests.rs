use super::*;

const ACTIONS_LOG: &str = "\
check (cargo tests)\tLand check\t2026-11-04T06:33:01.1234567Z land check: target/debug/deps/land_offload-0abc exited 101
check (cargo tests)\tLand check\t2026-11-04T06:33:01.1234568Z ---- the_remote_checks_exit_status_is_the_verdict stdout ----
check (cargo tests)\tLand check\t2026-11-04T06:33:01.1234569Z test landing::red_check::tests::x ... FAILED
check (cargo tests)\tLand check\t2026-11-04T06:33:01.1234570Z test landing::red_check::tests::y ... ok
check (harness)\tLand check\t2026-11-04T06:33:02.0000000Z FAIL grounding hook names the file
check (harness)\tLand check\t2026-11-04T06:33:02.0000001Z land check: harness/hooks/prompt-grounding.test.sh exited 1
";

#[test]
fn an_actions_failed_log_names_cargo_and_harness_failures() {
    let found = failures(ACTIONS_LOG);
    assert_eq!(
        found.into_iter().collect::<Vec<_>>(),
        vec![
            Failure::Cargo("landing::red_check::tests::x".into()),
            Failure::Cargo("the_remote_checks_exit_status_is_the_verdict".into()),
            Failure::Harness("harness/hooks/prompt-grounding.test.sh".into()),
            Failure::Line("grounding hook names the file".into()),
        ]
    );
}

#[test]
fn the_rerun_runs_only_the_named_tests_and_reports_them_in_the_same_format() {
    let command = rerun_command(&failures(ACTIONS_LOG)).unwrap();
    assert!(command.contains("--exact 'landing::red_check::tests::x' 'the_remote_checks_exit_status_is_the_verdict'"), "{command}");
    assert!(command.contains("bash 'harness/hooks/prompt-grounding.test.sh'"), "{command}");
    assert!(!command.contains("land_offload-0abc"), "{command}");
    assert_eq!(rerun_command(&failures("FAIL only a line\n")), None);
}

#[test]
fn the_event_names_the_tests_the_culprit_and_the_marker() {
    let failed = failures(ACTIONS_LOG);
    let red: Vec<&Failure> = failed.iter().take(2).collect();
    let line = event_line("a717bf640deadbeef", &red, "a717bf640 Offload land checks (since green 1234abcd)", Some("812"));
    assert_eq!(
        line,
        "main is red at a717bf640: landing::red_check::tests::x, the_remote_checks_exit_status_is_the_verdict fails on main's own tree; likely culprit a717bf640 Offload land checks (since green 1234abcd); a fix lane carrying Main-red: #812 lands first and blocked lanes retry after it"
    );
}

#[test]
fn quoted_repository_paths_are_test_inputs() {
    let source = r#"Command::new(concat!(env!("CARGO_MANIFEST_DIR"), "/bin/safe-push")); let x = "not a path"; "a/b c";"#;
    assert_eq!(quoted_paths(source).into_iter().collect::<Vec<_>>(), vec!["bin/safe-push".to_owned()]);
}

#[test]
fn only_a_named_main_red_trailer_counts_as_a_fix() {
    assert!(fix_landed("Fix land offload test\n\nMain-red: #812\n"));
    assert!(!fix_landed("Main-red:\nother\n"));
}
