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

#[derive(Default)]
struct Issues {
    open: Option<(u64, String)>,
    created: Vec<String>,
    edited: Vec<String>,
}

impl Tracker for Issues {
    fn open(&self, title: &str) -> Result<Option<rolling_issue::Issue>, String> {
        Ok(self.open.as_ref().filter(|_| title == ISSUE_TITLE).map(|(number, body)| rolling_issue::Issue { number: *number, body: body.clone() }))
    }

    fn body(&self, _: u64) -> Result<String, String> {
        Ok(self.open.as_ref().map(|(_, body)| body.clone()).unwrap_or_default())
    }

    fn listed(&self, _: &str) -> Result<Vec<(u64, String)>, String> {
        Ok(Vec::new())
    }

    fn create(&mut self, _: &str, body: &str) -> Result<(), String> {
        self.created.push(body.to_owned());
        self.open = Some((531, body.to_owned()));
        Ok(())
    }

    fn edit(&mut self, number: u64, body: &str) -> Result<(), String> {
        self.edited.push(body.to_owned());
        self.open = Some((number, body.to_owned()));
        Ok(())
    }

    fn close(&mut self, _: u64, _: &str) -> Result<(), String> {
        Err("a red check never closes main is red".into())
    }
}

const MAIN: &str = "a717bf640aa";
const NEXT: &str = "b828c0751bb";

fn red_line(main: &str) -> String {
    let red = Failure::Cargo("landing::red_check::tests::x".into());
    event_line(main, &[&red], "a717bf640 Offload land checks (since green 1234abcd)", None)
}

#[test]
fn one_red_verdict_posts_nothing_two_in_a_row_open_the_issue_once_and_an_unchanged_one_posts_nothing() {
    let (mut issues, mut breaches) = (Issues::default(), Breaches::default());
    assert_eq!(record_red(&mut issues, &mut breaches, MAIN, &red_line(MAIN)), Ok(None));
    assert!(issues.created.is_empty() && issues.edited.is_empty(), "one red verdict posts nothing");

    assert_eq!(record_red(&mut issues, &mut breaches, MAIN, &red_line(MAIN)), Ok(Some(531)));
    assert_eq!(issues.created.len(), 1);
    let body = &issues.created[0];
    assert!(body.starts_with(ISSUE_BODY) && body.contains(&format!("\n## Red mains\n- [ ] {}\n", red_line(MAIN))), "{body}");

    for _ in 0..3 {
        assert_eq!(record_red(&mut issues, &mut breaches, MAIN, &red_line(MAIN)), Ok(Some(531)));
    }
    assert!(issues.created.len() == 1 && issues.edited.is_empty(), "an unchanged red main posts nothing: {:?}", issues.edited);

    record_red(&mut issues, &mut breaches, NEXT, &red_line(NEXT)).unwrap();
    assert_eq!(issues.edited.len(), 1, "a new red main commit edits the one issue, never a comment");
    let edited = &issues.edited[0];
    assert!(edited.contains(&red_line(MAIN)) && edited.contains(&red_line(NEXT)) && edited.matches("## Red mains").count() == 1, "{edited}");
}

#[test]
fn a_green_verdict_between_two_red_ones_keeps_the_issue_unposted() {
    let (mut issues, mut breaches) = (Issues::default(), Breaches::default());
    record_red(&mut issues, &mut breaches, MAIN, &red_line(MAIN)).unwrap();
    record_green(&mut breaches);
    assert_eq!(record_red(&mut issues, &mut breaches, MAIN, &red_line(MAIN)), Ok(None));
    assert!(issues.created.is_empty() && issues.edited.is_empty());
}

#[test]
fn an_open_issue_from_before_the_gate_gains_the_red_mains_section_once_two_verdicts_confirm() {
    let old = format!("{ISSUE_BODY}\n\nmain is red at 0000000aa: y fails on main's own tree");
    let (mut issues, mut breaches) = (Issues { open: Some((531, old.clone())), ..Issues::default() }, Breaches::default());
    assert_eq!(record_red(&mut issues, &mut breaches, MAIN, &red_line(MAIN)), Ok(Some(531)), "the open issue's number still marks the fix lane");
    assert!(issues.edited.is_empty());
    record_red(&mut issues, &mut breaches, MAIN, &red_line(MAIN)).unwrap();
    assert_eq!(issues.edited, [format!("{old}\n\n## Red mains\n- [ ] {}\n", red_line(MAIN))]);
}
