use std::fs::{self, File};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::{Kept, clear_at};

fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_000_000_000)
}

fn lock(path: &Path, age: Duration) -> File {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let file = File::create(path).unwrap();
    file.set_modified(now() - age).unwrap();
    file
}

#[test]
fn only_a_lock_no_process_holds_and_older_than_a_minute_is_removed_north_937() {
    let checkout = tempfile::tempdir().unwrap();
    assert!(
        Command::new("git")
            .arg("init")
            .arg("-q")
            .arg(checkout.path())
            .status()
            .unwrap()
            .success()
    );
    let git = fs::canonicalize(checkout.path().join(".git")).unwrap();
    let old = Duration::from_secs(3600);
    let stale = [
        "index.lock",
        "HEAD.lock",
        "ORIG_HEAD.lock",
        "refs/remotes/origin/main.lock",
        "refs/remotes/origin/HEAD.lock",
    ]
    .map(|name| git.join(name));
    for path in &stale {
        drop(lock(path, old));
    }
    let _held = lock(&git.join("packed-refs.lock"), old);
    drop(lock(&git.join("config.lock"), Duration::from_secs(5)));
    drop(lock(&git.join("north-history.lock"), old));

    let sweep = clear_at(checkout.path(), now()).unwrap();

    let mut removed = sweep.removed.clone();
    removed.sort();
    let mut expected = stale.to_vec();
    expected.sort();
    assert_eq!(removed, expected);
    assert!(stale.iter().all(|path| !path.exists()));
    let kept: Vec<_> = sweep
        .kept
        .iter()
        .map(|(path, kept)| {
            (
                path.file_name().unwrap().to_str().unwrap(),
                matches!(kept, Kept::Held { pid } if *pid == std::process::id()),
            )
        })
        .collect();
    assert_eq!(kept, [("config.lock", false), ("packed-refs.lock", true)]);
    assert!(git.join("north-history.lock").exists());
}

fn git(directory: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

#[test]
fn worktrees_of_one_clone_fetching_a_moving_origin_at_once_all_succeed_north_1190() {
    let root = tempfile::tempdir().unwrap();
    let origin = root.path().join("origin");
    let clone = root.path().join("clone");
    git(
        root.path(),
        &["init", "-q", "-b", "main", origin.to_str().unwrap()],
    );
    git(&origin, &["commit", "-q", "--allow-empty", "-m", "first"]);
    git(
        root.path(),
        &[
            "clone",
            "-q",
            origin.to_str().unwrap(),
            clone.to_str().unwrap(),
        ],
    );
    let lanes: Vec<_> = (0..4)
        .map(|index| {
            let lane = root.path().join(format!("lane{index}"));
            git(
                &clone,
                &["worktree", "add", "-q", "--detach", lane.to_str().unwrap()],
            );
            lane
        })
        .collect();
    let moving = std::thread::spawn(move || {
        for _ in 0..40 {
            git(&origin, &["commit", "-q", "--allow-empty", "-m", "next"]);
        }
    });
    let fetchers: Vec<_> = lanes
        .into_iter()
        .map(|lane| {
            std::thread::spawn(move || {
                (0..40)
                    .filter_map(|_| {
                        super::retry_after_clearing(&lane, || {
                            let output = Command::new("git")
                                .arg("-C")
                                .arg(&lane)
                                .args([
                                    "fetch",
                                    "-q",
                                    "origin",
                                    "+refs/heads/main:refs/remotes/origin/main",
                                ])
                                .output()
                                .unwrap();
                            if output.status.success() {
                                Ok(())
                            } else {
                                Err(String::from_utf8_lossy(&output.stderr).into_owned())
                            }
                        })
                        .err()
                    })
                    .collect::<Vec<_>>()
            })
        })
        .collect();
    moving.join().unwrap();
    let failures: Vec<String> = fetchers
        .into_iter()
        .flat_map(|fetcher| fetcher.join().unwrap())
        .collect();
    assert!(failures.is_empty(), "{failures:?}");
}
