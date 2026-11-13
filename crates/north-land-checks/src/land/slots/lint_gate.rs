//! `north land lint -- COMMAND`: runs a land check's lint line on the host that executes it. A host whose
//! `[hosts.NAME] buck2_lint` is set runs the Buck2 clippy gate in its place (north#1115) and judges it by Buck2's exit
//! status alone; every other host, and every other line, runs the command as declared. The checks before a landing wrap
//! their lint line in it, so a check offloaded from a host without the key is linted the executing host's way. The run
//! after a landing wraps it too, but a host runs the gate for it only when its `buck2_after_run` is set; it keys its verdict apart.

use std::process::Command;

use north_base::foundation::exit::NO_VERDICT;

use super::unit_reuse::quote;

const USAGE: &str = "north land lint available | north land lint after-run-available | north land lint -- COMMAND";
const BASE_ENV: &str = "SAFE_PUSH_LAND_BASE";
/// Buck2's exit status for a build the tree's own targets or code failed; an engine or daemon failure exits with another.
const TREE_ERROR: i32 = 3;
const FAILED: u8 = 1;
/// Keys the lint a run after a landing does apart from the one before it, whose pass may be the gate's.
pub const AFTER_LANDING: (&str, &str) = ("NORTH_LAND_LINT", "cargo-after-landing");

/// Whether `command` is the lint line a host's `buck2_lint` may replace.
pub fn wraps(command: &str) -> bool {
    north_build_system::lints_with_cargo(command)
}

/// `command` handed to the executing host's `north`, or run as it is where that north has no such verb.
pub fn wrapped(north: &str, command: &str) -> String {
    wrapped_by(north, command, "available")
}

/// `command` handed to the executing host's `north` only where its `buck2_after_run` is set, else run as it is: the wrapper of the run after a landing.
pub fn wrapped_after(north: &str, command: &str) -> String {
    wrapped_by(north, command, "after-run-available")
}

fn wrapped_by(north: &str, command: &str, probe: &str) -> String {
    format!("n={north}; if \"$n\" land lint {probe} >/dev/null 2>&1; then exec \"$n\" land lint -- {}; fi\n{command}", quote(command))
}

/// The check's exit status for the gate's: Buck2's success and its tree error are verdicts, any other status is none.
fn verdict(status: Option<i32>) -> u8 {
    match status {
        Some(0) => 0,
        Some(TREE_ERROR) => FAILED,
        _ => NO_VERDICT,
    }
}

fn execute(declared: &str) -> u8 {
    let base = std::env::var(BASE_ENV).ok().filter(|base| !base.is_empty());
    let gate = std::env::current_dir().ok().and_then(|root| north_build_system::lint_gate(&root, base.as_deref(), declared));
    if let Some(line) = &gate {
        eprintln!("north land lint: this host lints with the Buck2 gate: {line}");
    }
    let status = Command::new("bash").args(["-euo", "pipefail", "-c", gate.as_deref().unwrap_or(declared)]).status().ok().and_then(|status| status.code());
    match gate {
        Some(_) => {
            let verdict = verdict(status);
            if verdict == NO_VERDICT {
                eprintln!("north land lint: buck2 exited {}, which says nothing of the tree, so the check has no verdict", status.map_or_else(|| "on a signal".to_owned(), |code| code.to_string()));
            }
            verdict
        }
        None => status.and_then(|code| u8::try_from(code).ok()).unwrap_or(FAILED),
    }
}

pub fn run(arguments: &[String]) -> u8 {
    match arguments {
        [verb] if verb == "available" => 0,
        [verb] if verb == "after-run-available" => u8::from(!north_build_system::after_run_here()),
        [separator, command] if separator == "--" => execute(command),
        _ => {
            eprintln!("usage: {USAGE}");
            64
        }
    }
}
