use crate::capacity::admission::{Class, dispatch_request_mib, memory_short, offload_room};
use crate::capacity::build_offload::Admission;
use crate::dispatch::measure::{self, Measure};
use crate::platform::process::run_within;
use std::time::Duration;

use super::ship::{OFFLOAD_ROOT, OTHER_JOBS, SLOTS, host_job_cap, jobs_part, quote};

const LIMIT: Duration = Duration::from_secs(15);
const KIB_PER_GIB: f64 = 1024.0 * 1024.0;
const STATUS_MARK: &str = "@ADMISSION@";
const SLOTS_MARK: &str = "@SLOTS@";
const SLOTS_PROBE: &str = r#"root=$1
@JOBS@n=$(host_jobs); q=$(ls "$root/queue" 2>/dev/null | grep -c "^[012]-" || true); a=$(ls "$root/queue" 2>/dev/null | grep -c "^[01]-" || true); echo "$n $q $(nproc) $a""#;
const STATUS_PROBE: &str = r#"${AGENT_CAPACITY_NORTH:-$HOME/.local/state/north/current/north} capacity host status"#;
const CLAIM: &str = r#"${AGENT_CAPACITY_NORTH:-$HOME/.local/state/north/current/north} capacity host land-waiting"#;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Room {
    pub measure: Measure,
    pub admission: Admission,
    pub disk_free_gib: f64,
    pub slots: Option<Slots>,
}

/// The host's offload slot jobs: those running in any checkout's slot, the land checks and red fixes queued for one, its cores, and the red fixes and lander jobs queued, which a new lander job waits behind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slots {
    pub running: u32,
    pub queued: u32,
    pub cores: u32,
    pub ahead: u32,
}

impl Slots {
    fn parse(text: &str) -> Option<Self> {
        let mut words = text.split_whitespace().map(str::parse::<u32>);
        let (running, queued, cores, ahead) = (words.next()?.ok()?, words.next()?.ok()?, words.next()?.ok()?, words.next()?.ok()?);
        Some(Self { running, queued, cores, ahead })
    }

    /// The jobs running or queued, with `taken` more on their way, per job the host runs at once.
    fn backlog(&self, cap: u32, taken: usize) -> f64 {
        f64::from(self.running + self.queued + taken as u32) / f64::from(cap)
    }
}

impl Room {
    /// How oversubscribed the host is for a land check of `cores` with `taken` more on their way: its slot backlog or its committed share of its CPUs, whichever is higher.
    fn busy(&self, cores: f64, taken: usize) -> f64 {
        let limit = self.admission.limit_cpus.max(1.0);
        let committed = (limit - self.admission.free_cpus() + cores * taken as f64) / limit;
        self.slots.map_or(committed, |slots| slots.backlog(host_job_cap(slots.cores, cores.ceil() as u32, self.measure.memory_total_mib as u32), taken).max(committed))
    }
}

pub(super) fn parse(text: &str) -> Option<Room> {
    let (text, status) = text.split_once(STATUS_MARK)?;
    let (status, slots) = status.split_once(SLOTS_MARK).map_or((status, None), |(status, slots)| (status, Slots::parse(slots)));
    let admission = Admission::from_status(&serde_json::from_str(status.trim()).ok()?)?;
    let kib: f64 = text.lines().rev().find(|line| !line.trim().is_empty())?.split_whitespace().nth(3)?.parse().ok()?;
    Some(Room { measure: measure::parse(text)?, admission, disk_free_gib: kib / KIB_PER_GIB, slots })
}

pub(super) fn read(ssh: &str) -> Option<Room> {
    read_within(ssh, LIMIT)
}

pub fn read_local() -> Option<Room> {
    let output = run_within("sh", &["-c", &script()], LIMIT)?;
    output.status.success().then(|| parse(&String::from_utf8_lossy(&output.stdout))).flatten()
}

/// The cores, load, memory and disk free the host's own admission reads, the AGENT_CAPACITY_ readings when pinned, so the room it reports agrees with what its admission decides.
const MEASURE_PROBE: &str = r#"echo "${AGENT_CAPACITY_CORES:-$(nproc)}"; cat "${AGENT_CAPACITY_LOADAVG:-/proc/loadavg}" "${AGENT_CAPACITY_MEMINFO:-/proc/meminfo}""#;

