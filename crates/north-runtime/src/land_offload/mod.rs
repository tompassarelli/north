use crate::capacity::{agent_load, laptop_auto, mode};
use crate::pacing::placement::{config, gate};
use ship::CommitId;
use std::path::Path;

mod destinations;
mod readiness;
pub mod warm;
mod trim;
pub mod verdict;
pub mod room;
pub mod ship;
pub(crate) mod slot_lock;

const USAGE: &str = "usage: north capacity offload choose [--any [--taken HOST]... [--avoid HOST]... [--needs PATH]... [--prefer HOST] [--count N]] (prints the host to run on, nothing when this host has the most room; each --taken counts one land check already on its way to HOST, each --avoid leaves HOST out, and each --needs names a host input the land check reads, a path such as ~/.local/state/north/graph.db that a host must hold to take it, and --prefer prints HOST itself when it answers and holds every input; --count prints a host for each of N land checks, this host by name, each counted as taken for the next) | cost | hosts (prints each offload host this host reaches and its ssh destination, one per line) | take [--once] (polls the hosts this host reaches and runs their oldest queued agent heavy run when it has room) | slots (prints the free then total land checks the reachable build hosts take at once, each host's free cores over the cores one check uses) | free --host NAME (prints how many of the host's slots for this checkout are free) | reap --owner OWNER | run [--background] --host NAME (this host's own name runs it in this host's slots) --base BASE --head HEAD --timeout SECONDS -- COMMAND (--background runs it in the checkout's one background slot, behind every land check and outside their job cap) | warm (brings this host's land-check slot furthest behind origin/main to that head and builds the repository's landing.warm command there under a heavy lease, while at most two land checks hold slots here) | closed (prints this host's name and exits 75 when `[hosts.NAME] offload_builds = false` closes it to offloaded checks, which the offload's script asks on each wait step so a job in its line leaves it) | trim-target TARGET KEEP_MIB (trims a slot's cargo target that no build holds to KEEP_MIB, least recently used build output first) | slot record NAME (records NAME as a build slot of the checkout it is run from, which the offload's script asks when its own domain cannot write the record) | line wait|record|report (this host's slot line, see `north capacity offload line`) | verdict hit KEY... | verdict record --host NAME --key KEY (--pass SHA | --fail) | verdict sync [--host NAME]...";

/// Set by the outside broker in the process it starts for a worker command's offload request: its `choose` only reads each host's room and claims no host for a waiting land check, since none is behind it and a claim stops every other worker's builds there until it lapses (south#118).
pub(crate) use north_base::foundation::offload::WORKER_REQUEST_ENV;

pub fn run(arguments: &[String]) -> u8 {
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    if worker_request(&words).is_some_and(|host| host.is_none_or(reaches))
        && let Some(code) = crate::worker_sandbox::reenter_outside(&["capacity", "offload"].into_iter().map(str::to_owned).chain(arguments.iter().cloned()).collect::<Vec<_>>())
    {
        return code;
    }
    match words.as_slice() {
        ["choose"] => choose(false, &Placement::default()),
        ["choose", "--any", rest @ ..] => Placement::parse(rest).map_or_else(usage, |placement| choose(true, &placement)),
        ["cost"] => cost(),
        ["hosts"] => hosts(),
        ["take", rest @ ..] => crate::build_offload::take::run(&rest.iter().map(|word| (*word).to_owned()).collect::<Vec<_>>()),
        ["verdict", rest @ ..] => verdict::run(rest),
        ["free" | "slots" | "slot", ..] => slot(&words),
        ["reap", "--owner", owner] if !owner.is_empty() => reap(owner),
        ["warm"] => warm::run(),
        ["closed"] => closed(),
        ["trim-target", rest @ ..] => trim::run(rest),
        ["run", "--background", rest @ ..] => run_job(rest, ship::SlotClass::Background),
        ["run", rest @ ..] => run_job(rest, ship::SlotClass::LandCheck),
        _ => usage(),
    }
}

fn closed() -> u8 {
    if warms_slots_here() {
        return 0;
    }
    println!("{}", crate::platform::host::name());
    75
}

fn slot(words: &[&str]) -> u8 {
    match words {
        ["free", "--host", host] => free(host),
        ["slots"] => slots(),
        ["slot", "record", name] => ship::slot_record::run(name),
        _ => usage(),
    }
}

