use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use super::{KeptLane, free_command, kept_lanes};

fn fake_ssh(directory: &Path, body: &str) -> std::path::PathBuf {
    let path = directory.join("ssh");
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[test]
fn a_kept_lane_ends_the_free_step_without_failure_and_is_recorded_while_other_ends_still_fail() {
    let directory = tempfile::tempdir().unwrap();
    let kept = directory.path().join("kept");
    let lane = Path::new("/code/north/worktrees/n1202-x");
    let run = |body: &str| free_command(&fake_ssh(directory.path(), body), "build-1", "gex45", lane, &kept).output().unwrap();

    let ended = run("echo 'kept /code/north/worktrees/n1202-x: holds 2 unlanded commits' >&2; exit 1");
    assert!(ended.status.success(), "{ended:?}");
    assert!(String::from_utf8_lossy(&ended.stdout).contains("lane /code/north/worktrees/n1202-x on gex45 kept: holds 2 unlanded commits"));
    let found = kept_lanes(&kept, 0);
    assert_eq!(
        found,
        [KeptLane { at_secs: found[0].at_secs, machine: "gex45".into(), lane: lane.display().to_string(), reason: "holds 2 unlanded commits".into() }]
    );

    let unreachable = run("echo 'ssh: connect to host build-1: Connection timed out' >&2; exit 255");
    assert_eq!(unreachable.status.code(), Some(255));
    assert!(String::from_utf8_lossy(&unreachable.stderr).contains("Connection timed out"));
    let usage = run("echo usage >&2; exit 2");
    assert_eq!(usage.status.code(), Some(2));
    assert_eq!(kept_lanes(&kept, 0).len(), 1, "a failure is no kept lane");
}
