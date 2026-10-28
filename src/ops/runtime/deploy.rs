use super::failure::{Failure, code, failed};
use super::layout::{Layout, PROGRAMS};
use super::store::references;
use super::update::{built_main, checkout, lock};
use rustix::fs::FlockOperation;
use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

pub(super) fn deploy(layout: &Layout, hosts: &[String]) -> Result<(), Failure> {
    let _lock = lock(layout, FlockOperation::LockExclusive)?;
    let (target, runtime) = built_main(layout, &checkout())?;
    let references = references(&runtime)?;
    for host in hosts {
        ship(host, &target, &runtime, &references)?;
    }
    Ok(())
}

fn ship(host: &str, target: &str, runtime: &Path, references: &BTreeSet<String>) -> Result<(), Failure> {
    let mut realise = vec!["nix-store", "--realise", "--quiet"];
    realise.extend(references.iter().map(String::as_str));
    run(host, "nix-store --realise", ssh(host).args(&realise).stdout(std::process::Stdio::null()))?;
    let destination = format!("{host}:.local/state/north/runtimes/{target}/");
    let mut copy = Command::new("rsync");
    copy.args(["-a", "--mkpath"]).args(PROGRAMS.map(|program| runtime.join(program))).arg(destination);
    run(host, "rsync", &mut copy)?;
    run(host, "north runtime update", ssh(host).args(["north", "runtime", "update"]))
}

fn ssh(host: &str) -> Command {
    let mut command = Command::new("ssh");
    command.args(["-o", "BatchMode=yes", host]);
    command
}

fn run(host: &str, step: &str, command: &mut Command) -> Result<(), Failure> {
    let status = command.status()?;
    if !status.success() {
        return Err(failed(code(status), format!("{step} on {host} failed: {status}")));
    }
    Ok(())
}