fn script() -> String {
    let disk = format!(r#"if [ -n "${{AGENT_CAPACITY_DISK_FREE_GIB:-}}" ]; then awk -v gib="$AGENT_CAPACITY_DISK_FREE_GIB" 'BEGIN {{ printf "pinned 0 0 %d 0%% /\n", gib * 1048576 }}'; else d="{OFFLOAD_ROOT}"; while [ ! -d "$d" ]; do d=$(dirname "$d"); done; df -Pk "$d"; fi"#);
    format!("{MEASURE_PROBE}; {disk}; echo {STATUS_MARK}; {STATUS_PROBE}; echo {SLOTS_MARK}; sh -c {} offload-slots \"{OFFLOAD_ROOT}\"", quote(&SLOTS_PROBE.replace("@JOBS@", &jobs_part())))
}

pub fn read_within(ssh: &str, limit: Duration) -> Option<Room> {
    let command = format!("sh -c {}", quote(&script()));
    let output = run_within("ssh", &["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", ssh, "--", &command], limit)?;
    output.status.success().then(|| parse(&String::from_utf8_lossy(&output.stdout))).flatten()
}

/// Claims `ssh`, or this host when None, for a waiting land check, so it admits no new worker build until the claim lapses; a host on an older runtime ignores it.
pub(super) fn claim(ssh: Option<&str>) {
    let _ = match ssh {
        Some(ssh) => run_within("ssh", &["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", ssh, "--", &format!("sh -c {}", quote(CLAIM))], LIMIT),
        None => run_within("sh", &["-c", CLAIM], LIMIT),
    };
}

pub(super) fn shortfall(room: &Room) -> Option<String> {
    shortfall_reserving(room, 0.0)
}

/// The shortfall once `reserved` cores are counted as taken by builds already on their way to the host.
pub fn shortfall_reserving(room: &Room, reserved: f64) -> Option<String> {
    shortfall_needing(room, reserved, offload_room().0)
}

/// The shortfall for a worker build of `cores`, which also waits while a land check claims the host.
pub fn build_shortfall(room: &Room, reserved: f64, cores: f64) -> Option<String> {
    if room.admission.land_check_waiting {
        return Some(crate::capacity::land_claim::REASON.to_owned());
    }
    shortfall_needing(room, reserved, cores)
}

fn shortfall_needing(room: &Room, reserved: f64, cores: f64) -> Option<String> {
    let disk = offload_room().1;
    let memory = dispatch_request_mib(Class::Heavy);
    if let Some(short) = room.admission.shortfall(cores, reserved) {
        return Some(short);
    }
    if memory_short(room.measure.memory_total_mib, room.measure.memory_available_mib, memory) {
        return Some(format!("{:.0} MiB memory available, needs {memory:.0} and its headroom", room.measure.memory_available_mib));
    }
    (room.disk_free_gib < disk).then(|| format!("{:.1} GiB disk free, needs {disk}", room.disk_free_gib))
}

/// The land checks of `cores` the hosts take at once, free for the lander and total: a host's total is the job cap its slot script enforces (its cores over the cores a check is charged, its memory over a job's, at most `SLOTS`) within the checks its admission limit holds; its free checks are those its free CPUs hold, capped at the checks it keeps for the lander (its total less the slots other jobs may hold) and at the slots its running jobs and the red fixes and lander jobs queued ahead leave; a host its admission refuses for a check of `cores` counts none free.
pub(super) fn check_slots<'a>(rooms: impl IntoIterator<Item = &'a Room>, cores: f64) -> (usize, usize) {
    let cores = cores.max(1.0);
    let held = |cpus: f64| (cpus.max(0.0) / cores).floor() as usize;
    rooms.into_iter().fold((0, 0), |(free, total), room| {
        let host_cores = room.slots.map_or(room.measure.cores as u32, |slots| slots.cores);
        let all = held(room.admission.limit_cpus).min(host_job_cap(host_cores, cores.ceil() as u32, room.measure.memory_total_mib as u32).min(SLOTS) as usize);
        let lander = all.saturating_sub(OTHER_JOBS as usize);
        let open = room.slots.map_or(all, |slots| all.saturating_sub((slots.running + slots.ahead) as usize));
        (free + if shortfall_needing(room, 0.0, cores).is_none() { held(room.admission.free_cpus()).min(lander).min(open) } else { 0 }, total + all)
    })
}

