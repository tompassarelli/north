use std::fs::{self, File};
use std::path::Path;
use std::process::{Command, Output};
use std::time::{Duration, SystemTime};

fn git(directory: &Path, args: &[&str]) {
    let status = Command::new("git").args(args).current_dir(directory).status().unwrap();
    assert!(status.success(), "git {args:?}");
}

fn lane_gc(scratch: &Path, now: SystemTime) -> Output {
    let stubs = scratch.join("stubs");
    fs::create_dir_all(&stubs).unwrap();
    let north = stubs.join("north");
    fs::write(&north, "#!/bin/sh\necho '[]'\n").unwrap();
    fs::set_permissions(&north, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let stamp = Command::new("date").args(["-u", "-d", &format!("@{}", now.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs()), "+%FT%TZ"]).output().unwrap();
    Command::new(std::env::current_dir().unwrap().join("bin/lane-gc"))
        .args(["--root"]).arg(scratch.join("code"))
        .args(["--repo", "north", "--dry-run", "--state"]).arg(scratch.join("unlanded.txt"))
        .env("HOME", scratch)
        .env("GIT_CONFIG_GLOBAL", scratch.join("gitconfig"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("LANE_GC_STALE_FILE", scratch.join("stale.json"))
        .env("LANE_GC_HANDOFF_DIR", scratch.join("handoffs"))
        .env("LANE_GC_NOW", String::from_utf8(stamp.stdout).unwrap().trim())
        .env("PATH", format!("{}:{}", stubs.display(), std::env::var("PATH").unwrap()))
        .output()
        .unwrap()
}

#[test]
fn a_merged_detached_lane_retires_although_a_branch_was_created_in_the_repo_within_the_idle_window() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    fs::write(root.join("gitconfig"), "[user]\n\tname = fixture\n\temail = fixture@example.invalid\n[init]\n\tdefaultBranch = main\n[advice]\n\tdetachedHead = false\n").unwrap();
    let origin = root.join("origin.git");
    let main = root.join("code/north/main");
    fs::create_dir_all(&origin).unwrap();
    fs::create_dir_all(&main).unwrap();
    fs::create_dir_all(root.join("code/north/worktrees")).unwrap();
    git(&origin, &["init", "-q", "--bare"]);
    git(&main, &["init", "-q"]);
    fs::write(main.join("readme"), "main\n").unwrap();
    git(&main, &["add", "readme"]);
    git(&main, &["commit", "-qm", "main"]);
    git(&main, &["remote", "add", "origin", origin.to_str().unwrap()]);
    git(&main, &["push", "-q", "origin", "main"]);
    git(&main, &["fetch", "-q", "origin"]);
    git(&main, &["worktree", "add", "-q", "--detach", root.join("code/north/worktrees/base").to_str().unwrap(), "main"]);
    let now = SystemTime::now() + Duration::from_secs(30 * 3600);
    File::open(main.join(".git/logs/refs/heads")).unwrap().set_modified(now - Duration::from_secs(1800)).unwrap();

    let output = lane_gc(root, now);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("retire ~/code/north/worktrees/base (detached, ancestor)"), "{stdout}\n{}", String::from_utf8_lossy(&output.stderr));
}
