use std::fs;
use std::path::Path;
use std::process::Command;

use super::{BOUND_SECONDS, current, fresh, refresh, south};

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
}

fn land(origin: &Path, body: &str) {
    fs::write(origin.join("agents/orchestration.toml"), body).unwrap();
    git(origin, &["add", "-A"]);
    git(origin, &["commit", "-q", "-m", "policy"]);
}

#[test]
fn a_host_refuses_configuration_older_than_main_by_more_than_the_bound() {
    let home = tempfile::tempdir().unwrap();
    let origin = home.path().join("origin");
    fs::create_dir_all(origin.join("agents/skills")).unwrap();
    git(&origin, &["init", "-q", "-b", "main"]);
    fs::write(origin.join("agents/skills/kept-out.md"), "skill").unwrap();
    land(&origin, "[hosts.gex45]\n");
    fs::create_dir_all(home.path().join("code/south")).unwrap();
    git(&home.path().join("code/south"), &["clone", "-q", origin.to_str().unwrap(), "main"]);
    land(&origin, "[hosts.gex45]\n[hosts.cpx62]\nprofiles = [\"claude-a\"]\n");

    let stamp = refresh(home.path(), &south(home.path()), 1_000).unwrap();

    let checkout = fs::read_to_string(south(home.path()).join("agents/orchestration.toml")).unwrap();
    let read = fs::read_to_string(current(home.path()).join("orchestration.toml")).unwrap();
    assert!(!checkout.contains("cpx62"));
    assert!(read.contains("[hosts.cpx62]"));
    assert!(!current(home.path()).join("kept-out.md").exists());
    assert_eq!(fresh(home.path(), 1_000 + BOUND_SECONDS), Ok(stamp));
    let stale = fresh(home.path(), 1_001 + BOUND_SECONDS).unwrap_err();
    assert!(stale.contains("past the 60 min bound"), "{stale}");
}
