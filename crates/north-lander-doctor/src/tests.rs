use north_base::test_inputs::git::git_in as git;
use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::os::unix::process::CommandExt;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

use north_lander::land::queue::{self, Ticket};

use super::*;
use crate::bounds::{OLDEST_QUEUED_SECS, ROUND_SILENCE_SECS, SLOT_DEADLINE_SECS};
use crate::facts::{LinkFacts, ReviewHold, PlaceFacts, Probe, QueueFacts, Reach};

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
            Remedy::RemoveQueue { queue } => hands::remove_queue_in(queue.parent().unwrap_or(queue), queue),
            Remedy::Prune { .. } => hands::Host.apply(remedy),
            Remedy::Rollback { home, from, to, .. } => hands::roll_back(home, from, to, &[]),
            Remedy::Alarm(text) => Ok(format!("paged: {}", self.page(text))),
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
    Place { name: "gex45".to_owned(), reach: Reach::Ssh("gex45".to_owned()) }
}

#[test]
fn a_stray_land_queue_on_an_offload_host_is_removed_and_one_that_holds_a_ticket_a_lander_or_a_foreign_name_is_kept() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path().join("north/land-queue");
    let stray = root.join("8733aa0a99314382");
    fs::create_dir_all(stray.join("logs")).unwrap();
    let ticketed = root.join("de2e0e8479546ba9");
    fs::create_dir_all(ticketed.join("done")).unwrap();
    fs::write(ticketed.join("done/1-x.json"), "{}").unwrap();
    let served = root.join("0123456789abcdef");
    fs::create_dir_all(&served).unwrap();
    fs::write(served.join("lander.lock"), "").unwrap();
    let named = root.join("worker");
    fs::create_dir_all(&named).unwrap();

    assert!(hands::remove_queue_at(&here(), &stray).unwrap().contains("removed"), "the stray queue is removed");
    assert!(!stray.exists());
    assert!(hands::remove_queue_at(&here(), &ticketed).is_err() && ticketed.join("done/1-x.json").is_file());
    assert!(hands::remove_queue_at(&here(), &served).is_err() && served.is_dir());
    assert!(hands::remove_queue_at(&here(), &named).is_err() && named.is_dir());
}

fn here() -> Place {
    Place { name: "here".to_owned(), reach: Reach::Here }
}