pub(super) fn first_with_room(hosts: impl IntoIterator<Item = (String, Option<Room>)>) -> Result<String, Vec<String>> {
    let mut refused = Vec::new();
    for (name, room) in hosts {
        match room.as_ref().map(shortfall) {
            Some(None) => return Ok(name),
            Some(Some(why)) => refused.push(format!("{name}: {why}")),
            None => refused.push(format!("{name}: unreachable or unreadable over ssh")),
        }
    }
    Err(refused)
}

/// Whether this host takes land checks: never, only when no build host answers, or beside the build hosts as one of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Standing {
    Reserved,
    Fallback,
    Peer,
}

/// Where a land check runs: on a host with room, queued on a build host that has none, or nowhere.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Chosen {
    Room(String),
    Queued(String),
}

/// The host a land check runs on: the least busy for a land check of `cores` (its slot backlog per job it runs at once, or its committed share of its CPUs, whichever is higher) once each name in `taken` counts as one more land check already on its way there, among `remotes`; when no remote has room, the land check queues on the least busy readable remote, and runs on this host as `here` only when it is a `Peer` and every readable remote's queue holds more than a round of land checks, or when it is the `Fallback` and no remote answers; a tie goes to the earlier remote.
pub(super) fn check_host(standing: Standing, here: &str, local: impl FnOnce() -> Option<Room>, remotes: Vec<(String, Option<Room>)>, taken: &[String], cores: f64) -> Result<Chosen, Vec<String>> {
    let mut refused = Vec::new();
    let chosen = match standing {
        Standing::Reserved => most_room(remotes.iter().cloned(), taken, cores, &mut refused).map(Chosen::Room).or_else(|| least_busy(&remotes, taken, cores).map(Chosen::Queued)),
        Standing::Peer => most_room(remotes.iter().cloned(), taken, cores, &mut refused)
            .map(Chosen::Room)
            .or_else(|| (!behind_a_round(&remotes, taken, cores)).then(|| least_busy(&remotes, taken, cores).map(Chosen::Queued)).flatten())
            .or_else(|| most_room([(here.to_owned(), local())], taken, cores, &mut refused).map(Chosen::Room)),
        Standing::Fallback => most_room(remotes.iter().cloned(), taken, cores, &mut refused).map(Chosen::Room).or_else(|| least_busy(&remotes, taken, cores).map(Chosen::Queued)).or_else(|| Some(Chosen::Room(here.to_owned()))),
    };
    chosen.ok_or(refused)
}

fn least_busy(remotes: &[(String, Option<Room>)], taken: &[String], cores: f64) -> Option<String> {
    let on = |name: &str| taken.iter().filter(|host| *host == name).count();
    remotes.iter().filter_map(|(name, room)| room.as_ref().map(|room| (name, room.busy(cores, on(name))))).fold(None::<(&String, f64)>, |best, (name, busy)| if best.is_none_or(|(_, least)| busy < least) { Some((name, busy)) } else { best }).map(|(name, _)| name.clone())
}

/// Whether every remote's queue holds more than a round of land checks of `cores`, its job cap, once each name in `taken` counts as one more queued there: only then does a `Peer` run one itself; a remote whose slots are unread counts as not behind.
fn behind_a_round(remotes: &[(String, Option<Room>)], taken: &[String], cores: f64) -> bool {
    let on = |name: &str| taken.iter().filter(|host| *host == name).count() as u32;
    remotes.iter().any(|(_, room)| room.is_some())
        && remotes.iter().filter_map(|(name, room)| room.as_ref().map(|room| (name, room))).all(|(name, room)| room.slots.is_some_and(|slots| slots.queued + on(name) > host_job_cap(slots.cores, cores.ceil() as u32, room.measure.memory_total_mib as u32)))
}

