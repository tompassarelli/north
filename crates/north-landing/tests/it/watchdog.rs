use std::cell::RefCell;
use std::path::{Path, PathBuf};

use north_landing::land::candidate::git;
use north_landing::land::queue::{self, Ticket};
use north_landing::land::watchdog::{self, Hands, Invariant, Observation, Page, Repair};

const NOW: u64 = 1_800_000_000_000_000;
const MINUTE: u64 = 60_000_000;

#[derive(Default)]
struct Fake {
    units: Vec<String>,
    refuse_pages: bool,
    restarted: RefCell<Vec<String>>,
    reasons: RefCell<Vec<String>>,
    pages: RefCell<Vec<(String, String)>>,
}

impl Hands for Fake {
    fn unit_exists(&self, unit: &str) -> bool {
        self.units.iter().any(|known| known == unit)
    }

    fn restart(&self, unit: &str, why: &str) -> Result<(), String> {
        self.restarted.borrow_mut().push(unit.to_owned());
        self.reasons.borrow_mut().push(why.to_owned());
        Ok(())
    }

    fn page(&self, session: &str, text: &str) -> bool {
        self.pages.borrow_mut().push((session.to_owned(), text.to_owned()));
        !self.refuse_pages
    }
}

struct Fixture {
    _dir: tempfile::TempDir,
    project: PathBuf,
    repo: PathBuf,
    root: PathBuf,
    queue: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("wisp");
        let repo = project.join("main");
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]).unwrap();
        for (key, value) in [("user.name", "t"), ("user.email", "t@example.com"), ("commit.gpgsign", "false")] {
            git(&repo, &["config", key, value]).unwrap();
        }
        git(&repo, &["commit", "-q", "--allow-empty", "-m", "seed"]).unwrap();
        let root = dir.path().join("land-queue");
        let queue = root.join("0123abcd");
        std::fs::create_dir_all(&queue).unwrap();
        Self { _dir: dir, project, repo, root, queue }
    }

    fn lander(&self) -> PathBuf {
        self.project.join("worktrees/lander")
    }

    fn worktree(&self, path: &Path, branch: &str) {
        let target = path.to_str().unwrap();
        if git(&self.repo, &["rev-parse", "-q", "--verify", &format!("refs/heads/{branch}")]).is_ok() {
            git(&self.repo, &["worktree", "add", "-q", "-f", target, branch]).unwrap();
        } else {
            git(&self.repo, &["worktree", "add", "-q", "-b", branch, target]).unwrap();
        }
    }

    fn round(&self, at: u64, slots: usize, busy: usize, waiting: usize, problem: Option<&str>) {
        let observation = Observation { at_micros: at, repo: self.repo.clone(), worktree: self.lander(), slots, busy, waiting, paused: false, hosts_full: false, problem: problem.map(str::to_owned) };
        assert!(watchdog::observe(&self.queue, observation, false).unwrap());
    }

    fn lane(&self, queued: u64, branch: &str) -> PathBuf {
        let path = self.queue.join(queue::file_name(u128::from(queued), branch));
        let ticket = Ticket { origin: "git@example.com:o/wisp.git".to_owned(), root: self.repo.clone(), host: "h".to_owned(), branch: branch.to_owned(), head: "0".repeat(40), queued_at: String::new(), ..Ticket::default() };
        queue::write(&path, &ticket).unwrap();
        path
    }

    fn patrol(&self, now: u64, hands: &Fake) -> watchdog::Report {
        watchdog::patrol(&self.root, &watchdog::queues(&self.root), now, hands).unwrap()
    }

    fn quarantined(&self) -> Vec<Ticket> {
        let mut found: Vec<PathBuf> = std::fs::read_dir(self.queue.join(watchdog::QUARANTINE)).map(|entries| entries.map(|entry| entry.unwrap().path()).collect()).unwrap_or_default();
        found.sort();
        found.iter().map(|path| queue::read(path).unwrap()).collect()
    }
}

fn invariants(report: &watchdog::Report) -> Vec<Invariant> {
    report.violations.iter().map(|violation| violation.invariant).collect()
}

