use std::cell::RefCell;
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::{ExitStatus, Output};
use std::time::Duration;

use super::super::{Detector, Finding};
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
fn a_repeated_cause_comments_on_the_open_box_instead_of_filing_a_second_one() {
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
