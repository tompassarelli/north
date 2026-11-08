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
    assert!(Command::new("git").arg("init").arg("-q").arg(checkout.path()).status().unwrap().success());
    let git = fs::canonicalize(checkout.path().join(".git")).unwrap();
    let old = Duration::from_secs(3600);
    let stale = ["index.lock", "HEAD.lock", "ORIG_HEAD.lock", "refs/remotes/origin/main.lock", "refs/remotes/origin/HEAD.lock"].map(|name| git.join(name));
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
    let kept: Vec<_> = sweep.kept.iter().map(|(path, kept)| (path.file_name().unwrap().to_str().unwrap(), matches!(kept, Kept::Held { pid } if *pid == std::process::id()))).collect();
    assert_eq!(kept, [("config.lock", false), ("packed-refs.lock", true)]);
    assert!(git.join("north-history.lock").exists());
}
