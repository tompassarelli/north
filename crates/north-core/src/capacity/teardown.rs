use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use rustix::process::{Pid, Signal, kill_process};

const PREFIX_VARIABLES: [&str; 2] = ["STEAM_COMPAT_DATA_PATH", "WINEPREFIX"];

#[derive(Clone, Debug)]
pub struct Entry {
    pub ppid: i32,
    pub comm: String,
    pub cgroup: String,
    pub args: String,
    pub prefix: Option<String>,
}

impl Entry {
    fn agents(&self) -> bool {
        self.cgroup.contains("/agent.slice/") || self.cgroup.contains("libpod")
    }

    fn native_scope(&self) -> Option<&str> {
        let mut parts = self.cgroup.split('/').skip_while(|part| *part != "native.slice");
        parts.next()?;
        parts.next().filter(|scope| scope.ends_with(".scope"))
    }

    fn named(&self, names: &[String]) -> bool {
        names.iter().any(|name| self.comm == name.chars().take(15).collect::<String>() || launches(&self.args, name))
    }

    fn wisp_window(&self) -> bool {
        let words = self.args.split(' ').collect::<Vec<_>>();
        words.get(1).is_some_and(|script| script.ends_with("wisp") || script.ends_with("wisp.ts"))
            && words.get(2) == Some(&"play")
            && words.windows(2).any(|pair| pair == ["--window", "native"])
            && !words.contains(&"--headless")
    }
}

#[derive(Clone, Debug)]
pub struct Names {
    pub game: Vec<String>,
    pub launcher: Vec<String>,
    pub native: Vec<String>,
}

impl Names {
    pub fn bundled() -> Self {
        Self {
            game: super::mode::strings("capacity_teardown_game_names"),
            launcher: super::mode::strings("capacity_teardown_launcher_names"),
            native: super::mode::strings("capacity_teardown_native_names"),
        }
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct Targets {
    pub game: BTreeSet<i32>,
    pub launcher: BTreeSet<i32>,
    pub native: BTreeSet<i32>,
}

impl Targets {
    pub fn all(&self) -> BTreeSet<i32> {
        self.game.iter().chain(&self.launcher).chain(&self.native).copied().collect()
    }
}

pub fn table(proc: &Path) -> BTreeMap<i32, Entry> {
    let Ok(entries) = fs::read_dir(proc) else { return BTreeMap::new() };
    entries
        .flatten()
        .filter_map(|entry| {
            let pid = entry.file_name().to_str()?.parse::<i32>().ok()?;
            let dir = entry.path();
            let stat = fs::read_to_string(dir.join("stat")).ok()?;
            let cgroup = fs::read_to_string(dir.join("cgroup")).ok()?;
            let cmdline = fs::read(dir.join("cmdline")).ok()?;
            let fields = stat.get(stat.rfind(')')? + 2..)?.split(' ').collect::<Vec<_>>();
            let ppid = fields.get(1)?.parse().ok()?;
            let comm = stat.get(stat.find('(')? + 1..stat.rfind(')')?)?.to_owned();
            let cgroup = cgroup.lines().find_map(|line| line.strip_prefix("0::")).unwrap_or_default().to_owned();
            let args = String::from_utf8_lossy(&cmdline).replace('\0', " ").trim_end().to_owned();
            let environ = fs::read(dir.join("environ")).unwrap_or_default();
            let prefix = environ.split(|byte| *byte == 0).find_map(|pair| {
                let text = std::str::from_utf8(pair).ok()?;
                let (name, value) = text.split_once('=')?;
                PREFIX_VARIABLES.contains(&name).then(|| value.to_owned())
            });
            Some((pid, Entry { ppid, comm, cgroup, args, prefix }))
        })
        .collect()
}

fn descendants(table: &BTreeMap<i32, Entry>, roots: impl IntoIterator<Item = i32>) -> BTreeSet<i32> {
    let mut children: BTreeMap<i32, Vec<i32>> = BTreeMap::new();
    for (pid, entry) in table {
        children.entry(entry.ppid).or_default().push(*pid);
    }
    let mut stack = roots.into_iter().collect::<Vec<_>>();
    let mut tree = BTreeSet::new();
    while let Some(pid) = stack.pop() {
        if table.get(&pid).is_some_and(|entry| !entry.agents()) && tree.insert(pid) {
            stack.extend(children.get(&pid).into_iter().flatten());
        }
    }
    tree
}

pub fn targets(table: &BTreeMap<i32, Entry>, names: &Names) -> Targets {
    let pick = |test: &dyn Fn(&Entry) -> bool| table.iter().filter(|(_, entry)| !entry.agents() && test(entry)).map(|(pid, _)| *pid).collect::<Vec<_>>();
    let game = descendants(table, pick(&|entry| entry.named(&names.game)));
    let prefixes = game.iter().filter_map(|pid| table.get(pid)?.prefix.clone()).collect::<BTreeSet<_>>();
    let same_prefix = |entry: &Entry| prefixes.is_empty() || entry.prefix.as_ref().is_none_or(|prefix| prefixes.contains(prefix));
    let launcher = descendants(table, pick(&|entry| entry.named(&names.launcher) && same_prefix(entry))).difference(&game).copied().collect::<BTreeSet<_>>();
    let native_roots = pick(&|entry| entry.native_scope().is_some() || entry.wisp_window() || names.native.contains(&entry.comm));
    let native = descendants(table, native_roots).difference(&game).copied().filter(|pid| !launcher.contains(pid)).collect();
    Targets { game, launcher, native }
}

pub trait Signaller {
    fn send(&mut self, pid: i32, signal: Signal);
    fn alive(&self, pid: i32) -> bool;
    fn wait(&mut self, step: Duration);
}

pub struct Live<'a> {
    pub proc: &'a Path,
}

impl Signaller for Live<'_> {
    fn send(&mut self, pid: i32, signal: Signal) {
        if let Some(pid) = Pid::from_raw(pid) {
            let _ = kill_process(pid, signal);
        }
    }

