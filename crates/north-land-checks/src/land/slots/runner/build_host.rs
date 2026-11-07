use std::process::Stdio;

use tokio::process::Command;

/// Where land checks run when `.safe-push` names `value`: that host, or for `build-host` the one `north capacity offload choose --any` picks with `away_from` left out, so a check that must not compile here is never answered with this host as the most room, which prints no host.
pub(super) async fn chosen(program: &str, value: &str, away_from: Option<&str>) -> Option<String> {
    if value != "build-host" {
        return Some(value.to_owned());
    }
    let mut command = Command::new(program);
    command.args(["capacity", "offload", "choose", "--any"]);
    if let Some(here) = away_from {
        command.args(["--avoid", here]);
    }
    let chosen = command.stdin(Stdio::null()).stderr(Stdio::inherit()).output().await;
    chosen.ok().filter(|output| output.status.success()).map(|output| String::from_utf8_lossy(&output.stdout).trim_end().to_owned()).filter(|host| !host.is_empty())
}

#[cfg(test)]
#[path = "build_host_tests.rs"]
mod tests;
