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
fn a_lane_older_than_twenty_minutes_pages_its_wait_and_stays_queued() {
    let fixture = Fixture::new();
    let hands = Fake::default();
    let stale = fixture.lane(NOW - 21 * MINUTE, "lane/stale");
    let fresh = fixture.lane(NOW - 19 * MINUTE, "lane/fresh");
    let report = fixture.patrol(NOW, &hands);
    assert_eq!(invariants(&report), vec![Invariant::StaleLane]);
    assert!(matches!(&report.violations[0].repairs[..], [Repair::Nothing(_)]), "{:?}", report.violations[0].repairs);
    assert!(stale.exists() && queue::read(&stale).unwrap().verdict.is_none());
    assert!(fresh.exists());
    assert!(fixture.quarantined().is_empty());
    assert!(report.violations[0].detail.contains("lane lane/stale (") && report.violations[0].detail.contains("waited 21 minutes"), "{}", report.violations[0].detail);
    assert!(matches!(&report.pages[..], [(Invariant::StaleLane, Page::Sent(text))] if text.contains("lane/stale")));
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
