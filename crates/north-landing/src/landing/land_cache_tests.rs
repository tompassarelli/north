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

fn key_of(fixture: &Fixture, head: &str, command: &str) -> String {
    key(fixture.repo.path(), head, command, &Machine { host_class: "local", south: fixture.south.path(), nixos_config: fixture.nixos.path() }).unwrap()
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
