use std::cell::RefCell;
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::{ExitStatus, Output};
use std::time::Duration;

use super::super::{Detector, Finding, mechanism_discovery};
use super::Filer;
use crate::flow::{Host, Places};

#[derive(Default)]
struct GitHub {
    body: RefCell<String>,
    edits: RefCell<Vec<String>>,
    comments: RefCell<Vec<String>>,
    closed: std::cell::Cell<bool>,
    reopens: RefCell<Vec<String>>,
}

impl Host for GitHub {
    fn output(&self, argv: &[&str], _: Option<&Path>, input: Option<&str>, _: Duration) -> Option<Output> {
        let answer = |text: String| Some(Output { status: ExitStatus::from_raw(0), stdout: text.into_bytes(), stderr: Vec::new() });
        match argv {
            ["gh", "issue", "view", ..] => answer(serde_json::json!({ "body": self.body.borrow().clone(), "state": if self.closed.get() { "CLOSED" } else { "OPEN" } }).to_string()),
            ["gh", "issue", "edit", ..] => {
                let text = input.unwrap_or_default().to_owned();
                self.edits.borrow_mut().push(text.clone());
                *self.body.borrow_mut() = text;
                answer(String::new())
            }
            ["gh", "issue", "reopen", ..] => {
                self.reopens.borrow_mut().push(argv.last().copied().unwrap_or_default().to_owned());
                self.closed.set(false);
                answer(String::new())
            }
            ["gh", "issue", "comment", ..] => {
                self.comments.borrow_mut().push(input.unwrap_or_default().to_owned());
                answer(String::new())
            }
            _ => None,
        }
    }
}

const WEEK: f64 = 7.0 * 86400.0;

fn places(root: &Path) -> Places {
    Places { home: root.join("home"), state: root.join("state"), ledger: root.join("threads.db"), graph: root.join("graph.db") }
}

fn finding(key: &str, title: &str) -> Finding {
    Finding {
        detector: Detector::MachineFix,
        key: key.to_owned(),
        owner: Some("north#777".to_owned()),
        title: title.to_owned(),
        detail: format!("{key} landed commit 1 narrates an incident"),
        done_when: "`north check machines` passes".to_owned(),
        instances: Vec::new(),
    }
}

