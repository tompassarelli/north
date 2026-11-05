use std::fs;
use std::path::Path;

use super::input_hash;

fn write(root: &Path, relative: &str, text: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn member(root: &Path, name: &str, uses: &[&str]) {
    let dependencies: String = uses.iter().map(|used| format!("{used} = {{ path = \"../{used}\" }}\n")).collect();
    write(root, &format!("{name}/Cargo.toml"), &format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\n{dependencies}"));
    write(root, &format!("{name}/src/lib.rs"), "pub fn answer() -> u8 { 1 }\n");
}

fn workspace(root: &Path) {
    write(root, "Cargo.toml", "[workspace]\nmembers = [\"lib\", \"app\", \"top\", \"other\"]\nresolver = \"2\"\n");
    write(root, "Cargo.lock", "version = 4\n");
    member(root, "lib", &[]);
    member(root, "app", &["lib"]);
    member(root, "top", &["app"]);
    member(root, "other", &[]);
    write(root, "scripts/check.sh", "#!/bin/bash\ncat data/input.txt\n");
    write(root, "data/input.txt", "one\n");
    write(root, "data/unrelated.txt", "one\n");
}

fn cargo(root: &Path, package: &str) -> String {
    input_hash(root, &format!("crates/ :: cargo test -p {package}")).unwrap()
}

#[test]
fn a_cargo_check_ignores_unrelated_crates_and_follows_its_dependents_and_dependencies() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    workspace(root);
    let app = cargo(root, "app");
    let lib = cargo(root, "lib");
    write(root, "other/src/lib.rs", "pub fn answer() -> u8 { 2 }\n");
    assert_eq!(cargo(root, "app"), app);
    assert_eq!(cargo(root, "lib"), lib);
    let other = cargo(root, "other");
    write(root, "top/src/lib.rs", "pub fn answer() -> u8 { 3 }\n");
    assert_ne!(cargo(root, "app"), app);
    assert_ne!(cargo(root, "lib"), lib);
    assert_eq!(cargo(root, "other"), other);
    let app = cargo(root, "app");
    write(root, "lib/src/lib.rs", "pub fn answer() -> u8 { 4 }\n");
    assert_ne!(cargo(root, "app"), app);
    assert_eq!(cargo(root, "other"), other);
}

#[test]
fn editing_the_lockfile_the_toolchain_or_the_root_manifest_changes_every_cargo_check() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    workspace(root);
    let before = (cargo(root, "lib"), cargo(root, "other"));
    write(root, "Cargo.lock", "version = 4\n# changed\n");
    let after = (cargo(root, "lib"), cargo(root, "other"));
    assert_ne!(before.0, after.0);
    assert_ne!(before.1, after.1);
    write(root, "rust-toolchain.toml", "[toolchain]\nchannel = \"1.96.1\"\n");
    assert_ne!(cargo(root, "lib"), after.0);
}

#[test]
fn the_hash_is_the_same_on_two_runs_and_in_checkouts_at_different_paths() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let nested = second.path().join("deeper/checkout");
    workspace(first.path());
    workspace(&nested);
    for line in ["crates/ :: cargo clippy -p app", "** :: cargo test --workspace", "** :: bash scripts/check.sh"] {
        let hash = input_hash(first.path(), line).unwrap();
        assert_eq!(hash.len(), 64);
        assert_eq!(input_hash(first.path(), line).unwrap(), hash);
        assert_eq!(input_hash(&nested, line).unwrap(), hash);
    }
}