fn most_room(hosts: impl IntoIterator<Item = (String, Option<Room>)>, taken: &[String], cores: f64, refused: &mut Vec<String>) -> Option<String> {
    let mut best: Option<(String, f64)> = None;
    for (name, room) in hosts {
        let on = taken.iter().filter(|host| **host == name).count();
        match room.as_ref().map(|room| (shortfall_needing(room, cores * on as f64, cores), room.busy(cores, on))) {
            Some((None, busy)) if best.as_ref().is_none_or(|(_, least)| busy < *least) => best = Some((name, busy)),
            Some((None, _)) => {}
            Some((Some(why), _)) => refused.push(format!("{name}: {why}")),
            None => refused.push(format!("{name}: unreachable or unreadable")),
        }
    }
    best.map(|(name, _)| name)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROBE: &str = "20\n3.10 4.00 5.00 2/900 1\nMemTotal:       65536000 kB\nMemAvailable:   40000000 kB\nFilesystem 1024-blocks Used Available Capacity Mounted on\n/dev/md1 490000000 400000000 %KIB% 90% /\n@ADMISSION@\n{\"profile\":\"unattended\",\"aggregateCpuLimit\":20,\"committedBatchCpus\":%COMMITTED%,\"cpuSomeAvg10\":%PRESSURE%}\n";

    fn reading(kib: u64, load: &str, committed: &str, pressure: &str) -> Room {
        parse(&PROBE.replace("%KIB%", &kib.to_string()).replace("3.10", load).replace("%COMMITTED%", committed).replace("%PRESSURE%", pressure)).unwrap()
    }

    #[test]
    fn a_host_reporting_24_free_cores_at_4_per_check_offers_the_lander_5_of_its_6_slots_and_two_such_hosts_10_of_12() {
        let gib = 1024 * 1024;
        let mut host = reading(80 * gib, "3.00", "0.0", "1");
        host.admission.limit_cpus = 24.0;
        host.measure.cores = 24.0;
        assert_eq!(check_slots([&host], 4.0), (5, 6), "one of its six checks is kept for lane pre-checks and worker builds");
        assert_eq!(check_slots([&host, &host], 4.0), (10, 12));
        let mut small = host;
        small.admission.limit_cpus = 8.0;
        assert_eq!(check_slots([&small], 4.0), (1, 2));
        let mut loaded = host;
        loaded.admission.committed_cpus = 10.0;
        assert_eq!(check_slots([&loaded, &host], 4.0), (8, 12), "a host with 14 of 24 cores free holds 3 more checks");
        let mut pressed = reading(80 * gib, "3.00", "0.0", "90");
        pressed.admission.limit_cpus = 24.0;
        pressed.measure.cores = 24.0;
        assert_eq!(check_slots([&pressed, &host], 4.0), (5, 12), "a host its admission refuses offers no free slot");
    }

    // KEEP(bug): at 10:20 AM on 2026-11-08 the lander read 33 free slots with land checks charged one core, and planned 33 probes of about 6 offload jobs each, while gex45 held its 6 slots with 35 jobs queued for them
    #[test]
    fn a_replay_of_the_1020_am_reading_offers_the_lander_only_the_slots_the_build_hosts_job_caps_and_queues_leave() {
        let gib = 1024 * 1024;
        let host = |cores: u32, memory_kib: u64, committed: &str, slots: &str| {
            let text = format!("{cores}\n{}@SLOTS@\n{slots}\n", PROBE[3..].replace("65536000", &memory_kib.to_string()).replace("40000000", &(memory_kib / 2).to_string()).replace("%KIB%", &(80 * gib).to_string()).replace("%COMMITTED%", committed).replace("%PRESSURE%", "1"));
            let mut room = parse(&text).unwrap();
            room.admission.limit_cpus = f64::from(cores);
            room
        };
        let gex45 = host(20, 65_536_000, "1.0", "6 35 20 35");
        let cpx62 = host(16, 32_505_856, "2.0", "3 0 16 0");
        let charge = 1.0;
        assert_eq!(check_slots([&gex45, &cpx62], charge), (0, 9), "gex45 caps its jobs at 6 and cpx62 at 3 by memory, every slot is held and 35 lander jobs wait for gex45's, where the free cores read 19 + 14 = 33");
        let idle = |room: Room| Room { slots: room.slots.map(|slots| Slots { running: 0, queued: 0, ahead: 0, ..slots }), ..room };
        let (free, _) = check_slots([&idle(gex45), &idle(cpx62)], charge);
        assert_eq!(free, 7, "idle, the hosts keep 5 and 2 slots for the lander, one each for other jobs");    }

    #[test]
    fn a_runtime_build_has_room_on_gex45_at_its_4_20_pm_reading_whose_load_average_is_the_ci_farm() {
        let gib = 1024 * 1024;
        let gex45 = reading(80 * gib, "61.00", "7.6", "35");
        assert_eq!(shortfall(&gex45), None, "committed 7.6 of 20 CPUs at 35% pressure and load 61 is room for 6 cores");
        assert!(shortfall(&reading(80 * gib, "3.00", "17.0", "10")).unwrap().contains("cores free of 20"));
        assert!(shortfall(&reading(80 * gib, "3.00", "7.6", "40")).unwrap().contains("CPU pressure 40%"));
    }

    // KEEP(bug): on 2026-11-07 gex45 held 18 queued CPUs behind the land-check claim, which left the lander no room and so renewed the claim every 20 s while the host sat idle
    #[test]
    fn jobs_the_land_check_claim_holds_back_leave_the_host_its_room_for_the_land_check_that_claimed_it() {
        let gib = 1024 * 1024;
        let held = "{\"owner\":\"heavy-command-guard\",\"class\":\"heavy\",\"requestedClass\":\"heavy\",\"requestedCpus\":6,\"reason\":\"DEFER_LAND_CHECK_WAITING\"}";
        let waiting = parse(&PROBE.replace("%KIB%", &(80 * gib).to_string()).replace("%COMMITTED%", "1.0").replace("%PRESSURE%", "0").replace("\"cpuSomeAvg10\":0", &format!("\"cpuSomeAvg10\":0,\"queued\":[{held},{held},{held}]"))).unwrap();
        assert_eq!(waiting.admission.queued_heavy_cpus, 0.0);
        assert_eq!(shortfall(&waiting), None);
    }

    #[test]
    fn a_host_with_heavy_jobs_or_slot_jobs_queued_loses_to_one_with_room_and_the_laptop_runs_a_land_check_only_when_no_build_host_answers() {
        let gib = 1024 * 1024;
        let queued = |jobs: &str| parse(&PROBE.replace("%KIB%", &(80 * gib).to_string()).replace("%COMMITTED%", "3.0").replace("%PRESSURE%", "5").replace("\"cpuSomeAvg10\":5", &format!("\"cpuSomeAvg10\":5,\"queued\":[{jobs}]"))).unwrap();
        let heavy = |cpus: u32| format!("{{\"owner\":\"gate\",\"class\":\"heavy\",\"requestedClass\":\"heavy\",\"requestedCpus\":{cpus}}}");
        let busy = queued(&format!("{},{}", heavy(6), heavy(6)));
        assert_eq!(busy.admission.queued_heavy_cpus, 12.0);
        let moderate = queued("{\"owner\":\"w\",\"class\":\"moderate\",\"requestedClass\":\"moderate\",\"requestedCpus\":4}");
        assert_eq!(moderate.admission.queued_heavy_cpus, 0.0);
        let mut idle = reading(80 * gib, "0.06", "0.0", "1");
        idle.admission.limit_cpus = 8.0;
        assert!(shortfall(&queued(&format!("{},{},{}", heavy(6), heavy(6), heavy(6)))).unwrap().contains("cores free of 20"));
        assert_eq!(check_host(Standing::Reserved, "laptop", || None, vec![("gex45".to_owned(), Some(busy)), ("ccx33".to_owned(), Some(idle))], &[], 6.0), Ok(Chosen::Room("ccx33".to_owned())));
        let mut quarter = reading(80 * gib, "2.00", "2.0", "1");
        quarter.admission.limit_cpus = 8.0;
        assert_eq!(check_host(Standing::Reserved, "laptop", || None, vec![("gex45".to_owned(), Some(moderate)), ("ccx33".to_owned(), Some(quarter))], &[], 6.0), Ok(Chosen::Room("gex45".to_owned())));
        let laptop = || Some(reading(80 * gib, "0.10", "0.0", "1"));
        let gex45 = reading(80 * gib, "3.10", "10.0", "5");
        let full = reading(80 * gib, "3.00", "17.0", "10");
        assert_eq!(check_host(Standing::Fallback, "laptop", laptop, vec![("gex45".to_owned(), Some(gex45))], &[], 6.0), Ok(Chosen::Room("gex45".to_owned())));
        assert_eq!(check_host(Standing::Peer, "laptop", laptop, vec![("gex45".to_owned(), Some(gex45))], &[], 6.0), Ok(Chosen::Room("gex45".to_owned())), "on 2026-11-08 at 8:02 AM the idler laptop out-roomed gex45 and took land checks at load 86 to 115; the desktop comes last while a build host has room");
        assert_eq!(check_host(Standing::Peer, "laptop", laptop, vec![("gex45".to_owned(), Some(full))], &[], 6.0), Ok(Chosen::Queued("gex45".to_owned())), "a full gex45 whose queue is unread is not a round behind, so the land check queues there");
        assert_eq!(check_host(Standing::Fallback, "laptop", laptop, vec![("gex45".to_owned(), Some(full))], &[], 6.0), Ok(Chosen::Queued("gex45".to_owned())));
        assert_eq!(check_host(Standing::Fallback, "laptop", laptop, vec![("gex45".to_owned(), None)], &[], 6.0), Ok(Chosen::Room("laptop".to_owned())));
        assert_eq!(check_host(Standing::Reserved, "laptop", laptop, vec![("gex45".to_owned(), Some(full))], &[], 6.0), Ok(Chosen::Queued("gex45".to_owned())), "a laptop off the land checks queues them on a full gex45, never runs them itself");
        assert!(check_host(Standing::Reserved, "laptop", laptop, vec![("gex45".to_owned(), None)], &[], 6.0).is_err());
        let slotted = |committed: &str, limit: f64, slots: &str| {
            let mut room = parse(&format!("{}@SLOTS@\n{slots}\n", PROBE.replace("%KIB%", &(80 * gib).to_string()).replace("%COMMITTED%", committed).replace("%PRESSURE%", "1"))).unwrap();
            room.admission.limit_cpus = limit;
            room
        };
        let ccx33 = slotted("1.0", 8.0, "2 9 8 0");
        let gex45 = slotted("8.0", 20.0, "3 0 20 0");
        assert_eq!(ccx33.slots, Some(Slots { running: 2, queued: 9, cores: 8, ahead: 0 }));
        assert_eq!(check_host(Standing::Reserved, "laptop", || None, vec![("ccx33".to_owned(), Some(ccx33)), ("gex45".to_owned(), Some(gex45))], &[], 4.0), Ok(Chosen::Room("gex45".to_owned())), "ccx33 runs 2 jobs at once with 9 queued; gex45 runs 5 with 2 free");
        let taken = ["gex45".to_owned(), "gex45".to_owned()];
        assert_eq!(check_host(Standing::Reserved, "laptop", || None, vec![("ccx33".to_owned(), Some(slotted("1.0", 8.0, "0 0 8 0"))), ("gex45".to_owned(), Some(gex45))], &taken, 4.0), Ok(Chosen::Room("ccx33".to_owned())), "an idle ccx33 takes the run once gex45's free slots are taken");
        let queued = |jobs: u32| slotted("17.0", 20.0, &format!("3 {jobs} 20 0"));
        assert_eq!(check_host(Standing::Peer, "laptop", laptop, vec![("gex45".to_owned(), Some(queued(3)))], &[], 6.0), Ok(Chosen::Queued("gex45".to_owned())), "on 2026-11-08 the laptop took land checks at load 73 whenever the build hosts were full; a queue of one round of gex45's 3 jobs waits there");
        assert_eq!(check_host(Standing::Peer, "laptop", laptop, vec![("gex45".to_owned(), Some(queued(4)))], &[], 6.0), Ok(Chosen::Room("laptop".to_owned())), "the laptop takes one once gex45 queues more than a round");
    }
}
