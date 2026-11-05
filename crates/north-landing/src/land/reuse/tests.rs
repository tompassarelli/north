use std::fs;
use std::path::Path;

use super::{Key, Verdict, input_hash, lookup, record, run};

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
fn the_check_line_text_is_part_of_the_hash() {
    let scratch = tempfile::tempdir().unwrap();
    workspace(scratch.path());
    let test = input_hash(scratch.path(), "crates/ :: cargo test -p lib").unwrap();
    assert_ne!(test, input_hash(scratch.path(), "crates/ :: cargo test -p lib --locked").unwrap());
    assert_ne!(test, input_hash(scratch.path(), "src/ :: cargo test -p lib").unwrap());
}

#[test]
fn the_workspace_flag_and_the_default_members_cover_every_crate() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    workspace(root);
    let everything = input_hash(root, "** :: cargo test --workspace").unwrap();
    write(root, "other/src/lib.rs", "pub fn answer() -> u8 { 9 }\n");
    assert_ne!(input_hash(root, "** :: cargo test --workspace").unwrap(), everything);
}

#[test]
fn a_script_check_hashes_the_script_and_the_files_it_names_but_nothing_else() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    workspace(root);
    let line = "** :: bash scripts/check.sh";
    let before = input_hash(root, line).unwrap();
    write(root, "data/unrelated.txt", "two\n");
    write(root, "other/src/lib.rs", "pub fn answer() -> u8 { 5 }\n");
    write(root, "Cargo.lock", "version = 4\n# changed\n");
    assert_eq!(input_hash(root, line).unwrap(), before);
    write(root, "data/input.txt", "two\n");
    let named = input_hash(root, line).unwrap();
    assert_ne!(named, before);
    write(root, "scripts/check.sh", "#!/bin/bash\ncat data/input.txt\ntrue\n");
    assert_ne!(input_hash(root, line).unwrap(), named);
}

#[test]
fn a_command_names_files_through_pwd_and_comma_lists_and_ignores_build_output() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    workspace(root);
    write(root, "target/debug/noise", "one\n");
    let line = "** :: CARGO_TARGET_DIR=target bun \"$PWD/data/input.txt\",lib/src/lib.rs";
    let before = input_hash(root, line).unwrap();
    write(root, "target/debug/noise", "two\n");
    assert_eq!(input_hash(root, line).unwrap(), before);
    write(root, "lib/src/lib.rs", "pub fn answer() -> u8 { 6 }\n");
    assert_ne!(input_hash(root, line).unwrap(), before);
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

#[test]
fn the_hash_command_prints_the_digest() {
    let scratch = tempfile::tempdir().unwrap();
    workspace(scratch.path());
    let arguments: Vec<String> = ["hash", "--repo", scratch.path().to_str().unwrap(), "--check", "** :: bash scripts/check.sh"].map(str::to_owned).to_vec();
    assert_eq!(run(&arguments), 0);
    assert_eq!(run(&["hash".to_owned()]), 64);
}

fn key(input: &str) -> Key {
    Key { lane_head: "a".repeat(40), main: "b".repeat(40), input: input.to_owned() }
}

#[test]
fn a_recorded_verdict_is_read_back_by_a_fresh_lookup_and_an_overwrite_wins() {
    let scratch = tempfile::tempdir().unwrap();
    let state = scratch.path().join("land-reuse");
    assert_eq!(lookup(&state, &key("one")), None);
    record(&state, &key("one"), Verdict::Pass).unwrap();
    record(&state, &key("two"), Verdict::Fail).unwrap();
    assert_eq!(lookup(&state, &key("one")), Some(Verdict::Pass));
    assert_eq!(lookup(&state, &key("two")), Some(Verdict::Fail));
    assert_eq!(lookup(&state, &key("three")), None);
    record(&state, &key("one"), Verdict::Fail).unwrap();
    assert_eq!(lookup(&state, &key("one")), Some(Verdict::Fail));
    let names: Vec<String> = fs::read_dir(&state).unwrap().map(|entry| entry.unwrap().file_name().into_string().unwrap()).collect();
    assert_eq!(names.len(), 2);
    assert!(names.contains(&key("one").file_name()));
}

#[test]
fn every_field_of_the_key_picks_its_own_file_and_a_foreign_file_is_not_a_hit() {
    let scratch = tempfile::tempdir().unwrap();
    let base = key("x");
    let other_head = Key { lane_head: "c".repeat(40), ..base.clone() };
    let other_main = Key { main: "c".repeat(40), ..base.clone() };
    assert_eq!(base.file_name().len(), 69);
    assert_ne!(base.file_name(), other_head.file_name());
    assert_ne!(base.file_name(), other_main.file_name());
    assert_ne!(base.file_name(), key("y").file_name());
    record(scratch.path(), &base, Verdict::Pass).unwrap();
    fs::copy(scratch.path().join(base.file_name()), scratch.path().join(other_head.file_name())).unwrap();
    assert_eq!(lookup(scratch.path(), &other_head), None);
}

#[test]
fn a_verdict_is_stored_as_lowercase_json_and_commands_round_trip_a_key_string() {
    assert_eq!(serde_json::to_string(&Verdict::Pass).unwrap(), "\"pass\"");
    assert_eq!(serde_json::from_str::<Verdict>("\"fail\"").unwrap(), Verdict::Fail);
    assert_eq!(Key::parse("aa:bb:cc"), Some(Key { lane_head: "aa".to_owned(), main: "bb".to_owned(), input: "cc".to_owned() }));
    assert_eq!(Key::parse("aa:bb"), None);
    assert_eq!(Key::parse("aa:bb:cc:dd"), None);
    let scratch = tempfile::tempdir().unwrap();
    let state = scratch.path().to_str().unwrap().to_owned();
    let command = |words: &[&str]| run(&words.iter().map(|word| (*word).to_owned()).collect::<Vec<_>>());
    assert_eq!(command(&["lookup", "--key", "aa:bb:cc", "--state", &state]), 1);
    assert_eq!(command(&["record", "--key", "aa:bb:cc", "--verdict", "pass", "--state", &state]), 0);
    assert_eq!(command(&["lookup", "--key", "aa:bb:cc", "--state", &state]), 0);
    assert_eq!(command(&["record", "--key", "aa:bb:cc", "--verdict", "maybe", "--state", &state]), 64);
}
