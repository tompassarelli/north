use super::*;
use std::cell::RefCell;
use std::path::PathBuf;
use std::time::Duration;

use crate::issue_body::{Github, drain, pending};

const SHA: &str = "4be79d6243e3823cc062757155d38dfd8f84be6b";

const PLAN: &str = "## Done when\n- [x] W1 the sender is a required value\n- [ ] L1 `Item:` and `Box:` trailers, checked before the push\n- [ ] L2 the tick function\n- [ ] L3 `Next:` and retirement\n- [ ] (observed) a week of ticks re-measures each metric\n- [ ] (headless run) runtime promotion reaches every host\n- [ ] Tom tries it on his pad in his next playtest\n";

fn ticked(body: &str, box_text: &str) -> String {
    match tick(body, box_text, Some(SHA)) {
        Tick::Ticked(edited) => edited,
        other => panic!("{box_text}: {other:?}"),
    }
}

#[test]
fn a_matching_box_is_ticked_with_its_landed_sha_and_nothing_else_changes() {
    let edited = ticked(PLAN, "L2 the tick function");
    assert_eq!(edited, PLAN.replace("- [ ] L2 the tick function\n", "- [x] L2 the tick function (landed 4be79d6243e3)\n"));
    assert_eq!(tick(&edited, "L2 the tick function", Some(SHA)), Tick::AlreadyTicked);
    assert_eq!(ticked("- [ ] only box\r\n", "only box"), "- [x] only box (landed 4be79d6243e3)\r\n");
    assert_eq!(ticked("* [ ] last line", "last line"), "* [x] last line (landed 4be79d6243e3)");
}

#[test]
fn a_box_matching_none_several_or_a_ticked_one_is_not_ticked() {
    assert_eq!(tick(PLAN, "R9 nothing like it", Some(SHA)), Tick::NoMatch);
    assert_eq!(tick(PLAN, "   ", Some(SHA)), Tick::NoMatch);
    assert_eq!(tick(PLAN, "the", Some(SHA)), Tick::Ambiguous(2));
    assert_eq!(tick(PLAN, "W1 the sender", Some(SHA)), Tick::AlreadyTicked);
}

#[test]
fn an_observed_headless_or_tom_check_box_needs_observation_and_is_never_ticked() {
    for box_text in ["a week of ticks", "runtime promotion reaches", "on his pad"] {
        assert_eq!(tick(PLAN, box_text, Some(SHA)), Tick::NeedsObservation, "{box_text}");
    }
    assert_eq!(tick("- [ ] (observed: decision changes from data) a default changes\n", "a default changes", None), Tick::NeedsObservation);
    assert_eq!(Tick::NeedsObservation.to_string(), "landed, box not ticked: needs observation");
}

#[test]
fn an_issue_with_no_checkboxes_gets_no_edit() {
    let numbered = "## Parts\n1. the first part\n2. the second part\n- a plain bullet\n";
    assert_eq!(tick(numbered, "the first part", Some(SHA)), Tick::NoBoxes);
    assert_eq!(tick("", "anything", None), Tick::NoBoxes);
    assert!(!closable(numbered));
}

#[test]
fn a_parent_with_an_unticked_child_is_not_ticked_and_ticking_its_last_child_ticks_it() {
    let body = "- [ ] Staffing governor bounds launches\n    - Plan: comment 6650256786\n    - [x] the queue check enforces\n    - [ ] the in-flight check enforces\n- [ ] Tool mining\n";
    assert_eq!(tick(body, "Staffing governor", Some(SHA)), Tick::OpenChildren(1));
    assert_eq!(
        ticked(body, "the in-flight check"),
        "- [x] Staffing governor bounds launches\n    - Plan: comment 6650256786\n    - [x] the queue check enforces\n    - [x] the in-flight check enforces (landed 4be79d6243e3)\n- [ ] Tool mining\n"
    );
    let observed_parent = "- [ ] (observed) the loop runs a week\n  - [ ] the tick lands\n";
    assert_eq!(ticked(observed_parent, "the tick lands"), "- [ ] (observed) the loop runs a week\n  - [x] the tick lands (landed 4be79d6243e3)\n");
    let two_open = "- [ ] parent\n  - [ ] first child\n  - [ ] second child\n";
    assert_eq!(ticked(two_open, "first child"), "- [ ] parent\n  - [x] first child (landed 4be79d6243e3)\n  - [ ] second child\n");
}

#[test]
fn an_issue_closes_only_when_at_least_one_box_parsed_and_every_box_is_ticked() {
    assert!(!closable(""));
    assert!(!closable("no boxes here\n"));
    assert!(!closable(PLAN));
    assert!(!closable("- [x] done\n- [ ] (observed) waits on a week\n"));
    assert!(closable("- [x] done\n  - [X] nested done\n* [x] star done\n"));
}