    fn alive(&self, pid: i32) -> bool {
        fs::read_to_string(self.proc.join(pid.to_string()).join("stat")).is_ok_and(|stat| stat.rsplit_once(") ").is_some_and(|(_, rest)| !rest.starts_with('Z')))
    }

    fn wait(&mut self, step: Duration) {
        std::thread::sleep(step);
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct Closed {
    pub terminated: BTreeSet<i32>,
    pub killed: BTreeSet<i32>,
}

pub fn close(pids: &BTreeSet<i32>, signaller: &mut impl Signaller, grace: Duration) -> Closed {
    pids.iter().for_each(|pid| signaller.send(*pid, Signal::Term));
    let step = Duration::from_millis(100);
    let started = Instant::now();
    let mut waited = Duration::ZERO;
    while waited < grace && started.elapsed() < grace && pids.iter().any(|pid| signaller.alive(*pid)) {
        signaller.wait(step);
        waited += step;
    }
    let killed = pids.iter().copied().filter(|pid| signaller.alive(*pid)).collect::<BTreeSet<_>>();
    killed.iter().for_each(|pid| signaller.send(*pid, Signal::Kill));
    Closed { terminated: pids.difference(&killed).copied().collect(), killed }
}

pub fn describe(targets: &Targets, closed: &Closed) -> String {
    let mut parts = Vec::new();
    for (label, pids) in [("Warcraft III", &targets.game), ("its launcher", &targets.launcher), ("the native test client", &targets.native)] {
        if !pids.is_empty() {
            parts.push(format!("{label} ({} processes)", pids.len()));
        }
    }
    if parts.is_empty() {
        return "no test environment was running".to_owned();
    }
    let forced = if closed.killed.is_empty() { String::new() } else { format!(", {} force-killed after the grace period", closed.killed.len()) };
    format!("closed {}{forced}", parts.join(", "))
}

pub fn run(proc: &Path, names: &Names, signaller: &mut impl Signaller, grace: Duration) -> String {
    let targets = targets(&table(proc), names);
    let closed = close(&targets.all(), signaller, grace);
    describe(&targets, &closed)
}

fn launches(args: &str, name: &str) -> bool {
    args.match_indices(name).any(|(at, _)| {
        let head = &args[..at];
        let path = head.starts_with('/') || head.get(1..3) == Some(":\\");
        let after = args[at + name.len()..].chars().next();
        (head.is_empty() || (path && (head.ends_with('/') || head.ends_with('\\')))) && after.is_none_or(char::is_whitespace)
    })
}