#[test]
fn a_check_slot_idle_while_lanes_wait_for_two_rounds_restarts_the_repository_loop_and_pages_once() {
    let fixture = Fixture::new();
    let hands = Fake { units: vec!["north-land@wisp.service".to_owned()], ..Fake::default() };
    fixture.lane(NOW - MINUTE, "lane/a");
    fixture.round(NOW - 4_000_000, 2, 0, 1, None);
    assert!(fixture.patrol(NOW, &hands).clean());
    fixture.round(NOW - 2_000_000, 2, 0, 1, None);
    let report = fixture.patrol(NOW, &hands);
    assert_eq!(invariants(&report), vec![Invariant::IdleSlot]);
    assert_eq!(report.violations[0].repairs, vec![Repair::RestartedLoop("north-land@wisp.service".to_owned())]);
    assert_eq!(*hands.restarted.borrow(), vec!["north-land@wisp.service".to_owned()]);
    assert!(hands.reasons.borrow()[0].contains("2 of 2 check slots idled while 1 lanes waited"), "{:?}", hands.reasons.borrow());
    assert!(matches!(&report.pages[..], [(Invariant::IdleSlot, Page::Sent(text))] if text.contains("2 of 2 check slots idled while 1 lanes waited")));
    assert_eq!(hands.pages.borrow()[0].0, "lander-lead");
    assert!(watchdog::rounds(&fixture.queue).is_empty());
    let printed = report.to_string();
    assert!(printed.contains("violated (a)"), "{printed}");
    assert!(printed.contains("repair: restarted north-land@wisp.service"), "{printed}");
    assert!(printed.contains("page: sent to lander-lead"), "{printed}");
    fixture.round(NOW + MINUTE, 2, 1, 3, None);
    fixture.round(NOW + 2 * MINUTE, 2, 1, 3, None);
    let again = fixture.patrol(NOW + 3 * MINUTE, &hands);
    assert_eq!(invariants(&again), vec![Invariant::IdleSlot]);
    assert!(matches!(again.pages[0].1, Page::Held { paged_micros: NOW, .. }));
    assert_eq!(hands.pages.borrow().len(), 1);
    fixture.round(NOW + 16 * MINUTE, 2, 1, 3, None);
    fixture.round(NOW + 17 * MINUTE, 2, 1, 3, None);
    let later = fixture.patrol(NOW + 18 * MINUTE, &hands);
    assert!(matches!(later.pages[0].1, Page::Sent(_)));
    assert_eq!(hands.pages.borrow().len(), 2);
}

#[test]
fn an_idle_slot_for_one_round_a_full_house_or_a_paused_queue_holds_the_invariant() {
    let fixture = Fixture::new();
    let hands = Fake::default();
    fixture.round(NOW - 6_000_000, 2, 2, 4, None);
    fixture.round(NOW - 4_000_000, 2, 2, 4, None);
    fixture.round(NOW - 2_000_000, 2, 0, 4, None);
    assert!(fixture.patrol(NOW, &hands).clean());
    fixture.round(NOW - 1_000_000, 2, 0, 0, None);
    assert!(fixture.patrol(NOW, &hands).clean());
    for at in [NOW, NOW + 1] {
        let observation = Observation { at_micros: at, repo: fixture.repo.clone(), worktree: fixture.lander(), slots: 2, busy: 0, waiting: 3, paused: true, hosts_full: false, problem: None };
        watchdog::observe(&fixture.queue, observation, false).unwrap();
    }
    assert!(fixture.patrol(NOW + 2, &hands).clean());
    assert!(hands.pages.borrow().is_empty());
}

#[test]
fn a_repeated_error_naming_a_stray_lander_worktree_removes_it_and_recreates_the_lander() {
    let fixture = Fixture::new();
    let hands = Fake::default();
    let stray = fixture.project.join("worktrees/worktrees/lander");
    fixture.worktree(&stray, "land-candidate");
    let problem = format!("git worktree: fatal: 'land-candidate' is already used by worktree at '{}'", stray.display());
    for at in 1..=3 {
        fixture.round(NOW - at, 0, 0, 1, Some(&problem));
    }
    let report = fixture.patrol(NOW, &hands);
    assert_eq!(invariants(&report), vec![Invariant::RepeatedError]);
    assert_eq!(report.violations[0].repairs, vec![Repair::RemovedWorktree(stray.clone())]);
    assert!(!stray.exists());
    assert!(!git(&fixture.repo, &["worktree", "list", "--porcelain"]).unwrap().contains(stray.to_str().unwrap()));
    assert!(report.to_string().contains("repair: removed the stray lander worktree"));
    assert!(hands.restarted.borrow().is_empty());
}