fn hosts() -> u8 {
    let needs = readiness::Needs::here();
    let hosts = build_hosts();
    let ready: Vec<bool> = std::thread::scope(|scope| {
        let probes: Vec<_> = hosts.iter().map(|(name, ssh)| scope.spawn(|| readiness::admits(&needs, name, ssh))).collect();
        probes.into_iter().map(|probe| probe.join().unwrap_or(false)).collect()
    });
    for ((name, ssh), _) in hosts.iter().zip(ready).filter(|(_, ready)| *ready) {
        println!("{name} {ssh}");
    }
    0
}

fn run_job(words: &[&str], class: ship::SlotClass) -> u8 {
    match words {
        ["--host", host, "--base", base, "--head", head, "--timeout", seconds, "--", command] => {
            let Ok(timeout_seconds) = seconds.parse() else { return usage() };
            let resolved = std::env::current_dir().map_err(|error| format!("current directory: {error}")).and_then(|checkout| commits_in(&checkout, base, head).map(|commits| (checkout, commits)));
            match resolved {
                Ok((checkout, (base, head))) => offload(host, ship::Land { ssh: "", checkout: &checkout, base, head, timeout_seconds, command, class, confined: false }),
                Err(why) => {
                    eprintln!("north capacity offload: {why}");
                    2
                }
            }
        }
        ["--host", _, "--base", _, "--head", _, "--timeout", _, "--", _, _, ..] => {
            eprintln!("north capacity offload run: COMMAND after -- is one shell string, so quote it: -- 'cargo nextest run -p north'");
            2
        }
        _ => usage(),
    }
}

fn commits_in(checkout: &Path, base: &str, head: &str) -> Result<(Option<CommitId>, CommitId), String> {
    let base = if base.is_empty() { None } else { Some(CommitId::resolve(checkout, base)?) };
    Ok((base, CommitId::resolve(checkout, head)?))
}

fn usage() -> u8 {
    eprintln!("{USAGE}");
    2
}

fn machines() -> Vec<gate::Machine> {
    config().map(|cfg| gate::machines(&cfg)).unwrap_or_default()
}

pub fn build_hosts() -> Vec<(String, String)> {
    config().map(|cfg| destinations::build_hosts(&cfg, &crate::platform::host::name())).unwrap_or_default()
}

/// Each build host this host reaches, with the login a lane's check runs as there (check_login); a host whose agent login does not parse, or that sets agent_offload with no agent login, is left out rather than dialed as the operator.
pub fn check_hosts() -> Vec<(String, String)> {
    config().map(|cfg| destinations::check_hosts(&cfg, &crate::platform::host::name())).unwrap_or_default()
}

fn check_login(machine: &gate::Machine) -> Option<String> {
    config().ok().and_then(|cfg| destinations::check_login(&cfg, machine))
}

/// Whether other hosts send builds to this one: `[hosts.HERE] offload_builds`.
pub(crate) fn warms_slots_here() -> bool {
    config().map_or(true, |cfg| destinations::warms_slots(&cfg, &crate::platform::host::name()))
}

/// Whether the configuration names `host` as one that takes offloaded builds, whichever host asks.
pub fn offloads_builds_in(host: &str) -> bool {
    config().is_ok_and(|cfg| destinations::offloads_builds(&cfg, host))
}

pub fn builds_for_peers() -> bool {
    config().is_ok_and(|cfg| destinations::offloads_builds(&cfg, &crate::platform::host::name()))
}

pub use destinations::Deployment;

pub fn deployment() -> Deployment {
    config().map(|cfg| destinations::deploy(&cfg, &crate::platform::host::name())).unwrap_or_default()
}

pub fn other_destinations() -> Vec<String> {
    deployment().destinations
}

pub fn pull_sources() -> Vec<String> {
    config().map(|cfg| pull_sources_in(&cfg, &crate::platform::host::name())).unwrap_or_default()
}

pub fn pull_sources_in(cfg: &toml_edit::DocumentMut, here: &str) -> Vec<String> {
    destinations::pull_sources(cfg, here)
}

pub fn publishes_host_facts_here() -> bool {
    config().map(|cfg| destinations::publishes_host_facts(&cfg, &crate::platform::host::name())).unwrap_or(true)
}

pub fn builds_runtime_here() -> bool {
    config().map(|cfg| destinations::builds_runtime(&cfg, &crate::platform::host::name())).unwrap_or(true)
}

pub fn settles_fleet_here() -> bool {
    config().is_ok_and(|cfg| destinations::settles_fleet(&cfg, &crate::platform::host::name()))
}

pub fn accepts_peer_runtimes_here() -> bool {
    config().is_ok_and(|cfg| !destinations::settles_fleet(&cfg, &crate::platform::host::name()))
}

