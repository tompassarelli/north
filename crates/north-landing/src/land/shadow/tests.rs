use std::fs;
use std::path::{Path, PathBuf};

use super::super::candidate::git;
use super::super::queue::{self, RedFix, Ticket};
use super::super::reuse::{self, Verdict};
use super::{Round, Settings, pick, tick};

fn ticket(branch: &str, head: &str, red_fix: Option<RedFix>) -> Ticket {
    Ticket { root: PathBuf::from("/lane"), host: "box".to_owned(), branch: branch.to_owned(), head: head.to_owned(), red_fix, ..Ticket::default() }
}

fn entry(micros: u128, ticket: Ticket) -> (PathBuf, Ticket) {
    (PathBuf::from(format!("/queue/{}", queue::file_name(micros, &ticket.branch))), ticket)
}

fn visited(tickets: Vec<(PathBuf, Ticket)>) -> Vec<String> {
    let mut seen = Vec::new();
    let _ = pick::first_with_work(tickets, "box", |_, ticket| {
        seen.push(ticket.branch.clone());
        None::<()>
    });
    seen
}

#[test]
fn red_fix_and_urgent_lanes_come_first_then_the_oldest_queued() {
    let tickets = vec![entry(30, ticket("c", "3", None)), entry(40, ticket("fix", "4", Some(RedFix::Marker))), entry(10, ticket("a", "1", None)), entry(20, ticket("b", "2", None)), entry(50, ticket("issue", "5", Some(RedFix::Issue(7)))), entry(60, Ticket { urgent: Some("Tom asked".to_owned()), ..ticket("hurry", "6", None) })];
    assert_eq!(visited(tickets.clone()), ["fix", "issue", "hurry", "a", "b", "c"]);
    let mut reversed = tickets;
    reversed.reverse();
    assert_eq!(visited(reversed), ["fix", "issue", "hurry", "a", "b", "c"]);
}

#[test]
fn only_the_newest_undecided_ticket_of_this_host_is_considered() {
    let mut decided = ticket("done", "9", None);
    decided.verdict = Some("landed".to_owned());
    let mut elsewhere = ticket("away", "8", None);
    elsewhere.host = "other".to_owned();
    let tickets = vec![entry(10, ticket("a", "1", None)), entry(20, ticket("a", "2", None)), entry(30, decided), entry(40, elsewhere), entry(50, ticket("b", "3", None))];
    assert_eq!(visited(tickets), ["a", "b"]);
}

struct Fixture {
    _dir: tempfile::TempDir,
    settings: Settings,
    repo: PathBuf,
    calls: PathBuf,
    next: u128,
}

fn commit(repo: &Path, message: &str) -> String {
    git(repo, &["-c", "user.name=lane", "-c", "user.email=lane@example.com", "commit", "-q", "-m", message]).unwrap();
    git(repo, &["rev-parse", "HEAD"]).unwrap()
}

impl Fixture {
    fn new(checker_code: i32, chooser: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_owned();
        let origin = root.join("origin.git");
        git(&root, &["init", "-q", "--bare", "-b", "main", "origin.git"]).unwrap();
        let repo = root.join("proj/main");
        fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]).unwrap();
        for (key, value) in [("user.name", "t"), ("user.email", "t@example.com"), ("commit.gpgsign", "false")] {
            git(&repo, &["config", key, value]).unwrap();
        }
        git(&repo, &["remote", "add", "origin", origin.to_str().unwrap()]).unwrap();
        fs::write(repo.join(".safe-push"), "[land]\n\tcheck = ** :: true\n\tcheck = ** :: false\n").unwrap();
        git(&repo, &["add", "."]).unwrap();
        commit(&repo, "base");
        git(&repo, &["push", "-q", "origin", "main"]).unwrap();
        let calls = root.join("calls.log");
        let script = format!("echo \"$@\" >> {}; exit {checker_code}", calls.display());
        let settings = Settings { repo: repo.clone(), state: root.join("state"), verdicts: root.join("verdicts"), queue: root.join("queue"), host: "box".to_owned(), checker: vec!["sh".to_owned(), "-c".to_owned(), script, "fake-checks".to_owned()], chooser: vec!["sh".to_owned(), "-c".to_owned(), chooser.to_owned()] };
        Self { _dir: dir, settings, repo, calls, next: 1 }
    }

    fn queue(&mut self, name: &str) -> String {
        git(&self.repo, &["checkout", "-q", "-b", name, "main"]).unwrap();
        fs::write(self.repo.join(name), "x\n").unwrap();
        git(&self.repo, &["add", "."]).unwrap();
        let head = commit(&self.repo, name);
        git(&self.repo, &["checkout", "-q", "main"]).unwrap();
        let mut queued = ticket(name, &head, None);
        queued.root.clone_from(&self.repo);
        let path = self.settings.queue.join(queue::file_name(self.next, name));
        self.next += 1;
        queue::write(&path, &queued).unwrap();
        head
    }

    fn calls(&self) -> Vec<String> {
        fs::read_to_string(&self.calls).unwrap_or_default().lines().map(str::to_owned).collect()
    }
}

