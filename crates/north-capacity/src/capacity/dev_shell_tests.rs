use std::path::PathBuf;

use super::{Invocation, placed};

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
