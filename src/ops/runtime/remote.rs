use super::failure::{Failure, code, failed};
use super::layout::PROGRAMS;
use crate::ops::land_offload;
use std::fs;
use std::path::Path;
use std::process::Command;

pub(super) fn build(target: &str, runtime: &Path) -> Result<(), Failure> {
    let machine = land_offload::host_with_room()
        .map_err(|why| failed(75, format!("this host is in game-reserve and {why}; runtime unchanged")))?;
    let host = &machine.name;
    println!("north runtime: building {} on {host} while this host is in game-reserve", &target[..8]);
    run(host, "north runtime update", ssh(&machine.ssh).args(["north", "runtime", "update"]))?;
    let built = format!("{}:.local/state/north/runtimes/{target}", machine.ssh);
    fs::create_dir_all(runtime)?;
    let mut copy = Command::new("rsync");
    copy.arg("-a").args(PROGRAMS.map(|program| format!("{built}/{program}"))).arg(runtime);
    run(host, "rsync", &mut copy)
}

pub(super) fn ssh(destination: &str) -> Command {
    let mut command = Command::new("ssh");
    command.args(["-o", "BatchMode=yes", destination]);
    command
}

pub(super) fn run(host: &str, step: &str, command: &mut Command) -> Result<(), Failure> {
    let status = command.status()?;
    if !status.success() {
        return Err(failed(code(status), format!("{step} on {host} failed: {status}")));
    }
    Ok(())
}
