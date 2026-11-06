use std::cell::RefCell;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::time::{Duration, Instant};

use north_landing::land::queue::{self, Ticket};

use super::*;
use crate::bounds::{OLDEST_QUEUED_SECS, ROUND_SILENCE_SECS, SLOT_DEADLINE_SECS};
use crate::facts::{LinkFacts, PlaceFacts, Probe, QueueFacts, SlotHold};

const NOW: u64 = 1_800_000_000;

#[derive(Default)]
struct Fake {
    applied: RefCell<Vec<Remedy>>,
    incidents: RefCell<Vec<(String, String)>>,
    pages: RefCell<Vec<String>>,
}

impl Hands for Fake {
    fn apply(&self, remedy: &Remedy) -> Result<String, String> {
        self.applied.borrow_mut().push(remedy.clone());
        match remedy {
            Remedy::Rerun { queue, ticket } => hands::rerun(queue, ticket),
            Remedy::Prune { .. } => hands::Host.apply(remedy),
            Remedy::Rollback { home, from, to, .. } => hands::roll_back(home, from, to, &[]),
            _ => Ok("done".to_owned()),
        }
    }

    fn incident(&self, class: &str, key: &str, _: &str, _: &str) -> bool {
        self.incidents.borrow_mut().push((class.to_owned(), key.to_owned()));
        true
    }

    fn page(&self, text: &str) -> bool {
        self.pages.borrow_mut().push(text.to_owned());
        true
    }
}

fn gex45() -> Place {
    Place { name: "gex45".to_owned(), ssh: Some("gex45".to_owned()) }
}

fn here() -> Place {
    Place { name: "here".to_owned(), ssh: None }
}

fn at(place: Place, probe: Probe) -> Facts {
    Facts { places: vec![PlaceFacts { place, probe: Ok(probe) }], ..Facts::default() }
}

fn roomy() -> Probe {
    Probe { free_percent: 60.0, free_gib: 300.0, ..Probe::default() }
}

fn busy_queue(silence: u64) -> QueueFacts {
    QueueFacts {
        name: "north".to_owned(),
        queue: PathBuf::from("/queue"),
        unit: "north-land@north.service".to_owned(),
        oldest_queued: Some(OLDEST_QUEUED_SECS * 2),
        round_silence: Some(silence),
        ..QueueFacts::default()
    }
}

