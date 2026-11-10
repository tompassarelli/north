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
fn a_lander_missing_lander_code_main_landed_over_fifteen_minutes_ago_names_its_build() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("north");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]).unwrap();
    for (key, value) in [("user.name", "t"), ("user.email", "t@example.com"), ("commit.gpgsign", "false")] {
        git(&repo, &["config", key, value]).unwrap();
    }
    let build = commit(&repo, "crates/north-lander/src/a.rs", "lander build");
    let docs = commit(&repo, "docs/a.md", "a docs landing");
    let landed: u64 = git(&repo, &["log", "-1", "--format=%ct", &docs]).unwrap().trim().parse().unwrap();
    assert_eq!(measure(&repo, Some(&build), &docs, landed + 3600), Lag::Current { build: build.clone() });
    let mut main = docs;
    for change in 0..6 {
        main = commit(&repo, "crates/north-lander/src/a.rs", &format!("lander fix {change}"));
    }
    let fresh = measure(&repo, Some(&build), &main, landed + 2 * 60);
    assert!(matches!(fresh, Lag::Behind { changes: 6, .. }) && !fresh.over_limit(), "six lander changes promote-on-landing has had two minutes to pass are not over the limit: {}", fresh.note());
    let stale = measure(&repo, Some(&build), &main, landed + BEHIND_LIMIT_SECS + 120);
    assert!(stale.over_limit() && stale.note().contains(&build[..10]) && stale.note().contains("over the limit of 15 minutes"), "{}", stale.note());
    assert!(!fresh.same_episode(&stale) && stale.same_episode(&measure(&repo, Some(&build), &main, landed + BEHIND_LIMIT_SECS + 600)));
    assert_eq!(report(&dir.path().join("south")), None, "only the lander serving north reports north lander lag");
    assert_eq!(runtime_of(Path::new(&format!("/s/north/runtimes/{build}/tree/bin/north"))).map(|(build, _)| build), Some(build));
}
