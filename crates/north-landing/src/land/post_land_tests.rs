use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::process::ExitStatus;
use std::time::Duration;

use super::{Landed, Run, execute, judged, record};
use crate::land::checks::Verdict;

fn run() -> Run {
    Run { runner: Vec::new(), worktree: PathBuf::new(), log: PathBuf::from("post-land.log"), outcome: PathBuf::from("absent"), ledger: PathBuf::new(), offload_owner: String::new(), capacity: String::new(), deadline: Duration::from_secs(60) }
}

// KEEP(bug): on 2026-11-06 a check host that could not find the laptop's runtime path ended an after-landing run with exit 127 and the run was reported as red main, putting innocent lanes up for revert (north#634)
#[test]
fn a_post_land_run_whose_command_was_not_found_or_not_executable_reaches_no_verdict() {
    for code in [126, 127] {
        let verdict = judged(ExitStatus::from_raw(code << 8), &run());
        assert!(matches!(verdict, Verdict::Transient(_)), "exit {code} became {verdict:?}");
    }
    assert!(matches!(judged(ExitStatus::from_raw(1 << 8), &run()), Verdict::RedMain(_)), "a check that failed is still red main");
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