fn at(place: Place, probe: Probe) -> Facts {
    Facts { places: vec![PlaceFacts { place, probe: Ok(probe) }], ..Facts::default() }
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

struct SlotJob {
    child: std::process::Child,
}

impl SlotJob {
    fn hold(lock: &Path) -> Self {
        let mut child = Command::new("bash").args(["-c", "exec 9>\"$1\"; flock 9; exec 4>>\"${1%.lock}.busy\"; flock -s 4; echo held; exec sleep 30", "slot-job"]).arg(lock).process_group(0).stdout(Stdio::piped()).spawn().unwrap();
        let mut said = String::new();
        BufReader::new(child.stdout.take().unwrap()).read_line(&mut said).unwrap();
        let job = Self { child };
        assert_eq!(said.trim(), "held", "the fixture job took no lock on {}", lock.display());
        job
    }

    fn pgid(&self) -> u32 {
        self.child.id()
    }
}

impl Drop for SlotJob {
    fn drop(&mut self) {
        let _ = Command::new("kill").args(["-KILL", "--", &format!("-{}", self.pgid())]).stderr(Stdio::null()).status();
        let _ = self.child.wait();
    }
}

fn age_holder(root: &Path, slot: &str) {
    let long_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(SLOT_DEADLINE_SECS * 60);
    fs::File::options().write(true).open(root.join(format!("{slot}.holder"))).unwrap().set_modified(long_ago).unwrap();
}

fn probed(root: &Path) -> Probe {
    let output = Command::new("bash").args(["-c", &facts::probe()]).env("NORTH_OFFLOAD_ROOT", root).env("HOME", root).output().unwrap();
    let probe = facts::parse_probe(&String::from_utf8_lossy(&output.stdout)).unwrap();
    Probe { free_percent: 60.0, free_gib: 300.0, size_gib: 500.0, ..probe }
}

#[test]
fn a_holder_file_whose_lock_is_free_is_removed_under_the_lock_and_a_locked_slot_without_a_group_stays_unknown() {
    let root = tempfile::tempdir().unwrap();
    let stale = "code-wisp-worktrees-extract-20261005";
    for suffix in ["holder", "pgid", "owner", "lock"] {
        fs::write(root.path().join(format!("{stale}.{suffix}")), "left\n").unwrap();
    }
    age_holder(root.path(), stale);
    let hands = Fake::default();
    let treated = cure(&at(gex45(), probed(root.path())), &mut Book::default(), &hands, NOW);
    assert_eq!(*hands.applied.borrow(), vec![Remedy::Clear { place: gex45(), slot: stale.to_owned() }], "{}", render(&treated));
    assert_eq!(hands.incidents.borrow().iter().map(|(class, _)| class.as_str()).collect::<Vec<_>>(), ["lander-slot-overdue"]);
    assert!(hands.pages.borrow().is_empty(), "the coordinator heard of a slot nothing holds");
    let clear = |slot: &str| facts::shell(&here(), hands::CLEAR, &[slot, &root.path().to_string_lossy()]);
    assert_eq!(clear(stale).unwrap().trim(), format!("removed the holder files of slot {stale}, whose lock is free"));
    assert!(["holder", "pgid", "owner"].iter().all(|suffix| !root.path().join(format!("{stale}.{suffix}")).exists()));
    assert!(probed(root.path()).slots.is_empty());

    let held = "code-held-main";
    let _job = SlotJob::hold(&root.path().join(format!("{held}.lock")));
    fs::write(root.path().join(format!("{held}.holder")), "left\n").unwrap();
    age_holder(root.path(), held);
    let locked = probed(root.path());
    assert_eq!(locked.slots.iter().map(|slot| (slot.locked, slot.pgid)).collect::<Vec<_>>(), [(true, None)]);
    assert!(clear(held).is_err(), "a held slot is never cleared");
    let paged = Fake::default();
    cure(&at(gex45(), locked), &mut Book::default(), &paged, NOW);
    assert!(paged.applied.borrow().is_empty() && paged.pages.borrow().len() == 1, "{:?}", paged.pages.borrow());
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
        deferral: None,
        keep_lane: false,
        urgent: None,
        urgent_at: None,
        lander_fix: None,
        decided_on: None,
        published: None,
        alone_green: None,
        alone_base: None,
        alone_unsettled: None,
        unjudged_on: None,
        gated_from: None,
        hand_in: None,
        failed_on_main: Vec::new(),
        tests_after_landing: false,
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
    let queues = [("north".to_owned(), queue_dir.clone())];
    assert_eq!(crate::host_gate::busy(&queues), ["north"], "a requeued ticket keeps the host gate waiting");
    assert_eq!((requeued.branch.as_str(), requeued.verdict), ("n634-lane", None));
    assert!(!done.exists());
    queue::write(&done, &ticket("n634-lane", Some(format!("failed land check again; log: {}", log.display())))).unwrap();
    fs::remove_file(queue_dir.join(&name)).unwrap();
    assert!(crate::host_gate::busy(&queues).is_empty(), "a queue holding only decided tickets lets the host gate go on");
    let again = facts::queue_facts("north", &queue_dir, Path::new("/home/tom/code/north/main"), now);
    assert!(again.infrastructure.is_empty(), "a ticket reruns once: {again:?}");
}

#[test]
fn disk_pressure_evicts_caches_and_each_remedy_and_unknown_state_writes_one_incident_row_and_one_page() {
    let large = Fake::default();
    cure(&at(gex45(), Probe { free_percent: 9.4, free_gib: 351.0, size_gib: 3726.0, ..Probe::default() }), &mut Book::default(), &large, NOW);
    assert!(large.applied.borrow().is_empty(), "a 3.7 TB disk at 351 GiB free sits over the 130 GiB its reaper holds, so no eviction is due (north#1545)");
    let facts = at(gex45(), Probe { free_percent: 4.0, free_gib: 12.0, size_gib: 300.0, ..Probe::default() });
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
    assert!(hands.pages.borrow()[0].starts_with("north land doctor: unknown lander state: disk-free gex45 reads 4% free (12 GiB) (bound the free GiB its lease reaper holds): "));
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
fn a_running_land_check_that_builds_a_toolchain_from_source_tells_the_coordinator_once() {
    let scratch = tempfile::tempdir().unwrap();
    let logs = scratch.path();
    let nix = include_str!("../../../tests/fixtures/lander-doctor/offline-binutils.log");
    let hour_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
    fs::write(logs.join("post-land-a.outcome"), "").unwrap();
    fs::File::options().write(true).open(logs.join("post-land-a.outcome")).unwrap().set_modified(hour_ago).unwrap();
    fs::write(logs.join("post-land-a.log"), nix).unwrap();
    fs::write(logs.join("post-land-b.log"), nix).unwrap();
    fs::write(logs.join("post-land-b.outcome"), "").unwrap();
    fs::write(logs.join("2-candidate.log"), nix.replace(" --offline", "")).unwrap();
    let found = facts::source_builds(logs, facts::now());
    assert_eq!(found.iter().map(|build| (build.log.file_name().unwrap().to_string_lossy().into_owned(), build.derivation.as_str(), build.offline)).collect::<Vec<_>>(), [("2-candidate.log".to_owned(), "binutils-2.46", false), ("post-land-a.log".to_owned(), "binutils-2.46", true)]);
    assert!(render(&cure(&Facts { queues: vec![QueueFacts { source_builds: vec![found[0].clone()], since_landing: Some(60), since_decision: Some(60), ..busy_queue(30) }], ..Facts::default() }, &mut Book::default(), &Fake::default(), NOW)).contains("without substituters, on a nixpkgs pin"));
    let facts = Facts { queues: vec![QueueFacts { source_builds: vec![found[1].clone()], since_landing: Some(60), since_decision: Some(60), ..busy_queue(30) }], ..Facts::default() };
    let hands = Fake::default();
    let mut book = Book::default();
    let text = render(&cure(&facts, &mut book, &hands, NOW));
    assert!(text.contains("bound toolchain-from-source north/post-land-a.log: 1 (binutils-2.46) (limit 0): over: toolchain built from source"), "{text}");
    assert_eq!(hands.pages.borrow().len(), 1);
    assert!(hands.pages.borrow()[0].contains(&format!("builds binutils-2.46 from source ({}), a toolchain the binary cache supplies: its check line runs nix with --offline", logs.join("post-land-a.log").display())), "{:?}", hands.pages.borrow());
    assert_eq!(hands.incidents.borrow()[0].0, "lander-source-build");
    cure(&facts, &mut book, &hands, NOW + 2 * PATIENCE_SECS);
    assert_eq!(hands.pages.borrow().len(), 1, "a second run in the same episode paged again");
}

#[test]
fn a_runtime_pinned_behind_a_review_hold_names_the_hold_once_and_pages_no_one() {
    let hold = ReviewHold { commit: "02bb1eeff35d93c6d4602c2dac8d8ade3f556589".to_owned(), session: "commit-reviewer".to_owned(), why: "the eval start reads defaults it never wrote".to_owned() };
    let link = LinkFacts { lander: "runtimes/3c75dbf3e2".to_owned(), current: "runtimes/3c75dbf3e2".to_owned(), lag: Some(3 * 60 * 60), review_hold: Some(hold), ..LinkFacts::default() };
    let hands = Fake::default();
    let mut book = Book::default();
    let shown = render(&cure(&Facts { link: Some(link.clone()), ..Facts::default() }, &mut book, &hands, NOW));
    cure(&Facts { link: Some(link), ..Facts::default() }, &mut book, &hands, NOW + REPAGE_SECS);
    assert_eq!(shown.lines().filter(|line| line.contains("02bb1eeff3") && line.contains("commit-reviewer")).count(), 1, "{shown}");
    assert!(!shown.contains("does not follow main"), "{shown}");
    assert!(hands.pages.borrow().is_empty(), "a runtime pinned on purpose pages no one: {:?}", hands.pages.borrow());
}

#[test]
fn a_lander_on_a_descendant_of_the_fleet_pin_is_on_current_and_pages_no_one() {
    let scratch = tempfile::tempdir().unwrap();
    let checkout = scratch.path().join("north");
    fs::create_dir_all(&checkout).unwrap();
    git(&checkout, &["init", "-q"]);
    let commit = |message: &str| {
        git(&checkout, &["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false", "commit", "-q", "--allow-empty", "-m", message]);
        git(&checkout, &["rev-parse", "--short=10", "HEAD"]).trim().to_owned()
    };
    let (pin, newer) = (commit("pin"), commit("newer"));
    let home = scratch.path().join("state");
    fs::create_dir_all(&home).unwrap();
    std::os::unix::fs::symlink(format!("runtimes/{newer}"), home.join("lander")).unwrap();
    std::os::unix::fs::symlink(format!("runtimes/{pin}"), home.join("current")).unwrap();
    let link = facts::link_facts(&home, Some(&checkout), Vec::new(), NOW, &[]).unwrap();
    assert!(link.behind.is_none(), "{:?}", link.behind);
    let hands = Fake::default();
    let shown = render(&cure(&Facts { link: Some(link), ..Facts::default() }, &mut Book::default(), &hands, NOW));
    assert!(shown.contains("bound link-behind lander: on current"), "{shown}");
    assert!(hands.pages.borrow().is_empty(), "{:?}", hands.pages.borrow());
}

#[test]
fn the_evict_remedy_reports_the_disk_pass_exit_and_last_line_when_it_fails() {
    let home = tempfile::tempdir().unwrap();
    let bin = home.path().join(".local/bin");
    fs::create_dir_all(&bin).unwrap();
    let north = bin.join("north");
    fs::write(&north, "#!/bin/sh\necho 'north capacity: disk pass reclaim failed: Permission denied' >&2\nexit 3\n").unwrap();
    fs::set_permissions(&north, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let place = Place { name: "here".to_owned(), reach: Reach::Scratch { env: vec![("HOME".to_owned(), home.path().to_string_lossy().into_owned())], namespace: None } };
    let said = hands::Host.apply(&Remedy::Evict { place }).unwrap();
    assert!(said.contains("disk pass exited 3: north capacity: disk pass reclaim failed: Permission denied"), "{said}");
}
