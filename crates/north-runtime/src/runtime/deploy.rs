use super::failure::{Failure, failed};
use super::host_facts;
use super::layout::{self, Layout, PROGRAMS, TREE};
use super::remote::{OWN_RSH, limit, output, run, run_reporting, ssh};
use super::store::references;
use super::update::{built_main, checkout, lock_waiting, named_lock_waiting};
use crate::land_offload;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

const LOCK_WAIT: Duration = Duration::from_secs(30 * 60);
const REALISE: Duration = Duration::from_secs(10 * 60);
const COPY: Duration = Duration::from_secs(10 * 60);
const PROBE: Duration = Duration::from_secs(20);
const INSTALL_LOCK_WAIT: u64 = 15;
const PROMOTE: Duration = Duration::from_secs(15 * 60);

pub(super) fn deploy(layout: &Layout, hosts: &[String]) -> Result<(), Failure> {
    let (target, runtime) = {
        let _building = lock_waiting(layout, limit(LOCK_WAIT), "north runtime deploy")?;
        built_main(layout, &checkout())?
    };
    ship_to(layout, &target, &runtime, hosts, &|| false)
}

pub(super) fn deploy_live(layout: &Layout) -> Result<(), Failure> {
    let target = layout.live_sha().ok_or_else(|| failed(1, "this host has no live runtime to ship"))?;
    ship_to(layout, &target, &layout.runtime(&target), &[], &|| layout.live_sha().is_some_and(|live| live != target))
}

/// Each host takes its turn on its own lock, so a host slow to promote delays only its own next copy; a copy whose runtime a newer one has replaced here while it waited is skipped, since the newer one is shipped on its own.
fn ship_to(layout: &Layout, target: &str, runtime: &Path, hosts: &[String], superseded: &(dyn Fn() -> bool + Sync)) -> Result<(), Failure> {
    let references = references(runtime)?;
    let hosts = if hosts.is_empty() {
        let plan = land_offload::deployment();
        if let Some(line) = plan.skip_line() {
            println!("{line}");
        }
        plan.destinations
    } else {
        hosts.to_vec()
    };
    if hosts.is_empty() {
        return Err(failed(64, "no host to deploy to: name one or declare [hosts.NAME] with an ssh or deploy_ssh destination"));
    }
    let failures: Vec<Failure> = std::thread::scope(|scope| {
        let references = &references;
        let shipping: Vec<_> = hosts.iter().map(|host| scope.spawn(move || shipped(layout, host, target, runtime, references, superseded))).collect();
        shipping.into_iter().filter_map(|shipped| shipped.join().unwrap_or_else(|_| Err(failed(1, "shipping panicked"))).err()).collect()
    });
    for failure in &failures {
        eprintln!("north runtime deploy: {}", failure.message);
    }
    match failures.into_iter().next() {
        Some(failure) => Err(failure),
        None => Ok(()),
    }
}

fn shipped(layout: &Layout, host: &str, target: &str, runtime: &Path, references: &BTreeSet<String>, superseded: &(dyn Fn() -> bool + Sync)) -> Result<(), Failure> {
    let _turn = named_lock_waiting(layout, &host_lock("deploy", host), limit(LOCK_WAIT), "north runtime deploy")?;
    if superseded() {
        println!("north runtime deploy: {host}: {} was replaced here while it waited its turn; the newer runtime is shipped on its own", layout::short(target));
        return Ok(());
    }
    ship(host, target, runtime, references)
}

pub(super) fn ship_runtime(host: &str, target: &str, runtime: &Path) -> Result<(), Failure> {
    ship(host, target, runtime, &references(runtime)?)
}

pub(super) fn stage_runtime(host: &str, target: &str, runtime: &Path) -> Result<(), Failure> {
    stage(host, target, runtime, &references(runtime)?)
}

fn stage(host: &str, target: &str, runtime: &Path, references: &BTreeSet<String>) -> Result<(), Failure> {
    let _staging = named_lock_waiting(&Layout::from_env(), &stage_lock(host), limit(COPY), "north runtime deploy")?;
    println!("north runtime deploy: {host}: nix-store --realise of {} store paths unless runtimes/{} is already whole there", references.len(), layout::short(target));
    let prepared = output(host, "prepare", ssh(host).args(["sh", "-c", prepare_script(target, references).as_str()]), REALISE)?;
    let basis = match Prepared::read(&prepared) {
        Prepared::Whole => {
            println!("north runtime deploy: {host}: runtimes/{} is already whole, so nothing is copied", layout::short(target));
            return Ok(());
        }
        Prepared::Staging(basis) => basis,
    };
    println!("north runtime deploy: {host}: copying {} and its {TREE} to runtimes/{}{}", PROGRAMS.join(" and "), layout::short(target), basis.as_deref().map_or_else(String::new, |basis| format!(", only what differs from runtimes/{}", layout::short(basis))));
    let destination = format!("{host}:.local/state/north/runtimes/{target}.staging/");
    let mut copy = Command::new("rsync");
    copy.args(["-a", "-z", "--timeout=120", "-e", OWN_RSH]).args(basis.map(|basis| format!("--copy-dest=../{basis}"))).args(PROGRAMS.map(|program| runtime.join(program))).arg(runtime.join(TREE)).arg(destination);
    run(host, "rsync", &mut copy, COPY)?;
    run(host, "install", ssh(host).args(["sh", "-c", install_script(target).as_str()]), PROBE)
}

