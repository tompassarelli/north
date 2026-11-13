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
    let outcome = current_main("south", &pair.clone, 0, &mut |line| said.push(line));
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

#[test]
fn a_clean_branch_checkout_with_no_unlanded_commits_returns_to_main_and_publishes() {
    let pair = pair();
    git(&pair.clone, &["switch", "-q", "-c", "n706-box"]);
    commit(&pair.origin, "remote");
    let (outcome, said) = sync(&pair);
    assert_eq!(outcome, Ok(()));
    assert_eq!(head(&pair.clone), head(&pair.origin));
    let branch = Command::new("git")
        .arg("-C")
        .arg(&pair.clone)
        .args(["branch", "--show-current"])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&branch.stdout).trim(), "main");
    assert_eq!(
        said,
        [
            "agents sync: south checkout was on branch n706-box at an ancestor of origin/main; switched to main"
        ]
    );
}

#[test]
fn stale_locks_a_killed_git_left_in_main_are_cleared_and_main_still_fast_forwards_north_937() {
    let pair = pair();
    git(&pair.clone, &["switch", "-q", "-c", "n706-box"]);
    commit(&pair.origin, "remote");
    let dot_git = pair.clone.join(".git");
    let hour_ago = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000);
    for name in ["refs/remotes/origin/main.lock", "index.lock", "ORIG_HEAD.lock", "HEAD.lock"] {
        fs::File::create(dot_git.join(name)).unwrap().set_modified(hour_ago).unwrap();
    }
    let (outcome, said) = sync(&pair);
    assert_eq!(outcome, Ok(()), "{said:?}");
    assert_eq!(head(&pair.clone), head(&pair.origin));
    assert_eq!(said.iter().filter(|line| line.contains("cleared the stale git lock")).count(), 4, "{said:?}");
}

#[test]
fn an_untracked_file_identical_to_the_incoming_blob_is_removed_and_main_fast_forwards_but_a_differing_one_still_refuses_north_959() {
    let pair = pair();
    fs::write(pair.origin.join("same.rs"), "fn same() {}\n").unwrap();
    fs::write(pair.origin.join("other.rs"), "fn incoming() {}\n").unwrap();
    git(&pair.origin, &["add", "."]);
    commit(&pair.origin, "adds two files");
    fs::write(pair.clone.join("same.rs"), "fn same() {}\n").unwrap();
    fs::write(pair.clone.join("other.rs"), "fn local() {}\n").unwrap();
    let before = head(&pair.clone);
    let (outcome, _) = sync(&pair);
    assert_eq!(
        outcome.unwrap_err(),
        "south main is behind origin by 1 and not fast-forwardable: untracked files with different bytes block the fast-forward: other.rs"
    );
    assert_eq!(head(&pair.clone), before);
    assert_eq!(fs::read_to_string(pair.clone.join("other.rs")).unwrap(), "fn local() {}\n");
    fs::remove_file(pair.clone.join("other.rs")).unwrap();
    let (outcome, said) = sync(&pair);
    assert_eq!(outcome, Ok(()), "{said:?}");
    assert_eq!(head(&pair.clone), head(&pair.origin));
}

#[test]
fn a_main_whose_fetch_fails_is_used_only_within_the_bound_of_its_last_fetch() {
    let pair = pair();
    let fetched_at = 1_000_000;
    let mut said = Vec::new();
    assert_eq!(current_main("south", &pair.clone, fetched_at, &mut |line| said.push(line)), Ok(()));
    fs::rename(&pair.origin, pair.origin.with_extension("gone")).unwrap();
    let bound = crate::platform::shared_config::BOUND_SECONDS;
    let stale = current_main("south", &pair.clone, fetched_at + bound + 60, &mut |line| said.push(line));
    assert_eq!(
        stale.unwrap_err(),
        "south main refused offline: its last fetch of origin/main is 61 min old, past the 60 min bound"
    );
    let recent = current_main("south", &pair.clone, fetched_at + 120, &mut |line| said.push(line));
    assert_eq!(recent, Ok(()));
    assert_eq!(said.last().unwrap(), "agents sync: south fetch failed; using local main, last fetched 2 min ago");
}

#[test]
fn a_main_with_no_commits_is_fast_forwarded_to_origin_and_given_its_upstream() {
    let pair = pair();
    let unborn = pair._dir.path().join("unborn");
    fs::create_dir(&unborn).unwrap();
    git(&unborn, &["init", "-q", "-b", "main"]);
    git(&unborn, &["remote", "add", "origin", &pair.origin.display().to_string()]);
    let outcome = current_main("south", &unborn, 0, &mut |_| {});
    assert_eq!(outcome, Ok(()));
    assert_eq!(head(&unborn), head(&pair.origin));
    let upstream = Command::new("git").arg("-C").arg(&unborn).args(["rev-parse", "--abbrev-ref", "main@{upstream}"]).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&upstream.stdout).trim(), "origin/main");
}

#[test]
fn a_main_with_no_origin_is_its_own_revision_and_is_not_refused() {
    let dir = tempfile::tempdir().unwrap();
    let local = dir.path().join("local");
    fs::create_dir(&local).unwrap();
    git(&local, &["init", "-q", "-b", "main"]);
    commit(&local, "synthetic");
    let mut said = Vec::new();
    let outcome = current_main("operator", &local, 0, &mut |line| said.push(line));
    assert_eq!(outcome, Ok(()));
    assert_eq!(said, vec!["agents sync: operator has no origin; its local main is the revision".to_owned()]);
}
