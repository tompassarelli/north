use super::failure::{Failure, failed};
use super::layout::{self, Layout, PROGRAMS};
use super::remote::{limit, run, ssh};
use super::store::references;
use super::update::{built_main, checkout, lock};
use crate::ops::land_offload;
use rustix::fs::FlockOperation;
use std::collections::BTreeSet;
use std::fs::File;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const LOCK_WAIT: Duration = Duration::from_secs(30 * 60);
const REALISE: Duration = Duration::from_secs(10 * 60);
const COPY: Duration = Duration::from_secs(10 * 60);
const PROMOTE: Duration = Duration::from_secs(15 * 60);

pub(super) fn deploy(layout: &Layout, hosts: &[String]) -> Result<(), Failure> {
    let _lock = lock_within(layout, limit(LOCK_WAIT))?;
    let checkout = checkout();
    let (target, runtime) = built_main(layout, &checkout)?;
    let references = references(&runtime)?;
    let hosts = if hosts.is_empty() { land_offload::other_destinations() } else { hosts.to_vec() };
    if hosts.is_empty() {
        return Err(failed(64, "no host to deploy to: name one or declare [hosts.NAME] with an ssh or deploy_ssh destination"));
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

fn lock_within(layout: &Layout, limit: Duration) -> Result<File, Failure> {
    let started = Instant::now();
    let mut announced = false;
    loop {
        if let Some(held) = lock(layout, FlockOperation::NonBlockingLockExclusive)? {
            return Ok(held);
        }
        if !announced {
            println!("north runtime deploy: waiting up to {}s for runtime-update.lock, held by another north runtime update", limit.as_secs());
            announced = true;
        }
        if started.elapsed() >= limit {
            return Err(failed(1, format!("runtime-update.lock stayed held by another north runtime update for {}s", limit.as_secs())));
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}

fn ship(host: &str, target: &str, runtime: &Path, references: &BTreeSet<String>) -> Result<(), Failure> {
    println!("north runtime deploy: {host}: nix-store --realise of {} store paths", references.len());
    let mut realise = vec!["nix-store", "--realise", "--quiet"];
    realise.extend(references.iter().map(String::as_str));
    run(host, "nix-store --realise", ssh(host).args(&realise).stdout(Stdio::null()), REALISE)?;
    println!("north runtime deploy: {host}: copying {} to runtimes/{}", PROGRAMS.join(" and "), layout::short(target));
    let destination = format!("{host}:.local/state/north/runtimes/{target}/");
    let mut copy = Command::new("rsync");
    copy.args(["-a", "--timeout=120", "--mkpath"]).args(PROGRAMS.map(|program| runtime.join(program))).arg(destination);
    run(host, "rsync", &mut copy, COPY)?;
    println!("north runtime deploy: {host}: waiting for north runtime update to promote {}", layout::short(target));
    run(host, "north runtime update", ssh(host).args(["north", "runtime", "update"]), PROMOTE)
}
