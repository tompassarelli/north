use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

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

    pub fn waiting(self) -> String {
        let read = |name: &str| fs::read_to_string(format!("/proc/{}/{name}", self.pid)).unwrap_or_default();
        let state = read("stat").rsplit_once(')').and_then(|(_, rest)| rest.split_whitespace().next().map(str::to_owned)).unwrap_or_default();
        let held: Vec<String> = fs::read_dir(format!("/proc/{}/fd", self.pid)).into_iter().flatten().flatten().filter_map(|fd| fs::read_link(fd.path()).ok()).map(|target| target.display().to_string()).collect();
        let threads = threads(self.pid);
        let stalled = threads.first().is_some_and(|thread| thread.waits_in.starts_with("futex"));
        let listed: String = threads.iter().map(|thread| format!("\n  thread {} {} state {} in {}", thread.id, thread.name, thread.state, thread.waits_in)).collect();
        let traced = if stalled { backtrace(self.pid) } else { String::new() };
        format!("{} state {state} in {}: {}\n  holds {}{listed}{traced}", self.pid, read("wchan"), self.command_line(), held.join(", "))
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

pub fn follows(root: &Path) -> Vec<Process> {
    every()
        .into_iter()
        .filter(|(pid, stat)| !stat.zombie && is_follow(&arguments(*pid), root))
        .map(|(pid, stat)| Process { pid, start: stat.start })
        .collect()
}

fn is_follow(words: &[String], root: &Path) -> bool {
    words.windows(2).any(|pair| pair[0] == "land" && pair[1] == "follow") && words.windows(2).any(|pair| pair[0] == "--repo" && Path::new(&pair[1]).starts_with(root))
}

pub fn within(root: &Path) -> Vec<Process> {
    let marker = root.to_string_lossy();
    every()
        .into_iter()
        .filter(|(pid, stat)| !stat.zombie && *pid != std::process::id())
        .filter(|(pid, _)| arguments(*pid).iter().any(|word| word.contains(marker.as_ref())) || fs::read_link(format!("/proc/{pid}/cwd")).is_ok_and(|cwd| cwd.starts_with(root)))
        .map(|(pid, stat)| Process { pid, start: stat.start })
        .collect()
}

struct Thread {
    id: u32,
    name: String,
    state: String,
    waits_in: String,
}

fn threads(pid: u32) -> Vec<Thread> {
    let mut ids: Vec<u32> = fs::read_dir(format!("/proc/{pid}/task")).into_iter().flatten().flatten().filter_map(|task| task.file_name().to_str()?.parse().ok()).collect();
    ids.sort_unstable_by_key(|id| (*id != pid, *id));
    ids.into_iter()
        .map(|id| {
            let read = |name: &str| fs::read_to_string(format!("/proc/{pid}/task/{id}/{name}")).unwrap_or_default().trim().to_owned();
            let state = read("stat").rsplit_once(')').and_then(|(_, rest)| rest.split_whitespace().next().map(str::to_owned)).unwrap_or_default();
            Thread { id, name: read("comm"), state, waits_in: read("wchan") }
        })
        .collect()
}

fn backtrace(pid: u32) -> String {
    let (Ok(gdb), Ok(timeout)) = (super::product::installed("gdb"), super::product::installed("timeout")) else { return String::new() };
    let traced = std::process::Command::new(timeout).args(["--kill-after=1", "9"]).arg(gdb).args(["-batch", "-p", &pid.to_string(), "-ex", "thread apply all bt"]).stdin(std::process::Stdio::null()).output();
    let text = traced.map_or_else(|error| format!("gdb: {error}"), |output| format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr)));
    let lines: String = text.lines().filter(|line| !line.is_empty()).map(|line| format!("\n    {line}")).collect();
    format!("\n  gdb thread apply all bt:{lines}")
}
