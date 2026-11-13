use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;
use tokio::time::Instant;

use super::arguments::Environment;
use super::{OFF_HOST_PATIENCE, OFF_HOST_RETRY, say, warn};

const NO_ROOM: &str = "north capacity offload: no host has room: ";
const UNANSWERED: &str = "unreachable or unreadable";
const NO_DESTINATION: &str = "none has an ssh destination";

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Choice {
    Host(String),
    Full,
    Silent,
    NoHosts,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Placed {
    Host(String),
    Here,
    Unanswered,
}

fn refusal(stderr: &str) -> Choice {
    let Some(reasons) = stderr.lines().find_map(|line| line.trim().strip_prefix(NO_ROOM)) else { return Choice::Silent };
    if reasons.trim() == NO_DESTINATION {
        return Choice::NoHosts;
    }
    if reasons.split("; ").any(|reason| !reason.contains(UNANSWERED)) { Choice::Full } else { Choice::Silent }
}

/// What `north capacity offload choose --any` says with each of `away` left out: a host, every answering host full, no host answering, or no host configured.
pub(super) async fn choice(program: &str, away: &[String]) -> Choice {
    let mut command = Command::new(program);
    command.args(["capacity", "offload", "choose", "--any"]);
    for host in away {
        command.args(["--avoid", host]);
    }
    let Ok(output) = command.stdin(Stdio::null()).output().await else { return Choice::Silent };
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprint!("{stderr}");
    if !output.status.success() {
        return refusal(&stderr);
    }
    let host = String::from_utf8_lossy(&output.stdout).trim_end().to_owned();
    Choice::Host(if host.is_empty() { north_base::platform::host::name() } else { host })
}

/// Where land checks run when `.safe-push` names `value`: that host, or for `build-host` the one `north capacity offload choose --any` picks with each host in `away` left out, so a check that must not compile here is never answered with this host as the most room. When this host has the most room it is named like any other, so its checks run in its warm offload slots rather than building the candidate's own cold target; None only when no host has room.
pub(super) async fn chosen(program: &str, value: &str, away: &[String]) -> Option<String> {
    if value != "build-host" {
        return Some(value.to_owned());
    }
    match choice(program, away).await {
        Choice::Host(host) => Some(host),
        _ => None,
    }
}

/// The host a check before queueing runs on. With every answering build host full it waits in line for one, as long as it takes, asking again every `OFF_HOST_RETRY`; hosts in `failed` are left out until nothing else answers; when no build host answers for `OFF_HOST_PATIENCE` it gives up, and when none is configured the check is `Here`.
pub(super) async fn placed(program: &str, value: &str, here: &str, failed: &[String]) -> Placed {
    if value != "build-host" {
        return Placed::Host(value.to_owned());
    }
    let mut away = vec![here.to_owned()];
    away.extend(failed.iter().cloned());
    let (started, mut silent) = (Instant::now(), None::<Instant>);
    loop {
        match choice(program, &away).await {
            Choice::Host(host) => {
                if started.elapsed() >= Duration::from_secs(1) {
                    say(&format!("offload: waited {}s in line for a build host", started.elapsed().as_secs()));
                }
                return Placed::Host(host);
            }
            Choice::NoHosts if away.len() == 1 => return Placed::Here,
            Choice::NoHosts | Choice::Silent if away.len() > 1 => away.truncate(1),
            Choice::NoHosts | Choice::Silent => {
                if silent.get_or_insert_with(Instant::now).elapsed() + OFF_HOST_RETRY > OFF_HOST_PATIENCE {
                    return Placed::Unanswered;
                }
                say(&format!("safe-push: no build host answers yet; asking again in {} s", OFF_HOST_RETRY.as_secs()));
                tokio::time::sleep(OFF_HOST_RETRY).await;
            }
            Choice::Full => {
                silent = None;
                say(&format!("safe-push: no build host has room; this check waits in line for one ({} s so far), asking again in {} s", started.elapsed().as_secs(), OFF_HOST_RETRY.as_secs()));
                tokio::time::sleep(OFF_HOST_RETRY).await;
            }
        }
    }
}

/// The host a land check set runs on, leaving out those in `avoid`, which the selection's test listing runs on too: an off-host set (a replay's) only ever on a build host, so it is `None` when none has room, whatever `.safe-push` names; any other set on the host `.safe-push` or the order names, or on this host under the land-check capacity lease when no build host has room.
pub(super) async fn runner_host(program: &str, environment: &Environment, configured: Option<&str>, avoid: &[String]) -> Option<String> {
    if environment.off_host {
        return match placed(program, "build-host", &north_base::platform::host::name(), avoid).await {
            Placed::Host(host) => Some(host),
            Placed::Here | Placed::Unanswered => None,
        };
    }
    let land_runner = environment.runner.as_deref().or(configured).unwrap_or_default();
    if land_runner.is_empty() {
        return Some(String::new());
    }
    Some(resolve_runner_host(program, land_runner, avoid).await)
}

pub(super) async fn resolve_runner_host(program: &str, value: &str, avoid: &[String]) -> String {
    chosen(program, value, avoid).await.unwrap_or_else(|| {
        warn("safe-push: no build host has room; the land checks run on this host under the land-check capacity lease");
        String::new()
    })
}

#[cfg(test)]
#[path = "build_host_tests.rs"]
mod tests;
