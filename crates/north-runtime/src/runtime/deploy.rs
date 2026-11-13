use super::deadline::{self, Reached};
use super::failure::{Failure, failed};
use super::host_facts;
use super::layout::{self, BUILT_FROM, Layout, PROGRAMS, TREE};
use super::pin::{self, Event};
use super::remote::{OWN_RSH, limit, output, run, run_reporting, ssh};
use super::store::references;
use super::update::{built_main, checkout, named_lock_waiting, promotion_waiting};
use north_offload::land_offload;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

type Superseded = Arc<dyn Fn() -> bool + Send + Sync>;

const LOCK_WAIT: Duration = Duration::from_secs(30 * 60);
const REALISE: Duration = Duration::from_secs(10 * 60);
const COPY: Duration = Duration::from_secs(10 * 60);
const PROBE: Duration = Duration::from_secs(20);
const INSTALL_LOCK_WAIT: u64 = 15;
const PROMOTE: Duration = Duration::from_secs(15 * 60);
const UNKNOWN_FLAG: u8 = 64;

pub(super) fn deploy(layout: &Layout, hosts: &[String]) -> Result<(), Failure> {
    let (target, runtime) = {
        let _building = promotion_waiting(layout, limit(LOCK_WAIT), "north runtime deploy")?;
        built_main(layout, &checkout())?
    };
    ship_to(layout, &target, &runtime, hosts, Arc::new(|| false))
}

pub(super) fn deploy_live(layout: &Layout) -> Result<(), Failure> {
    let target = layout.live_sha().ok_or_else(|| failed(1, "this host has no live runtime to ship"))?;
    let here = Layout { state: layout.state.clone() };
    let shipped = target.clone();
    ship_to(layout, &target, &layout.runtime(&target), &[], Arc::new(move || here.live_sha().is_some_and(|live| live != shipped)))
}

/// Each host takes its turn on its own lock, so a host slow to promote delays only its own next copy; a copy whose runtime a newer one has replaced here while it waited is skipped, since the newer one is shipped on its own. A host that has not answered by its deadline (deadline.rs) is named skipped and left to the next tick's settle, which ships every host still behind.
fn ship_to(layout: &Layout, target: &str, runtime: &Path, hosts: &[String], superseded: Superseded) -> Result<(), Failure> {
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
    let references = Arc::new(references);
    let jobs: Vec<deadline::Job<Result<(), Failure>>> = hosts.into_iter().map(|host| {
        let within = deadline::of(layout, &host);
        let (here, name, target, runtime, references, superseded) = (Layout { state: layout.state.clone() }, host.clone(), target.to_owned(), runtime.to_owned(), Arc::clone(&references), Arc::clone(&superseded));
        let job: Box<dyn FnOnce() -> Result<(), Failure> + Send> = Box::new(move || shipped(&here, &name, &target, &runtime, &references, &superseded, within));
        (host, within, job)
    }).collect();
    let failures: Vec<Failure> = deadline::each_within(jobs).into_iter().filter_map(|(host, reached)| match reached {
        Reached::Answered(shipped) => shipped.err(),
        Reached::Skipped(within) => {
            println!("north runtime deploy: {host}: skipped: no answer within its {}s deadline, twice the p90 of its ships; the next tick ships it", within.as_secs());
            None
        }
    }).collect();
    for failure in &failures {
        eprintln!("north runtime deploy: {}", failure.message);
    }
    match failures.into_iter().next() {
        Some(failure) => Err(failure),
        None => Ok(()),
    }
}

