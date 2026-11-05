use crate::capacity::admission::{Class, dispatch_request_mib, memory_short, offload_room};
use crate::capacity::build_offload::Admission;
use crate::dispatch::measure::{self, Measure};
use crate::platform::process::run_within;
use std::time::Duration;

use super::ship::{OFFLOAD_ROOT, quote};

const LIMIT: Duration = Duration::from_secs(15);
const KIB_PER_GIB: f64 = 1024.0 * 1024.0;
const STATUS_MARK: &str = "@ADMISSION@";
const STATUS_PROBE: &str = r#"${AGENT_CAPACITY_NORTH:-$HOME/.local/state/north/current/north} capacity host status"#;
const CLAIM: &str = r#"${AGENT_CAPACITY_NORTH:-$HOME/.local/state/north/current/north} capacity host land-waiting"#;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Room {
    pub measure: Measure,
    pub admission: Admission,
    pub disk_free_gib: f64,
}

pub(super) fn parse(text: &str) -> Option<Room> {
    let (text, status) = text.split_once(STATUS_MARK)?;
    let admission = Admission::from_status(&serde_json::from_str(status.trim()).ok()?)?;
    let kib: f64 = text.lines().rev().find(|line| !line.trim().is_empty())?.split_whitespace().nth(3)?.parse().ok()?;
    Some(Room { measure: measure::parse(text)?, admission, disk_free_gib: kib / KIB_PER_GIB })
}

pub(super) fn read(ssh: &str) -> Option<Room> {
    read_within(ssh, LIMIT)
}

pub(super) fn read_local() -> Option<Room> {
    let output = run_within("sh", &["-c", &script()], LIMIT)?;
    output.status.success().then(|| parse(&String::from_utf8_lossy(&output.stdout))).flatten()
}

fn script() -> String {
    let disk = format!("d=\"{OFFLOAD_ROOT}\"; while [ ! -d \"$d\" ]; do d=$(dirname \"$d\"); done; df -Pk \"$d\"");
    format!("{}; {disk}; echo {STATUS_MARK}; {STATUS_PROBE}", measure::PROBE)
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

/// The shortfall for a worker build, which also waits while a land check claims the host.
pub fn build_shortfall_reserving(room: &Room, reserved: f64) -> Option<String> {
    if room.admission.land_check_waiting {
        return Some(crate::capacity::land_claim::REASON.to_owned());
    }
    shortfall_reserving(room, reserved)
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

/// The host a land check runs on: among `remotes` and, unless `reserved`, this host as `here`, the one with the most free cores for a land check of `cores` once each name in `taken` counts as one more land check already on its way there; a tie goes to the earlier remote.
pub(super) fn check_host(reserved: bool, here: &str, local: impl FnOnce() -> Option<Room>, remotes: Vec<(String, Option<Room>)>, taken: &[String], cores: f64) -> Result<String, Vec<String>> {
    let hosts: Vec<(String, Option<Room>)> = remotes.into_iter().chain((!reserved).then(|| (here.to_owned(), local()))).collect();
    let mut refused = Vec::new();
    let mut best: Option<(String, f64)> = None;
    for (name, room) in hosts {
        let pending = cores * taken.iter().filter(|host| **host == name).count() as f64;
        match room.as_ref().map(|room| (shortfall_needing(room, pending, cores), room.admission.free_cpus() - pending)) {
            Some((None, free)) if best.as_ref().is_none_or(|(_, most)| free > *most) => best = Some((name, free)),
            Some((None, _)) => {}
            Some((Some(why), _)) => refused.push(format!("{name}: {why}")),
            None => refused.push(format!("{name}: unreachable or unreadable")),
        }
    }
    best.map(|(name, _)| name).ok_or(refused)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROBE: &str = "20\n3.10 4.00 5.00 2/900 1\nMemTotal:       65536000 kB\nMemAvailable:   40000000 kB\nFilesystem 1024-blocks Used Available Capacity Mounted on\n/dev/md1 490000000 400000000 %KIB% 90% /\n@ADMISSION@\n{\"profile\":\"unattended\",\"aggregateCpuLimit\":20,\"committedBatchCpus\":%COMMITTED%,\"cpuSomeAvg10\":%PRESSURE%}\n";

    fn reading(kib: u64, load: &str, committed: &str, pressure: &str) -> Room {
        parse(&PROBE.replace("%KIB%", &kib.to_string()).replace("3.10", load).replace("%COMMITTED%", committed).replace("%PRESSURE%", pressure)).unwrap()
    }

    #[test]
    fn a_runtime_build_has_room_on_gex45_at_its_4_20_pm_reading_whose_load_average_is_the_ci_farm() {
        let gib = 1024 * 1024;
        let gex45 = reading(80 * gib, "61.00", "7.6", "35");
        assert_eq!(shortfall(&gex45), None, "committed 7.6 of 20 CPUs at 35% pressure and load 61 is room for 6 cores");
        assert!(shortfall(&reading(80 * gib, "3.00", "17.0", "10")).unwrap().contains("cores free of 20"));
        assert!(shortfall(&reading(80 * gib, "3.00", "7.6", "40")).unwrap().contains("CPU pressure 40%"));
    }

    #[test]
    fn a_host_a_land_check_claims_refuses_worker_builds_and_still_takes_the_land_check() {
        let gib = 1024 * 1024;
        let claimed = parse(&PROBE.replace("%KIB%", &(80 * gib).to_string()).replace("%COMMITTED%", "2.0").replace("%PRESSURE%", "5").replace("\"cpuSomeAvg10\":5", "\"cpuSomeAvg10\":5,\"landCheckWaiting\":true")).unwrap();
        assert_eq!(build_shortfall_reserving(&claimed, 0.0).as_deref(), Some("land check waiting"));
        assert_eq!(check_host(true, "laptop", || None, vec![("gex45".to_owned(), Some(claimed))], &[], 6.0), Ok("gex45".to_owned()));
        assert_eq!(build_shortfall_reserving(&reading(80 * gib, "3.10", "2.0", "5"), 0.0), None);
    }

    #[test]
    fn a_host_with_heavy_jobs_queued_in_its_admission_loses_to_an_idle_host_with_fewer_free_cores() {
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
        assert_eq!(check_host(true, "laptop", || None, vec![("gex45".to_owned(), Some(busy)), ("ccx33".to_owned(), Some(idle))], &[], 6.0), Ok("ccx33".to_owned()));
        assert_eq!(check_host(true, "laptop", || None, vec![("gex45".to_owned(), Some(moderate)), ("ccx33".to_owned(), Some(idle))], &[], 6.0), Ok("gex45".to_owned()));
    }
}
