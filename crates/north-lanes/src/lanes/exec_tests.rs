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
    let member = lane_with_lock(&own_crate);
    std::fs::create_dir_all(member.path().join("crates/north-broker")).unwrap();
    std::fs::write(member.path().join("crates/north-broker/Cargo.toml"), "[package]\nname = \"north-broker\"\nversion = \"0.1.0\"\n").unwrap();
    std::fs::write(member.path().join("Cargo.toml"), "[package]\nname = \"north\"\nversion = \"0.1.0\"\n\n[workspace]\nmembers = [\"crates/*\"]\n").unwrap();
    assert_eq!(changed_locks(member.path()).unwrap(), Vec::<String>::new());
    let outside = format!("{own_crate}\n[[package]]\nname = \"ring\"\nversion = \"0.17.0\"\nsource = \"registry+https://example.invalid/index\"\nchecksum = \"bb\"\n");
    assert_eq!(changed_locks(lane_with_lock(&outside).path()).unwrap(), ["Cargo.lock"]);
    let bumped = WORKSPACE.replace("checksum = \"aa\"", "checksum = \"cc\"");
    assert_eq!(changed_locks(lane_with_lock(&bumped).path()).unwrap(), ["Cargo.lock"]);
}

// KEEP(bug): a path dependency outside the workspace is written to Cargo.lock without a source, like a member, so lane exec admitted it (south#94 review, finding 6)
#[test]
fn a_source_less_package_counts_as_unchanged_only_when_the_workspace_holds_its_manifest() {
    let manifest = "[package]\nname = \"north\"\nversion = \"0.1.0\"\n\n[workspace]\nmembers = [\"crates/inside\"]\n";
    let with_lock = |extra: &str| {
        let directory = lane_with_lock(&format!("{WORKSPACE}\n[[package]]\nname = \"{extra}\"\nversion = \"0.1.0\"\n"));
        std::fs::create_dir_all(directory.path().join("crates/inside")).unwrap();
        std::fs::write(directory.path().join("crates/inside/Cargo.toml"), "[package]\nname = \"inside\"\nversion = \"0.1.0\"\n").unwrap();
        std::fs::write(directory.path().join("Cargo.toml"), manifest).unwrap();
        directory
    };
    assert_eq!(changed_locks(with_lock("inside").path()).unwrap(), Vec::<String>::new());
    assert_eq!(changed_locks(with_lock("evil").path()).unwrap(), ["Cargo.lock"]);
}

// KEEP(bug): lane exec took the checkout to vouch for from --lane or the working directory, not from the program it runs (south#94 review, finding 7)
#[test]
fn a_program_built_in_another_checkout_than_the_named_lane_is_refused() {
    let main = lane_with_lock(WORKSPACE);
    let lane = lane_with_lock(WORKSPACE);
    let binary = lane.path().join("tool");
    std::fs::write(&binary, "#!/bin/sh\n").unwrap();
    let arguments = ["--lane".to_owned(), main.path().display().to_string(), "--".to_owned(), binary.display().to_string()];
    assert_eq!(run(&arguments, true, |_| None), 1);
}
