use std::path::Path;

use super::checked_words;
use crate::worker_sandbox::{CHECKED, FLAG};

#[test]
fn a_done_when_command_that_compiles_runs_as_the_asking_sessions_fleet_compile_north_1553() {
    let north = Path::new("/var/lib/agent-runtime/current/north");
    let shell = |command: &str| ["sh", "-c", command].map(str::to_owned);
    let compiled = checked_words(north, Some("w-asking"), &shell("cargo nextest run --workspace"));
    let gate = ["capacity", "host", "run", "--class", "heavy", "--owner", "gate:w-asking", "--timeout-seconds", "3600", "--", "/var/lib/agent-runtime/current/north", FLAG, CHECKED, "--", "sh", "-c", "cargo nextest run --workspace"];
    assert_eq!(compiled, gate, "the checker's box command that compiles Rust escaped the capacity gate's run as the asking session's fleet compile, so the laptop compiled it under gate:done-when");
    assert!(north_capacity_host::capacity::gate::compiles_rust(&compiled[10..]), "the gate's run does not read the checked command as a Rust compile");
    assert_eq!(north_capacity_host::capacity::build_host_command::for_build_host(&compiled[10..], false), ["north", "depbox", "run", "--", "sh", "-c", "cargo nextest run --workspace"], "a build host was sent the laptop's worker-domain wrapper");
    assert_eq!(checked_words(north, Some("w-asking"), &shell("echo ran")), [FLAG, CHECKED, "--", "sh", "-c", "echo ran"], "a box command that compiles nothing left its domain for the capacity gate");
    assert_eq!(checked_words(north, None, &shell("cargo nextest run --workspace")), [FLAG, CHECKED, "--", "sh", "-c", "cargo nextest run --workspace"], "a checker run for no session was charged to one");
}
