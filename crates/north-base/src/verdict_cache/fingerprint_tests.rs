use std::fs;
use std::path::Path;

use super::fingerprint::{Artifacts, Tree, reusable_set, strong, without_selection};
use super::path_set::{Entry, Kind, render};
use super::{Pass, Store};
use crate::foundation::clock::Timestamp;
use crate::test_inputs::git::git_in;

fn commit(root: &Path, files: &[(&str, &str)]) -> String {
    for (path, text) in files {
        let target = root.join(path);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(target, text).unwrap();
    }
    git_in(root, &["add", "-A"]);
    git_in(root, &["commit", "-q", "--allow-empty", "-m", "fixture"]);
    git_in(root, &["rev-parse", "HEAD"])
}

fn content(path: &str) -> Entry {
    Entry { kind: Kind::Content, path: path.to_owned() }
}

fn record(store: &Store, weak: &str, tree: &Tree, artifacts: &mut Artifacts, entries: &[Entry]) {
    store.record_set(weak, render(entries).as_bytes()).unwrap();
    let strong = strong(weak, entries, tree, artifacts).unwrap();
    store.record_pass(weak, &strong, &Pass { at: Timestamp::from_unix_seconds(1), sha: "ab12".to_owned() }, "gex45").unwrap();
}

#[test]
fn a_pass_recorded_for_a_test_that_read_a_file_outside_its_crate_is_not_reused_after_that_file_changes_and_a_test_that_read_only_its_crate_is_reused_across_a_change_elsewhere() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path().join("tree");
    fs::create_dir_all(&root).unwrap();
    git_in(&root, &["init", "-q", "-b", "main"]);
    let before = commit(&root, &[("crates/a/src/lib.rs", "fn a() {}"), ("harness/x", "one")]);
    let after = commit(&root, &[("harness/x", "two")]);
    let (tree_before, tree_after) = (Tree::load(&root, &before).unwrap(), Tree::load(&root, &after).unwrap());
    let mut artifacts = Artifacts::open(&root, None, &root.join("target"), None);
    let store = Store(scratch.path().join("store"));
    record(&store, "aa01", &tree_before, &mut artifacts, &[content("crates/a/src/lib.rs")]);
    record(&store, "bb02", &tree_before, &mut artifacts, &[content("crates/a/src/lib.rs"), content("harness/x")]);

    assert!(reusable_set(&store, "bb02", &tree_before, &mut artifacts).is_some(), "the unchanged tree reuses the pass");
    assert!(reusable_set(&store, "aa01", &tree_after, &mut artifacts).is_some(), "a test that read only its own crate is reused across a change elsewhere");
    assert!(reusable_set(&store, "bb02", &tree_after, &mut artifacts).is_none(), "a test that read harness/x is not reused after harness/x changes");
}

#[test]
fn a_compiled_test_binary_is_keyed_by_the_sources_of_every_crate_it_links_and_by_no_other() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path().join("tree");
    fs::create_dir_all(&root).unwrap();
    git_in(&root, &["init", "-q", "-b", "main"]);
    let files = [("crates/a/src/lib.rs", "a"), ("crates/b/src/lib.rs", "b"), ("crates/c/src/lib.rs", "c"), ("harness/y", "y")];
    let first = commit(&root, &files);
    let deps = root.join("target/debug/deps");
    fs::create_dir_all(&deps).unwrap();
    fs::write(deps.join("a-0123456789abcdef.d"), "target/debug/deps/a-0123456789abcdef: crates/a/src/lib.rs\n").unwrap();
    fs::write(deps.join("libb-1111111111111111.d"), "target/debug/deps/libb-1111111111111111.rlib: crates/b/src/lib.rs harness/y\n").unwrap();
    fs::write(deps.join("libc-2222222222222222.d"), "target/debug/deps/libc-2222222222222222.rlib: crates/c/src/lib.rs\n").unwrap();
    let package = |name: &str, id: usize| format!(r#"{{"id":"{id}","name":"{name}","source":null,"manifest_path":"{}/crates/{name}/Cargo.toml","targets":[{{"name":"{name}","kind":["lib"],"src_path":"{}/crates/{name}/src/lib.rs"}}]}}"#, root.display(), root.display());
    let metadata = format!(r#"{{"packages":[{},{},{}],"resolve":{{"nodes":[{{"id":"0","dependencies":["1"]}},{{"id":"1","dependencies":[]}},{{"id":"2","dependencies":[]}}]}}}}"#, package("a", 0), package("b", 1), package("c", 2));
    let artifact = "debug/deps/a-0123456789abcdef";
    let answer = |commit: &str| {
        let mut artifacts = Artifacts::open(&root, None, &root.join("target"), Some(&metadata));
        artifacts.answer(&Tree::load(&root, commit).unwrap(), artifact)
    };
    let unrelated = commit(&root, &[("crates/c/src/lib.rs", "c changed")]);
    let linked = commit(&root, &[("crates/b/src/lib.rs", "b changed")]);
    let read = commit(&root, &[("harness/y", "y changed")]);

    let base = answer(&first).unwrap();
    assert_eq!(answer(&unrelated).unwrap(), base, "a crate the binary does not link does not key it");
    assert_ne!(answer(&linked).unwrap(), base, "a linked crate's source keys it");
    assert_ne!(answer(&read).unwrap(), answer(&linked).unwrap(), "a file a linked crate's dep-info names keys it");
}

#[test]
fn selection_variables_leave_the_check_line_the_same_whatever_their_defaults() {
    let narrow = "cargo nextest run ${SAFE_PUSH_LAND_PACKAGES:--p one} --lib ${SAFE_PUSH_LAND_IT:---test it -E 'binary_id(north::it)'}";
    let wide = "cargo nextest run ${SAFE_PUSH_LAND_PACKAGES:---workspace} --lib ${SAFE_PUSH_LAND_IT:---test it}";
    assert_eq!(without_selection(narrow), without_selection(wide));
    assert_eq!(without_selection("a ${HOME} b"), "a ${HOME} b");
}

#[test]
fn a_pass_recorded_for_a_test_that_read_an_absolute_host_file_is_keyed_by_that_files_bytes() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path().join("tree");
    fs::create_dir_all(&root).unwrap();
    git_in(&root, &["init", "-q", "-b", "main"]);
    let head = commit(&root, &[("crates/a/src/lib.rs", "fn a() {}")]);
    let tree = Tree::load(&root, &head).unwrap();
    let mut artifacts = Artifacts::open(&root, None, &root.join("target"), None);
    let directory = scratch.path().join("host");
    fs::create_dir_all(&directory).unwrap();
    let host = directory.join("os-release");
    fs::write(&host, "one").unwrap();
    let key = |kind: Kind, path: &Path, artifacts: &mut Artifacts| strong("aa01", &[Entry { kind, path: path.display().to_string() }], &tree, artifacts);
    let (before, listing_before) = (key(Kind::Content, &host, &mut artifacts), key(Kind::Listing, &directory, &mut artifacts));
    assert!(before.is_some() && listing_before.is_some());
    fs::write(&host, "two").unwrap();
    assert_ne!(before, key(Kind::Content, &host, &mut artifacts), "a changed host file changes the key");
    assert_eq!(listing_before, key(Kind::Listing, &directory, &mut artifacts), "a listing keys names and modes, not bytes");
    fs::remove_file(&host).unwrap();
    assert!(key(Kind::Absent, &host, &mut artifacts).is_some(), "an absent host path is an answer");
    assert_eq!(key(Kind::Content, &host, &mut artifacts), None, "a host file that cannot be read has no answer");
}
