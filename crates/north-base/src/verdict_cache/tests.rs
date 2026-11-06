use std::fs;
use std::path::Path;
use std::process::Command;

use super::{Check, HEAD_NORTH_DEFERRED, NORTH_IGNORES, Pass, Store, built_equivalent, key};
use crate::foundation::clock::Timestamp;

const SAFE_PUSH: &str = "[land]\n\tcheck = src/ :: bash bin/check\n\tafter = @rust-input :: cargo clippy\n\tafter = docs/ :: ./bin/head-north docs check\n\tcheck = docs/ :: ./bin/head-north --no-build docs check\n";
const CHECK: &str = "bash bin/check";

fn git(root: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git").arg("-C").arg(root).args(["-c", "user.name=fixture", "-c", "user.email=fixture@example.invalid", "-c", "commit.gpgsign=false"]).args(arguments).output().unwrap();
    assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn commit(root: &Path, files: &[(&str, &str)], message: &str) -> String {
    for (path, text) in files {
        let target = root.join(path);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(target, text).unwrap();
    }
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "--allow-empty", "-m", message]);
    git(root, &["rev-parse", "HEAD"])
}

fn repository() -> (tempfile::TempDir, String) {
    let scratch = tempfile::tempdir().unwrap();
    git(scratch.path(), &["init", "-q", "-b", "main"]);
    let head = commit(scratch.path(), &[(".safe-push", SAFE_PUSH), ("src/lib.rs", "fn a() {}"), ("bin/check", "#!/bin/bash\ncat data/input.txt \"$SAFE_PUSH_LAND_PACKAGES\"\n"), ("data/input.txt", "one"), ("docs/a.md", "a"), ("Cargo.toml", "[workspace]")], "base");
    (scratch, head)
}

fn key_with(root: &Path, tree: &str, command: &str, runtime: &str, environment: &[(String, String)]) -> String {
    key(&Check { repo: root, tree, command, runtime, environment, machine_repos: &[] }).unwrap()
}

fn key_of(root: &Path, tree: &str, command: &str) -> String {
    key_with(root, tree, command, "runtime-a", &[])
}

#[test]
fn a_changed_input_misses_and_an_unread_path_does_not() {
    let (repo, first) = repository();
    let before = key_of(repo.path(), &first, CHECK);
    let docs = commit(repo.path(), &[("docs/a.md", "b")], "docs only");
    assert_eq!(key_of(repo.path(), &docs, CHECK), before);
    let source = commit(repo.path(), &[("src/lib.rs", "fn b() {}")], "selected path");
    assert_ne!(key_of(repo.path(), &source, CHECK), before);
    let read = key_of(repo.path(), &source, CHECK);
    let data = commit(repo.path(), &[("data/input.txt", "two")], "a file the check script reads");
    assert_ne!(key_of(repo.path(), &data, CHECK), read);
    let clippy = key_of(repo.path(), &data, "cargo clippy");
    let manifest = commit(repo.path(), &[("Cargo.toml", "[workspace]\nmembers = []")], "a rust input");
    assert_ne!(key_of(repo.path(), &manifest, "cargo clippy"), clippy);
}

#[test]
fn a_check_that_builds_north_from_the_tree_misses_when_any_rust_input_changes() {
    let (repo, first) = repository();
    let command = "./bin/head-north docs check";
    let before = key_of(repo.path(), &first, command);
    let unread = commit(repo.path(), &[("data/input.txt", "two")], "a file it does not read");
    assert_eq!(key_of(repo.path(), &unread, command), before);
    let source = commit(repo.path(), &[("src/lib.rs", "fn c() {}")], "a crate it builds");
    assert_ne!(key_of(repo.path(), &source, command), before);
}

#[test]
fn the_runtime_and_the_check_environment_are_part_of_the_key() {
    let (repo, head) = repository();
    let narrow = [("SAFE_PUSH_LAND_PACKAGES".to_owned(), "-p a".to_owned())];
    let wide = [("SAFE_PUSH_LAND_PACKAGES".to_owned(), "--workspace".to_owned())];
    let base = key_with(repo.path(), &head, CHECK, "runtime-a", &narrow);
    assert_ne!(key_with(repo.path(), &head, CHECK, "runtime-b", &narrow), base);
    assert_ne!(key_with(repo.path(), &head, CHECK, "runtime-a", &wide), base);
    let same_tree = commit(repo.path(), &[], "empty");
    let at = |revision: &str| [("SAFE_PUSH_LAND_BASE".to_owned(), revision.to_owned())];
    assert_eq!(key_with(repo.path(), &head, CHECK, "runtime-a", &at(&head)), key_with(repo.path(), &head, CHECK, "runtime-a", &at(&same_tree)));
    let other = commit(repo.path(), &[("docs/a.md", "c")], "another base");
    assert_eq!(key_with(repo.path(), &head, "cargo clippy", "runtime-a", &at(&head)), key_with(repo.path(), &head, "cargo clippy", "runtime-a", &at(&other)));
    let north = "./bin/head-north docs check";
    assert_ne!(key_with(repo.path(), &head, north, "runtime-a", &at(&head)), key_with(repo.path(), &head, north, "runtime-a", &at(&other)));
    a_variable_the_check_never_reads_leaves_its_key_alone();
    a_pre_push_no_build_line_keys_the_after_line_it_stands_for_until_an_input_changes();
    head_north_still_defers_a_no_build_line_with_the_words_the_runner_reads();
    north_reads_none_of_the_variables_its_check_keys_ignore();
}

