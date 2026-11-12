use std::os::unix::process::CommandExt;
use std::process::Command;

use super::super::notes::Manifest;
use super::super::processes::{self, Process};
use super::sweep;

#[test]
fn a_follow_started_by_the_walk_is_gone_before_cleanup_counts_processes() {
    let root = tempfile::Builder::new().prefix("verify-north-").tempdir_in(std::env::temp_dir()).unwrap().keep();
    let repo = root.join("project/main");
    let mut follow = Command::new("sh")
        .args(["-c", "sleep 600; :", "north", "land", "follow", "--repo"])
        .arg(&repo)
        .args(["--head", "0123456789abcdef"])
        .process_group(0)
        .spawn()
        .unwrap();
    let shell = Process::of(follow.id()).unwrap();
    let gates = super::super::exec::wait_for(&super::super::exec::Wall, std::time::Duration::from_secs(5), std::time::Duration::from_millis(20), || Ok::<_, String>(processes::descendants(&[shell]).first().copied())).unwrap().unwrap();
    let manifest = Manifest { scratch: Some(root.clone()), ..Manifest::default() };
    let outcome = sweep(&manifest);
    let _ = follow.wait();
    assert!(outcome.is_ok(), "{outcome:?}");
    assert!(!gates.alive());
    assert!(!root.exists());
}
