use super::bounded;
use super::failure::{DEFERRED, Failure, code, failed};
use super::layout::PROGRAMS;
use crate::ops::land_offload;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

const BUILD_ELSEWHERE: Duration = Duration::from_secs(30 * 60);
const COPY: Duration = Duration::from_secs(10 * 60);
const RSYNC_SOURCE_MISSING: u8 = 23;

pub(super) fn build(target: &str, runtime: &Path) -> Result<(), Failure> {
    let machine = land_offload::host_with_room()
        .map_err(|why| failed(DEFERRED, format!("this host is in game-reserve and {why}; runtime unchanged")))?;
    let host = &machine.name;
    println!("north runtime: building {} on {host} while this host is in game-reserve", &target[..8]);
    run(host, "north runtime update", ssh(&machine.ssh).args(["north", "runtime", "update"]), BUILD_ELSEWHERE)?;
    let built = format!("{}:.local/state/north/runtimes/{target}", machine.ssh);
    fs::create_dir_all(runtime)?;
    let mut copy = Command::new("rsync");
    copy.args(["-a", "--timeout=120"]).args(PROGRAMS.map(|program| format!("{built}/{program}"))).arg(runtime);
    run(host, "rsync", &mut copy, COPY).map_err(|failure| match failure.code {
        RSYNC_SOURCE_MISSING => failed(DEFERRED, format!("{host} has not finished building {}; retrying on the next tick ({})", &target[..8], failure.message)),
        _ => failure,
    })
}

pub(super) fn ssh(destination: &str) -> Command {
    let mut command = Command::new("ssh");
    command.args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=20", "-o", "ServerAliveInterval=15", "-o", "ServerAliveCountMax=4", destination]);
    command
}

pub(super) fn limit(default: Duration) -> Duration {
    std::env::var("NORTH_RUNTIME_STEP_SECONDS")
        .ok()
        .and_then(|seconds| seconds.parse().ok())
        .map_or(default, Duration::from_secs)
}

pub(super) fn run(host: &str, step: &str, command: &mut Command, default: Duration) -> Result<(), Failure> {
    let limit = limit(default);
    let status = bounded::status(command, limit).map_err(|error| match error.kind() {
        ErrorKind::TimedOut => failed(1, format!("{step} on {host} did not finish within {}s and was stopped", limit.as_secs())),
        _ => Failure::from(error),
    })?;
    if !status.success() {
        return Err(failed(code(status), format!("{step} on {host} failed: {status}")));
    }
    Ok(())
}