#[test]
fn a_failure_removes_the_pass_and_is_never_stored() {
    let (repo, head) = repository();
    let store = tempfile::tempdir().unwrap();
    let store = Store(store.path().to_owned());
    let recorded = key_of(repo.path(), &head, CHECK);
    store.record(&recorded, Some(&Pass { at: Timestamp::from_unix_seconds(1_000), sha: head.clone() })).unwrap();
    assert_eq!(store.hit(&recorded).map(|pass| pass.sha), Some(head));
    store.record(&recorded, None).unwrap();
    assert!(store.hit(&recorded).is_none());
    store.record(&recorded, None).unwrap();
    assert!(store.hit(&recorded).is_none());
    assert_eq!(fs::read_dir(&store.0).unwrap().count(), 0);
}

fn variables(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs.iter().map(|(name, value)| ((*name).to_owned(), (*value).to_owned())).collect()
}

fn a_variable_the_check_never_reads_leaves_its_key_alone() {
    let (repo, head) = repository();
    let north = "./bin/head-north docs check";
    let range = [("SAFE_PUSH_LAND_BASE", head.as_str()), ("SAFE_PUSH_LAND_HEAD", head.as_str())];
    let scoped = [range[0], range[1], ("SAFE_PUSH_LAND_PACKAGES", "-p a"), ("SAFE_PUSH_LAND_CHANGED", "-p a"), ("SAFE_PUSH_LAND_IT", "--test it"), ("SAFE_PUSH_LAND_CPUS", "4")];
    assert_eq!(key_with(repo.path(), &head, north, "runtime-a", &variables(&scoped)), key_with(repo.path(), &head, north, "runtime-a", &variables(&range)));
    let sized = [("SAFE_PUSH_LAND_CPUS", "4")];
    assert_eq!(key_with(repo.path(), &head, CHECK, "runtime-a", &variables(&sized)), key_of(repo.path(), &head, CHECK));
    let selected = [range[0], range[1], ("SAFE_PUSH_LAND_SELECT_PATHS", "src/lib.rs")];
    assert_ne!(key_with(repo.path(), &head, north, "runtime-a", &variables(&selected)), key_with(repo.path(), &head, north, "runtime-a", &variables(&range)));
}

fn a_pre_push_no_build_line_keys_the_after_line_it_stands_for_until_an_input_changes() {
    let (repo, head) = repository();
    let after = built_equivalent("./bin/head-north --no-build docs check").unwrap();
    assert_eq!(after, "./bin/head-north docs check");
    assert_eq!(built_equivalent(&after), None);
    let pre_push = [("SAFE_PUSH_LAND_BASE", head.as_str()), ("SAFE_PUSH_LAND_HEAD", head.as_str()), ("SAFE_PUSH_LAND_RUNTIME_HEAD", head.as_str())];
    let recorded = key_with(repo.path(), &head, &after, "runtime-a", &variables(&pre_push));
    let landed = commit(repo.path(), &[], "the lane landed on the tree it was checked on");
    let after_run = [("SAFE_PUSH_LAND_BASE", head.as_str()), ("SAFE_PUSH_LAND_HEAD", landed.as_str()), ("SAFE_PUSH_LAND_RUNTIME_HEAD", landed.as_str()), ("SAFE_PUSH_LAND_PACKAGES", "--workspace"), ("SAFE_PUSH_LAND_CPUS", "6")];
    assert_eq!(key_with(repo.path(), &landed, &after, "runtime-a", &variables(&after_run)), recorded);
    let changed = commit(repo.path(), &[("docs/a.md", "changed")], "an input the after line reads");
    let after_change = [after_run[0], ("SAFE_PUSH_LAND_HEAD", changed.as_str()), ("SAFE_PUSH_LAND_RUNTIME_HEAD", changed.as_str())];
    assert_ne!(key_with(repo.path(), &changed, &after, "runtime-a", &variables(&after_change)), recorded);
}

fn workspace() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn head_north_still_defers_a_no_build_line_with_the_words_the_runner_reads() {
    let script = fs::read_to_string(workspace().join("bin/head-north")).unwrap();
    assert!(script.contains("--no-build"));
    assert!(script.contains(HEAD_NORTH_DEFERRED), "bin/head-north no longer prints {HEAD_NORTH_DEFERRED:?} when it defers, so a deferred pre-push pass would be reused after landing");
}

fn sources(directory: &Path, into: &mut Vec<std::path::PathBuf>) {
    for entry in fs::read_dir(directory).unwrap().flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() && name != "tests" && name != "target" {
            sources(&path, into);
        } else if name.ends_with(".rs") && !name.contains("tests") {
            into.push(path);
        }
    }
}

fn north_reads_none_of_the_variables_its_check_keys_ignore() {
    let allowed = [
        "crates/north-base/src/verdict_cache/key.rs",
        "crates/north-landing/src/land/slots/affected.rs",
        "crates/north-landing/src/land/slots/it_modules.rs",
        "crates/north-capacity/src/capacity/carry.rs",
        "crates/north-capacity-host/src/capacity/host/mod.rs",
        "crates/north-runtime/src/land_offload/ship/script.rs",
    ];
    let root = workspace();
    let mut files = Vec::new();
    sources(&root.join("crates"), &mut files);
    sources(&root.join("src"), &mut files);
    let readers: Vec<String> = files
        .iter()
        .filter_map(|file| {
            let relative = file.strip_prefix(&root).unwrap().to_string_lossy().into_owned();
            let text = fs::read_to_string(file).unwrap_or_default();
            (!allowed.contains(&relative.as_str()) && NORTH_IGNORES.iter().any(|(name, _)| text.contains(&format!("\"{name}\"")))).then_some(relative)
        })
        .collect();
    assert!(readers.is_empty(), "these sources name a variable the verdict key leaves out of every north check, so a reuse could skip a check whose verdict depends on it: {readers:?}");
}
