use crate::capacity::admission::{Class, dispatch_request_mib, memory_short, offload_room};
use crate::capacity::build_offload::Admission;
use crate::ops::dispatch::measure::{self, Measure};
use crate::platform::process::run_within;
use std::time::Duration;

use super::ship::{OFFLOAD_ROOT, quote};

const LIMIT: Duration = Duration::from_secs(15);
const KIB_PER_GIB: f64 = 1024.0 * 1024.0;
const STATUS_MARK: &str = "@ADMISSION@";
const STATUS_PROBE: &str = r#"${AGENT_CAPACITY_NORTH:-$HOME/.local/state/north/current/north} capacity host status"#;

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

pub(super) fn shortfall(room: &Room) -> Option<String> {
    shortfall_reserving(room, 0.0)
}

/// The shortfall once `reserved` cores are counted as taken by builds already on their way to the host.
pub fn shortfall_reserving(room: &Room, reserved: f64) -> Option<String> {
    shortfall_needing(room, reserved, offload_room().0)
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
        match room.as_ref().map(|room| (shortfall_needing(room, pending, cores), room.admission.limit_cpus - room.admission.committed_cpus - pending)) {
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

    fn room(kib: u64, committed: &str) -> Room {
        reading(kib, "3.10", committed, "5")
    }

    #[test]
    fn a_host_needs_cores_memory_and_twenty_gib_of_disk() {
        let gib = 1024 * 1024;
        assert_eq!(shortfall(&room(21 * gib, "3.1")), None);
        assert!(shortfall(&room(17 * gib, "3.1")).unwrap().contains("17.0 GiB disk free"));
        assert!(shortfall(&room(80 * gib, "16.00")).unwrap().contains("cores free"));
        let mut tight = room(80 * gib, "3.1");
        tight.measure.memory_available_mib = 9000.0;
        assert!(shortfall(&tight).unwrap().contains("MiB memory available"));
    }

    #[test]
    fn a_runtime_build_has_room_on_gex45_at_its_4_20_pm_reading_whose_load_average_is_the_ci_farm() {
        let gib = 1024 * 1024;
        let gex45 = reading(80 * gib, "61.00", "7.6", "35");
        assert_eq!(shortfall(&gex45), None, "committed 7.6 of 20 CPUs at 35% pressure and load 61 is room for 6 cores");
        assert!(shortfall(&reading(80 * gib, "3.00", "15.0", "10")).unwrap().contains("cores free of 20"));
        assert!(shortfall(&reading(80 * gib, "3.00", "7.6", "40")).unwrap().contains("CPU pressure 40%"));
    }

    #[test]
    fn the_first_host_with_room_wins_and_each_refusal_is_named() {
        let gib = 1024 * 1024;
        let hosts = vec![("full".to_owned(), Some(room(5 * gib, "1.00"))), ("dark".to_owned(), None), ("open".to_owned(), Some(room(30 * gib, "1.00")))];
        assert_eq!(first_with_room(hosts.clone()), Ok("open".to_owned()));
        let refused = first_with_room(hosts[..2].to_vec()).unwrap_err();
        assert_eq!(refused.len(), 2, "{refused:?}");
        assert!(refused[1].starts_with("dark: unreachable"), "{refused:?}");
    }

    #[test]
    fn the_laptop_is_a_check_host_when_it_has_room_and_the_remote_has_none_and_never_while_reserved() {
        let gib = 1024 * 1024;
        let busy_remote = vec![("gex45".to_owned(), Some(room(80 * gib, "19.00")))];
        let open_laptop = || Some(room(80 * gib, "1.00"));
        assert_eq!(check_host(false, "laptop", open_laptop, busy_remote.clone(), &[], 6.0), Ok("laptop".to_owned()));
        let refused = check_host(true, "laptop", || panic!("a reserved laptop is not read"), busy_remote, &[], 6.0).unwrap_err();
        assert_eq!(refused.len(), 1, "{refused:?}");
        assert!(refused[0].starts_with("gex45: "), "{refused:?}");
    }

    #[test]
    fn the_host_with_the_most_free_cores_wins_and_a_tie_keeps_the_remote() {
        let gib = 1024 * 1024;
        let remote = |committed| vec![("gex45".to_owned(), Some(room(80 * gib, committed)))];
        assert_eq!(check_host(false, "laptop", || Some(room(80 * gib, "1.00")), remote("7.60"), &[], 6.0), Ok("laptop".to_owned()));
        assert_eq!(check_host(false, "laptop", || Some(room(80 * gib, "7.60")), remote("1.00"), &[], 6.0), Ok("gex45".to_owned()));
        assert_eq!(check_host(false, "laptop", || Some(room(80 * gib, "1.00")), remote("1.00"), &[], 6.0), Ok("gex45".to_owned()));
        assert_eq!(check_host(true, "laptop", || None, remote("7.60"), &[], 6.0), Ok("gex45".to_owned()));
        assert_eq!(check_host(false, "laptop", || None, remote("7.60"), &[], 6.0), Ok("gex45".to_owned()));
    }

    #[test]
    fn a_measured_four_core_land_check_fits_a_host_with_five_cores_free_that_a_six_core_one_does_not() {
        let gib = 1024 * 1024;
        let ccx33 = || vec![("ccx33".to_owned(), Some(room(80 * gib, "15.00")))];
        assert!(check_host(true, "laptop", || None, ccx33(), &[], 6.0).is_err(), "the 12:47 PM reading: 5.3 cores free, needs 6");
        assert_eq!(check_host(true, "laptop", || None, ccx33(), &[], 4.0), Ok("ccx33".to_owned()));
    }

    #[test]
    fn a_check_already_on_its_way_to_a_host_counts_against_its_free_cores() {
        let gib = 1024 * 1024;
        let remote = vec![("gex45".to_owned(), Some(room(80 * gib, "1.00")))];
        let laptop = || Some(room(80 * gib, "1.00"));
        assert_eq!(check_host(false, "laptop", laptop, remote.clone(), &["gex45".to_owned()], 6.0), Ok("laptop".to_owned()));
        let both = ["gex45".to_owned(), "laptop".to_owned(), "gex45".to_owned(), "laptop".to_owned(), "laptop".to_owned()];
        assert_eq!(check_host(false, "laptop", laptop, remote.clone(), &both[..4], 6.0), Ok("gex45".to_owned()));
        let refused = check_host(false, "laptop", laptop, remote, &[both.to_vec(), both.to_vec()].concat(), 6.0).unwrap_err();
        assert_eq!(refused.len(), 2, "{refused:?}");
    }
}
