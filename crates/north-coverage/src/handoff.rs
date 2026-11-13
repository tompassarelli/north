//! Handing a coverage refresh to a build host: a host that is not one starts no instrumented build
//! itself, so `refresh --start` runs on a build host over ssh and the unit starts there.

use std::process::{Command, Stdio};
use std::time::Duration;

use north_base::platform::{host, orchestration, process};

use crate::run_selected::host as chosen_host;

const HAND_OFF: Duration = Duration::from_secs(60);
const HOP: &str = "NORTH_COVERAGE_HOP";

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Place {
    Here,
    On(String),
}

/// `None` when this host runs the refresh itself; otherwise the build host's answer to
/// `refresh --start ARGUMENTS`, or why none could take it.
pub(crate) fn forwarded(repository: &str, arguments: &[String]) -> Option<Result<String, String>> {
    let hopped = std::env::var_os(HOP).is_some();
    let placed = build_hosts().and_then(|builders| placed(&host::name(), &builders, hopped, chosen_host));
    match placed {
        Ok(Place::Here) => None,
        Ok(Place::On(destination)) => Some(forward(&destination, repository, arguments)),
        Err(why) => Some(Err(why)),
    }
}

/// The orchestration's build hosts; an orchestration that cannot be read names none, so no refresh
/// starts rather than one running here.
fn build_hosts() -> Result<Vec<(String, String)>, String> {
    orchestration::read().map(|cfg| orchestration::build_hosts(&cfg)).map_err(|why| format!("the orchestration cannot be read ({why}), so no build host is known and none starts"))
}

/// A build host, or a host whose orchestration lists none, runs the refresh unit itself; any other
/// host hands the refresh to a build host, so the instrumented build and its output never land here.
/// A refresh that was already handed on once is not handed on again.
pub(crate) fn placed(here: &str, builders: &[(String, String)], hopped: bool, choose: impl FnOnce() -> String) -> Result<Place, String> {
    if builders.is_empty() || builders.iter().any(|(name, _)| name == here) {
        return Ok(Place::Here);
    }
    if hopped {
        return Err(format!("{here} is not a build host by its own list and the refresh was already handed on once, so none starts"));
    }
    let chosen = choose();
    builders.iter().find(|(name, _)| *name == chosen).map(|(_, destination)| Place::On(destination.clone())).ok_or_else(|| format!("{here} is not a build host and no build host took the refresh (chose {chosen:?}), so none starts"))
}

pub(crate) fn script(repository: &str, arguments: &[String]) -> String {
    let words: String = arguments.iter().map(|word| format!(" '{}'", word.replace('\'', "'\\''"))).collect();
    format!("cd \"$HOME/code/{repository}/main\" && n=\"${{AGENT_CAPACITY_NORTH:-$HOME/.local/state/north/current/north}}\" && {HOP}=1 \"$n\" test coverage refresh --start{words}")
}

fn forward(destination: &str, repository: &str, arguments: &[String]) -> Result<String, String> {
    if !crate::hosts::plain(repository) {
        return Err(format!("{repository:?} is not a plain repository name, so its refresh is not handed to a build host"));
    }
    let mut command = Command::new("ssh");
    command.args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=10", "-o", "ServerAliveInterval=5", "-o", "ServerAliveCountMax=2", "-T", "--", destination, &script(repository, arguments)]).stdin(Stdio::null());
    match process::try_run_command_traced_within(command, None, HAND_OFF) {
        Ok(Ok(output)) if output.status.success() => Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned()),
        Ok(Ok(output)) => Err(format!("ssh {destination} exited {}: {}", output.status.code().map_or("on a signal".to_owned(), |code| code.to_string()), String::from_utf8_lossy(&output.stderr).trim())),
        Ok(Err(stalled)) => Err(format!("ssh {destination} gave no answer within {} s and was killed: {stalled}", HAND_OFF.as_secs())),
        Err(error) => Err(format!("ssh {destination}: {error}")),
    }
}