fn head_of(call: &str) -> String {
    call.split_whitespace().skip_while(|word| *word != "--head").nth(1).unwrap().to_owned()
}

#[test]
fn a_tick_checks_the_oldest_lane_then_the_next_and_skips_a_lane_whose_verdicts_are_stored() {
    let mut fixture = Fixture::new(0, "echo gex45");
    let first = fixture.queue("first");
    fixture.queue("second");
    let Round::Ran { branch, recorded, unrecorded } = tick(&fixture.settings).unwrap() else { panic!("the first tick ran nothing") };
    assert_eq!((branch.as_str(), recorded, unrecorded), ("first", 2, 0));
    let calls = fixture.calls();
    assert_eq!(calls.len(), 2);
    assert!(calls[0].contains("--runner-host gex45") && calls[0].contains("--command true") && calls[1].contains("--command false"), "{calls:?}");
    let candidate_head = head_of(&calls[0]);
    assert_ne!(candidate_head, first);
    assert_eq!(git(&fixture.repo, &["rev-parse", "--verify", "refs/heads/first"]).unwrap(), first);
    let Round::Ran { branch, .. } = tick(&fixture.settings).unwrap() else { panic!("the second tick ran nothing") };
    assert_eq!(branch, "second");
    assert_eq!(fixture.calls().len(), 4);
    assert_ne!(head_of(&fixture.calls()[2]), head_of(&calls[0]));
    assert_eq!(tick(&fixture.settings).unwrap(), Round::Idle);
    assert_eq!(fixture.calls().len(), 4);
}

#[test]
fn a_lane_whose_key_is_already_stored_is_not_checked_again() {
    let mut fixture = Fixture::new(0, "true");
    let head = fixture.queue("lane");
    let worktree = super::candidate::lander_worktree(&fixture.settings.repo, "shadow").unwrap();
    let main = super::candidate::prepare(&fixture.settings.repo, &worktree).unwrap();
    let path = fixture.settings.queue.join(queue::file_name(1, "lane"));
    let logs = fixture.settings.state.join("logs");
    let found = super::run::missing(&fixture.settings, &worktree, &main, &path, &queue::read(&path).unwrap(), &logs).unwrap();
    assert_eq!(found.lines.len(), 2);
    for (_, key) in &found.lines {
        assert_eq!(key.lane_head, head);
        reuse::record(&fixture.settings.verdicts, key, Verdict::Pass).unwrap();
    }
    assert_eq!(tick(&fixture.settings).unwrap(), Round::Idle);
    assert!(fixture.calls().is_empty());
}

#[test]
fn a_failing_check_records_a_fail_and_infrastructure_exits_record_nothing() {
    let mut failing = Fixture::new(1, "true");
    failing.queue("lane");
    assert!(matches!(tick(&failing.settings).unwrap(), Round::Ran { recorded: 2, unrecorded: 0, .. }));
    assert_eq!(tick(&failing.settings).unwrap(), Round::Idle);
    let mut busy = Fixture::new(75, "true");
    busy.queue("lane");
    assert!(matches!(tick(&busy.settings).unwrap(), Round::Ran { recorded: 0, unrecorded: 2, .. }));
    assert!(matches!(tick(&busy.settings).unwrap(), Round::Ran { .. }));
}

#[test]
fn a_round_with_no_host_that_has_room_runs_no_check() {
    let mut fixture = Fixture::new(0, "echo no room >&2; exit 1");
    fixture.queue("lane");
    assert_eq!(tick(&fixture.settings).unwrap(), Round::NoRoom("no room".to_owned()));
    assert!(fixture.calls().is_empty());
}

#[test]
fn a_second_tick_while_one_holds_the_lock_does_nothing() {
    let mut fixture = Fixture::new(0, "true");
    fixture.queue("lane");
    let held = super::lock(&fixture.settings.state).unwrap().unwrap();
    assert_eq!(tick(&fixture.settings).unwrap(), Round::Locked);
    drop(held);
    assert!(matches!(tick(&fixture.settings).unwrap(), Round::Ran { .. }));
}

