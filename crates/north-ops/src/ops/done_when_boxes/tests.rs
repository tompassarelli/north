use super::*;
use crate::platform::rolling_issue::Issue;

struct Fixture {
    read: String,
    edits: Vec<String>,
}

impl Tracker for Fixture {
    fn open(&self, _: &str) -> Result<Option<Issue>, String> {
        Ok(None)
    }

    fn body(&self, _: u64) -> Result<String, String> {
        Ok(self.read.clone())
    }

    fn listed(&self, _: &str) -> Result<Vec<(u64, String)>, String> {
        Ok(Vec::new())
    }

    fn create(&mut self, _: &str, _: &str) -> Result<(), String> {
        Err("no create".into())
    }

    fn edit(&mut self, _: u64, body: &str) -> Result<(), String> {
        self.edits.push(body.to_owned());
        Ok(())
    }

    fn close(&mut self, _: u64, _: &str) -> Result<(), String> {
        Err("no close".into())
    }
}

fn main_with_marker() -> TempDir {
    let main = tempfile::tempdir().unwrap();
    std::fs::write(main.path().join("marker"), "shipped on main\n").unwrap();
    main
}

fn stamp() -> Stamp {
    Stamp { sha: "0123456789abcdef0123".into(), date: "2026-11-05".into() }
}

fn verdict_of(line: &str, main: &Path) -> (Outcome, String) {
    let found = verdicts(line, &|command| judge(command, main));
    assert_eq!(found.len(), 1, "{line}");
    (found[0].outcome, found[0].line("north#360"))
}

fn git_main_with(files: &[&str]) -> TempDir {
    let main = tempfile::tempdir().unwrap();
    for file in files {
        let path = main.path().join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "#!/bin/sh\n").unwrap();
    }
    assert!(Command::new("sh").args(["-c", "git init -q && git add -A"]).current_dir(main.path()).status().unwrap().success());
    main
}

const SAFE_PUSH_BOX: &str = "- [ ] bin/safe-push.test.sh is retired. Done when: `git ls-files bin/safe-push.test.sh` prints nothing.";

#[test]
fn a_threshold_or_comparison_the_program_cannot_read_is_a_judge_verdict_and_is_never_ticked() {
    let main = main_with_marker();
    for line in [
        "- [ ] Fast. Done when: `echo 3` prints a count of at most 5.",
        "- [ ] Fast. Done when: `echo 200` shows a median under 240 s.",
        "- [ ] Fast. Done when: `echo 2` prints a number < 5.",
        "- [ ] Fast. Done when: `true` passes within 25% of last week.",
    ] {
        let (outcome, printed) = verdict_of(line, main.path());
        assert_eq!(outcome, Outcome::Judge, "{line}");
        assert!(printed.contains("threshold"), "{printed}");
    }
    let body = "- [ ] Fast. Done when: `echo 3` prints a count of at most 5.\n";
    let mut issue = Fixture { read: body.into(), edits: Vec::new() };
    settle(&mut issue, 7, &verdicts(body, &|command| judge(command, main.path())), &stamp()).unwrap();
    assert!(issue.edits.is_empty());
}

#[test]
fn a_command_box_is_ticked_only_when_its_expectation_holds_on_main_and_a_landed_box_alone_gets_the_evidence_stamp_the_climber_counts_as_checker_verified() {
    let still_there = git_main_with(&["bin/safe-push.test.sh"]);
    assert_eq!(judge("git ls-files bin/safe-push.test.sh", still_there.path()).exit, 0);
    assert_eq!(verdict_of(SAFE_PUSH_BOX, still_there.path()).0, Outcome::Fail);
    let retired = git_main_with(&["bin/safe-push"]);
    assert_eq!(verdict_of(SAFE_PUSH_BOX, retired.path()).0, Outcome::Pass);
    let main = main_with_marker();
    let body = "- [ ] Climb shares. Done when: `true` passes.\n- [ ] Other box. Done when: `false` passes.\n";
    let mut issue = Fixture { read: body.into(), edits: Vec::new() };
    let ran = std::cell::RefCell::new(Vec::new());
    let run = |command: &str| {
        ran.borrow_mut().push(command.to_owned());
        judge(command, main.path())
    };
    settle(&mut issue, 7, &verdicts(&only(body, Some("Climb shares")), &run), &stamp()).unwrap();
    assert_eq!(ran.into_inner(), ["true"], "only the landed box's command runs");
    let [edited] = issue.edits.as_slice() else { panic!("{:?}", issue.edits) };
    assert!(edited.lines().next().unwrap().starts_with("- [x] Climb shares. Done when: `true` passes. Evidence: `true` passed on main "), "{edited}");
    assert!(edited.contains("- [ ] Other box."), "{edited}");
    assert!(north_providers::ledger::closed::checker_verified(edited), "{edited}");
}

#[test]
fn a_passing_box_whose_output_doubles_a_short_body_is_ticked_with_evidence_the_growth_screen_lets_through() {
    let open = "- [ ] Ladder scripts live under harness. Done when: `ladder --self-test` exits 0 on main";
    let body = format!("{open}\n{}\n", "x".repeat(1_200 - open.len() - 1));
    assert_eq!(body.trim().chars().count(), 1_200);
    let output = format!("{}\n{}", vec!["y".repeat(383); 5].join("\n"), "y".repeat(380));
    assert_eq!((output.chars().count(), output.lines().count()), (2_300, 6));
    let run = |_: &str| Run { exit: 0, stdout: output.clone(), output: output.clone() };
    let mut issue = Fixture { read: body.clone(), edits: Vec::new() };
    settle(&mut issue, 1130, &verdicts(&body, &run), &stamp()).unwrap();
    let [edited] = issue.edits.as_slice() else { panic!("{:?}", issue.edits) };
    assert!(edited.starts_with("- [x] Ladder scripts"), "{edited}");
    assert!(edited.contains("```text\n  yyy"), "{edited}");
    assert!(edited.trim().chars().count() <= 2 * body.trim().chars().count(), "{} characters", edited.trim().chars().count());
}

#[test]
fn only_a_quoted_phrase_after_prints_is_text_to_find_single_quoted_included_and_an_unquoted_one_is_judge() {
    let Expected::Judge(reason) = expectation::expected("exits 0 and prints the guard refusal of its probe command") else { panic!("an unquoted phrase is matched literally") };
    assert!(reason.contains("the guard refusal of its probe command"), "{reason}");
    assert_eq!(expectation::expected("exits 0 and prints \"blind staging refused\""), Expected::All(vec![Expectation::Contains("blind staging refused".into()), Expectation::Exit(0)]));
    assert_eq!(expectation::expected("passes with the phrase \"exits 1 and prints the guard refusal\" read as judge"), Expected::All(vec![Expectation::Exit(0)]));
    let ten = expectation::expected("`echo 10` prints '10'");
    assert_eq!(ten, Expected::All(vec![Expectation::Contains("10".into())]));
    assert_eq!(expectation::outcome(&ten, 0, "10\n", "10\n"), Outcome::Pass);
    assert_eq!(expectation::outcome(&ten, 0, "11\n", "11\n"), Outcome::Fail);
    assert_eq!(expectation::expected("`echo 'prints 10'` exits 0"), Expected::All(vec![Expectation::Exit(0)]));
    assert_eq!(expectation::expected("prints 'at most 5'"), Expected::All(vec![Expectation::Contains("at most 5".into())]));
}
