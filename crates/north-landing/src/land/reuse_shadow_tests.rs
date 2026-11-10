use std::path::Path;
use std::process::Command;

use north_base::foundation::clock::Timestamp;
use north_base::verdict_cache::fingerprint::{self, Artifacts, Tree, Weak};
use north_base::verdict_cache::path_set::{self, Entry, Kind};
use north_base::verdict_cache::{Pass, Store};

use super::slots::unit_reuse::checked_environment;

const LINE: &str = "CARGO_TARGET_DIR=target ./bin/land-shell --dev -- cargo nextest run --locked --offline ${SAFE_PUSH_LAND_PACKAGES:---workspace} --lib --bins";
const UNIT: &str = "north-base verdict_cache::tests::a";

fn variables(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs.iter().map(|(name, value)| ((*name).to_owned(), (*value).to_owned())).collect()
}

fn repository() -> tempfile::TempDir {
    let repo = tempfile::tempdir().unwrap();
    std::fs::write(repo.path().join("a.txt"), "one\n").unwrap();
    for arguments in [&["init", "-q", "-b", "main"][..], &["add", "a.txt"], &["-c", "user.name=lander", "-c", "user.email=lander@example.invalid", "commit", "-q", "-m", "a"]] {
        assert!(Command::new("git").arg("-C").arg(repo.path()).args(arguments).status().unwrap().success());
    }
    repo
}

fn weak(root: &Path, tree: &Tree, environment: &[(String, String)]) -> String {
    let target = root.join("target");
    let environment = checked_environment(environment.iter().cloned(), &[(&target, "<target>"), (root, "<tree>")]);
    fingerprint::weak(&Weak { unit: UNIT, line: &fingerprint::without_selection(LINE), rustc: "rustc 1.96.0", environment: &environment }, tree)
}

#[test]
fn a_recorded_pass_whose_inputs_match_is_reused_by_the_shadow() {
    let repo = repository();
    let root = repo.path();
    let tree = Tree::load(root, "HEAD").unwrap();
    let mut artifacts = Artifacts::open(root, None, &root.join("target"), None);
    let kept = tempfile::tempdir().unwrap();
    let store = Store(kept.path().to_owned());

    let recording = variables(&[("HOME", "/home/tom"), ("LANG", "C.UTF-8"), ("XDG_SESSION_ID", "41"), ("INVOCATION_ID", "aaaa"), ("NORTH_FLEET_SESSION", "worker-1"), ("SAFE_PUSH_LAND_HEAD", "one")]);
    let recorded = weak(root, &tree, &recording);
    let entries = vec![Entry { kind: Kind::Content, path: "a.txt".into() }];
    let bytes = path_set::render(&entries);
    store.record_set(&recorded, bytes.as_bytes()).unwrap();
    let strong = fingerprint::strong(&recorded, &entries, &tree, &mut artifacts).unwrap();
    store.record_pass(&recorded, &strong, &Pass { at: Timestamp::now(), sha: "e4d65cb473158c329bbdcd4ec6a53ff7c478cf3e".into() }, "gex45").unwrap();

    let lookup = variables(&[("HOME", "/home/tom"), ("LANG", "C.UTF-8"), ("XDG_SESSION_ID", "97"), ("INVOCATION_ID", "bbbb"), ("NORTH_FLEET_SESSION", "worker-2"), ("SSH_CONNECTION", "10.0.0.1 22"), ("SAFE_PUSH_LAND_HEAD", "two")]);
    let looked = weak(root, &tree, &lookup);
    assert_eq!(looked, recorded, "a different login session and invocation of the same host leaves the weak key alone");
    assert!(fingerprint::reusable_set(&store, &looked, &tree, &mut artifacts).is_some(), "the recorded pass is found for the same inputs");

    let no_session = variables(&[("HOME", "/home/tom"), ("LANG", "C.UTF-8"), ("SAFE_PUSH_LAND_HEAD", "one")]);
    assert_ne!(weak(root, &tree, &no_session), recorded, "a run with no fleet session is another input: code under test branches on it");
    let other_home = variables(&[("HOME", "/home/other"), ("LANG", "C.UTF-8")]);
    assert_ne!(weak(root, &tree, &other_home), recorded, "a variable the sandbox carries still selects the key");
}
