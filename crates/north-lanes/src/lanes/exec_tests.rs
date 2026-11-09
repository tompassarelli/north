use super::*;
use crate::lanes::test_git::{git_in, init_repo};

const WORKSPACE: &str = "[[package]]\nname = \"north\"\nversion = \"0.1.0\"\n\n[[package]]\nname = \"serde\"\nversion = \"1.0.0\"\nsource = \"registry+https://example.invalid/index\"\nchecksum = \"aa\"\n";

fn lane_with_lock(lock: &str) -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    init_repo(directory.path(), None);
    std::fs::write(directory.path().join("Cargo.lock"), WORKSPACE).unwrap();
    git_in(directory.path(), &["add", "Cargo.lock"]);
    git_in(directory.path(), &["commit", "-qm", "lock"]);
    git_in(directory.path(), &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    std::fs::write(directory.path().join("Cargo.lock"), lock).unwrap();
    directory
}

#[test]
fn a_lock_that_adds_only_a_workspace_crate_is_unchanged_and_one_that_adds_outside_code_is_changed() {
    let own_crate = format!("{WORKSPACE}\n[[package]]\nname = \"north-broker\"\nversion = \"0.1.0\"\ndependencies = [\"serde\"]\n");
    assert_eq!(changed_locks(lane_with_lock(&own_crate).path()).unwrap(), Vec::<String>::new());
    let outside = format!("{own_crate}\n[[package]]\nname = \"ring\"\nversion = \"0.17.0\"\nsource = \"registry+https://example.invalid/index\"\nchecksum = \"bb\"\n");
    assert_eq!(changed_locks(lane_with_lock(&outside).path()).unwrap(), ["Cargo.lock"]);
    let bumped = WORKSPACE.replace("checksum = \"aa\"", "checksum = \"cc\"");
    assert_eq!(changed_locks(lane_with_lock(&bumped).path()).unwrap(), ["Cargo.lock"]);
}
