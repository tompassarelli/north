use std::path::PathBuf;

use super::{Invocation, inside, placed};

fn words(text: &str) -> Vec<String> {
    text.split_whitespace().map(str::to_owned).collect()
}

fn run(directory: &str, command: &str) -> Invocation {
    Invocation { directory: PathBuf::from(directory), prepare_only: false, command: words(command) }
}

#[test]
fn a_fleet_session_cargo_test_on_the_laptop_is_leased_through_remote_admission() {
    let fleet_compile = |command: &[String]| command.first().is_some_and(|program| program == "cargo");
    assert_eq!(
        placed("north", &run("/lane", "cargo test -p x"), true, &fleet_compile),
        Some(words("north capacity gate cargo test -p x")),
        "`north capacity dev-shell run -- cargo test` from a worker shell on the laptop entered the dev shell, whose own cargo comes before the gate's shim on PATH, so the test ran in no lease and was never offered to a build host (north#1036)"
    );
    assert_eq!(
        placed("north", &run("/other", "cargo test"), false, &fleet_compile),
        Some(words("north capacity gate north capacity dev-shell run --dir /other -- cargo test")),
        "a run in another flake's dev shell reaches the gate whole, so its lease enters that shell"
    );
    assert_eq!(placed("north", &run("/lane", "cargo fmt"), true, &|_| false), None, "a run the gate leases nothing for, or one already inside its lease, enters the dev shell here");
    assert_eq!(placed("north", &Invocation { directory: PathBuf::from("/lane"), prepare_only: true, command: Vec::new() }, true, &fleet_compile), None);
}

#[test]
fn a_buck2_lane_made_after_the_shell_was_entered_enters_it_again_for_its_toolchain_link() {
    let lane = tempfile::tempdir().unwrap();
    assert!(inside(Some("/nix/store/shell.drv"), "/nix/store/shell.drv", lane.path()), "a checkout with no Buck2 project runs directly in the shell it is in");
    std::fs::write(lane.path().join(".buckconfig"), "").unwrap();
    assert!(
        !inside(Some("/nix/store/shell.drv"), "/nix/store/shell.drv", lane.path()),
        "a fresh lane whose command started inside another checkout's identical dev shell ran directly, so the hook never linked .buckconfig.flake and buck2 stopped at `buckconfig [nix] clippy_driver is unset` (north#1116, live run on gex45 2026-11-12)"
    );
    std::os::unix::fs::symlink(lane.path().join(".buckconfig"), lane.path().join(".buckconfig.flake")).unwrap();
    assert!(inside(Some("/nix/store/shell.drv"), "/nix/store/shell.drv", lane.path()));
    assert!(!inside(None, "/nix/store/shell.drv", lane.path()));
}