#[test]
fn a_repeated_error_about_the_lander_worktree_itself_recreates_it() {
    let fixture = Fixture::new();
    let hands = Fake::default();
    fixture.worktree(&fixture.lander(), "land-candidate");
    std::fs::write(fixture.lander().join(".git"), "gitdir: /nowhere\n").unwrap();
    let problem = format!("transient: git -C {} fetch: fatal: not a git repository", fixture.lander().display());
    for at in 1..=3 {
        fixture.round(NOW - at, 0, 0, 0, Some(&problem));
    }
    let report = fixture.patrol(NOW, &hands);
    assert_eq!(report.violations[0].repairs, vec![Repair::RecreatedWorktree(fixture.lander())]);
    assert!(git(&fixture.lander(), &["rev-parse", "HEAD"]).is_ok());
}

#[test]
fn a_repeated_error_restarts_the_repository_loop_when_its_unit_exists() {
    let fixture = Fixture::new();
    let hands = Fake { units: vec!["north-land@wisp.service".to_owned()], ..Fake::default() };
    for at in 1..=3 {
        fixture.round(NOW - at, 0, 0, 1, Some("transient: ssh gex45 exited 255: connection reset"));
    }
    fixture.lane(NOW - MINUTE, "lane/a");
    let report = fixture.patrol(NOW, &hands);
    assert_eq!(report.violations[0].repairs, vec![Repair::RestartedLoop("north-land@wisp.service".to_owned())]);
    assert_eq!(queue::pending(&fixture.queue).unwrap().len(), 1);
}

#[test]
fn a_repeated_error_without_a_unit_quarantines_the_repository_with_the_reason() {
    let fixture = Fixture::new();
    let hands = Fake::default();
    fixture.lane(NOW - MINUTE, "lane/a");
    fixture.lane(NOW - 2 * MINUTE, "lane/b");
    for at in 1..=3 {
        fixture.round(NOW - at, 0, 0, 2, Some("candidate build: conflict in Cargo.lock"));
    }
    let report = fixture.patrol(NOW, &hands);
    assert_eq!(report.violations[0].repairs, vec![Repair::QuarantinedRepository(2)]);
    assert!(queue::pending(&fixture.queue).unwrap().is_empty());
    let quarantined = fixture.quarantined();
    assert_eq!(quarantined.len(), 2);
    assert!(quarantined.iter().all(|ticket| ticket.verdict.as_deref().is_some_and(|verdict| verdict.contains("(b)") && verdict.contains("conflict in Cargo.lock"))));
    assert!(watchdog::rounds(&fixture.queue).is_empty());
}

#[test]
fn two_errors_or_differing_errors_hold_the_invariant() {
    let fixture = Fixture::new();
    let hands = Fake::default();
    fixture.round(NOW - 3, 0, 0, 0, Some("one"));
    fixture.round(NOW - 2, 0, 0, 0, Some("two"));
    fixture.round(NOW - 1, 0, 0, 0, Some("two"));
    assert!(fixture.patrol(NOW, &hands).clean());
    fixture.round(NOW, 0, 0, 0, None);
    assert!(fixture.patrol(NOW, &hands).clean());
}

#[test]
fn a_lane_older_than_twenty_minutes_is_quarantined_with_the_reason() {
    let fixture = Fixture::new();
    let hands = Fake::default();
    let stale = fixture.lane(NOW - 21 * MINUTE, "lane/stale");
    let fresh = fixture.lane(NOW - 19 * MINUTE, "lane/fresh");
    let report = fixture.patrol(NOW, &hands);
    assert_eq!(invariants(&report), vec![Invariant::StaleLane]);
    let moved = fixture.queue.join(watchdog::QUARANTINE).join(stale.file_name().unwrap());
    assert_eq!(report.violations[0].repairs, vec![Repair::QuarantinedLane(moved.clone())]);
    assert!(!stale.exists());
    assert!(fresh.exists());
    let verdict = queue::read(&moved).unwrap().verdict.unwrap();
    assert!(verdict.contains("(c)") && verdict.contains("lane lane/stale (") && verdict.contains("waited 21 minutes"), "{verdict}");
    assert!(matches!(&report.pages[..], [(Invariant::StaleLane, Page::Sent(text))] if text.contains("lane/stale")));
}

