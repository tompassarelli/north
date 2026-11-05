use super::*;

const SAFE_PUSH: &str = "[land]\n\tcheck = src/,Cargo.toml :: cargo test\n\tcheck = ** :: owners south/main\n\tcheck = bin/ :: ./bin/head-north land-tests --select changed --only machine-bound\n";

fn git_ok(root: &Path, args: &[&str]) -> String {
    git(root, args).unwrap_or_else(|| panic!("git {args:?} failed"))
}

fn commit(root: &Path, files: &[(&str, &str)], message: &str) -> String {
    for (path, text) in files {
        let target = root.join(path);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(target, text).unwrap();
    }
    git_ok(root, &["add", "-A"]);
    git_ok(root, &["-c", "commit.gpgsign=false", "commit", "-qm", message]);
    git_ok(root, &["rev-parse", "HEAD"])
}

fn init(root: &Path) {
    git_ok(root, &["init", "-q", "-b", "main"]);
    git_ok(root, &["config", "user.name", "fixture"]);
    git_ok(root, &["config", "user.email", "fixture@example.invalid"]);
}

struct Fixture {
    repo: tempfile::TempDir,
    south: tempfile::TempDir,
    nixos: tempfile::TempDir,
    store: tempfile::TempDir,
}

fn fixture() -> (Fixture, String) {
    let fixture = Fixture {
        repo: tempfile::tempdir().unwrap(),
        south: tempfile::tempdir().unwrap(),
        nixos: tempfile::tempdir().unwrap(),
        store: tempfile::tempdir().unwrap(),
    };
    init(fixture.repo.path());
    init(fixture.south.path());
    init(fixture.nixos.path());
    commit(fixture.south.path(), &[("a", "1")], "south");
    commit(fixture.nixos.path(), &[("a", "1")], "nixos");
    let head = commit(
        fixture.repo.path(),
        &[(".safe-push", SAFE_PUSH), ("src/lib.rs", "fn a() {}"), ("Cargo.toml", "[package]"), ("docs/a.md", "a"), ("bin/tool", "x")],
        "base",
    );
    (fixture, head)
}

fn key_on(fixture: &Fixture, head: &str, command: &str, host_class: &str) -> String {
    key(fixture.repo.path(), head, command, &Machine { host_class, south: fixture.south.path(), nixos_config: fixture.nixos.path() }).unwrap()
}

fn key_of(fixture: &Fixture, head: &str, command: &str) -> String {
    key(fixture.repo.path(), head, command, &Machine { host_class: "local", south: fixture.south.path(), nixos_config: fixture.nixos.path() }).unwrap()
}

#[test]
fn docs_only_move_reuses_the_cargo_tests_result() {
    let (fixture, first) = fixture();
    let store = Store(fixture.store.path().to_owned());
    let before = key_of(&fixture, &first, "cargo test");
    store.record(&before, &first, 0, Timestamp::from_unix_seconds(1_000)).unwrap();
    let second = commit(fixture.repo.path(), &[("docs/a.md", "changed"), ("docs/b.md", "new")], "docs only");
    let after = key_of(&fixture, &second, "cargo test");
    assert_eq!(before, after);
    let pass = store.hit(&after).expect("docs-only move must reuse the pass");
    assert_eq!(pass.sha, first);
    assert!(reused_line("cargo test", &pass).starts_with("land check: cargo test reused (passed for these inputs at "));
}

#[test]
fn change_under_a_filtered_path_misses() {
    let (fixture, first) = fixture();
    let store = Store(fixture.store.path().to_owned());
    let before = key_of(&fixture, &first, "cargo test");
    store.record(&before, &first, 0, Timestamp::from_unix_seconds(1_000)).unwrap();
    let second = commit(fixture.repo.path(), &[("src/lib.rs", "fn b() {}")], "source change");
    let after = key_of(&fixture, &second, "cargo test");
    assert_ne!(before, after);
    assert!(store.hit(&after).is_none());
}

#[test]
fn failed_result_is_never_reused() {
    let (fixture, head) = fixture();
    let store = Store(fixture.store.path().to_owned());
    let key = key_of(&fixture, &head, "cargo test");
    store.record(&key, &head, 101, Timestamp::from_unix_seconds(1_000)).unwrap();
    assert!(store.hit(&key).is_none());
    store.record(&key, &head, 0, Timestamp::from_unix_seconds(1_000)).unwrap();
    assert!(store.hit(&key).is_some());
    store.record(&key, &head, 1, Timestamp::from_unix_seconds(2_000)).unwrap();
    assert!(store.hit(&key).is_none());
}

#[test]
fn whole_tree_check_misses_on_any_change() {
    let (fixture, first) = fixture();
    let before = key_of(&fixture, &first, "owners south/main");
    let second = commit(fixture.repo.path(), &[("docs/a.md", "changed")], "docs only");
    assert_ne!(before, key_of(&fixture, &second, "owners south/main"));
}

#[test]
fn machine_bound_check_misses_when_south_or_nixos_config_moves() {
    let (fixture, head) = fixture();
    let command = "./bin/head-north land-tests --select changed --only machine-bound";
    let before = key_of(&fixture, &head, command);
    assert_eq!(before, key_of(&fixture, &head, command));
    commit(fixture.south.path(), &[("a", "2")], "south moved");
    let after_south = key_of(&fixture, &head, command);
    assert_ne!(before, after_south);
    commit(fixture.nixos.path(), &[("a", "2")], "nixos moved");
    assert_ne!(after_south, key_of(&fixture, &head, command));
    let unbound = key_of(&fixture, &head, "cargo test");
    commit(fixture.south.path(), &[("a", "3")], "south moved again");
    assert_eq!(unbound, key_of(&fixture, &head, "cargo test"));
}

#[test]
fn command_outside_the_land_checks_has_no_key() {
    let (fixture, head) = fixture();
    let machine = Machine { host_class: "local", south: fixture.south.path(), nixos_config: fixture.nixos.path() };
    assert!(key(fixture.repo.path(), &head, "unlisted", &machine).is_none());
}

#[test]
fn verdict_from_another_host_class_is_not_reused() {
    let (fixture, head) = fixture();
    let local = key_on(&fixture, &head, "cargo test", "local");
    assert_eq!(local, key_on(&fixture, &head, "cargo test", "local"));
    assert_ne!(local, key_on(&fixture, &head, "cargo test", "gex45"));
    assert_ne!(local, key_on(&fixture, &head, "cargo test", "actions"));
    assert_ne!(key_on(&fixture, &head, "cargo test", "gex45"), key_on(&fixture, &head, "cargo test", "actions"));
}

#[test]
fn toolchain_change_misses_even_outside_the_filtered_paths() {
    let (fixture, first) = fixture();
    let before = key_of(&fixture, &first, "cargo test");
    let lock = commit(fixture.repo.path(), &[("flake.lock", "{\"nodes\":1}")], "add flake lock");
    let with_lock = key_of(&fixture, &lock, "cargo test");
    assert_ne!(before, with_lock);
    let moved = commit(fixture.repo.path(), &[("flake.lock", "{\"nodes\":2}")], "bump flake lock");
    let after_bump = key_of(&fixture, &moved, "cargo test");
    assert_ne!(with_lock, after_bump);
    let pinned = commit(fixture.repo.path(), &[("rust-toolchain.toml", "[toolchain]\nchannel = \"1.96\"")], "pin toolchain");
    assert_ne!(after_bump, key_of(&fixture, &pinned, "cargo test"));
}
