use std::path::Path;
use std::process::{Command, Stdio};

use super::super::after::follow;
use super::super::setting;

const BUILD_HOST: &str = "build-host";

/// Where one precheck runs: this host, or a build host its checks ship to through `north capacity offload run`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CheckHost {
    Here(String),
    Remote(String),
}

impl CheckHost {
    pub fn name(&self) -> &str {
        match self {
            Self::Here(name) | Self::Remote(name) => name,
        }
    }

    /// The `SAFE_PUSH_LAND_RUNNER` value that pins the checker to this host: empty runs every check here.
    pub fn runner(&self) -> &str {
        match self {
            Self::Here(_) => "",
            Self::Remote(name) => name,
        }
    }
}

/// Whether the repository's land checks pick a build host, so each precheck slot is placed by free cores.
pub fn placed(repo: &Path) -> bool {
    setting::landing(repo, "refs/remotes/origin/main", "land.runner") == BUILD_HOST
}

/// The host with the most free cores once `taken` (one name per precheck already running) is counted and every host in `avoid` is left out, or why none has room.
pub fn choose(taken: &[String], avoid: &[String]) -> Result<CheckHost, String> {
    let mut command = Command::new(follow::program());
    command.args(["capacity", "offload", "choose", "--any"]);
    for host in taken {
        command.args(["--taken", host]);
    }
    for host in avoid {
        command.args(["--avoid", host]);
    }
    let output = command.stdin(Stdio::null()).output().map_err(|error| format!("{} capacity offload choose: {error}", follow::program()))?;
    let chosen = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if !output.status.success() {
        let why = String::from_utf8_lossy(&output.stderr);
        return Err(why.lines().rev().find(|line| !line.trim().is_empty()).unwrap_or("no host has room").trim().to_owned());
    }
    Ok(if chosen.is_empty() { CheckHost::Here(north_base::platform::host::name()) } else { CheckHost::Remote(chosen) })
}
