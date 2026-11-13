use super::failure::Failure;
use super::layout::{self, BUILT_FROM, BUILT_ON, Layout, PROGRAMS};
use super::pin::{self, Event};
use super::pull;
use super::remote;
use crate::land_offload::BuildHost;
use crate::platform::ssh_retry::Rerun;
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

const BUILD: Duration = Duration::from_secs(30 * 60);

/// Where a runtime was built, how long it took and the hash of the north it made, for the pin journal.
pub(super) struct Built {
    pub host: String,
    pub seconds: i64,
    pub binary: String,
}

/// The build hosts that answered, most free cores first, else each host's reason.
pub(super) fn answered(hosts: Vec<BuildHost>) -> Result<Vec<BuildHost>, String> {
    if hosts.is_empty() {
        return Err("this host reaches no build host".to_owned());
    }
    let unanswered: Vec<String> = hosts.iter().filter(|host| host.free_cpus.is_none()).map(|host| format!("{}: unreachable or unreadable over ssh", host.name)).collect();
    let mut answered: Vec<BuildHost> = hosts.into_iter().filter(|host| host.free_cpus.is_some()).collect();
    if answered.is_empty() {
        return Err(format!("no build host answered ({})", unanswered.join("; ")));
    }
    answered.sort_by(|one, other| other.free_cpus.partial_cmp(&one.free_cpus).unwrap_or(std::cmp::Ordering::Equal));
    Ok(answered)
}

/// Builds `target` on the build host with the most free cores, then on each next one that answered while one fails, and takes its binaries into `runtime`, marked with the host and the binary's hash and verified as built from `target`; an error names why each build host did not build it, and the caller builds here.
pub(super) fn build(target: &str, runtime: &Path, hosts: Vec<BuildHost>, mut build_on: impl FnMut(&BuildHost, &str, &Path) -> Result<(), Failure>) -> Result<Built, String> {
    let short = layout::short(target);
    let mut refused = Vec::new();
    for host in answered(hosts)? {
        let which = if refused.is_empty() { "the build host with the most free cores" } else { "the next build host" };
        println!("north runtime: building {short} on {}, {which} ({:.0})", host.name, host.free_cpus.unwrap_or_default());
        match take(&host, target, runtime, &mut build_on) {
            Ok(built) => return Ok(built),
            Err(why) => refused.push(why),
        }
    }
    Err(refused.join("; "))
}

pub(super) fn take(host: &BuildHost, target: &str, runtime: &Path, build_on: &mut impl FnMut(&BuildHost, &str, &Path) -> Result<(), Failure>) -> Result<Built, String> {
    let short = layout::short(target);
    let started = Instant::now();
    let taken = build_on(host, target, runtime).map_err(|failure| failure.message).and_then(|()| layout::built_from_own_commit(runtime));
    if let Err(why) = taken {
        for program in PROGRAMS.iter().chain([&BUILT_FROM]) {
            let _ = fs::remove_file(runtime.join(program));
        }
        return Err(format!("{} did not build {short}: {why}", host.name));
    }
    let binary = layout::binary_hash(&runtime.join("north")).map_err(|error| error.to_string())?;
    fs::write(runtime.join(BUILT_ON), format!("{} {binary}\n", host.name)).map_err(|error| error.to_string())?;
    let built = Built { host: host.name.clone(), seconds: started.elapsed().as_secs() as i64, binary };
    println!("north runtime: built {short} on {} in {}s and took its binaries", built.host, built.seconds);
    Ok(built)
}

pub(super) fn note(layout: &Layout, target: &str, built: Built) {
    pin::note(layout, &Event::Built { sha: target.to_owned(), host: built.host, at: pin::now(), seconds: built.seconds, binary: built.binary });
}

/// Asks `host` to build `target` with its own runtime under its capacity admission, then copies the programs and their built-from stamp here and realises their store closure.
pub(super) fn build_on(host: &BuildHost, target: &str, runtime: &Path) -> Result<(), Failure> {
    let command = format!("\"$HOME/.local/state/north/current/north\" runtime build {target}");
    remote::run_reporting_again(&host.ssh, &host.name, "north runtime build", Rerun::Anywhere, |link| {
        let mut ssh = remote::ssh_on(&host.ssh, link);
        ssh.arg(&command);
        ssh
    }, BUILD)?;
    pull::fetch(&host.ssh, target, runtime, &[BUILT_FROM]).map_err(|why| super::failure::failed(1, format!("taking the build from {}: {why}", host.name)))
}

#[cfg(test)]
mod tests;
