use std::cell::RefCell;
use std::path::Path;

use super::*;
use crate::ops::flow::boxes::{Standing, standing};

const BODY: &str = "Class: tooling\n\nDone when:\n- [x] The code landed (abc1234)\n- [ ] `journalctl --user -u fleet-tick.service` shows no start timeout for a day after it lands.\n- [ ] Another box that is still work\n";

struct Fake {
    body: RefCell<String>,
    open: RefCell<bool>,
    edits: RefCell<u32>,
    refuse_edit: bool,
}

impl Fake {
    fn new(body: &str, open: bool) -> Self {
        Self { body: RefCell::new(body.to_owned()), open: RefCell::new(open), edits: RefCell::new(0), refuse_edit: false }
    }
}

impl Issues for Fake {
    fn body(&self, _: &Issue) -> Result<(String, bool), String> {
        Ok((self.body.borrow().clone(), *self.open.borrow()))
    }

    fn set_body(&self, _: &Issue, body: &str) -> Result<(), String> {
        if self.refuse_edit {
            return Err("gh issue edit: refused".into());
        }
        *self.edits.borrow_mut() += 1;
        *self.body.borrow_mut() = body.to_owned();
        Ok(())
    }

    fn reopen(&self, _: &Issue) -> Result<(), String> {
        *self.open.borrow_mut() = true;
        Ok(())
    }

    fn close(&self, _: &Issue) -> Result<(), String> {
        *self.open.borrow_mut() = false;
        Ok(())
    }
}

fn check(command: &str, expect: Option<&str>) -> Check {
    Check {
        id: "n637-day-without-timeouts".into(),
        text: "shows no start timeout for a day".into(),
        command: command.into(),
        expect: expect.map(str::to_owned),
        review: Day::parse("2026-11-07").unwrap(),
        owner: issue(),
    }
}

fn issue() -> Issue {
    Issue::parse("tompassarelli/north#637").unwrap()
}

fn only_box(body: &str) -> String {
    body.lines().find(|line| line.contains("start timeout")).unwrap().to_owned()
}

fn at(day: &str) -> f64 {
    Day::parse(day).unwrap().unix() as f64
}

fn registry_in(directory: &Path) {
    let path = directory.join(registry::PATH);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "[[experiment]]\nid = \"seen\"\nchange = \"x\"\nlanded = \"1885127c6\"\nmetric = \"landed\"\ndirection = \"up\"\nbar = 0.0\nmin_n = 0\nafter = [\"2026-11-01\", \"2026-11-02\"]\nreview = \"2026-11-05\"\nowner = \"tompassarelli/north#1\"\n").unwrap();
}

fn deferring_a_box_makes_it_wait_until_its_review_day_ends_and_leaves_the_other_boxes_alone() {
    let edited = deferred_body(BODY, &check("true", None)).unwrap();
    let line = only_box(&edited);
    assert!(line.contains("(waits: 2026-11-07, deferred to docs/rationale/experiments.toml check n637-day-without-timeouts)"));
    assert!(line.ends_with("<!-- check n637-day-without-timeouts@2026-11-07 -->"));
    assert_eq!(standing(&line, at("2026-11-07")), Standing::Waiting);
    assert_eq!(standing(&line, at("2026-11-08")), Standing::Workable);
    assert_eq!(edited.lines().filter(|line| line.starts_with("- [ ]")).count(), 2);
    assert!(edited.ends_with('\n'));
    assert!(deferred_body(&edited, &check("true", None)).unwrap_err().contains("already waits"));
    assert!(deferred_body(BODY, &Check { text: "no such box".into(), ..check("true", None) }).unwrap_err().contains("no unticked box"));
    assert!(deferred_body(BODY, &Check { text: "a".into(), ..check("true", None) }).unwrap_err().contains("2 unticked boxes"));
}