fn shipped(layout: &Layout, host: &str, target: &str, runtime: &Path, references: &BTreeSet<String>, superseded: &Superseded, within: Duration) -> Result<(), Failure> {
    let _turn = named_lock_waiting(layout, &host_lock("deploy", host), limit(LOCK_WAIT).min(within), "north runtime deploy")?;
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
    let layout = Layout::from_env();
    let _staging = named_lock_waiting(&layout, &stage_lock(host), limit(COPY), "north runtime deploy")?;
    let started = Instant::now();
    println!("north runtime deploy: {host}: nix-store --realise of {} store paths unless runtimes/{} is already whole there", references.len(), layout::short(target));
    let prepared = output(host, "prepare", ssh(host).args(["sh", "-c", &land_offload::ship::quote(&prepare_script(target, references))]), REALISE)?;
    let basis = match Prepared::read(&prepared) {
        Prepared::Whole => {
            println!("north runtime deploy: {host}: runtimes/{} is already whole, so nothing is copied", layout::short(target));
            return Ok(());
        }
        Prepared::Staging(basis) => basis,
    };
    println!("north runtime deploy: {host}: copying {}, its {BUILT_FROM} record and its {TREE} to runtimes/{}{}", PROGRAMS.join(" and "), layout::short(target), basis.as_deref().map_or_else(String::new, |basis| format!(", only what differs from runtimes/{}", layout::short(basis))));
    let destination = format!("{host}:.local/state/north/runtimes/{target}.staging/");
    let mut copy = Command::new("rsync");
    copy.args(["-a", "-z", "--timeout=120", "-e", OWN_RSH]).args(basis.map(|basis| format!("--copy-dest=../{basis}"))).args(PROGRAMS.map(|program| runtime.join(program))).arg(runtime.join(BUILT_FROM)).arg(runtime.join(TREE)).arg(destination);
    run(host, "rsync", &mut copy, COPY)?;
    run(host, "install", ssh(host).args(["sh", "-c", &land_offload::ship::quote(&install_script(target))]), PROBE)?;
    pin::note(&layout, &Event::Shipped { sha: target.to_owned(), host: host.to_owned(), at: pin::now(), seconds: i64::try_from(started.elapsed().as_secs()).unwrap_or(i64::MAX) });
    Ok(())
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

/// The one connection before a copy: says `whole` when `runtimes/TARGET` already holds every program, its built-from record and its tree, else realises the store paths the runtime references, makes the staging directory and names the runtime this host currently runs, whose unchanged files rsync copies on the host and whose changed ones it sends only the differences of; a host with no whole current names its newest whole runtime instead.
fn prepare_script(target: &str, references: &BTreeSet<String>) -> String {
    let programs = PROGRAMS.iter().map(|program| format!("[ -x \"$1/{program}\" ]")).collect::<Vec<_>>().join(" && ");
    let realise = references.iter().map(|reference| land_offload::ship::quote(reference)).collect::<Vec<_>>().join(" ");
    format!(
        "set -e; r=.local/state/north/runtimes; whole() {{ [ -d \"$1/{TREE}\" ] && [ -f \"$1/{BUILT_FROM}\" ] && {programs}; }}; if whole \"$r/{target}\"; then echo {WHOLE}; exit 0; fi; {}mkdir -p \"$r/{target}.staging\"; live=$(readlink \"$r/../current\" 2>/dev/null || true); case $live in runtimes/*) live=${{live#runtimes/}} ;; *) live= ;; esac; if [ -n \"$live\" ] && [ \"$live\" != {target} ] && whole \"$r/$live\"; then echo \"{BASIS}$live\"; exit 0; fi; for d in $(ls -t \"$r\"); do case $d in {target} | *[!0-9a-f]*) continue ;; esac; if whole \"$r/$d\"; then echo \"{BASIS}$d\"; break; fi; done",
        if realise.is_empty() { String::new() } else { format!("nix-store --realise --quiet {realise} >/dev/null; ") }
    )
}

/// Moves the staged runtime into place only whole, so `runtimes/SHA` never holds a north without its tree and built-from record: the offloaded land checks resolve their harness tables from it.
pub(super) fn install_script(target: &str) -> String {
    format!("set -e; t=.local/state/north/runtimes/{target}; exec 9>>\"$t.lock\"; flock -w {INSTALL_LOCK_WAIT} 9; if [ -d \"$t/{TREE}\" ] && [ -f \"$t/{BUILT_FROM}\" ] && [ -x \"$t/north\" ]; then rm -rf \"$t.staging\"; exit 0; fi; rm -rf \"$t.old\"; if [ -e \"$t\" ]; then mv \"$t\" \"$t.old\"; fi; mv \"$t.staging\" \"$t\"; rm -rf \"$t.old\"")
}

pub(super) fn promote_staged(host: &str, target: &str, runtime: &Path) -> Result<(), Failure> {
    println!("north runtime deploy: {host}: promoting {} with the shipped north", layout::short(target));
    let shipped = format!(".local/state/north/runtimes/{target}/north");
    run_reporting(host, &format!("north runtime update --sha {}", layout::short(target)), ssh(host).args([shipped.as_str(), "runtime", "update", "--sha", target]), PROMOTE)?;
    if let Some(relayed) = land_offload::deployment().relays_facts.into_iter().find(|relayed| relayed.destination == host) {
        host_facts::relay(&Layout::from_env(), &runtime.join("north"), host, &relayed.host);
    }
    Ok(())
}

pub(super) fn move_back_to_ordered(host: &str, target: &str) -> Result<(), Failure> {
    println!("north runtime deploy: {host}: moving back to the ordered {}", layout::short(target));
    run_reporting(host, &format!("north runtime update --sha {} --ordered", layout::short(target)), ssh(host).args([".local/state/north/current/north", "runtime", "update", "--sha", target, "--ordered"]), PROMOTE).map_err(|failure| not_moved_back(host, target, failure))
}

/// A runtime from before an order could move a host back answers `--ordered` with its usage, exit 64, and stays live: only an order of a commit newer than it, which a ship takes forward, puts a runtime that knows the flag there.
fn not_moved_back(host: &str, target: &str, failure: Failure) -> Failure {
    match failure.code == UNKNOWN_FLAG {
        true => failed(failure.code, format!("{host} cannot be moved back to the ordered {}: the runtime live on {host} is from before an order could move a host back and does not take `north runtime update --sha SHA --ordered`, so {host} keeps it until an order of a newer commit ships it a runtime that does", layout::short(target))),
        false => failure,
    }
}

fn ship(host: &str, target: &str, runtime: &Path, references: &BTreeSet<String>) -> Result<(), Failure> {
    stage(host, target, runtime, references)?;
    promote_staged(host, target, runtime)
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
