use std::fs;
use std::path::Path;
use std::process::Command;

use north_base::foundation::exit::NO_VERDICT;
use north_landing::land::checks::Verdict;
use north_landing::land::post_land::{Landed, Run, settle};

const LOCK: &str = "error: cannot lock ref 'refs/remotes/origin/main': is at aaa but expected bbb";

fn git(repo: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git").arg("-C").arg(repo).args(["-c", "user.name=t", "-c", "user.email=t@example.com", "-c", "commit.gpgsign=false"]).args(arguments).output().unwrap();
    assert!(output.status.success(), "{arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

#[test]
fn a_ref_lock_failure_in_an_after_landing_check_is_rerun_and_judged_environment() {
    let scratch = tempfile::tempdir().unwrap();
    let repo = scratch.path().join("repo");
    fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    fs::write(repo.join("README"), "x\n").unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "x"]);
    let head = git(&repo, &["rev-parse", "HEAD"]);
    let (log, outcome, runs) = (scratch.path().join("post-land.log"), scratch.path().join("post-land.outcome"), scratch.path().join("runs"));
    let script = format!(
        "echo run >> {runs}; if [ \"$(wc -l < {runs})\" -eq 1 ]; then echo \"{LOCK}\"; printf 'failed-step\\tbash bin/catalog-owner-check\\t/x.log\\n' > {outcome}; exit 1; fi; exit 0",
        runs = runs.display(),
        outcome = outcome.display(),
    );
    let run = Run { runner: vec!["sh".into(), "-c".into(), script, "post-land".into()], worktree: scratch.path().join("probe"), log: log.clone(), outcome, ledger: scratch.path().join("post-land.tsv"), store: scratch.path().join("store.db"), offload_owner: String::new(), capacity: String::new(), deadline: super::serve::HANG, host: None, cancel: Default::default(), adoptable: false };
    let landed = Landed { repo, repository: "owner/repo".into(), base: head.clone(), head: head.clone(), heads: vec![head.clone()], lanes: vec!["lane".into()], tips: Vec::new(), ran: Vec::new() };
    let verdict = settle(&landed, &run, |_, _| true);
    assert_eq!(verdict, Verdict::Green, "the rerun passed, so main is not red");
    assert_eq!(fs::read_to_string(&runs).unwrap().lines().count(), 2, "the check ran once more before main was judged");

    fs::write(&log, format!("{LOCK}\n")).unwrap();
    let judged = north_lander::landing::red_check::run(&["--main".into(), head, "--log".into(), log.display().to_string(), "--lane".into(), "lane".into()]);
    assert_eq!(judged, NO_VERDICT, "red-check answers environment rather than blaming the lane");
}
