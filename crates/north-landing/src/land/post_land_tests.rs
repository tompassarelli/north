use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::process::ExitStatus;
use std::time::Duration;

use super::{Landed, Run, announce, execute, judged, record, settle};
use crate::land::checks::Verdict;
use crate::landing::red_check::Failure;

const SWITCHED: &str = "thread 'main' panicked at crates/north-base/src/platform/harness.rs:16:\nnorth: runtime 3c5a1869 has no tree and the checkout ~/code/north/main is at a3ecc7a51; run north runtime update\n";

fn run() -> Run {
    Run { runner: Vec::new(), worktree: PathBuf::new(), log: PathBuf::from("post-land.log"), outcome: PathBuf::from("absent"), ledger: PathBuf::new(), offload_owner: String::new(), capacity: String::new(), deadline: Duration::from_secs(60), host: None }
}

// KEEP(bug): on 2026-11-06 a check host that could not find the laptop's runtime path ended an after-landing run with exit 127 and the run was reported as red main, putting innocent lanes up for revert (north#662)
#[test]
fn a_post_land_run_is_red_main_only_when_a_check_recorded_a_failing_verdict_whatever_the_log_says() {
    for code in [126, 127] {
        let verdict = judged(ExitStatus::from_raw(code << 8), &run());
        assert!(matches!(verdict, Verdict::Transient(_)), "exit {code} became {verdict:?}");
    }
    let scratch = tempfile::tempdir().unwrap();
    let mut switched = run();
    switched.log = scratch.path().join("post-land.log");
    switched.outcome = scratch.path().join("post-land.outcome");
    assert!(matches!(judged(ExitStatus::from_raw(1 << 8), &switched), Verdict::Transient(_)), "a run that failed before any check reached a verdict is not red main");
    std::fs::write(&switched.log, "error: a message no list of infrastructure signatures has seen\n").unwrap();
    assert!(matches!(judged(ExitStatus::from_raw(1 << 8), &switched), Verdict::Transient(_)), "an unseen infrastructure message reaches no verdict");
    std::fs::write(&switched.log, SWITCHED).unwrap();
    assert!(matches!(judged(ExitStatus::from_raw(101 << 8), &switched), Verdict::Transient(_)), "a runtime switch under the check reaches no verdict");
    std::fs::write(&switched.log, "fatal: '/home/tom/.cache/land-offload/code-north-main-4' is a missing but already registered worktree;\n").unwrap();
    assert!(matches!(judged(ExitStatus::from_raw(128 << 8), &switched), Verdict::Transient(_)), "an evicted build slot reaches no verdict");
    std::fs::write(&switched.log, "bash: line 1: north: command not found\n").unwrap();
    assert!(matches!(judged(ExitStatus::from_raw(1 << 8), &switched), Verdict::Transient(_)), "a missing tool reaches no verdict");
    std::fs::write(&switched.log, "test land::x ... FAILED\n").unwrap();
    std::fs::write(&switched.outcome, "event\tland check\tfailed\tcargo clippy; log: /x.log\nfailed-step\tcargo clippy\t/x.log\n").unwrap();
    assert!(matches!(judged(ExitStatus::from_raw(101 << 8), &switched), Verdict::RedMain(_)), "a check that recorded its failing verdict is red main");
    assert_eq!(super::failing(&switched.log, &switched.outcome), std::collections::BTreeSet::from([Failure::Cargo("land::x".to_owned()), Failure::Step("cargo clippy".to_owned())]));
    assert_eq!(judged(ExitStatus::from_raw(0), &run()), Verdict::Green);
}

fn landed(heads: &[&str]) -> Landed {
    let heads: Vec<String> = heads.iter().map(|head| (*head).to_owned()).collect();
    Landed { repo: PathBuf::new(), repository: "owner/repo".to_owned(), base: "base".to_owned(), head: heads.last().cloned().unwrap_or_default(), heads, lanes: vec!["lane-a".to_owned(), "lane-b".to_owned()], tips: Vec::new() }
}

// KEEP(bug): on 2026-11-06 coalescing made one after-landing run cover every journaled landing, so a run stuck on a build host held every later verdict (north#634)
#[test]
fn an_after_landing_run_past_its_deadline_is_killed_with_its_group_reaches_no_verdict_and_records_every_covered_head() {
    let scratch = tempfile::tempdir().unwrap();
    let grandchild = scratch.path().join("grandchild");
    let mut stuck = run();
    stuck.runner = vec!["sh".to_owned(), "-c".to_owned(), format!("sleep 600 & echo $! > {}; wait", grandchild.display()), "stuck".to_owned()];
    stuck.worktree = scratch.path().to_owned();
    stuck.log = scratch.path().join("post-land.log");
    stuck.outcome = scratch.path().join("post-land.outcome");
    stuck.ledger = scratch.path().join("post-land.tsv");
    stuck.deadline = Duration::from_secs(1);
    let landed = landed(&["b", "c", "d"]);
    let started = std::time::Instant::now();
    let checked = execute("sh", &stuck.runner[1..], &landed, "d", &stuck);
    record(&stuck, &landed, &checked.verdict, checked.expired, 1);
    assert!(started.elapsed() < Duration::from_secs(20), "the deadline ended the run");
    assert!(checked.expired && matches!(checked.verdict, Verdict::Transient(_)), "{:?}", checked.verdict);
    let pid = rustix::process::Pid::from_raw(std::fs::read_to_string(&grandchild).unwrap().trim().parse().unwrap()).unwrap();
    let gone = (0..100).any(|_| {
        let dead = process_ended(pid.as_raw_nonzero().get());
        if !dead {
            std::thread::sleep(Duration::from_millis(50));
        }
        dead
    });
    assert!(gone, "the run's whole process group was killed");
    let rows = std::fs::read_to_string(&stuck.ledger).unwrap();
    let rows: Vec<Vec<&str>> = rows.lines().map(|row| row.split('\t').collect()).collect();
    assert_eq!(rows.iter().map(|row| row[2]).collect::<Vec<_>>(), ["b", "c", "d"]);
    assert!(rows.iter().all(|row| row[3] == "transient" && row[5] == "lane-a,lane-b"), "{rows:?}");
}