#[test]
fn trailers_pair_each_box_with_the_item_before_it_and_refuse_an_orphan_box() {
    let message = "Land the tick function\n\nBody text.\n\nItem: north#112\nBox: L1 `Item:` and `Box:` trailers\nBox: L2 the tick function\nItem: wisp#9 title words\nBox: a wisp box\n";
    assert_eq!(
        trailers(message).unwrap(),
        vec![
            Trailer { item: "north#112".into(), box_text: "L1 `Item:` and `Box:` trailers".into() },
            Trailer { item: "north#112".into(), box_text: "L2 the tick function".into() },
            Trailer { item: "wisp#9".into(), box_text: "a wisp box".into() },
        ]
    );
    assert_eq!(trailers("Subject\n\nItem: north#112\n").unwrap(), vec![]);
    assert!(trailers("Subject\n\nBox: orphan\n").unwrap_err().contains("no Item:"));
    assert!(trailers("Subject\n\nItem: north\nBox: a box\n").unwrap_err().contains("REPO#N"));
    assert!(trailers("Subject\n\nItem: north#112\nBox:   \n").unwrap_err().contains("empty Box:"));
}

struct Fake {
    body: RefCell<String>,
    closed: RefCell<Vec<String>>,
    close_fails: bool,
}

impl Fake {
    fn with(body: &str) -> Fake {
        Fake { body: RefCell::new(body.to_owned()), closed: RefCell::new(Vec::new()), close_fails: false }
    }
}

impl Github for Fake {
    fn body(&self, _: &Issue) -> Result<String, String> {
        Ok(self.body.borrow().clone())
    }

    fn set_body(&self, _: &Issue, body: &str) -> Result<(), String> {
        *self.body.borrow_mut() = body.to_owned();
        Ok(())
    }

    fn close(&self, issue: &Issue, comment: &str) -> Result<(), String> {
        if self.close_fails {
            return Err("gh issue close timed out after 30 s".to_owned());
        }
        self.closed.borrow_mut().push(format!("{issue}: {comment}"));
        Ok(())
    }
}

fn land(box_text: &str) -> Edit {
    Edit::Land { box_text: box_text.to_owned(), sha: SHA.to_owned() }
}

#[test]
fn the_writer_ticks_a_landed_box_and_closes_the_issue_only_once_every_box_is_ticked() {
    let outbox = tempfile::tempdir().unwrap();
    let issue = Issue::parse("north#112").unwrap();
    let github = Fake::with("- [ ] first box\n- [ ] second box\n- [ ] (observed) third box\n");
    issue_body::submit(outbox.path(), &issue, &land("first box")).unwrap();
    issue_body::submit(outbox.path(), &issue, &land("third box")).unwrap();
    let drained = drain(&issue_body::directory(outbox.path()), &github, Duration::from_secs(10)).unwrap();
    assert_eq!((drained.written, drained.refused, drained.closed), (1, 1, 0));
    assert!(github.closed.borrow().is_empty());
    let refused = std::fs::read_dir(issue_body::directory(outbox.path()).join(issue_body::REFUSED)).unwrap().filter_map(Result::ok).map(|entry| entry.path()).find(|path| path.extension().is_some_and(|extension| extension == "why")).unwrap();
    assert!(std::fs::read_to_string(refused).unwrap().contains("needs observation"));

    let github = Fake::with("- [x] first box\n- [ ] second box\n");
    issue_body::submit(outbox.path(), &issue, &land("second box")).unwrap();
    let drained = drain(&issue_body::directory(outbox.path()), &github, Duration::from_secs(10)).unwrap();
    assert_eq!((drained.written, drained.closed), (1, 1));
    assert_eq!(*github.body.borrow(), "- [x] first box\n- [x] second box (landed 4be79d6243e3)\n");
    assert_eq!(*github.closed.borrow(), vec![format!("tompassarelli/north#112: Closed by the landing of {SHA}: every box on this issue is ticked.")]);

    let github = Fake::with("no boxes at all\n");
    issue_body::submit(outbox.path(), &issue, &land("anything")).unwrap();
    let drained = drain(&issue_body::directory(outbox.path()), &github, Duration::from_secs(10)).unwrap();
    assert_eq!((drained.refused, drained.closed), (1, 0));
    assert_eq!(*github.body.borrow(), "no boxes at all\n");
}

