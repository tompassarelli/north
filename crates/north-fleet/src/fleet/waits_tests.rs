use std::process::Command;

use super::*;

fn git(cwd: &Path, args: &[&str]) -> String {
    let out = Command::new("git").args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"]).args(args).current_dir(cwd).output().unwrap();
    assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

#[test]
fn a_runtime_wait_settles_when_the_live_runtime_contains_the_commit() {
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-q", "-b", "main"]);
    git(repo.path(), &["commit", "-q", "--allow-empty", "-m", "base"]);
    let base = git(repo.path(), &["rev-parse", "HEAD"]);
    git(repo.path(), &["commit", "-q", "--allow-empty", "-m", "fix"]);
    let fix = git(repo.path(), &["rev-parse", "HEAD"]);
    let runtimes = tempfile::tempdir().unwrap();
    let live = |sha: &str| {
        let runtime = runtimes.path().join("runtimes").join(sha);
        fs::create_dir_all(&runtime).unwrap();
        Some(runtime)
    };

    assert_eq!(runtime_holds(live(&base).as_deref(), &fix, repo.path()), None, "the live runtime lacks the commit");
    assert!(runtime_holds(live(&fix).as_deref(), &fix, repo.path()).is_some(), "the live runtime contains the commit");
    assert!(runtime_holds(live(&fix).as_deref(), &fix[..8], repo.path()).is_some());
    assert_eq!(runtime_holds(None, &fix, repo.path()), None, "an unreadable runtime settles nothing");
    assert_eq!(runtime_holds(live(&"0".repeat(40)).as_deref(), &fix, repo.path()), None, "an ancestry check that fails to run settles nothing");
    assert_eq!("runtime:abcd1234".parse::<Run>().unwrap().request("s").unwrap().kind, WaitKind::Runtime);
    assert!("runtime:--all".parse::<Run>().is_err());
}
