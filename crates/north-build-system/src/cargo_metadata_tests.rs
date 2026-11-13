use std::fs;
use std::path::Path;

use north_base::test_inputs::git::git_in;

use super::{manifests_key, no_deps_in};

fn workspace(root: &Path) {
    fs::create_dir_all(root.join("member/src")).unwrap();
    fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = [\"member\"]\nresolver = \"2\"\n").unwrap();
    fs::write(root.join("member/Cargo.toml"), "[package]\nname = \"member\"\nversion = \"0.1.0\"\nedition = \"2021\"\n").unwrap();
    fs::write(root.join("member/src/lib.rs"), "").unwrap();
    git_in(root, &["init", "-q", "-b", "main"]);
    git_in(root, &["add", "-A"]);
    git_in(root, &["commit", "-qm", "workspace"]);
}

#[test]
fn cargo_metadata_paths_swap_between_checkouts_and_a_changed_manifest_or_new_file_misses() {
    let scratch = tempfile::tempdir().unwrap();
    let base = fs::canonicalize(scratch.path()).unwrap();
    let (first, second, memo) = (base.join("first"), base.join("first-other"), base.join("memo"));
    fs::create_dir_all(&first).unwrap();
    workspace(&first);
    let ran = no_deps_in(&first, &memo).unwrap();
    assert!(ran.contains(&*first.to_string_lossy()));
    assert_eq!(fs::read_dir(&memo).unwrap().count(), 1);
    git_in(scratch.path(), &["clone", "-q", &first.to_string_lossy(), &second.to_string_lossy()]);
    assert_eq!(manifests_key(&second), manifests_key(&first));
    let memoized = no_deps_in(&second, &memo).unwrap();
    assert_eq!(memoized, ran.replace(&*first.to_string_lossy(), &second.to_string_lossy()));
    let before = manifests_key(&second).unwrap();
    fs::write(second.join("member/src/extra.rs"), "").unwrap();
    assert_eq!(manifests_key(&second), None);
    git_in(&second, &["add", "-A"]);
    git_in(&second, &["commit", "-qm", "a new file"]);
    let named = manifests_key(&second).unwrap();
    assert_ne!(named, before);
    fs::write(second.join("member/Cargo.toml"), "[package]\nname = \"member\"\nversion = \"0.2.0\"\nedition = \"2021\"\n").unwrap();
    git_in(&second, &["commit", "-qam", "a manifest change"]);
    assert_ne!(manifests_key(&second).unwrap(), named);
}
