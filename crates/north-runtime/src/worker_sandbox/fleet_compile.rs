//! A check the outside broker runs for a session, whose command compiles Rust, run as that session's own fleet compile (north#1553): `north capacity host run` under the session's gate owner starts the worker domain around it, so the compile meets what one the session starts in its own shell meets, offered first to a build host and, on a host that compiles for no session once none took it, refused with `NO_BUILD_HOST` (offering.rs), never compiled there. Run in a domain alone it reached no gate that counts a fleet compile: the domain's own `cargo` asked the broker for the gate, which no longer knew the session, and the laptop compiled a session's `cargo nextest run --workspace` in place twice, at 11:45 PM and 11:48 PM on 2026-11-14, until laptop-load stopped it. Both checks the broker runs for a session take it: the finish check (finish_request.rs) and the Done-when checker's box command (`checked_shell`).

use std::path::Path;
use std::process::Command;

use super::{CHECKED, FLAG, lane_grant};
use north_capacity_host::capacity::gate::{class_of, compiles_rust};

/// How long a check that compiles may wait for and hold its capacity lease: what the capacity gate gives a compile the session starts itself.
const SECONDS: &str = "3600";

/// The words of `north` that run a worker domain around `program` as `session`'s fleet compile, ahead of that domain's own words, or `None` when `program` compiles no Rust.
pub(super) fn gate(session: &str, program: &[String]) -> Option<Vec<String>> {
    let class = class_of(program, 0).filter(|_| compiles_rust(program))?;
    let owner = format!("gate:{session}");
    Some(["capacity", "host", "run", "--class", class.name(), "--owner", &owner, "--timeout-seconds", SECONDS, "--"].map(str::to_owned).to_vec())
}

/// The command that runs the program `program` in a new worker domain whatever role starts it, judged by no hook: what the checker the broker starts for a session runs a box's Done-when command as, since the command's text comes from an issue a session may have written. A command that compiles Rust for the session the checker runs for (`NORTH_FLEET_SESSION`, which the broker gives it, check_request.rs) runs as that session's fleet compile, where the checker's own domain compiled it on the laptop under the owner `gate:done-when`.
pub fn checked_shell(program: &[String]) -> Command {
    let north = std::env::current_exe().unwrap_or_else(|_| north_base::platform::north_binary::north_binary());
    let session = std::env::var(lane_grant::SESSION_ENV).ok().filter(|session| !session.is_empty());
    let mut shell = Command::new(&north);
    shell.args(checked_words(&north, session.as_deref(), program));
    shell
}

/// The words `north` (at `north`) takes to run `program` in a new worker domain, ahead of them the capacity gate's run for `session` when `program` compiles Rust for one.
fn checked_words(north: &Path, session: Option<&str>, program: &[String]) -> Vec<String> {
    let domain = [FLAG, CHECKED, "--"].map(str::to_owned).into_iter().chain(program.iter().cloned());
    match session.and_then(|session| gate(session, program)) {
        Some(gate) => gate.into_iter().chain([north.to_string_lossy().into_owned()]).chain(domain).collect(),
        None => domain.collect(),
    }
}

#[cfg(test)]
#[path = "fleet_compile_tests.rs"]
mod tests;
