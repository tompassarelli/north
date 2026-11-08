use std::path::{Path, PathBuf};
use std::process::Command;

use landlock::RulesetStatus;

use super::{Context, confine, removable};

const COMMAND: &str = "NORTH_706_LOCKS_COMMAND";

fn git(directory: &Path, arguments: &[&str]) {
    let output = Command::new("git").arg("-C").arg(directory).args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "protocol.file.allow=always"]).args(arguments).output().unwrap();
    assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
}

fn locks(under: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(under).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() && !path.is_symlink() {
            found.extend(locks(&path));
        } else if path.extension().is_some_and(|extension| extension == "lock") {
            found.push(path);
        }
    }
    found
}

#[test]
fn a_confined_command_child() {
    let Some(command) = std::env::var_os(COMMAND) else {
        return;
    };
    let command = command.to_string_lossy().into_owned();
    assert!(matches!(confine(&removable(&Context::from_env(&command))), Ok(RulesetStatus::FullyEnforced)), "Landlock does not enforce here");
    let output = Command::new("bash").arg("-c").arg(&command).output().unwrap();
    assert!(output.status.success(), "{command}: {}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn a_sandboxed_git_fetch_leaves_no_lock_in_a_common_dir_it_cannot_remove_north_706() {
    let outside_tmp = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
    std::fs::create_dir_all(&outside_tmp).unwrap();
    let scratch = tempfile::Builder::new().prefix("north-706-locks").tempdir_in(outside_tmp).unwrap();
    let home = scratch.path().canonicalize().unwrap();
    let remote = home.join("remote.git");
    let base = home.join("code/alpha/worktrees/base");
    let beta = home.join("code/beta/main");
    std::fs::create_dir_all(&base).unwrap();
    std::fs::create_dir_all(beta.parent().unwrap()).unwrap();
    git(&home, &["init", "-q", "--bare", "-b", "main", "remote.git"]);
    git(&base, &["init", "-q", "-b", "main"]);
    git(&base, &["commit", "-q", "--allow-empty", "-m", "one"]);
    git(&base, &["remote", "add", "origin", remote.to_str().unwrap()]);
    git(&base, &["push", "-q", "origin", "main"]);
    git(&base, &["worktree", "add", "-q", "../../main", "-b", "trunk"]);
    git(&base, &["worktree", "add", "-q", "../lane", "-b", "lane"]);
    git(&home, &["clone", "-q", remote.to_str().unwrap(), beta.to_str().unwrap()]);
    git(&base, &["commit", "-q", "--allow-empty", "-m", "two"]);
    git(&base, &["push", "-q", "origin", "main"]);
    assert!(home.join("code/alpha/main/.git").is_file(), "the alpha checkout's .git must be a gitfile");

    let lane = home.join("code/alpha/worktrees/lane");
    let own = "git fetch -q origin".to_owned();
    let named = format!("cd {} && git fetch -q origin", beta.display());
    for command in [own, named] {
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "worker_sandbox::tests::a_confined_command_child", "--nocapture"])
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap())
            .env("HOME", &home)
            .env(COMMAND, &command)
            .current_dir(&lane)
            .output()
            .unwrap();
        assert!(output.status.success(), "{command}: {}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        assert_eq!(locks(&home), Vec::<PathBuf>::new(), "{command} left a lock behind");
    }
    let fetched = |repository: &Path| Command::new("git").arg("-C").arg(repository).args(["rev-list", "--count", "origin/main"]).output().unwrap().stdout;
    assert_eq!(fetched(&base), b"2\n");
    assert_eq!(fetched(&beta), b"2\n");
}

#[test]
fn a_command_run_from_main_may_build_in_the_done_when_lane_north_task_check_uses() {
    let outside_tmp = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
    std::fs::create_dir_all(&outside_tmp).unwrap();
    let scratch = tempfile::Builder::new().prefix("north-done-when-lane").tempdir_in(outside_tmp).unwrap();
    let home = scratch.path().canonicalize().unwrap();
    let main = home.join("code/alpha/main");
    std::fs::create_dir_all(&main).unwrap();
    std::fs::create_dir_all(home.join("code/alpha/worktrees")).unwrap();
    let lane = "../worktrees/done-when/target";
    let command = format!("mkdir -p {lane}/a && mv {lane}/a {lane}/b && rm -r {lane}");
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "worker_sandbox::tests::a_confined_command_child", "--nocapture"])
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap())
        .env("HOME", &home)
        .env(COMMAND, &command)
        .current_dir(&main)
        .output()
        .unwrap();
    assert!(output.status.success(), "{command}: {}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
}