/// Whether a host other than this one settles the fleet, or why the configuration that would say so cannot be read (destinations::settled_elsewhere_read).
pub fn settled_elsewhere() -> Result<bool, String> {
    destinations::settled_elsewhere_read(config(), &crate::platform::host::name(), &crate::platform::orchestration::path(), crate::platform::orchestration::south_kept())
}

pub fn runs_pinned_runtime_here() -> bool {
    config().is_ok_and(|cfg| destinations::pinned_by_settler(&cfg, &crate::platform::host::name()))
}

pub fn in_game_reserve() -> bool {
    let local = agent_load::Host::from_env();
    agent_load::gaming_declared(&local)
}

pub fn checks_confined_here() -> bool {
    crate::capacity::mode_tick::confined_agent_cpus(&agent_load::Host::from_env()).is_some()
}

/// A build host this host reaches and the free cores its admission reported, none when it did not answer.
pub struct BuildHost {
    pub name: String,
    pub ssh: String,
    pub free_cpus: Option<f64>,
}

/// Each build host this host reaches, with the free cores its admission reports, read in parallel; a host that does not answer has none.
pub fn build_host_rooms() -> Vec<BuildHost> {
    let hosts = build_hosts();
    std::thread::scope(|scope| {
        let reads: Vec<_> = hosts.iter().map(|(_, ssh)| scope.spawn(|| room::read(ssh).map(|room| room.admission.free_cpus()))).collect();
        hosts.iter().zip(reads).map(|((name, ssh), read)| BuildHost { name: name.clone(), ssh: ssh.clone(), free_cpus: read.join().ok().flatten() }).collect()
    })
}

/// Prints each offload host's newest offloaded runs with their ship time and cache state, then its median ship time, read from the host's own records over ssh.
fn cost() -> u8 {
    let hosts = check_hosts();
    if hosts.is_empty() {
        eprintln!("north capacity offload: no build host is configured");
        return 1;
    }
    let remote = "\"${AGENT_CAPACITY_NORTH:-$HOME/.local/state/north/current/north}\" capacity host offload-cost";
    let mut code = 0;
    for (name, ssh) in hosts {
        println!("{name}");
        let read = std::process::Command::new(north_base::foundation::offload::ssh()).args(["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=5", &ssh, "--", remote]).status();
        if !read.is_ok_and(|status| status.success()) {
            eprintln!("north capacity offload: could not read {name}'s offload records");
            code = 1;
        }
    }
    code
}

fn reap(owner: &str) -> u8 {
    let mut code = 0;
    for (name, ssh) in check_hosts() {
        if ship::reap(&ssh, owner) != 0 {
            eprintln!("north capacity offload: could not reap {owner}'s runs on {name}");
            code = 1;
        }
    }
    code
}

/// What `choose --any` weighs besides each host's room: the land checks already on their way to a host, and the hosts a lane found unfit.
#[derive(Debug, Default, PartialEq, Eq)]
struct Placement {
    taken: Vec<String>,
    avoid: Vec<String>,
    needs: Vec<String>,
    prefer: Option<String>,
    count: Option<usize>,
}

impl Placement {
    fn parse(words: &[&str]) -> Option<Self> {
        let mut placement = Self::default();
        for pair in words.chunks(2) {
            match pair {
                ["--taken", host] if !host.is_empty() => placement.taken.push((*host).to_owned()),
                ["--avoid", host] if !host.is_empty() => placement.avoid.push((*host).to_owned()),
                ["--needs", input] if !input.is_empty() => placement.needs.push((*input).to_owned()),
                ["--prefer", host] if !host.is_empty() => placement.prefer = Some((*host).to_owned()),
                ["--count", count] => placement.count = Some(count.parse().ok().filter(|count| *count > 0)?),
                _ => return None,
            }
        }
        Some(placement)
    }

    fn admits(&self, host: &str) -> bool {
        !self.avoid.iter().any(|avoided| avoided == host)
    }
}

/// A reserved host or a laptop on battery takes no land check; one `cfg` names as offloading builds competes with the others for room; any other, such as the laptop in any capacity mode, takes one only when no build host answers, and queues it on the least busy build host while they are all full: on 2026-11-10 at 2:05 PM the away laptop counted as a build host and ran 6 land checks at load 40 while cpx62 sat at 8 (north#1036).
fn standing(reserved: bool, on_battery: bool, cfg: Option<&toml_edit::DocumentMut>, here: &str) -> room::Standing {
    if reserved || on_battery {
        room::Standing::Reserved
    } else if cfg.is_some_and(|cfg| destinations::offloads_builds(cfg, here)) {
        room::Standing::Peer
    } else {
        room::Standing::Fallback
    }
}