#[test]
fn a_failed_page_is_retried_on_the_next_round() {
    let fixture = Fixture::new();
    let refusing = Fake { refuse_pages: true, ..Fake::default() };
    fixture.lane(NOW - 30 * MINUTE, "lane/a");
    let report = fixture.patrol(NOW, &refusing);
    assert!(matches!(report.pages[0].1, Page::Failed(_)));
    assert!(report.to_string().contains("page: failed to reach lander-lead"));
    fixture.lane(NOW - 30 * MINUTE + 1, "lane/b");
    let hands = Fake::default();
    let retried = fixture.patrol(NOW + 1, &hands);
    assert!(matches!(retried.pages[0].1, Page::Sent(_)));
}

#[test]
fn a_branch_claimed_by_two_worktrees_recreates_the_lander_claimant() {
    let fixture = Fixture::new();
    let hands = Fake::default();
    let person = fixture.project.join("worktrees/feature");
    fixture.worktree(&person, "topic");
    fixture.worktree(&fixture.lander(), "topic");
    fixture.round(NOW, 0, 0, 0, None);
    let report = fixture.patrol(NOW, &hands);
    assert_eq!(invariants(&report), vec![Invariant::SharedBranch]);
    assert!(report.violations[0].detail.contains("branch topic is claimed by"));
    assert_eq!(report.violations[0].repairs, vec![Repair::RecreatedWorktree(fixture.lander())]);
    assert!(watchdog::shared_branches(&fixture.repo).is_empty());
    assert!(fixture.lander().join(".git").exists());
    assert!(person.join(".git").exists());
}

#[test]
fn a_branch_shared_by_two_worktrees_of_people_is_paged_without_a_repair() {
    let fixture = Fixture::new();
    let hands = Fake::default();
    fixture.worktree(&fixture.project.join("worktrees/one"), "topic");
    fixture.worktree(&fixture.project.join("worktrees/two"), "topic");
    fixture.round(NOW, 0, 0, 0, None);
    let report = fixture.patrol(NOW, &hands);
    assert_eq!(invariants(&report), vec![Invariant::SharedBranch]);
    assert!(matches!(&report.violations[0].repairs[..], [Repair::Nothing(_)]));
    assert_eq!(watchdog::shared_branches(&fixture.repo).len(), 1);
    assert_eq!(hands.pages.borrow().len(), 1);
}

#[test]
fn the_standalone_command_requires_once_and_passes_a_clean_root() {
    let fixture = Fixture::new();
    let root = fixture.root.to_str().unwrap().to_owned();
    assert_eq!(north_landing::land::run(&["watchdog".to_owned(), "--queue-root".to_owned(), root.clone()]), 64);
    assert_eq!(north_landing::land::run(&["watchdog".to_owned(), "--once".to_owned(), "--queue-root".to_owned(), root]), 0);
}

#[test]
fn slots_idle_only_because_every_check_host_is_full_do_not_restart_the_loop() {
    let fixture = Fixture::new();
    let hands = Fake { units: vec!["north-land@wisp.service".to_owned()], ..Fake::default() };
    fixture.lane(NOW - MINUTE, "lane/a");
    for at in [NOW - 4_000_000, NOW - 2_000_000] {
        let observation = Observation { at_micros: at, repo: fixture.repo.clone(), worktree: fixture.lander(), slots: 8, busy: 7, waiting: 4, paused: false, hosts_full: true, problem: None };
        assert!(watchdog::observe(&fixture.queue, observation, false).unwrap());
    }
    assert!(fixture.patrol(NOW, &hands).clean());
    assert!(hands.restarted.borrow().is_empty());
}

#[test]
fn an_urgent_lane_is_aged_from_when_its_ticket_was_written_not_its_ordering_prefix() {
    let fixture = Fixture::new();
    let hands = Fake::default();
    let path = fixture.queue.join(queue::file_name(1_700_000_000_000_033, "lane/urgent"));
    let ticket = Ticket { origin: "git@example.com:o/wisp.git".to_owned(), root: fixture.repo.clone(), host: "h".to_owned(), branch: "lane/urgent".to_owned(), head: "0".repeat(40), ..Ticket::default() };
    queue::write(&path, &ticket).unwrap();
    let written = std::time::UNIX_EPOCH + std::time::Duration::from_micros(NOW - 5 * MINUTE);
    std::fs::File::options().write(true).open(&path).unwrap().set_modified(written).unwrap();
    assert!(fixture.patrol(NOW, &hands).clean());
    assert!(path.exists());
}
