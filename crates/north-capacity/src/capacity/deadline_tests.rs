use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use super::{Clock, Lease, NO_VERDICT, TIMED_OUT, guard};

fn clock(root: &Path, driven: &Path, wall: bool) -> Clock {
    Clock { lease: Lease { file: None, root: root.to_owned() }, timeout: Duration::from_secs(2), wall, started: Instant::now(), driven: Some(driven.to_owned()), first: None, paused: Duration::ZERO, queued: Duration::ZERO, queued_since: None }
}

fn judged(driven: &Path, milliseconds: u64) -> bool {
    fs::read_to_string(driven.join(format!("seen.{}", std::process::id()))).is_ok_and(|seen| seen == milliseconds.to_string())
}

fn wait_until(what: &str, ready: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !ready() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn queued_job(root: &Path, wall: bool) -> (tempfile::TempDir, std::thread::JoinHandle<u8>) {
    let driven = tempfile::tempdir().unwrap();
    fs::write(driven.path().join("now"), "0").unwrap();
    let ticket = root.join("queue").join("ticket.json");
    let script = format!("printf '{{\"wrapperPid\": %s}}' $$ > {}; exec sleep 60", ticket.display());
    let clock = clock(root, driven.path(), wall);
    let job = std::thread::spawn(move || tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(guard(clock, "sh", &["-c".to_owned(), script])));
    wait_until("the job to take its queue ticket", || ticket.exists());
    let seen = driven.path().join(format!("seen.{}", std::process::id()));
    let _ = fs::remove_file(&seen);
    wait_until("the clock to judge its start with the ticket queued", || seen.exists());
    (driven, job)
}

// KEEP(bug): on 2026-11-06 gex45 slot jobs with 900 to 1,800 s deadlines held code-north-main slots for 6,300 to 7,000 s, because the deadline stopped its clock while a capacity gate beneath it sat in the queue (north#634)
#[test]
fn a_slot_job_queued_past_its_wall_deadline_is_killed_with_no_verdict_where_a_queue_excused_clock_waits() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("queue")).unwrap();
    let (excused, held) = queued_job(root.path(), false);
    fs::write(excused.path().join("now"), "3000").unwrap();
    wait_until("the queue-excused clock to judge 3 s", || judged(excused.path(), 3000));
    assert!(!held.is_finished(), "a queue-excused clock ended a job that was only queued");
    fs::remove_file(root.path().join("queue/ticket.json")).unwrap();
    fs::write(excused.path().join("now"), "3500").unwrap();
    wait_until("the queue-excused clock to judge 3.5 s with the ticket gone", || judged(excused.path(), 3500));
    assert!(!held.is_finished(), "the 3.5 s spent queued still excuses the job");
    fs::write(excused.path().join("now"), "10000").unwrap();
    assert_eq!(held.join().unwrap(), TIMED_OUT, "a queue-excused clock still ends a job that ran its 2 s");

    let (driven, slot) = queued_job(root.path(), true);
    fs::write(driven.path().join("now"), "3000").unwrap();
    assert_eq!(slot.join().unwrap(), NO_VERDICT, "a slot job past its wall deadline while queued ends with no verdict");
}
