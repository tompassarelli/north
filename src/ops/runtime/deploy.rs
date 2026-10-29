use super::failure::{Failure, failed};
use crate::ops::land_offload;
use super::layout::{Layout, PROGRAMS};
use super::remote::{run, ssh};
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
    let hosts = if hosts.is_empty() { land_offload::other_destinations() } else { hosts.to_vec() };
    if hosts.is_empty() {
        return Err(failed(64, "no host to deploy to: name one or declare [hosts.NAME] with an ssh destination"));
    }
    let mut failures = Vec::new();
    for host in &hosts {
        if let Err(failure) = ship(host, &target, &runtime, &references) {
            eprintln!("north runtime deploy: {}", failure.message);
            failures.push(failure);
        }
    }
    match failures.into_iter().next() {
        Some(failure) => Err(failure),
        None => Ok(()),
    }
}

fn ship(host: &str, target: &str, runtime: &Path, references: &BTreeSet<String>) -> Result<(), Failure> {
    let mut realise = vec!["nix-store", "--realise", "--quiet"];
    realise.extend(references.iter().map(String::as_str));
    run(host, "nix-store --realise", ssh(host).args(&realise).stdout(std::process::Stdio::null()))?;
    let destination = format!("{host}:.local/state/north/runtimes/{target}/");
    let mut copy = Command::new("rsync");
    copy.args(["-a", "--timeout=120", "--mkpath"]).args(PROGRAMS.map(|program| runtime.join(program))).arg(destination);
    run(host, "rsync", &mut copy)?;
    run(host, "north runtime update", ssh(host).args(["north", "runtime", "update"]))
}
