use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::process::ExitStatus;

use super::{Run, judged};
use crate::land::checks::Verdict;

fn run() -> Run {
    Run { runner: Vec::new(), worktree: PathBuf::new(), log: PathBuf::from("post-land.log"), outcome: PathBuf::from("absent"), ledger: PathBuf::new(), offload_owner: String::new(), capacity: String::new() }
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
