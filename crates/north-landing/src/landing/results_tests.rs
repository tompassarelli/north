use super::*;

fn env(south: &str) -> Env {
    Env { south_revision: south.into(), nixos_config_revision: "nixos1".into(), shell_closure: "/nix/store/shell".into(), runner: "ubuntu-24.04".into() }
}

fn case(test: &str, name: &str, passed: bool) -> Case {
    Case { test: test.into(), name: name.into(), passed }
}

const TREE: &str = "0123456789abcdef0123456789abcdef01234567";

#[test]
fn the_same_key_hits_and_returns_what_was_recorded() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::at(dir.path().to_owned());
    let key = Key::new(TREE, "cargo-tests.sh", &env("south1"));
    assert_eq!(store.lookup(&key), None);
    store.record(&key, &[case("a.rs", "x", true), case("a.rs", "y", false)]).unwrap();
    let again = Key::new(TREE, "cargo-tests.sh", &env("south1"));
    let found = store.lookup(&again).unwrap();
    assert_eq!(found.failed(), vec![("a.rs", "y")]);
    assert_eq!(found.get("a.rs", "x"), Some(Seen { passed: true, failed: false }));
    assert!(found.flaky().is_empty());
}

#[test]
fn a_changed_south_revision_command_or_tree_misses() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::at(dir.path().to_owned());
    store.record(&Key::new(TREE, "cargo-tests.sh", &env("south1")), &[case("a.rs", "x", false)]).unwrap();
    assert_eq!(store.lookup(&Key::new(TREE, "cargo-tests.sh", &env("south2"))), None);
    assert_eq!(store.lookup(&Key::new(TREE, "harness-tests.sh", &env("south1"))), None);
    assert_eq!(store.lookup(&Key::new(&TREE.replace('0', "f"), "cargo-tests.sh", &env("south1"))), None);
    assert!(store.lookup(&Key::new(TREE, "cargo-tests.sh", &env("south1"))).is_some());
}

#[test]
fn a_case_recorded_both_ways_on_one_key_is_flaky_even_across_recordings() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::at(dir.path().to_owned());
    let key = Key::new(TREE, "cargo-tests.sh", &env("south1"));
    store.record(&key, &[case("a.rs", "x", true), case("a.rs", "steady", true)]).unwrap();
    store.record(&key, &[case("a.rs", "x", false)]).unwrap();
    let found = store.lookup(&key).unwrap();
    assert_eq!(found.flaky(), vec![("a.rs", "x")]);
    assert_eq!(found.get("a.rs", "steady"), Some(Seen { passed: true, failed: false }));
}

#[test]
fn a_key_whose_tree_is_not_a_hex_object_id_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::at(dir.path().to_owned());
    let key = Key::new("../escape", "cargo-tests.sh", &env("south1"));
    assert!(store.record(&key, &[case("a.rs", "x", true)]).is_err());
    assert_eq!(store.lookup(&key), None);
}
