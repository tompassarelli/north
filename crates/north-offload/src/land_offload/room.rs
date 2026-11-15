use north_capacity_host::capacity::admission::{Class, dispatch_request_mib, memory_short, offload_room};
use north_capacity_host::capacity::build_offload::Admission;
use north_capacity_host::capacity::job_admission;
use north_dispatch::dispatch::measure::{self, Measure};
use north_base::platform::process::run_within;
use std::time::Duration;

use super::ship::{OFFLOAD_ROOT, SLOTS, jobs_part, quote};

const LIMIT: Duration = Duration::from_secs(15);
const KIB_PER_GIB: f64 = 1024.0 * 1024.0;
const STATUS_MARK: &str = "@ADMISSION@";
const SLOTS_MARK: &str = "@SLOTS@";
const SLOTS_PROBE: &str = r#"root=$1
@JOBS@s=@LINE_SLOTS@; [ -z "${NORTH_OFFLOAD_MAX_JOBS:-}" ] || [ "$NORTH_OFFLOAD_MAX_JOBS" -ge "$s" ] || s=$NORTH_OFFLOAD_MAX_JOBS; n=$(host_jobs); q=$(ls "$root/queue" 2>/dev/null | grep -c "^[012]-" || true); a=$(ls "$root/queue" 2>/dev/null | grep -c "^[01]-" || true); w=$(${AGENT_CAPACITY_NORTH:-$HOME/.local/state/north/current/north} capacity offload line wait --rank 2 --slots "$s" 2>/dev/null </dev/null | sed -n 's/^expected wait \([0-9]*\) s.*/\1/p'); echo "$n $q $(nproc) $a ${w:--}""#;
const STATUS_PROBE: &str = r#"${AGENT_CAPACITY_NORTH:-$HOME/.local/state/north/current/north} capacity host status"#;
const CLAIM: &str = r#"${AGENT_CAPACITY_NORTH:-$HOME/.local/state/north/current/north} capacity host land-waiting"#;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Room {
    pub measure: Measure,
    pub admission: Admission,
    pub disk_free_gib: f64,
    pub slots: Option<Slots>,
}

/// The host's offload slot jobs: those running in any checkout's slot, the land checks and red fixes queued for one, its cores, and the red fixes and lander jobs queued, which a new lander job waits behind, and the seconds the host's slot line expects a landing-rank job queued now to wait, when the host's north answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slots {
    pub running: u32,
    pub queued: u32,
    pub cores: u32,
    pub ahead: u32,
    pub wait: Option<u64>,
}

impl Slots {
    fn parse(text: &str) -> Option<Self> {
        let mut words = text.split_whitespace();
        let mut count = || words.next()?.parse::<u32>().ok();
        let (running, queued, cores, ahead) = (count()?, count()?, count()?, count()?);
        Some(Self { running, queued, cores, ahead, wait: words.next().and_then(|word| word.parse().ok()) })
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
        self.slots.map_or(committed, |slots| slots.backlog(self.jobs_at_once(cores), taken).max(committed))
    }

    fn jobs_at_once(&self, cores: f64) -> u32 {
        checks_at_once(self.admission.limit_cpus, cores)
    }
}

/// The land checks of `cores` a host whose admission holds `limit_cpus` runs at once: at least one and at most the `SLOTS` of one checkout.
pub fn checks_at_once(limit_cpus: f64, cores: f64) -> u32 {
    ((limit_cpus.max(0.0) / cores.max(1.0)).floor() as u32).clamp(1, SLOTS)
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
    format!("{MEASURE_PROBE}; {disk}; echo {STATUS_MARK}; {STATUS_PROBE}; echo {SLOTS_MARK}; sh -c {} offload-slots \"{OFFLOAD_ROOT}\"", quote(&SLOTS_PROBE.replace("@JOBS@", &jobs_part()).replace("@LINE_SLOTS@", &SLOTS.to_string())))
}

pub fn read_within(ssh: &str, limit: Duration) -> Option<Room> {
    let command = format!("sh -c {}", quote(&script()));
    let output = run_within(&north_base::foundation::offload::ssh(), &["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", ssh, "--", &command], limit)?;
    output.status.success().then(|| parse(&String::from_utf8_lossy(&output.stdout))).flatten()
}

/// Claims `ssh`, or this host when None, for a waiting land check, so it admits no new worker build until the claim lapses; a host on an older runtime ignores it.
pub(super) fn claim(ssh: Option<&str>) {
    let _ = match ssh {
        Some(ssh) => run_within(&north_base::foundation::offload::ssh(), &["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", ssh, "--", &format!("sh -c {}", quote(CLAIM))], LIMIT),
        None => run_within("sh", &["-c", CLAIM], LIMIT),
    };
}