fn on_battery() -> bool {
    laptop_auto::settled_battery(&laptop_auto::power_supply(), &laptop_auto::discharging_since(&agent_load::Host::from_env()), mode::now()).is_some()
}

fn choose(any: bool, placement: &Placement) -> u8 {
    let in_reserve = in_game_reserve();
    if !any && !in_reserve {
        eprintln!("north capacity offload: this host is not in game-reserve, so the land check runs here");
        return 1;
    }
    if !any && checks_confined_here() {
        eprintln!("north capacity offload: this host keeps its game CPUs to game-reserve.slice and confines agent slices to the rest, so the land check runs here");
        return 1;
    }
    let Survey { here, reserved, holds_here, standing, others, remotes } = survey(in_reserve, placement);
    let claims = std::env::var_os(WORKER_REQUEST_ENV).is_none();
    if let Some(preferred) = placement.prefer.as_ref().filter(|preferred| if **preferred == here { holds_here } else { remotes.iter().any(|(name, room)| name == *preferred && room.is_some()) }) {
        println!("{preferred}");
        return 0;
    }
    if let Some(count) = placement.count {
        return spread(standing, &here, remotes, placement, count);
    }
    match room::check_host(standing, &here, room::read_local, remotes, &placement.taken, crate::capacity::host::land_check_cpus()) {
        Ok(room::Chosen::Room(name)) if name == here && standing == room::Standing::Fallback => {
            eprintln!("north capacity offload: no build host answers, so the land check runs here on {here}");
            0
        }
        Ok(room::Chosen::Room(name)) if name == here => {
            eprintln!("north capacity offload: {here} has the most room, so the land check runs here");
            0
        }
        Ok(room::Chosen::Room(name)) => {
            println!("{name}");
            0
        }
        Ok(room::Chosen::Queued(name)) => {
            if let Some(machine) = others.iter().find(|machine| claims && machine.name == name) {
                room::claim(Some(&machine.ssh));
            }
            eprintln!("north capacity offload: no build host has room, so the land check queues on {name}{}", if claims { ", which admits no new worker build until it starts" } else { "" });
            println!("{name}");
            0
        }
        Err(refused) => {
            let claimed = if claims { others.as_slice() } else { &[] };
            std::thread::scope(|scope| {
                for machine in claimed {
                    scope.spawn(|| room::claim(Some(&machine.ssh)));
                }
                if claims && !reserved {
                    scope.spawn(|| room::claim(None));
                }
            });
            eprintln!("north capacity offload: no host has room: {}", if refused.is_empty() { "none has an ssh destination".to_owned() } else { refused.join("; ") });
            1
        }
    }
}

struct Survey {
    here: String,
    reserved: bool,
    holds_here: bool,
    standing: room::Standing,
    others: Vec<gate::Machine>,
    remotes: Vec<(String, Option<room::Room>)>,
}

/// The hosts a land check may run on and what each reports, read over ssh at once.
fn survey(in_reserve: bool, placement: &Placement) -> Survey {
    let here = crate::platform::host::name();
    let needs = readiness::Needs::here().with_inputs(&placement.needs);
    let lacks = needs.lacks_here();
    if !lacks.is_empty() {
        eprintln!("north capacity offload: {here} lacks {}, which this land check reads, so it runs the check only when no host that holds it answers", lacks.join(" and "));
    }
    let reserved = in_reserve || checks_confined_here() || !placement.admits(&here) || !lacks.is_empty();
    let reached = |name: &str| config().is_ok_and(|cfg| crate::platform::orchestration::reaches(&cfg, &here, name));
    let standing = standing(reserved, on_battery(), config().ok().as_ref(), &here);
    let takes = |name: &str| config().is_ok_and(|cfg| destinations::takes_land_checks(&cfg, standing, name));
    let others: Vec<gate::Machine> = machines().into_iter().filter(|machine| machine.name != here && reached(&machine.name) && placement.admits(&machine.name) && takes(&machine.name)).collect();
    let remotes = std::thread::scope(|scope| {
        let reads: Vec<_> = others.iter().map(|machine| (machine.name.clone(), scope.spawn(|| room::read(&machine.ssh)), scope.spawn(|| check_login(machine).is_some_and(|login| readiness::admits(&needs, &machine.name, &login))))).collect();
        reads.into_iter().map(|(name, room, ready)| (name, room.join().ok().flatten().filter(|_| ready.join().unwrap_or(false)))).collect::<Vec<_>>()
    });
    Survey { here, reserved, holds_here: lacks.is_empty(), standing, others, remotes }
}