fn git(directory: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git").args(arguments).current_dir(directory).output().unwrap();
    assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn the_probe_lines_parse_into_free_disk_held_slots_and_stale_registrations_and_the_report_prints_each_bound() {
    let probe = facts::parse_probe("disk 750 250\nslot code-north-main-1 3600 991\nslot code-wisp-main 10 -\nprunable 3 /home/tom/code/north/main\n").unwrap();
    assert_eq!(probe.free_percent, 25.0);
    assert_eq!(probe.slots, [SlotHold { slot: "code-north-main-1".to_owned(), held: 3600, pgid: Some(991) }, SlotHold { slot: "code-wisp-main".to_owned(), held: 10, pgid: None }]);
    assert_eq!(probe.prunable, [(PathBuf::from("/home/tom/code/north/main"), 3)]);
    assert!(facts::parse_probe("").is_err());
    let link = LinkFacts { lander: "runtimes/a".to_owned(), current: "runtimes/a".to_owned(), lag: Some(60), ..LinkFacts::default() };
    let facts = Facts { queues: vec![QueueFacts { since_landing: Some(60), since_decision: Some(60), ..busy_queue(30) }], link: Some(link), places: at(gex45(), roomy()).places };
    let text = render(&cure(&facts, &mut Book::default(), &Fake::default(), NOW));
    for bound in ["oldest-queued-lane north: 40 min", "since-landing north: 1 min", "round-silence north: 0 min", "link-lag lander: 1 min", "link-held lander: 0 min", "slot-hold gex45: no slot held", "disk-free gex45: 60% free", "stale-worktrees gex45: 0", "infrastructure-failures north: 0"] {
        assert!(text.contains(&format!("bound {bound}")), "{bound} missing from:\n{text}");
    }
}

#[test]
fn a_missing_registered_worktree_that_git_lists_prunable_is_pruned() {
    let scratch = tempfile::tempdir().unwrap();
    let repo = scratch.path().join("repo");
    fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--allow-empty", "-m", "base"]);
    let slot = scratch.path().join("slot");
    git(&repo, &["worktree", "add", "-q", "--detach", slot.to_str().unwrap()]);
    fs::remove_dir_all(&slot).unwrap();
    let listed = git(&repo, &["worktree", "list", "--porcelain"]);
    let stale = listed.lines().filter(|line| line.starts_with("prunable")).count();
    let probe = facts::parse_probe(&format!("disk 1 9\nprunable {stale} {}\n", repo.display())).unwrap();
    let hands = Fake::default();
    let treated = cure(&at(here(), probe), &mut Book::default(), &hands, NOW);
    assert_eq!(*hands.applied.borrow(), vec![Remedy::Prune { place: here(), repo: repo.clone() }]);
    assert!(render(&treated).contains("applied \"prune the missing worktree registrations"), "{}", render(&treated));
    assert_eq!(hands.incidents.borrow()[0].0, "lander-stale-worktree");
    assert!(!git(&repo, &["worktree", "list", "--porcelain"]).contains("prunable"));
}

#[test]
fn a_slot_past_its_deadline_is_released_by_ending_the_process_group_its_slot_files_name() {
    let root = tempfile::tempdir().unwrap();
    let slot = "code-north-main-2";
    let lock = root.path().join(format!("{slot}.lock"));
    let job = Command::new("bash").args(["-c", "exec 9>\"$1\"; flock -n 9; exec sleep 30", "slot-job"]).arg(&lock).process_group(0).spawn().unwrap();
    let pgid = job.id();
    let reaper = std::thread::spawn(move || {
        let mut job = job;
        job.wait()
    });
    let started = Instant::now();
    while Command::new("flock").args(["-n"]).arg(&lock).arg("true").status().unwrap().success() && started.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(10));
    }
    for (suffix, text) in [("pgid", format!("{pgid}\n")), ("holder", "0123456789ab since 13:00:00\n".to_owned()), ("owner", "north-land\n".to_owned())] {
        fs::write(root.path().join(format!("{slot}.{suffix}")), text).unwrap();
    }
    let held = SlotHold { slot: slot.to_owned(), held: SLOT_DEADLINE_SECS + 60, pgid: Some(pgid) };
    let fresh = SlotHold { slot: "code-north-main".to_owned(), held: 120, pgid: Some(77) };
    let hands = Fake::default();
    cure(&at(gex45(), Probe { slots: vec![held, fresh], ..roomy() }), &mut Book::default(), &hands, NOW);
    assert_eq!(*hands.applied.borrow(), vec![Remedy::Release { place: gex45(), slot: slot.to_owned(), pgid }]);
    assert_eq!(hands.incidents.borrow()[0].0, "lander-slot-overdue");
    let said = facts::shell(&here(), hands::RELEASE, &[slot, &pgid.to_string(), &root.path().to_string_lossy()]).unwrap();
    assert_eq!(said.trim(), format!("ended process group {pgid}"));
    assert!(!reaper.join().unwrap().unwrap().success(), "the job ended on a signal");
    assert!(facts::shell(&here(), hands::RELEASE, &[slot, &pgid.to_string(), &root.path().to_string_lossy()]).is_err(), "a released slot is not released twice");
    assert!(!root.path().join(format!("{slot}.pgid")).exists() && !root.path().join(format!("{slot}.holder")).exists());
}

fn ticket(branch: &str, verdict: Option<String>) -> Ticket {
    Ticket {
        origin: "git@github.com:tompassarelli/north.git".to_owned(),
        root: PathBuf::from("/lane"),
        host: "whiterabbit".to_owned(),
        branch: branch.to_owned(),
        head: "0123456789abcdef0123456789abcdef01234567".to_owned(),
        session: None,
        queued_at: "2026-11-06T13:00:00Z".to_owned(),
        queued_micros: None,
        first_commit_secs: None,
        several_commits: None,
        red_fix: None,
        verdict,
        keep_lane: false,
        urgent: None,
        decided_on: None,
        published: None,
    }
}

