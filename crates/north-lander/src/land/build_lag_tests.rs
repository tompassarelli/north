use super::*;
use std::cell::RefCell;
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
fn a_lander_more_than_the_limit_of_lander_changes_behind_main_names_its_build_and_pages_the_proxy_once() {
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

    let pages = RefCell::new(Vec::new());
    let page = |text: &str| {
        pages.borrow_mut().push(text.to_owned());
        true
    };
    let home = dir.path().join("state");
    assert!(alarm_once(&home, &lag, &page));
    assert!(!alarm_once(&home, &lag, &page));
    assert_eq!(pages.borrow().len(), 1);
    assert!(!alarm_once(&home, &Lag::Current { build: build.clone() }, &page));
    assert!(alarm_once(&home, &lag, &page), "a lag that cleared and passed the limit again pages again");
    assert_eq!(pages.borrow().len(), 2);
    let other = Lag::Behind { build: "9a896708b0".repeat(4)[..40].to_owned(), main: "f".repeat(40), changes: BEHIND_LIMIT + 1 };
    assert!(alarm_once(&home, &other, &page));
    for _ in 0..3 {
        assert!(!alarm_once(&home, &lag, &page) && !alarm_once(&home, &other, &page), "two builds reporting alternately page once each");
    }
    assert_eq!(pages.borrow().len(), 3);
    assert_eq!(report(&dir.path().join("south")), None, "only the lander serving north reports north lander lag");
    assert_eq!(runtime_of(Path::new(&format!("/s/north/runtimes/{build}/tree/bin/north"))).map(|(build, _)| build), Some(build));
}