#[test]
fn a_failed_close_keeps_the_entry_so_the_next_drain_closes_the_issue() {
    let outbox = tempfile::tempdir().unwrap();
    let issue = Issue::parse("north#112").unwrap();
    let mut github = Fake::with("- [ ] the only box\n");
    github.close_fails = true;
    issue_body::submit(outbox.path(), &issue, &land("the only box")).unwrap();
    let drained = drain(&issue_body::directory(outbox.path()), &github, Duration::from_secs(10)).unwrap();
    assert_eq!((drained.written, drained.closed, drained.waiting), (0, 0, 1));
    assert!(drained.failure.unwrap().contains("timed out"));
    github.close_fails = false;
    let drained = drain(&issue_body::directory(outbox.path()), &github, Duration::from_secs(10)).unwrap();
    assert_eq!((drained.already, drained.closed, drained.waiting), (1, 1, 0));
    assert_eq!(github.closed.borrow().len(), 1);
}

fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git").arg("-C").arg(dir).args(args).output().unwrap();
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn commit(dir: &Path, file: &str, message: &str) -> String {
    std::fs::write(dir.join(file), message).unwrap();
    git(dir, &["add", file]);
    git(dir, &["commit", "-q", "-m", message]);
    git(dir, &["rev-parse", "HEAD"])
}

fn repository() -> (tempfile::TempDir, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.name", "tick-test"]);
    git(&repo, &["config", "user.email", "tick-test@example.invalid"]);
    git(&repo, &["config", "core.hooksPath", "/dev/null"]);
    (root, repo)
}

fn queued(outbox: &Path) -> Vec<(Issue, String)> {
    pending(&issue_body::directory(outbox))
        .unwrap()
        .into_iter()
        .map(|(_, entry)| match entry.edit {
            Edit::Land { box_text, .. } => (entry.issue, box_text),
            other => panic!("{other:?}"),
        })
        .collect()
}

#[test]
fn a_direct_safe_push_landing_and_a_train_candidate_queue_the_same_tick() {
    let (_root, repo) = repository();
    let base = commit(&repo, "base.txt", "base");
    let lane_message = "Land the tick function\n\nItem: north#112\nBox: L2 the tick function\n";
    git(&repo, &["checkout", "-q", "-b", "lane"]);
    let lane = commit(&repo, "lane.txt", lane_message);
    let plain = commit(&repo, "plain.txt", "A commit with no trailers");
    git(&repo, &["checkout", "-q", "main"]);
    let moved = commit(&repo, "other.txt", "Another lane landed first");
    git(&repo, &["checkout", "-q", "-b", "train"]);
    git(&repo, &["cherry-pick", &lane, &plain]);
    let candidate = git(&repo, &["rev-parse", "HEAD"]);
    assert_ne!(git(&repo, &["rev-parse", "HEAD~1"]), lane);

    let direct = tempfile::tempdir().unwrap();
    let train = tempfile::tempdir().unwrap();
    let direct_lines = queue(direct.path(), &repo, &base, &plain);
    let train_lines = queue(train.path(), &repo, &moved, &candidate);
    let expected = vec![(Issue::parse("north#112").unwrap(), "L2 the tick function".to_owned())];
    assert_eq!(queued(direct.path()), expected);
    assert_eq!(queued(train.path()), expected);
    assert_eq!(direct_lines.len(), 1);
    assert!(direct_lines[0].contains(&lane[..12]), "{direct_lines:?}");
    assert!(train_lines[0].starts_with("queued the tick of tompassarelli/north#112 box \"L2 the tick function\""), "{train_lines:?}");

    let body = "- [ ] L1 trailers\n- [ ] L2 the tick function\n";
    let direct_github = Fake::with(body);
    let train_github = Fake::with(body);
    drain(&issue_body::directory(direct.path()), &direct_github, Duration::from_secs(10)).unwrap();
    drain(&issue_body::directory(train.path()), &train_github, Duration::from_secs(10)).unwrap();
    assert_eq!(direct_github.body.borrow().replace(&lane[..12], "SHA"), "- [ ] L1 trailers\n- [x] L2 the tick function (landed SHA)\n");
    assert_eq!(train_github.body.borrow().replace(&candidate_commit(&repo, &candidate)[..12], "SHA"), "- [ ] L1 trailers\n- [x] L2 the tick function (landed SHA)\n");
}

fn candidate_commit(repo: &Path, candidate: &str) -> String {
    git(repo, &["rev-parse", &format!("{candidate}~1")])
}

#[test]
fn a_range_with_an_orphan_box_queues_nothing_and_says_why() {
    let (_root, repo) = repository();
    let base = commit(&repo, "base.txt", "base");
    let head = commit(&repo, "lane.txt", "Lane\n\nBox: a box with no item\n");
    let outbox = tempfile::tempdir().unwrap();
    let lines = queue(outbox.path(), &repo, &base, &head);
    assert_eq!(lines.len(), 1);
    assert!(lines[0].starts_with("no box ticked: ") && lines[0].contains("no Item:"), "{lines:?}");
    assert!(queued(outbox.path()).is_empty());
}