#[test]
fn a_check_refused_by_an_infrastructure_failure_its_log_names_is_rerun_once() {
    let scratch = tempfile::tempdir().unwrap();
    let queue_dir = scratch.path().join("queue");
    fs::create_dir_all(queue_dir.join("done")).unwrap();
    let log = scratch.path().join("land-check.log");
    fs::write(&log, "fatal: '/home/tom/.cache/land-offload/code-north-main' is a missing but already registered worktree;\n").unwrap();
    let plain = scratch.path().join("plain.log");
    fs::write(&plain, "test result: FAILED. 1 passed; 1 failed\n").unwrap();
    let name = queue::file_name(1_793_000_000_000_000, "n634-lane");
    let done = queue_dir.join("done").join(&name);
    queue::write(&done, &ticket("n634-lane", Some(format!("failed land check cargo test exited 128; log: {}", log.display())))).unwrap();
    queue::write(&queue_dir.join("done").join(queue::file_name(1_793_000_000_000_001, "red-lane")), &ticket("red-lane", Some(format!("failed land check cargo test exited 101; log: {}", plain.display())))).unwrap();
    let now = facts::now();
    let found = facts::queue_facts("north", &queue_dir, Path::new("/home/tom/code/north/main"), now);
    assert_eq!(found.infrastructure.iter().map(|failed| failed.branch.as_str()).collect::<Vec<_>>(), ["n634-lane"]);
    let hands = Fake::default();
    cure(&Facts { queues: vec![found], ..Facts::default() }, &mut Book::default(), &hands, now);
    assert_eq!(hands.incidents.borrow()[0].0, "lander-infrastructure-failure");
    let requeued = queue::read(&queue_dir.join(&name)).unwrap();
    assert_eq!((requeued.branch.as_str(), requeued.verdict), ("n634-lane", None));
    assert!(!done.exists());
    queue::write(&done, &ticket("n634-lane", Some(format!("failed land check again; log: {}", log.display())))).unwrap();
    fs::remove_file(queue_dir.join(&name)).unwrap();
    let again = facts::queue_facts("north", &queue_dir, Path::new("/home/tom/code/north/main"), now);
    assert!(again.infrastructure.is_empty(), "a ticket reruns once: {again:?}");
}

#[test]
fn disk_pressure_evicts_caches_and_each_remedy_and_unknown_state_writes_one_incident_row_and_one_page() {
    let facts = at(gex45(), Probe { free_percent: 4.0, free_gib: 12.0, ..Probe::default() });
    let mut book = Book::default();
    let hands = Fake::default();
    cure(&facts, &mut book, &hands, NOW);
    assert_eq!(*hands.applied.borrow(), vec![Remedy::Evict { place: gex45() }]);
    let treated = cure(&facts, &mut book, &hands, NOW + 120);
    assert!(treated.iter().any(|entry| entry.outcomes.iter().any(|outcome| outcome.starts_with("waiting: applied"))));
    cure(&facts, &mut book, &hands, NOW + PATIENCE_SECS + 1);
    cure(&facts, &mut book, &hands, NOW + PATIENCE_SECS + 120);
    assert_eq!(hands.applied.borrow().len(), 1);
    assert_eq!(hands.pages.borrow().len(), 1);
    assert!(hands.pages.borrow()[0].starts_with("north land doctor: unknown lander state: disk-free gex45 reads 4% free (12 GiB) (bound 10% free): "));
    let rows = hands.incidents.borrow().clone();
    assert_eq!(rows, [("lander-disk-pressure".to_owned(), format!("lander-disk-pressure:disk-free:gex45@{NOW}#1")), (UNKNOWN.to_owned(), format!("{UNKNOWN}:disk-free:gex45@{NOW}"))]);
    let unserved = QueueFacts { name: "clause".to_owned(), unit: "north-land@clause.service".to_owned(), unit_state: Some(facts::MISSING_UNIT.to_owned()), ..busy_queue(ROUND_SILENCE_SECS * 50) };
    let healthy = Facts::default();
    cure(&healthy, &mut book, &hands, NOW + 900);
    for later in [NOW + 1000, NOW + 1120] {
        cure(&Facts { queues: vec![unserved.clone()], ..Facts::default() }, &mut book, &hands, later);
    }
    assert_eq!(hands.pages.borrow().len(), 2, "one page per episode: {:?}", hands.pages.borrow());
    assert!(hands.pages.borrow()[1].contains("no north-land@clause.service unit exists"));
}