fn a_due_check_that_passes_ticks_its_box_with_the_evidence_and_closes_an_issue_with_nothing_else_open() {
    let body = deferred_body(&BODY.replace("- [ ] Another box that is still work", "- [x] Another box that is done"), &check("true", None)).unwrap();
    let issues = Fake::new(&body, true);
    let directory = tempfile::tempdir().unwrap();
    let mut filed = Filed::load(directory.path().join("filed"));
    let checks = [check("printf 0", Some("0"))];
    let ran = |_: &Check, _: &Path| Ran::Passed("exit 0, printed \"0\"".into());
    let early = settle_due(&checks, Day::parse("2026-11-06").unwrap(), directory.path(), &ran, Some((&issues, &mut filed)));
    assert_eq!(early, (vec![], false), "nothing runs before the review day");
    let (lines, failed) = settle_due(&checks, Day::parse("2026-11-07").unwrap(), directory.path(), &ran, Some((&issues, &mut filed)));
    assert!(!failed, "{lines:?}");
    let line = only_box(&issues.body.borrow());
    assert!(line.starts_with("- [x] "), "{line}");
    assert!(line.contains("Evidence: `printf 0` passed on 2026-11-07: exit 0, printed \"0\""));
    assert!(!line.contains("waits:"));
    assert!(!*issues.open.borrow(), "the last box ticked closes the issue");
    let again = settle_due(&checks, Day::parse("2026-11-08").unwrap(), directory.path(), &ran, Some((&issues, &mut filed)));
    assert_eq!(again, (vec![], false), "a settled check is not run again");
    assert_eq!(*issues.edits.borrow(), 1);
}

fn a_due_check_that_misses_reopens_its_box_with_the_output_and_reopens_a_closed_issue() {
    let body = deferred_body(BODY, &check("true", None)).unwrap();
    let issues = Fake::new(&body, false);
    let directory = tempfile::tempdir().unwrap();
    let mut filed = Filed::load(directory.path().join("filed"));
    let checks = [check("false", None)];
    let ran = |_: &Check, _: &Path| Ran::Missed("exit 1, printed \"3\"".into());
    let (lines, failed) = settle_due(&checks, Day::parse("2026-11-07").unwrap(), directory.path(), &ran, Some((&issues, &mut filed)));
    assert!(!failed, "{lines:?}");
    let line = only_box(&issues.body.borrow());
    assert!(line.starts_with("- [ ] "));
    assert!(line.contains("Check `false` missed on 2026-11-07: exit 1, printed \"3\""));
    assert!(!line.contains("waits:"));
    assert_eq!(standing(&line, at("2026-11-07")), Standing::Workable);
    assert!(*issues.open.borrow());
}

fn a_check_that_cannot_run_files_nothing_and_is_tried_again_on_the_next_tick() {
    let body = deferred_body(BODY, &check("true", None)).unwrap();
    let issues = Fake::new(&body, true);
    let directory = tempfile::tempdir().unwrap();
    let mut filed = Filed::load(directory.path().join("filed"));
    let checks = [check("nonesuch", None)];
    let ran = |_: &Check, _: &Path| Ran::Unrun("exit 127: sh: nonesuch: not found".into());
    let (lines, failed) = settle_due(&checks, Day::parse("2026-11-07").unwrap(), directory.path(), &ran, Some((&issues, &mut filed)));
    assert!(!failed);
    assert!(lines.iter().any(|line| line.contains("not run") && line.contains("next tick")), "{lines:?}");
    assert_eq!(*issues.edits.borrow(), 0);
    assert!(!filed.has_key("n637-day-without-timeouts@2026-11-07"));
}

fn running_a_check_reads_its_exit_and_its_expected_output() {
    let root = tempfile::tempdir().unwrap();
    assert!(matches!(run(&check("printf 0", Some("0")), root.path()), Ran::Passed(_)));
    assert!(matches!(run(&check("true", None), root.path()), Ran::Passed(_)));
    let wrong = run(&check("printf 3", Some("0")), root.path());
    assert!(matches!(&wrong, Ran::Missed(why) if why.contains("printed \"3\"") && why.contains("expected \"0\"")), "{wrong:?}");
    let failing = run(&check("echo oops >&2; exit 4", None), root.path());
    assert!(matches!(&failing, Ran::Missed(why) if why.contains("exit 4") && why.contains("oops")), "{failing:?}");
    assert!(matches!(run(&check("exit 127", None), root.path()), Ran::Unrun(_)));
    let here = run(&check("pwd", Some(root.path().to_str().unwrap())), root.path());
    assert!(matches!(here, Ran::Passed(_)), "the command runs from the root: {here:?}");
}

