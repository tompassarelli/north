use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use rustix::process::{Pid, Signal, kill_process};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Process {
    pub pid: u32,
    start: u64,
}

struct Stat {
    parent: u32,
    start: u64,
    zombie: bool,
}

impl Process {
    pub fn of(pid: u32) -> Option<Self> {
        let stat = stat(pid)?;
        (!stat.zombie).then_some(Self { pid, start: stat.start })
    }

    pub fn alive(self) -> bool {
        stat(self.pid).is_some_and(|stat| stat.start == self.start && !stat.zombie)
    }

    pub fn terminate(self) {
        let Some(pid) = i32::try_from(self.pid).ok().and_then(Pid::from_raw) else { return };
        if self.alive() {
            let _ = kill_process(pid, Signal::Term);
        }
    }

    pub fn command_line(self) -> String {
        arguments(self.pid).join(" ")
    }

    pub fn encode(self) -> String {
        format!("{} {}", self.pid, self.start)
    }

    pub fn decode(text: &str) -> Option<Self> {
        let (pid, start) = text.split_once(' ')?;
        Some(Self { pid: pid.parse().ok()?, start: start.parse().ok()? })
    }
}

pub fn session_hosts() -> BTreeSet<Process> {
    every()
        .into_iter()
        .filter(|(pid, _)| arguments(*pid).get(1).is_some_and(|word| word == "session-host"))
        .filter_map(|(pid, stat)| (!stat.zombie).then_some(Process { pid, start: stat.start }))
        .collect()
}

pub fn descendants(roots: &[Process]) -> Vec<Process> {
    let table = every();
    let mut children: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for (pid, stat) in &table {
        children.entry(stat.parent).or_default().push(*pid);
    }
    let mut found = Vec::new();
    let mut frontier: Vec<u32> = roots.iter().filter(|root| root.alive()).map(|root| root.pid).collect();
    while let Some(pid) = frontier.pop() {
        for child in children.get(&pid).into_iter().flatten() {
            if let Some(stat) = table.get(child).filter(|stat| !stat.zombie) {
                found.push(Process { pid: *child, start: stat.start });
                frontier.push(*child);
            }
        }
    }
    found
}

fn every() -> BTreeMap<u32, Stat> {
    let Ok(entries) = fs::read_dir("/proc") else { return BTreeMap::new() };
    entries
        .filter_map(|entry| entry.ok()?.file_name().to_str()?.parse::<u32>().ok())
        .filter_map(|pid| Some((pid, stat(pid)?)))
        .collect()
}

fn stat(pid: u32) -> Option<Stat> {
    let text = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let fields: Vec<&str> = text.rsplit_once(')')?.1.split_whitespace().collect();
    Some(Stat {
        parent: fields.get(1)?.parse().ok()?,
        start: fields.get(19)?.parse().ok()?,
        zombie: *fields.first()? == "Z",
    })
}

fn arguments(pid: u32) -> Vec<String> {
    fs::read(format!("/proc/{pid}/cmdline"))
        .map(|bytes| bytes.split(|byte| *byte == 0).filter(|word| !word.is_empty()).map(|word| String::from_utf8_lossy(word).into_owned()).collect())
        .unwrap_or_default()
}
