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

#[test]
fn a_program_built_in_another_checkout_than_the_named_lane_is_refused_but_a_runtime_in_a_store_repository_runs() {
    let main = lane_with_lock(WORKSPACE);
    let lane = lane_with_lock(WORKSPACE);
    let binary = lane.path().join("target/debug/tool");
    std::fs::create_dir_all(binary.parent().unwrap()).unwrap();
    std::fs::write(&binary, "#!/bin/sh\n").unwrap();
    let arguments = |program: &Path| ["--lane".to_owned(), main.path().display().to_string(), "--".to_owned(), program.display().to_string()];
    assert_eq!(run(&arguments(&binary), true, |_| None), 1);
    let store = tempfile::tempdir().unwrap();
    init_repo(store.path(), None);
    let runtime = store.path().join("runtimes/b1c5d036dd/tree/bin/verify-north");
    std::fs::create_dir_all(runtime.parent().unwrap()).unwrap();
    std::fs::write(&runtime, "#!/bin/sh\n").unwrap();
    assert_eq!(run_with(&arguments(&runtime), true, |_| Some(0), |_, _| 7), 0);
}

#[test]
fn a_lane_bumping_a_registry_crate_runs_its_pre_queue_checks_inside_depbox() {
    let bumped = lane_with_lock(&WORKSPACE.replace("checksum = \"aa\"", "checksum = \"cc\""));
    assert!(matches!(route(bumped.path()), Ok(Route::Depbox)));
    assert!(matches!(route(lane_with_lock(WORKSPACE).path()), Ok(Route::Leave)));
    let program = ["target/debug/north".to_owned(), "docs".to_owned(), "rationale-index".to_owned()];
    let command = depbox_command(&program);
    let words: Vec<String> = command.get_args().map(|word| word.to_string_lossy().into_owned()).collect();
    assert_eq!(words, ["depbox", "run", "--", "target/debug/north", "docs", "rationale-index"]);
}

#[test]
fn a_confined_run_of_a_bumped_lane_goes_through_depbox_and_a_failed_replacement_reports_why() {
    let through_depbox: fn(&mut Command, &str) -> u8 = |command, _| if command.get_args().eq(["depbox", "run", "--", "target/debug/north"]) { 42 } else { 7 };
    let run_in_lane = |lock: &str| {
        let lane = lane_with_lock(lock);
        let arguments = ["--lane".to_owned(), lane.path().display().to_string(), "--".to_owned(), "target/debug/north".to_owned()];
        run_with(&arguments, true, |_| Some(9), through_depbox)
    };
    assert_eq!(run_in_lane(&WORKSPACE.replace("checksum = \"aa\"", "checksum = \"cc\"")), 42);
    assert_eq!(run_in_lane(WORKSPACE), 9);
    assert_eq!(replace_with(&mut Command::new("/nonexistent/tool"), "tool"), 127);
    assert_eq!(replace_with(&mut Command::new("/"), "tool"), 126);
}
