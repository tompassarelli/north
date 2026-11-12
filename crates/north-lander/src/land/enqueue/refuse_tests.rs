use std::fs;
use std::path::Path;
use std::process::Command;

use super::refuse_unlocked;

fn run(directory: &Path, program: &str, arguments: &[&str]) -> String {
    let output = Command::new(program).current_dir(directory).args(arguments).output().unwrap();
    assert!(output.status.success(), "{program} {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn commit(repo: &Path, message: &str) -> String {
    run(repo, "cargo", &["metadata", "--offline", "--format-version", "1"]);
    run(repo, "git", &["add", "--all"]);
    run(repo, "git", &["-c", "user.name=Lock fixture", "-c", "user.email=fixture@example.invalid", "-c", "commit.gpgsign=false", "commit", "-q", "-m", message]);
    run(repo, "git", &["rev-parse", "HEAD"])
}

fn package(repo: &Path, path: &str, dependencies: &str) {
    let name = path.rsplit('/').next().unwrap();
    fs::create_dir_all(repo.join(path).join("src")).unwrap();
    fs::write(repo.join(path).join("src/lib.rs"), "").unwrap();
    fs::write(repo.join(path).join("Cargo.toml"), format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\n{dependencies}")).unwrap();
}

#[test]
fn a_consistent_lane_whose_merge_onto_a_moved_main_drops_a_lock_entry_main_needs_is_refused_before_it_queues_as_10d4456515_turned_main_red() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    run(repo, "git", &["init", "-q", "-b", "main"]);
    fs::write(repo.join("Cargo.toml"), "[workspace]\nmembers = [\"first\", \"middle\"]\nexclude = [\"vendor\"]\nresolver = \"3\"\n").unwrap();
    package(repo, "vendor/foo", "");
    package(repo, "first", "foo = { path = \"../vendor/foo\" }\n");
    package(repo, "middle", "");
    let fork = commit(repo, "first uses a vendored crate");

    package(repo, "middle", "foo = { path = \"../vendor/foo\" }\n");
    let main = commit(repo, "main has middle use it too");

    run(repo, "git", &["checkout", "-q", "-b", "lane", &fork]);
    package(repo, "first", "");
    let lane = commit(repo, "the lane drops first's use and so the crate's lock entry");
    run(repo, "cargo", &["metadata", "--locked", "--offline", "--format-version", "1"]);
    assert_eq!(refuse_unlocked(repo, &fork, &lane), Ok(()), "the lane's own tree is consistent");

    let refusal = refuse_unlocked(repo, &main, &lane).unwrap_err();
    assert!(refusal.contains("Cargo.lock") && refusal.contains("cargo metadata --locked --offline") && !refusal.contains('\n'), "{refusal}");
}
