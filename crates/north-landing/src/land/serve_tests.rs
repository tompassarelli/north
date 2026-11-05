use super::*;

fn commit(repo: &Path, files: &[&str]) -> String {
    for file in files {
        let path = repo.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, format!("{}\n", micros())).unwrap();
    }
    git(repo, &["add", "-A"]).unwrap();
    git(repo, &["commit", "-q", "--allow-empty", "-m", "change"]).unwrap();
    git(repo, &["rev-parse", "HEAD"]).unwrap()
}

fn point(link: &Path, sha: &str) {
    let _ = std::fs::remove_file(link);
    std::os::unix::fs::symlink(format!("runtimes/{sha}"), link).unwrap();
}

fn repository(dir: &Path) -> PathBuf {
    let repo = dir.join("north");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]).unwrap();
    for (key, value) in [("user.name", "t"), ("user.email", "t@example.com"), ("commit.gpgsign", "false")] {
        git(&repo, &["config", key, value]).unwrap();
    }
    repo
}

#[test]
fn a_runtime_move_that_touches_only_docs_and_fleet_code_keeps_the_lander_serving() {
    let dir = tempfile::tempdir().unwrap();
    let repo = repository(dir.path());
    let a = commit(&repo, &["crates/north-landing/src/lib.rs", "docs/a.md"]);
    let b = commit(&repo, &["docs/rationale/landing-train.md", "crates/north-core/src/fleet/launch.rs"]);
    let link = dir.path().join("current");
    point(&link, &a);
    let mut runtime = RuntimeWatch::new(link.clone());
    point(&link, &b);
    assert!(!runtime.retires([repo.as_path()]));
    assert!(!runtime.retires([repo.as_path()]));
    let c = commit(&repo, &["crates/north-landing/src/land/serve.rs"]);
    point(&link, &c);
    assert!(runtime.retires([repo.as_path()]));
}

#[test]
fn a_runtime_move_that_touches_the_lander_crate_retires_it() {
    let dir = tempfile::tempdir().unwrap();
    let repo = repository(dir.path());
    let a = commit(&repo, &["docs/a.md"]);
    let b = commit(&repo, &["crates/north-landing/src/land/serve.rs", "docs/b.md"]);
    let link = dir.path().join("current");
    point(&link, &a);
    let mut runtime = RuntimeWatch::new(link.clone());
    assert!(!runtime.retires([repo.as_path()]));
    point(&link, &b);
    assert!(runtime.retires([repo.as_path()]));
}

#[test]
fn a_runtime_move_the_repositories_cannot_diff_retires_the_lander() {
    let dir = tempfile::tempdir().unwrap();
    let repo = repository(dir.path());
    let a = commit(&repo, &["docs/a.md"]);
    let link = dir.path().join("current");
    point(&link, &a);
    let mut runtime = RuntimeWatch::new(link.clone());
    point(&link, &"f".repeat(40));
    assert!(runtime.retires([repo.as_path()]));
    point(&link, "not-a-sha");
    assert!(runtime.retires([repo.as_path()]));
    std::fs::remove_file(&link).unwrap();
    assert!(runtime.retires([repo.as_path()]));
}
