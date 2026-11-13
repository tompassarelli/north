use std::fs;
use std::path::Path;
use std::process::Command;

use super::{BOUND_SECONDS, checkout, current, fresh, overdue, refresh, warned};

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
    let root = checkout(home.path());
    fs::create_dir_all(root.parent().unwrap()).unwrap();
    git(home.path(), &["clone", "-q", origin.to_str().unwrap(), root.to_str().unwrap()]);
    land(&origin, "[hosts.gex45]\n[hosts.cpx62]\nprofiles = [\"claude-a\"]\n");

    let stamp = refresh(home.path(), &checkout(home.path()), 1_000).unwrap();

    let read_checkout = fs::read_to_string(checkout(home.path()).join("agents/orchestration.toml")).unwrap();
    let read = fs::read_to_string(current(home.path()).join("orchestration.toml")).unwrap();
    assert!(!read_checkout.contains("cpx62"));
    assert!(read.contains("[hosts.cpx62]"));
    assert!(!current(home.path()).join("kept-out.md").exists());
    assert_eq!(fresh(home.path(), 1_000 + BOUND_SECONDS), Ok(stamp.clone()));
    let stale = fresh(home.path(), 1_001 + BOUND_SECONDS).unwrap_err();
    assert!(stale.contains("past the 60 min bound, though the fleet tick (north fleet tick) refreshes it every 5 min"), "{stale}");

    let late = 1_001 + BOUND_SECONDS * 3 / 4;
    assert_eq!(overdue(home.path(), late - 1), None, "a snapshot under three quarters of its bound was reported late");
    fs::rename(&origin, home.path().join("gone")).unwrap();
    let failed = refresh(home.path(), &checkout(home.path()), late).unwrap_err();
    assert!(failed.contains("git fetch"), "{failed}");
    assert_eq!(fresh(home.path(), late), Ok(stamp.clone()), "a failed fetch replaced the snapshot or its stamp");
    assert_eq!(overdue(home.path(), late), Some(stamp.clone()), "a snapshot whose fetch is failing went unreported past three quarters of its bound");
    warned(home.path(), &stamp).unwrap();
    assert_eq!(overdue(home.path(), late + 600), None, "one late episode was reported twice");
    fs::rename(home.path().join("gone"), &origin).unwrap();
    let refreshed = refresh(home.path(), &checkout(home.path()), late + 600).unwrap();
    assert_eq!(overdue(home.path(), late + 600 + BOUND_SECONDS * 3 / 4 + 1), Some(refreshed), "the next late episode after a fetch went unreported");
}
