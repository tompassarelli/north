use crate::capacity::admission::{Class, dispatch_request_mib, memory_short, offload_room};
use crate::ops::dispatch::measure::{self, Measure};
use crate::platform::process::run_within;
use std::time::Duration;

use super::ship::{OFFLOAD_ROOT, quote};

const LIMIT: Duration = Duration::from_secs(15);
const KIB_PER_GIB: f64 = 1024.0 * 1024.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Room {
    pub(super) measure: Measure,
    pub(super) disk_free_gib: f64,
}

pub(super) fn parse(text: &str) -> Option<Room> {
    let kib: f64 = text.lines().rev().find(|line| !line.trim().is_empty())?.split_whitespace().nth(3)?.parse().ok()?;
    Some(Room { measure: measure::parse(text)?, disk_free_gib: kib / KIB_PER_GIB })
}

pub(super) fn read(ssh: &str) -> Option<Room> {
    let disk = format!("d=\"{OFFLOAD_ROOT}\"; while [ ! -d \"$d\" ]; do d=$(dirname \"$d\"); done; df -Pk \"$d\"");
    let probe = quote(&format!("{}; {disk}", measure::PROBE));
    let command = format!("sh -c {probe}");
    let output = run_within("ssh", &["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", ssh, "--", &command], LIMIT)?;
    output.status.success().then(|| parse(&String::from_utf8_lossy(&output.stdout))).flatten()
}

pub(super) fn shortfall(room: &Room) -> Option<String> {
    let (cores, disk) = offload_room();
    let free_cores = room.measure.cores - room.measure.load;
    let memory = dispatch_request_mib(Class::Heavy);
    if free_cores < cores {
        return Some(format!("{free_cores:.1} cores free of {}, needs {cores}", room.measure.cores));
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

#[cfg(test)]
mod tests {
    use super::*;

    const PROBE: &str = "20\n3.10 4.00 5.00 2/900 1\nMemTotal:       65536000 kB\nMemAvailable:   40000000 kB\nFilesystem 1024-blocks Used Available Capacity Mounted on\n/dev/md1 490000000 400000000 %KIB% 90% /\n";

    fn room(kib: u64, load: &str) -> Room {
        parse(&PROBE.replace("%KIB%", &kib.to_string()).replace("3.10", load)).unwrap()
    }

    #[test]
    fn a_host_needs_cores_memory_and_twenty_gib_of_disk() {
        let gib = 1024 * 1024;
        assert_eq!(shortfall(&room(21 * gib, "3.10")), None);
        assert!(shortfall(&room(17 * gib, "3.10")).unwrap().contains("17.0 GiB disk free"));
        assert!(shortfall(&room(80 * gib, "18.00")).unwrap().contains("cores free"));
        let mut tight = room(80 * gib, "3.10");
        tight.measure.memory_available_mib = 9000.0;
        assert!(shortfall(&tight).unwrap().contains("MiB memory available"));
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
}