/// Prints the land checks the reachable build hosts take at once, free then total, from each host's free cores over the cores one check uses.
fn slots() -> u8 {
    let Survey { standing, remotes, .. } = survey(in_game_reserve(), &Placement::default());
    let local = (standing == room::Standing::Peer).then(room::read_local).flatten();
    let rooms: Vec<&room::Room> = remotes.iter().filter_map(|(_, room)| room.as_ref()).chain(local.as_ref()).collect();
    if rooms.is_empty() {
        eprintln!("north capacity offload: no build host answers, so its slots are unknown");
        return 1;
    }
    let (free, total) = room::check_slots(rooms, crate::capacity::host::land_check_cpus());
    println!("{free} {total}");
    0
}

fn spread(standing: room::Standing, here: &str, remotes: Vec<(String, Option<room::Room>)>, placement: &Placement, count: usize) -> u8 {
    let local = (standing != room::Standing::Reserved).then(room::read_local).flatten();
    let cores = crate::capacity::host::land_check_cpus();
    let mut taken = placement.taken.clone();
    for _ in 0..count {
        match room::check_host(standing, here, || local, remotes.clone(), &taken, cores) {
            Ok(room::Chosen::Room(name) | room::Chosen::Queued(name)) => {
                println!("{name}");
                taken.push(name);
            }
            Err(refused) => {
                eprintln!("north capacity offload: no host has room: {}", refused.join("; "));
                return 1;
            }
        }
    }
    0
}

/// The host an offload request names, when it is one the outside broker runs for a command whose domain reaches no other host (south#118): `hosts`, `slots` and `choose` bare, `slot record NAME`, which the offload's script asks from inside a worker's domain and whose project the broker's side names (ship/slot_record.rs), `choose --any` with the words `Placement::parse` reads apart from `--needs`, whose path would be read as the operator, each `choose` a read that claims no host (`WORKER_REQUEST_ENV`), `free --host NAME`, and `run` in exactly the words `run_job` reads with a base and head that are no option. Every other verb, word or option stays in the caller's domain, so a worker command picks no ssh option and no remote command beside the one the offload script runs under its lease.
pub(crate) fn worker_request<'a>(words: &[&'a str]) -> Option<Option<&'a str>> {
    let commit = |name: &str| !name.starts_with('-');
    match words {
        ["hosts"] | ["slots"] | ["choose"] | ["slot", "record", _] => Some(None),
        ["choose", "--any", rest @ ..] => Placement::parse(rest).filter(|placement| placement.needs.is_empty()).map(|_| None),
        ["free", "--host", host] => Some(Some(host)),
        ["run", "--background", rest @ ..] | ["run", rest @ ..] => match rest {
            ["--host", host, "--base", base, "--head", head, "--timeout", seconds, "--", _] if commit(base) && commit(head) && seconds.parse::<u64>().is_ok() => Some(Some(host)),
            _ => None,
        },
        _ => None,
    }
}

/// The ssh destination of `host` when this host reaches it over its `reaches` list, else why an offload to it is refused before any dial: the only kind a brokered offload request names. This host's own name runs in its own slots, which a worker command does inside its domain.
pub(crate) fn reached_destination(host: &str) -> Result<String, String> {
    destinations::reached_destination(&config()?, &crate::platform::host::name(), host)
}

pub(crate) fn reaches(host: &str) -> bool {
    reached_destination(host).is_ok()
}

fn destination(host: &str) -> Option<String> {
    if host == crate::platform::host::name() {
        return Some(String::new());
    }
    reached_destination(host).inspect_err(|why| eprintln!("north capacity offload: {why}")).ok()
}

fn free(host: &str) -> u8 {
    let Some(ssh) = destination(host) else { return 1 };
    match std::env::current_dir().map_err(|error| format!("current directory: {error}")).and_then(|directory| ship::free_slots(&ssh, &directory)) {
        Ok(free) => {
            println!("{free}");
            0
        }
        Err(why) => {
            eprintln!("north capacity offload: {why}");
            1
        }
    }
}

fn offload(host: &str, land: ship::Land) -> u8 {
    let Some(ssh) = destination(host) else { return ship::TRANSPORT_FAILED };
    let confined = config().is_ok_and(|cfg| destinations::runs_as_agent(&cfg, host, &ssh));
    ship::run(&ship::Land { ssh: &ssh, confined, ..land })
}

