use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use north_base::platform::finish_record::{FinishRecords, Finished};

use super::*;
use crate::issue_body::{Ledger, Standing, directory, drain};

const DONE_WHEN: &str = "cargo nextest run -p north-landing -E 'test(/a_leaf/)'";

struct Filed {
    body: String,
    open_children: u64,
}

#[derive(Default)]
struct Issues {
    filed: HashMap<u64, Filed>,
    closed: RefCell<Vec<(u64, String)>>,
    labelled: RefCell<Vec<(u64, String)>>,
}

impl Issues {
    fn of(filed: Vec<(u64, &str, u64)>) -> Issues {
        Issues { filed: filed.into_iter().map(|(number, body, open_children)| (number, Filed { body: body.to_owned(), open_children })).collect(), ..Issues::default() }
    }
}

impl Tracker for Issues {
    fn body(&self, issue: &Issue) -> Result<String, String> {
        Ok(self.filed[&issue.number].body.clone())
    }

    fn set_body(&self, issue: &Issue, _: &str) -> Result<(), String> {
        Err(format!("{issue}: a close edits no body"))
    }

    fn close(&self, issue: &Issue, comment: &str) -> Result<(), String> {
        self.closed.borrow_mut().push((issue.number, comment.to_owned()));
        Ok(())
    }

    fn standing(&self, issue: &Issue) -> Result<Standing, String> {
        Ok(Standing { open: !self.closed.borrow().iter().any(|(number, _)| *number == issue.number), open_children: self.filed[&issue.number].open_children })
    }

    fn label(&self, issue: &Issue, label: &str) -> Result<(), String> {
        self.labelled.borrow_mut().push((issue.number, label.to_owned()));
        Ok(())
    }
}

struct Unread;

impl Ledger for Unread {
    fn record(&self, item: &str, _: &str) -> Result<(), String> {
        Err(format!("{item}: a close sends no box row"))
    }
}

fn git(repo: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git").arg("-C").arg(repo).args(["-c", "user.name=a", "-c", "user.email=a@b"]).args(arguments).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn lane(repo: &Path, file: &str, message: &str) -> String {
    git(repo, &["checkout", "-q", "-B", file, "main"]);
    std::fs::write(repo.join(file), file).unwrap();
    git(repo, &["add", file]);
    git(repo, &["commit", "-q", "-m", message]);
    let tip = git(repo, &["rev-parse", "HEAD"]);
    git(repo, &["checkout", "-q", "main"]);
    git(repo, &["merge", "-q", "--no-ff", "-m", "land", &tip]);
    tip
}

#[test]
fn a_landed_lane_closes_its_issue_and_a_failed_finish_check_leaves_it_open() {
    let scratch = tempfile::tempdir().unwrap();
    let (repo, queue) = (scratch.path().join("repo"), scratch.path().join("queue"));
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "base"]);
    let records = FinishRecords(scratch.path().join("finished"));
    let finish = |session: &str, head: &str, command: &str, passed: bool| records.record(&Finished { session: session.to_owned(), command: command.to_owned(), head: head.to_owned(), passed, at: 0 }).unwrap();
    let leaf = format!("The lander closes a leaf.\n\nDone when: `{DONE_WHEN}` exits 0.\nNeeds: cargo\n");
    let boxed = format!("- [ ] One box of several. Done when: `{DONE_WHEN}` exits 0.\n");
    let issues = Issues::of(vec![(1, leaf.as_str(), 0), (2, leaf.as_str(), 0), (3, leaf.as_str(), 0), (4, boxed.as_str(), 0), (5, leaf.as_str(), 2), (6, leaf.as_str(), 0)]);

    let passed_tip = lane(&repo, "passed", "Close a leaf\n\nItem: north#1");
    finish("passed", &passed_tip, DONE_WHEN, true);
    let failed_tip = lane(&repo, "failed", "Land with a failed check\n\nItem: north#2");
    finish("failed", &failed_tip, DONE_WHEN, false);
    let other_tip = lane(&repo, "other", "Land with another check\n\nItem: north#3");
    finish("other", &other_tip, "true", true);
    let boxed_tip = lane(&repo, "boxed", "Name an issue that has boxes\n\nItem: north#4");
    finish("boxed", &boxed_tip, DONE_WHEN, true);
    let parent_tip = lane(&repo, "parent", "Name a parent\n\nItem: north#5");
    finish("parent", &parent_tip, DONE_WHEN, true);
    let box_tip = lane(&repo, "box", "Tick a box\n\nItem: north#6\nBox: One box");
    finish("box", &box_tip, DONE_WHEN, true);
    git(&repo, &["checkout", "-q", "-b", "unlanded", "main"]);
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "Not on main\n\nItem: north#1"]);
    let unlanded_tip = git(&repo, &["rev-parse", "HEAD"]);
    finish("unlanded", &unlanded_tip, DONE_WHEN, true);

    assert_eq!(passed(&records, "failed", &repo, &failed_tip), None, "a failed finish check proves nothing");
    assert_eq!(passed(&records, "passed", &repo, &failed_tip), None, "a check that passed on another tree proves nothing about this one");
    for (session, tip) in [("passed", &passed_tip), ("failed", &failed_tip), ("other", &other_tip), ("boxed", &boxed_tip), ("parent", &parent_tip), ("box", &box_tip), ("unlanded", &unlanded_tip)] {
        let check = passed(&records, session, &repo, tip);
        super::queue(&queue, &Landed { repo: &repo, tip, main: "main", check: check.as_deref() });
    }
    let drained = drain(&directory(&queue), &issues, &Unread, Duration::from_secs(5)).unwrap();

    let closed = issues.closed.borrow();
    assert_eq!(closed.iter().map(|(number, _)| *number).collect::<Vec<_>>(), [1], "only the leaf whose Done-when command passed on its landed commit is closed: {:?}", drained.leaves);
    assert!(closed[0].1.contains(&passed_tip[..12]) && closed[0].1.contains(DONE_WHEN), "the closing comment names the commit and the command: {}", closed[0].1);
    let mut waiting = issues.labelled.borrow().clone();
    waiting.sort();
    assert_eq!(waiting, [(2, WAITS.to_owned()), (3, WAITS.to_owned())], "a leaf whose command has not passed stays open as waiting, and an issue with boxes or open children is left alone");
    assert_eq!((drained.closed, drained.refused, drained.waiting, drained.failure), (1, 0, 0, None));
}