#[test]
fn mechanism_discovery_and_machine_fix_comment_on_the_open_box_instead_of_a_second_one_and_each_turn_is_recorded_once() {
    let root = tempfile::tempdir().unwrap();
    let places = places(root.path());
    let github = GitHub::default();
    let title = "Extend the pet machine model to the fixes its check let through";

    let first = finding("machine-fix:pet:2026-W44", title);
    let filed = Filer::new(&places, &github).file(&[first], 1.0).unwrap();
    assert!(filed[0].filed, "{}", filed[0].note);
    assert_eq!(github.edits.borrow().len(), 1);
    assert!(github.body.borrow().contains("(detector machine-fix:pet:2026-W44)"));

    let second = finding("machine-fix:pet:2026-W45", title);
    let repeated = Filer::new(&places, &github).file(&[second], 2.0).unwrap();
    assert!(!repeated[0].filed);
    assert!(repeated[0].note.contains("commented the repeated cause on north#777"), "{}", repeated[0].note);
    assert_eq!(github.edits.borrow().len(), 1, "the repeated cause files no second box");
    assert_eq!(github.comments.borrow().len(), 1);
    let comment = &github.comments.borrow()[0];
    assert!(comment.contains("machine-fix:pet:2026-W45"), "{comment}");
    assert!(comment.contains("2026-W45 landed commit 1 narrates an incident"), "{comment}");
    assert!(comment.contains(title), "{comment}");
    assert!(!github.body.borrow().contains("(detector machine-fix:pet:2026-W45)"), "the comment is not a box");

    let ticked = github.body.borrow().replace("- [ ]", "- [x]");
    *github.body.borrow_mut() = ticked;
    let third = finding("machine-fix:pet:2026-W46", title);
    let again = Filer::new(&places, &github).file(&[third], 3.0 * WEEK).unwrap();
    assert!(again[0].filed, "a cause whose box is ticked files a new box: {}", again[0].note);
    assert_eq!(github.edits.borrow().len(), 2);

    let turns = GitHub::default();
    let turn_facts = |today: &str| mechanism_discovery::Facts {
        owner: Some("north#1279".to_owned()),
        today: today.to_owned(),
        losses: vec![("first-try-lost:capacity".to_owned(), 9), ("first-try-lost:landing refusal".to_owned(), 14), ("first-try-lost:waits".to_owned(), 5), ("first-try-lost:halted".to_owned(), 2)],
        held: Vec::new(),
    };
    let day_one = mechanism_discovery::due(&turn_facts("2026-11-13"));
    assert_eq!(day_one.len(), 1);
    assert_eq!(day_one[0].key, "mechanism-discovery:2026-11-13:first-try-lost:landing refusal");
    assert!(day_one[0].detail.contains("harness/mechanism-discovery/turn-brief.md"), "{}", day_one[0].detail);
    assert!(day_one[0].detail.contains("first-try-lost:waits") && !day_one[0].detail.contains("first-try-lost:halted"), "the three largest classes are named: {}", day_one[0].detail);
    let turned = Filer::new(&places, &turns).file(&day_one, 4.0 * WEEK).unwrap();
    assert!(turned[0].filed, "{}", turned[0].note);
    assert!(turns.body.borrow().contains("(detector mechanism-discovery:2026-11-13:first-try-lost:landing refusal)"));
    let day_two = mechanism_discovery::due(&turn_facts("2026-11-14"));
    let repeated = Filer::new(&places, &turns).file(&day_two, 4.0 * WEEK + 1.0).unwrap();
    assert!(!repeated[0].filed && repeated[0].note.contains("commented"), "a day's turn while the last one is open files no box: {}", repeated[0].note);
    assert_eq!(turns.edits.borrow().len(), 1, "the open turn box is not filed twice");
    assert_eq!(turns.comments.borrow().len(), 1);

    let evidence = |dates: &[&str]| mechanism_discovery::Evidence {
        turns: dates.iter().map(|date| mechanism_discovery::Turn { date: (*date).to_owned(), landed: format!("{date}T09:00:00+00:00") }).collect(),
        mechanisms: ["- [x] Keep discovery-a (abc): kept", "- [ ] Revert abc (x): origin experiment discovery-b", "", "- [ ] Tom decides discovery-d"].iter().map(|verdict| mechanism_discovery::Mechanism { verdict: (*verdict).to_owned() }).collect(),
    };
    mechanism_discovery::write(root.path(), &evidence(&["2026-11-12", "2026-11-13"])).unwrap();
    let mut names: Vec<String> = std::fs::read_dir(root.path().join("north/mechanism-discovery/turns")).unwrap().flatten().map(|entry| entry.file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    assert_eq!(names, ["2026-11-12.txt", "2026-11-13.txt"]);
    assert_eq!(std::fs::read_to_string(root.path().join("north/mechanism-discovery/score.txt")).unwrap(), "turns 2 in 28 d, newest 2026-11-13; mechanisms registered 4, kept on held-out cases 1, dropped 1, awaiting review 2\n");
    let bare = tempfile::tempdir().unwrap();
    mechanism_discovery::write(bare.path(), &evidence(&[])).unwrap();
    assert!(!bare.path().join("north/mechanism-discovery/turns").exists(), "no turns leaves the record unwritten, so north loops check reads it stale");
    assert!(std::fs::read_to_string(bare.path().join("north/mechanism-discovery/score.txt")).unwrap().starts_with("turns 0 in 28 d, newest none;"));
}

fn tick_box(github: &GitHub, evidence: &str) {
    let ticked = github.body.borrow().replacen("- [ ]", "- [x]", 1) + evidence;
    *github.body.borrow_mut() = ticked;
}

#[test]
fn a_subject_waits_twice_as_long_after_a_box_that_changed_nothing_and_returns_to_its_floor_after_a_change() {
    let root = tempfile::tempdir().unwrap();
    let places = places(root.path());
    let github = GitHub::default();
    let file = |key: &str, title: &str, at: f64| Filer::new(&places, &github).file(&[finding(key, title)], at).unwrap().remove(0);

    assert!(file("machine-fix:pet:W1", "Model pet", 0.0).filed);
    tick_box(&github, "");
    let held = file("machine-fix:pet:W2", "Model pet again", WEEK + 1.0);
    assert!(!held.filed && held.note.contains("waiting"), "a box that closed with nothing changed doubles the floor: {}", held.note);
    assert!(file("machine-fix:cat:W2", "Model cat", WEEK + 1.0).filed, "another machine keeps its own wait");
    assert!(file("machine-fix:pet:W3", "Model pet once more", 2.0 * WEEK + 1.0).filed);

    let landed = github.body.borrow().replace("- [ ] **Model pet once more.**", "- [x] **Model pet once more.** Landed as 0123abc.");
    *github.body.borrow_mut() = landed;
    let after_change = file("machine-fix:pet:W4", "Model pet a fourth time", 3.0 * WEEK + 2.0);
    assert!(after_change.filed, "a box that led to a change returns the wait to the floor: {}", after_change.note);
}

#[test]
fn a_finding_whose_owner_is_closed_reopens_it_and_stays_unfiled_until_its_box_is_written() {
    let root = tempfile::tempdir().unwrap();
    let places = places(root.path());
    let github = GitHub::default();
    github.closed.set(true);
    let first = finding("machine-fix:pet:2026-W44", "Extend the pet machine model");

    let reopened = Filer::new(&places, &github).file(std::slice::from_ref(&first), 1.0).unwrap();
    assert!(!reopened[0].filed);
    assert!(reopened[0].note.contains("reopened north#777"), "{}", reopened[0].note);
    assert_eq!(github.reopens.borrow().len(), 1);
    assert!(github.reopens.borrow()[0].contains("machine-fix:pet:2026-W44"), "the comment names the finding");
    assert!(github.edits.borrow().is_empty());
    assert!(Filer::new(&places, &github).filed_keys().is_empty(), "the key is unfiled until the box is written");

    let written = Filer::new(&places, &github).file(&[first], 2.0).unwrap();
    assert!(written[0].filed, "{}", written[0].note);
    assert_eq!(github.reopens.borrow().len(), 1);
    assert!(Filer::new(&places, &github).filed_keys().contains("machine-fix:pet:2026-W44"));
}
