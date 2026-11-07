use std::process::Stdio;

use tokio::process::Command;

/// Where land checks run when `.safe-push` names `value`: that host, or for `build-host` the one `north capacity offload choose --any` picks with `away_from` left out, so a check that must not compile here is never answered with this host as the most room. When this host has the most room it is named like any other, so its checks run in its warm offload slots rather than building the candidate's own cold target; None only when no host has room.
pub(super) async fn chosen(program: &str, value: &str, away_from: Option<&str>) -> Option<String> {
    if value != "build-host" {
        return Some(value.to_owned());
    }
    let mut command = Command::new(program);
    command.args(["capacity", "offload", "choose", "--any"]);
    if let Some(here) = away_from {
        command.args(["--avoid", here]);
    }
    let chosen = command.stdin(Stdio::null()).stderr(Stdio::inherit()).output().await.ok().filter(|output| output.status.success())?;
    let host = String::from_utf8_lossy(&chosen.stdout).trim_end().to_owned();
    Some(if host.is_empty() { north_base::platform::host::name() } else { host })
}

#[cfg(test)]
#[path = "build_host_tests.rs"]
mod tests;