#[test]
fn a_wedged_lander_is_restarted_then_rolled_back_to_the_last_build_that_landed_a_batch_green_and_promote_reads_it_refused() {
    let home = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink("runtimes/bad", home.path().join("lander")).unwrap();
    let link = LinkFacts {
        home: home.path().to_owned(),
        lander: "runtimes/bad".to_owned(),
        current: "runtimes/bad".to_owned(),
        builds: ["runtimes/bad", "runtimes/good"].map(str::to_owned).into_iter().collect::<BTreeSet<_>>(),
        units: vec!["north-land@north.service".to_owned()],
        ..LinkFacts::default()
    };
    let stopped = QueueFacts { unit_state: Some("inactive".to_owned()), ..busy_queue(ROUND_SILENCE_SECS + 60) };
    let hands = Fake::default();
    assert!(render(&cure(&Facts { queues: vec![stopped], ..Facts::default() }, &mut Book::default(), &hands, NOW)).contains("over: stopped"));
    assert!(hands.applied.borrow().is_empty(), "a lander stopped on purpose stays stopped");
    let wedged = Facts { queues: vec![busy_queue(ROUND_SILENCE_SECS + 60)], link: Some(link), ..Facts::default() };
    let mut book = Book { good: vec!["runtimes/good".to_owned()], ..Book::default() };
    cure(&wedged, &mut book, &hands, NOW);
    cure(&wedged, &mut book, &hands, NOW + 120);
    cure(&wedged, &mut book, &hands, NOW + PATIENCE_SECS + 60);
    let rollback = Remedy::Rollback { home: home.path().to_owned(), from: "runtimes/bad".to_owned(), to: "runtimes/good".to_owned(), units: vec!["north-land@north.service".to_owned()] };
    assert_eq!(*hands.applied.borrow(), vec![Remedy::Restart { unit: "north-land@north.service".to_owned() }, rollback]);
    assert_eq!(hands.incidents.borrow().iter().map(|(class, _)| class.as_str()).collect::<Vec<_>>(), ["lander-wedged", "lander-rollback"]);
    assert_eq!(fs::read_link(home.path().join("lander")).unwrap(), Path::new("runtimes/good"));
    assert_eq!(fs::read_to_string(home.path().join(REFUSED)).unwrap(), "runtimes/bad\n");
}

#[test]
fn a_build_is_good_once_a_green_after_landing_run_in_the_ledger_started_after_its_link_moved() {
    let scratch = tempfile::tempdir().unwrap();
    let ledger = scratch.path().join("post-land.tsv");
    fs::write(&ledger, format!("{}\tnorth\tabc\tgreen\t\tlane\t100\t\n{}\tnorth\tdef\tred-main\tx\tlane\t50\t\n", NOW - 1000, NOW - 10)).unwrap();
    assert!(facts::green_since(&ledger, NOW - 1200));
    assert!(!facts::green_since(&ledger, NOW - 1050), "the green run started before the link moved");
    let mut book = Book::default();
    book.learn(&LinkFacts { lander: "runtimes/a".to_owned(), green_since_link: true, ..LinkFacts::default() });
    book.learn(&LinkFacts { lander: "runtimes/b".to_owned(), green_since_link: false, ..LinkFacts::default() });
    assert_eq!(book.good, ["runtimes/a"]);
}