fn process_ended(pid: i32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat")).map_or(true, |stat| stat.rsplit_once(") ").is_some_and(|(_, rest)| rest.starts_with('Z')))
}


fn committed_repo(directory: &std::path::Path) -> String {
    let git = |arguments: &[&str]| {
        let output = std::process::Command::new("git").arg("-C").arg(directory).args(["-c", "user.name=t", "-c", "user.email=t@example.com", "-c", "commit.gpgsign=false"]).args(arguments).output().unwrap();
        assert!(output.status.success(), "{arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    };
    git(&["init", "-q", "-b", "main"]);
    std::fs::write(directory.join("README"), "x\n").unwrap();
    git(&["add", "README"]);
    git(&["commit", "-q", "-m", "start"]);
    git(&["rev-parse", "HEAD"])
}

fn a_check_that_a_runtime_switch_killed_is_rerun_once_and_neither_alarms_nor_records_a_red_main() {
    let scratch = tempfile::tempdir().unwrap();
    let repo = scratch.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let head = committed_repo(&repo);
    let counter = scratch.path().join("ran");
    let script = format!("if [ -e {counter} ]; then exit 0; fi; : > {counter}; printf '%s\\n' \"north: runtime 3c5a1869 has no tree and the checkout /x is at a3ecc7a51; run north runtime update\"; exit 101", counter = counter.display());
    let mut switching = run();
    switching.runner = vec!["sh".to_owned(), "-c".to_owned(), script, "check".to_owned()];
    switching.worktree = scratch.path().join("post-land");
    switching.log = scratch.path().join("post-land.log");
    switching.outcome = scratch.path().join("post-land.outcome");
    switching.ledger = scratch.path().join("post-land.tsv");
    let mut landed = landed(&[&head]);
    landed.repo = repo;
    landed.head = head;
    let alarms = std::cell::RefCell::new(Vec::new());
    let verdict = settle(&landed, &switching, |_, message| {
        alarms.borrow_mut().push(message.to_owned());
        true
    });
    assert_eq!(verdict, Verdict::Green);
    assert!(alarms.borrow().is_empty(), "{:?}", alarms.borrow());
    assert!(counter.exists());
}

// KEEP(bug): on 2026-11-06 main stayed red for over an hour with the proxy learning it only from Tom or its own watch (north#634)
#[test]
fn the_first_red_main_verdict_of_an_episode_messages_the_proxy_with_the_suspect_the_failing_tests_and_the_log_and_green_messages_once() {
    a_check_that_a_runtime_switch_killed_is_rerun_once_and_neither_alarms_nor_records_a_red_main();
    let scratch = tempfile::tempdir().unwrap();
    let mut episode = run();
    episode.log = scratch.path().join("post-land.log");
    episode.ledger = scratch.path().join("post-land.tsv");
    std::fs::write(&episode.log, "---- land::probe::tests::capacity_agent_compile stdout ----\nthread panicked\n").unwrap();
    let landed = landed(&["abc123"]);
    let sent = std::cell::RefCell::new(Vec::new());
    let alarm = |session: &str, message: &str| {
        sent.borrow_mut().push((session.to_owned(), message.to_owned()));
        true
    };
    announce(&landed, &episode, &Verdict::RedMain("exit 1".to_owned()), alarm);
    announce(&landed, &episode, &Verdict::RedMain("exit 1".to_owned()), alarm);
    assert_eq!(sent.borrow().len(), 1, "a second red verdict of the episode is silent: {:?}", sent.borrow());
    let (session, message) = sent.borrow()[0].clone();
    assert_eq!(session, "proxy");
    assert!(message.contains("land::probe::tests::capacity_agent_compile") && message.contains("suspect commit abc123") && message.contains(&episode.log.display().to_string()) && message.contains("lane-a, lane-b"), "{message}");
    announce(&landed, &episode, &Verdict::Green, alarm);
    announce(&landed, &episode, &Verdict::Green, alarm);
    assert_eq!(sent.borrow().len(), 2, "green after the episode messages once: {:?}", sent.borrow());
    assert!(sent.borrow()[1].1.contains("main is green again"), "{:?}", sent.borrow());
    announce(&landed, &episode, &Verdict::RedMain("exit 1".to_owned()), alarm);
    assert_eq!(sent.borrow().len(), 3, "a new episode messages again");
}
