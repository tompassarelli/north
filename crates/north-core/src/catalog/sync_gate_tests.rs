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
fn a_clean_behind_main_fast_forwards() {
    let pair = pair();
    commit(&pair.origin, "remote");
    let (outcome, said) = sync(&pair);
    assert!(outcome.is_ok() && said.is_empty());
    assert_eq!(head(&pair.clone), head(&pair.origin));
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
fn a_current_main_is_untouched() {
    let pair = pair();
    let before = head(&pair.clone);
    let (outcome, said) = sync(&pair);
    assert!(outcome.is_ok() && said.is_empty());
    assert_eq!(head(&pair.clone), before);
}

#[test]
fn a_failed_fetch_continues_from_local_main() {
    let pair = pair();
    commit(&pair.origin, "remote");
    git(
        &pair.clone,
        &["remote", "set-url", "origin", "/nonexistent/gone"],
    );
    let before = head(&pair.clone);
    let (outcome, said) = sync(&pair);
    assert!(outcome.is_ok());
    assert_eq!(said, ["agents sync: south fetch failed; using local main"]);
    assert_eq!(head(&pair.clone), before);
}

#[test]
fn a_failed_fetch_refuses_a_main_that_is_not_publishable() {
    let pair = pair();
    git(
        &pair.clone,
        &["remote", "set-url", "origin", "/nonexistent/gone"],
    );
    commit(&pair.clone, "local");
    let (outcome, _) = sync(&pair);
    assert_eq!(
        outcome.unwrap_err(),
        "south main refused offline: diverged with 1 local commits"
    );
}

#[test]
fn a_main_that_is_not_checked_out_is_refused() {
    let pair = pair();
    commit(&pair.origin, "remote");
    git(&pair.clone, &["checkout", "-q", "-b", "side"]);
    let (outcome, _) = sync(&pair);
    assert_eq!(
        outcome.unwrap_err(),
        "south main is behind origin by 1 and not fast-forwardable: checkout is not on main"
    );
}

#[test]
fn the_north_checkout_is_the_configured_root_then_north_home_then_the_home_checkout_never_the_runtime_tree()
 {
    use super::sync_gate::north_checkout;
    let home = Path::new("/home/fixture");
    let resolve = |roots: Option<&str>, north_home: Option<&str>| {
        north_checkout(roots, north_home.map(Into::into), home).unwrap()
    };
    assert_eq!(
        resolve(Some(r#"{"north": "/roots/north"}"#), Some("/env/north")),
        Path::new("/roots/north")
    );
    assert_eq!(
        resolve(Some(r#"{"south": "/roots/south"}"#), Some("/env/north")),
        Path::new("/env/north")
    );
    assert_eq!(resolve(None, Some("/env/north")), Path::new("/env/north"));
    assert_eq!(
        resolve(None, None),
        Path::new("/home/fixture/code/north/main")
    );
    assert_eq!(
        resolve(None, Some("")),
        Path::new("/home/fixture/code/north/main")
    );
}