/// The shortfall once `reserved` cores are counted as taken by builds already on their way to the host.
pub fn shortfall_reserving(room: &Room, reserved: f64) -> Option<String> {
    shortfall_needing(room, reserved, offload_room().0)
}

/// The shortfall for a worker build, which also waits while a land check claims the host: the host's own job admission answers, by its whole host's CPU pressure and the memory one job peaks at there, so the build waits only while the host is over that target and its committed cores no longer gate it.
pub fn build_shortfall(room: &Room) -> Option<String> {
    if room.admission.land_check_waiting {
        return Some(north_capacity_host::capacity::land_claim::REASON.to_owned());
    }
    let load = job_admission::HostLoad { cpu_some_percent: room.admission.host_cpu_pressure_percent, memory_total_mib: room.measure.memory_total_mib, memory_available_mib: room.measure.memory_available_mib };
    job_admission::refusal(&load, room.admission.job_peak_mib).or_else(|| disk_shortfall(room))
}

fn shortfall_needing(room: &Room, reserved: f64, cores: f64) -> Option<String> {
    let memory = dispatch_request_mib(Class::Heavy);
    if let Some(short) = room.admission.shortfall(cores, reserved) {
        return Some(short);
    }
    if memory_short(room.measure.memory_total_mib, room.measure.memory_available_mib, memory) {
        return Some(format!("{:.0} MiB memory available, needs {memory:.0} and its headroom", room.measure.memory_available_mib));
    }
    disk_shortfall(room)
}

fn disk_shortfall(room: &Room) -> Option<String> {
    let disk = offload_room().1;
    (room.disk_free_gib < disk).then(|| format!("{:.1} GiB disk free, needs {disk}", room.disk_free_gib))
}

/// The land checks of `cores` the hosts take at once, free for the lander and total: a host's total is the checks it runs at once (`Room::jobs_at_once`); its free checks are those its free CPUs hold within the slots its running jobs and the red fixes and lander jobs queued ahead leave, since the lander's jobs never wait on the pressure other jobs wait on; a host its admission refuses for a check of `cores` counts none free.
pub(super) fn check_slots<'a>(rooms: impl IntoIterator<Item = &'a Room>, cores: f64) -> (usize, usize) {
    let cores = cores.max(1.0);
    let held = |cpus: f64| (cpus.max(0.0) / cores).floor() as usize;
    rooms.into_iter().fold((0, 0), |(free, total), room| {
        let all = room.jobs_at_once(cores) as usize;
        let open = room.slots.map_or(all, |slots| all.saturating_sub((slots.running + slots.ahead) as usize));
        (free + if shortfall_needing(room, 0.0, cores).is_none() { held(room.admission.free_cpus()).min(open) } else { 0 }, total + all)
    })
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

/// The readable remote a land check queues on: the one whose slot line expects the shortest wait when every readable remote reports one, each name in `taken` adding a job ahead that waits as long as the line gives one queued job, else the one with the lowest backlog; a tie goes to the earlier remote.
fn least_busy(remotes: &[(String, Option<Room>)], taken: &[String], cores: f64) -> Option<String> {
    let on = |name: &str| taken.iter().filter(|host| *host == name).count();
    let readable: Vec<(&String, Option<u64>, f64)> = remotes.iter().filter_map(|(name, room)| room.as_ref().map(|room| (name, room.slots.and_then(|slots| slots.wait.map(|wait| wait + on(name) as u64 * wait / (u64::from(slots.queued) + 1))), room.busy(cores, on(name))))).collect();
    let by_wait = readable.iter().all(|(_, wait, _)| wait.is_some());
    readable.into_iter().min_by(|(_, first_wait, first), (_, second_wait, second)| if by_wait { first_wait.cmp(second_wait).then(first.total_cmp(second)) } else { first.total_cmp(second) }).map(|(name, _, _)| name.clone())
}

/// Whether every remote's queue holds more than a round of land checks of `cores`, its job cap, once each name in `taken` counts as one more queued there: only then does a `Peer` run one itself; a remote whose slots are unread counts as not behind.
fn behind_a_round(remotes: &[(String, Option<Room>)], taken: &[String], cores: f64) -> bool {
    let on = |name: &str| taken.iter().filter(|host| *host == name).count() as u32;
    remotes.iter().any(|(_, room)| room.is_some())
        && remotes.iter().filter_map(|(name, room)| room.as_ref().map(|room| (name, room))).all(|(name, room)| room.slots.is_some_and(|slots| slots.queued + on(name) > room.jobs_at_once(cores)))
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