/// One lock per destination, so the land checks that find a runtime missing on a host at once ship it once and the rest find it whole, instead of each sending it over the same link.
fn stage_lock(host: &str) -> String {
    host_lock("stage", host)
}

fn host_lock(kind: &str, host: &str) -> String {
    format!("runtime-{kind}-{}.lock", host.chars().map(|character| if character.is_ascii_alphanumeric() || character == '.' { character } else { '-' }).collect::<String>())
}

#[derive(Debug, PartialEq, Eq)]
enum Prepared {
    Whole,
    Staging(Option<String>),
}

impl Prepared {
    fn read(said: &str) -> Self {
        if said.lines().any(|line| line == WHOLE) {
            return Self::Whole;
        }
        Self::Staging(said.lines().find_map(|line| line.strip_prefix(BASIS)).filter(|basis| layout::is_sha(basis)).map(str::to_owned))
    }
}

const WHOLE: &str = "whole";
const BASIS: &str = "basis ";

/// The one connection before a copy: says `whole` when `runtimes/TARGET` already holds every program and its tree, else realises the store paths the runtime references, makes the staging directory and names the newest whole runtime there, whose unchanged files rsync copies on the host and whose changed ones it sends only the differences of.
fn prepare_script(target: &str, references: &BTreeSet<String>) -> String {
    let programs = PROGRAMS.iter().map(|program| format!("[ -x \"$1/{program}\" ]")).collect::<Vec<_>>().join(" && ");
    let realise = references.iter().map(|reference| land_offload::ship::quote(reference)).collect::<Vec<_>>().join(" ");
    format!(
        "set -e; r=.local/state/north/runtimes; whole() {{ [ -d \"$1/{TREE}\" ] && {programs}; }}; if whole \"$r/{target}\"; then echo {WHOLE}; exit 0; fi; {}mkdir -p \"$r/{target}.staging\"; for d in $(ls -t \"$r\"); do case $d in {target} | *[!0-9a-f]*) continue ;; esac; if whole \"$r/$d\"; then echo \"{BASIS}$d\"; break; fi; done",
        if realise.is_empty() { String::new() } else { format!("nix-store --realise --quiet {realise} >/dev/null; ") }
    )
}

/// Moves the staged runtime into place only whole, so `runtimes/SHA` never holds a north without its tree: the offloaded land checks resolve their harness tables from it.
pub(super) fn install_script(target: &str) -> String {
    format!("set -e; t=.local/state/north/runtimes/{target}; exec 9>>\"$t.lock\"; flock -w {INSTALL_LOCK_WAIT} 9; if [ -d \"$t/{TREE}\" ] && [ -x \"$t/north\" ]; then rm -rf \"$t.staging\"; exit 0; fi; rm -rf \"$t.old\"; if [ -e \"$t\" ]; then mv \"$t\" \"$t.old\"; fi; mv \"$t.staging\" \"$t\"; rm -rf \"$t.old\"")
}

fn ship(host: &str, target: &str, runtime: &Path, references: &BTreeSet<String>) -> Result<(), Failure> {
    stage(host, target, runtime, references)?;
    println!("north runtime deploy: {host}: promoting {} with the shipped north", layout::short(target));
    let shipped = format!(".local/state/north/runtimes/{target}/north");
    run_reporting(host, &format!("north runtime update --sha {}", layout::short(target)), ssh(host).args([shipped.as_str(), "runtime", "update", "--sha", target]), PROMOTE)?;
    if let Some(relayed) = land_offload::deployment().relays_facts.into_iter().find(|relayed| relayed.destination == host) {
        host_facts::relay(&Layout::from_env(), &runtime.join("north"), host, &relayed.host);
    }
    Ok(())
}

/// Detached, so the update lock is released for the next build while the copy continues; `promote` stays alive until it has shipped, and its own copy then finds the runtime whole or waits on the host's stage lock.
pub(super) fn stage_for_lander(layout: &Layout, target: &str, runtime: &Path) {
    if fs::symlink_metadata(layout.lander()).is_err() {
        return;
    }
    for host in land_offload::deployment().destinations {
        let (target, runtime) = (target.to_owned(), runtime.to_owned());
        std::thread::spawn(move || {
            if let Err(failure) = stage_runtime(&host, &target, &runtime) {
                eprintln!("north runtime update: staging on {host} before the swap: {}; its first offloaded land check stages it instead", failure.message);
            }
        });
    }
}

#[cfg(test)]
#[path = "deploy_tests.rs"]
mod tests;
