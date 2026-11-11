//! A lock in north's state directory held by a process that is not north: a stop someone set by hand through a unit or a
//! shell, such as a records-mirror copy held off by `flock records-mirror.lock sleep infinity`, which no record names.
use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use super::{Hold, Lift};

const OWN_EXECUTABLES: [&str; 2] = ["north", "verify-north"];
const STABLE_HOLD_UNIT: &str = "north-stable-hold.service";
const USER_HZ: i64 = 100;

pub(super) struct Holder {
    pub pid: u32,
    pub unit: Option<String>,
    pub command: String,
    pub started: Option<i64>,
}

fn device(dev: u64) -> (u64, u64) {
    (((dev >> 8) & 0xfff) | ((dev >> 32) & !0xfff), (dev & 0xff) | ((dev >> 12) & !0xff))
}

fn lock_files(state: &Path) -> BTreeMap<(u64, u64, u64), String> {
    fs::read_dir(state)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".lock"))
        .filter_map(|entry| {
            let metadata = entry.metadata().ok()?;
            let (major, minor) = device(metadata.dev());
            Some(((major, minor, metadata.ino()), entry.file_name().to_string_lossy().into_owned()))
        })
        .collect()
}

fn locked(line: &str) -> Option<(u32, (u64, u64, u64))> {
    let fields: Vec<&str> = line.split_whitespace().skip_while(|field| !field.ends_with(':')).skip(1).collect();
    let [kind, _, _, pid, file, ..] = fields.as_slice() else { return None };
    if *kind != "FLOCK" {
        return None;
    }
    let mut parts = file.split(':');
    let major = u64::from_str_radix(parts.next()?, 16).ok()?;
    let minor = u64::from_str_radix(parts.next()?, 16).ok()?;
    Some((pid.parse().ok()?, (major, minor, parts.next()?.parse().ok()?)))
}

fn unit(proc_root: &Path, pid: u32) -> Option<String> {
    let cgroup = fs::read_to_string(proc_root.join(format!("{pid}/cgroup"))).ok()?;
    cgroup.lines().flat_map(|line| line.rsplit('/')).find(|part| part.ends_with(".service") || part.ends_with(".scope")).map(str::to_owned)
}

fn started(proc_root: &Path, pid: u32) -> Option<i64> {
    let stat = fs::read_to_string(proc_root.join(format!("{pid}/stat"))).ok()?;
    let ticks: i64 = stat.rsplit_once(')')?.1.split_whitespace().nth(19)?.parse().ok()?;
    let boot: i64 = fs::read_to_string(proc_root.join("stat")).ok()?.lines().find_map(|line| line.strip_prefix("btime ")?.trim().parse().ok())?;
    Some(boot + ticks / USER_HZ)
}

fn holder(proc_root: &Path, pid: u32) -> Option<Holder> {
    let words: Vec<String> = fs::read(proc_root.join(format!("{pid}/cmdline"))).ok()?.split(|byte| *byte == 0).filter(|word| !word.is_empty()).map(|word| String::from_utf8_lossy(word).into_owned()).collect();
    let executable = fs::read_link(proc_root.join(format!("{pid}/exe"))).ok();
    let own = |path: &Path| path.file_name().is_some_and(|name| OWN_EXECUTABLES.iter().any(|own| name == *own));
    if executable.as_deref().is_some_and(own) || words.first().is_some_and(|first| own(Path::new(first))) {
        return None;
    }
    let command = words.join(" ");
    let unit = unit(proc_root, pid);
    if unit.as_deref() == Some(STABLE_HOLD_UNIT) {
        return None;
    }
    Some(Holder { pid, unit, command, started: started(proc_root, pid) })
}

/// Each lock in `state` a process other than north holds, by what `proc_root`'s `locks` table says, with its holder.
pub(super) fn held(state: &Path, proc_root: &Path) -> Vec<(String, Holder)> {
    let files = lock_files(state);
    let table = fs::read_to_string(proc_root.join("locks")).unwrap_or_default();
    let mut seen = BTreeMap::new();
    for (pid, file) in table.lines().filter_map(locked) {
        if let Some(name) = files.get(&file) {
            seen.entry(name.clone()).or_insert(pid);
        }
    }
    seen.into_iter().filter_map(|(name, pid)| Some((name, holder(proc_root, pid)?))).collect()
}

pub(super) fn holds(state: &Path, proc_root: &Path, describe: impl Fn(&str) -> Option<String>) -> Vec<Hold> {
    held(state, proc_root)
        .into_iter()
        .map(|(name, holder)| {
            let who = holder.unit.clone().unwrap_or_else(|| format!("pid {}", holder.pid));
            let why = holder.unit.as_deref().and_then(&describe).filter(|text| !text.is_empty()).unwrap_or_else(|| holder.command.clone());
            let lifts = match &holder.unit {
                Some(unit) if unit.ends_with(".service") || unit.ends_with(".scope") => format!("nothing recorded: it stands until `systemctl --user stop {unit}`"),
                _ => format!("nothing recorded: it stands until pid {} exits", holder.pid),
            };
            Hold { kind: format!("lock {name} held by hand"), set_by: who, why, since: holder.started, lifts: Lift::Unnamed(lifts), lifts_when: String::new(), host: String::new(), red: None }
        })
        .collect()
}

#[cfg(test)]
#[path = "locks_tests.rs"]
mod tests;
