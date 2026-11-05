use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

use super::sync_gate::current_main;

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn commit(dir: &Path, message: &str) {
    git(dir, &["commit", "-q", "--allow-empty", "-m", message]);
}

fn head(dir: &Path) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

struct Pair {
    _dir: TempDir,
    origin: std::path::PathBuf,
    clone: std::path::PathBuf,
}

fn pair() -> Pair {
    let dir = tempfile::tempdir().unwrap();
    let origin = dir.path().join("origin");
    let clone = dir.path().join("clone");
    fs::create_dir(&origin).unwrap();
    git(&origin, &["init", "-q", "-b", "main"]);
    commit(&origin, "base");
    git(dir.path(), &["clone", "-q", "origin", "clone"]);
    Pair {
        _dir: dir,
        origin,
        clone,
    }
}

fn sync(pair: &Pair) -> (Result<(), String>, Vec<String>) {
    let mut said = Vec::new();
    let outcome = current_main("south", &pair.clone, &mut |line| said.push(line));
    (outcome, said)
}

#[test]
fn a_diverged_main_is_refused_and_left_alone() {
    let pair = pair();
    commit(&pair.origin, "remote");
    commit(&pair.clone, "local");
    let before = head(&pair.clone);
    let (outcome, _) = sync(&pair);
    assert_eq!(
        outcome.unwrap_err(),
        "south main is behind origin by 1 and not fast-forwardable: diverged with 1 local commits"
    );
    assert_eq!(head(&pair.clone), before);
}