fn defer_writes_the_entry_into_the_registry_and_marks_and_closes_the_issue() {
    let root = tempfile::tempdir().unwrap();
    registry_in(root.path());
    let issues = Fake::new(&BODY.replace("- [ ] Another box that is still work", "- [x] Another box that is done"), true);
    let arguments: Vec<String> = ["north#637", "shows no start timeout", "--check", "journalctl -c | rg -c 'timed out' || echo 0", "--expect", "0", "--by", "2026-11-07", "--id", "n637-day-without-timeouts", "--root", root.path().to_str().unwrap()]
        .iter()
        .map(ToString::to_string)
        .collect();
    let done = defer(&arguments, Day::parse("2026-11-06").unwrap(), &issues).unwrap();
    assert!(done.contains("closes until the check misses"), "{done}");
    let written = registry::parse(&std::fs::read_to_string(root.path().join(registry::PATH)).unwrap()).unwrap();
    assert_eq!(written.experiments.len(), 1);
    let [entry] = written.checks.as_slice() else { panic!("{written:?}") };
    assert_eq!((entry.command.as_str(), entry.expect.as_deref(), entry.review.text().as_str()), ("journalctl -c | rg -c 'timed out' || echo 0", Some("0"), "2026-11-07".to_owned().as_str()));
    assert_eq!(entry.owner, issue());
    assert!(only_box(&issues.body.borrow()).contains("<!-- check n637-day-without-timeouts@2026-11-07 -->"));
    assert!(!*issues.open.borrow());
}

fn defer_refuses_a_past_day_an_unknown_box_and_a_refused_edit_without_leaving_an_entry() {
    let root = tempfile::tempdir().unwrap();
    registry_in(root.path());
    let before = std::fs::read_to_string(root.path().join(registry::PATH)).unwrap();
    let call = |box_text: &str, by: &str, issues: &Fake| {
        let arguments: Vec<String> = ["north#637", box_text, "--check", "true", "--by", by, "--root", root.path().to_str().unwrap()].iter().map(ToString::to_string).collect();
        defer(&arguments, Day::parse("2026-11-06").unwrap(), issues)
    };
    assert!(call("shows no start timeout", "2026-11-05", &Fake::new(BODY, true)).unwrap_err().contains("before today"));
    assert!(call("no such box", "2026-11-07", &Fake::new(BODY, true)).unwrap_err().contains("no unticked box"));
    let refusing = Fake { refuse_edit: true, ..Fake::new(BODY, true) };
    assert!(call("shows no start timeout", "2026-11-07", &refusing).unwrap_err().contains("refused"));
    assert_eq!(std::fs::read_to_string(root.path().join(registry::PATH)).unwrap(), before);
    let kept_open = Fake::new(BODY, true);
    assert!(call("shows no start timeout", "2026-11-07", &kept_open).unwrap().contains("stays open for its other boxes"));
    assert!(*kept_open.open.borrow());
}

#[test]
fn the_registry_reads_check_entries_beside_experiments_and_refuses_a_repeated_or_shared_id() {
    let one = "[[check]]\nid = \"a\"\nbox = \"b\"\ncommand = \"true\"\nreview = \"2026-11-07\"\nowner = \"tompassarelli/north#1\"\n";
    assert_eq!(registry::parse(one).unwrap().checks.len(), 1);
    assert!(registry::parse(&format!("{one}\n{one}")).unwrap_err().iter().any(|problem| problem.contains("appears twice")));
    assert!(registry::parse(&one.replace("true", "  ")).unwrap_err().iter().any(|problem| problem.contains("command is empty")));
    assert!(registry::parse(&one.replace("2026-11-07", "tomorrow")).unwrap_err().iter().any(|problem| problem.contains("not YYYY-MM-DD")));
    assert!(registry::parse(&one.replace("owner", "own")).is_err());
}

#[test]
fn the_issue_body_marks_a_deferred_box_and_the_loop_ticks_it_on_a_pass_reopens_it_on_a_miss_and_retries_a_check_that_cannot_run() {
    deferring_a_box_makes_it_wait_until_its_review_day_ends_and_leaves_the_other_boxes_alone();
    a_due_check_that_passes_ticks_its_box_with_the_evidence_and_closes_an_issue_with_nothing_else_open();
    a_due_check_that_misses_reopens_its_box_with_the_output_and_reopens_a_closed_issue();
    a_check_that_cannot_run_files_nothing_and_is_tried_again_on_the_next_tick();
}

#[test]
fn defer_runs_a_check_command_and_writes_the_registry_entry_and_the_marked_box() {
    running_a_check_reads_its_exit_and_its_expected_output();
    defer_writes_the_entry_into_the_registry_and_marks_and_closes_the_issue();
    defer_refuses_a_past_day_an_unknown_box_and_a_refused_edit_without_leaving_an_entry();
}
