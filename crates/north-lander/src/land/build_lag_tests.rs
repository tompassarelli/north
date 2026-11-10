use super::*;
use std::fs;
use std::path::Path;

fn commit(repo: &Path, path: &str, message: &str) -> String {
    let file = repo.join(path);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, message).unwrap();
    git(repo, &["add", "-A"]).unwrap();
    git(repo, &["commit", "-q", "-m", message]).unwrap();
    git(repo, &["rev-parse", "HEAD"]).unwrap().trim().to_owned()
}

#[test]
fn a_lander_more_than_the_limit_of_lander_changes_behind_main_names_its_build() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("north");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]).unwrap();
    for (key, value) in [("user.name", "t"), ("user.email", "t@example.com"), ("commit.gpgsign", "false")] {
        git(&repo, &["config", key, value]).unwrap();
    }
    let build = commit(&repo, "crates/north-lander/src/a.rs", "lander build");
    let docs = commit(&repo, "docs/a.md", "a docs landing");
    assert_eq!(measure(&repo, Some(&build), &docs), Lag::Current { build: build.clone() });
    let mut main = docs;
    for change in 0..=BEHIND_LIMIT {
        main = commit(&repo, "crates/north-lander/src/a.rs", &format!("lander fix {change}"));
    }
    let lag = measure(&repo, Some(&build), &main);
    assert_eq!(lag, Lag::Behind { build: build.clone(), main, changes: BEHIND_LIMIT + 1 });
    assert!(lag.note().contains(&build[..10]) && lag.note().contains("over the limit"));
    assert_eq!(report(&dir.path().join("south")), None, "only the lander serving north reports north lander lag");
    assert_eq!(runtime_of(Path::new(&format!("/s/north/runtimes/{build}/tree/bin/north"))).map(|(build, _)| build), Some(build));
}
