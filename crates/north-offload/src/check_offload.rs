//! Where a check command that compiles runs: on a host that is not a build host it goes to one through `north capacity offload run`, so no check compiles on the laptop (north#1508).

use std::process::Command;

const TOOLCHAIN: [&str; 5] = ["cargo", "rustc", "cargo-nextest", "bun", "nix"];
const TIMEOUT_SECONDS: &str = "900";
pub const NO_HOST_EXIT: i32 = 75;

pub fn compiles(command: &str) -> bool {
    command.split(|character: char| character.is_whitespace() || "|&;()<>$`'\"".contains(character)).any(|word| TOOLCHAIN.contains(&word.rsplit('/').next().unwrap_or_default()))
}

pub fn this_host_builds() -> bool {
    let host = north_base::platform::host::name();
    north_base::platform::orchestration::read().ok().and_then(|policy| policy.get("hosts")?.get(host.as_str())?.get("offload_builds")?.as_bool()) == Some(true)
}

pub fn chosen_host(north: &str) -> Option<String> {
    let output = Command::new(north).args(["capacity", "offload", "choose"]).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).output().ok().filter(|output| output.status.success())?;
    let host = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (!host.is_empty() && !host.contains(char::is_whitespace)).then_some(host)
}

/// The command line that runs `command` on a build host against `base..head`, `None` when it runs where it stands (this host builds, or the command compiles nothing), or why no build host takes it.
pub fn offloaded(north: &str, command: &str, (base, head): (&str, &str), builds_here: bool, choose: impl FnOnce() -> Option<String>) -> Result<Option<Vec<String>>, String> {
    if builds_here || !compiles(command) {
        return Ok(None);
    }
    let host = choose().ok_or_else(|| "no build host answers, and this host compiles nothing: the check runs on a build host or not at all".to_owned())?;
    Ok(Some(["capacity", "offload", "run", "--host", &host, "--base", base, "--head", head, "--timeout", TIMEOUT_SECONDS, "--", command].into_iter().map(str::to_owned).fold(vec![north.to_owned()], |mut words, word| {
        words.push(word);
        words
    })))
}
