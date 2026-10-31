//! Runs a worker detached from its CLI (`setsid`, `nohup`, `job-run --notify`): the live processes in
//! the session host's own scope that are neither the host nor its descendants. The CLI's own
//! background tasks descend from the host and are tracked from its stream instead. A process handed
//! the host's pid (safe-push's lane reaper) watches the session rather than works for it, and is no run.
//! A revived session's earlier hosts leave their scopes behind (`host.priors`, one `UNIT PID` per
//! line) holding only the runs that host's tree did not own; the new host reads those scopes too.
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use super::runtime::summarize;
use super::waits::Run;

const UNIT_PREFIX: &str = "north-session-";
const COMMAND_CHARS: usize = 120;
const PRIORS: &str = "host.priors";

/// A scope an earlier host of the session left behind, and the pid of that host.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Prior {
    pub unit: String,
    pub host: u32,
}

impl Prior {
    pub fn all(dir: &Path) -> Vec<Prior> {
        let text = fs::read_to_string(dir.join(PRIORS)).unwrap_or_default();
        text.lines().filter_map(|line| line.split_once(' ')).filter_map(|(unit, host)| Some(Prior { unit: unit.to_owned(), host: host.parse().ok()? })).collect()
    }

    pub fn remember(&self, dir: &Path) -> std::io::Result<()> {
        let mut text = fs::read_to_string(dir.join(PRIORS)).unwrap_or_default();
        text.push_str(&format!("{} {}\n", self.unit, self.host));
        fs::write(dir.join(PRIORS), text)
    }

    pub fn forget_all(dir: &Path) {
        let _ = fs::remove_file(dir.join(PRIORS));
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Detached {
    pub pid: u32,
    pub started: u64,
    pub command: String,
}

impl Detached {
    pub fn run(&self) -> Run {
        Run::Process { pid: self.pid, started: self.started }
    }

    fn same(&self, other: &Detached) -> bool {
        self.pid == other.pid && self.started == other.started
    }
}

/// The detached runs of one session host.
pub trait Scope: Send + Sync {
    fn detached(&self) -> Vec<Detached>;
}

pub struct NoScope;

impl Scope for NoScope {
    fn detached(&self) -> Vec<Detached> {
        Vec::new()
    }
}

/// The host's `north-session-*.scope` cgroup, read through procfs and cgroupfs; a host outside such
/// a scope shares its cgroup with unrelated processes and sees no detached runs.
pub struct Procfs {
    proc: PathBuf,
    cgroups: PathBuf,
    host: u32,
    priors: Vec<Prior>,
}

impl Procfs {
    pub fn here() -> Self {
        Self::new("/proc".into(), "/sys/fs/cgroup".into(), std::process::id())
    }

    pub fn new(proc: PathBuf, cgroups: PathBuf, host: u32) -> Self {
        Self { proc, cgroups, host, priors: Vec::new() }
    }

    pub fn after(mut self, priors: Vec<Prior>) -> Self {
        self.priors = priors;
        self
    }

    fn scope(&self) -> Option<PathBuf> {
        session_scope(&self.proc, &self.cgroups, self.host)
    }

    fn runs(&self, scope: &Path, host: u32) -> Vec<Detached> {
        let stats = members(&self.proc, scope);
        let detached = |pid: u32| pid != host && !descends(pid, host, &stats);
        let mut roots: Vec<Detached> = stats
            .iter()
            .filter(|(pid, stat)| detached(**pid) && !(stats.contains_key(&stat.parent) && detached(stat.parent)))
            .map(|(pid, stat)| (*pid, stat.started, self.arguments(*pid)))
            .filter(|(_, _, arguments)| !arguments.contains(&host.to_string()))
            .map(|(pid, started, arguments)| Detached { pid, started, command: summarize(&arguments.join(" "), COMMAND_CHARS) })
            .collect();
        roots.sort_by_key(|run| run.pid);
        roots
    }

    fn arguments(&self, pid: u32) -> Vec<String> {
        let raw = fs::read(self.proc.join(pid.to_string()).join("cmdline")).unwrap_or_default();
        String::from_utf8_lossy(&raw).split('\0').filter(|word| !word.is_empty()).map(str::to_owned).collect()
    }
}

impl Scope for Procfs {
    fn detached(&self) -> Vec<Detached> {
        let Some(scope) = self.scope() else { return Vec::new() };
        let mut runs = self.runs(&scope, self.host);
        for prior in &self.priors {
            if let Some(slice) = scope.parent() {
                runs.extend(self.runs(&slice.join(&prior.unit), prior.host));
            }
        }
        runs
    }
}

/// The cgroup directory of the `north-session-*.scope` that `pid` runs in.
fn session_scope(proc: &Path, cgroups: &Path, pid: u32) -> Option<PathBuf> {
    let membership = fs::read_to_string(proc.join(pid.to_string()).join("cgroup")).ok()?;
    let path = membership.lines().find_map(|line| line.strip_prefix("0::"))?.trim();
    let unit = Path::new(path).file_name()?.to_str()?;
    (unit.starts_with(UNIT_PREFIX) && unit.ends_with(".scope")).then(|| cgroups.join(path.trim_start_matches('/')))
}

/// The host `pid` and every live process that descends from it in its scope: the tree a revival stops
/// while the runs the session detached go on.
pub fn host_tree(proc: &Path, cgroups: &Path, host: u32) -> Vec<u32> {
    let Some(scope) = session_scope(proc, cgroups, host) else { return Vec::new() };
    let stats = members(proc, &scope);
    let mut tree: Vec<u32> = stats.keys().copied().filter(|pid| *pid == host || descends(*pid, host, &stats)).collect();
    tree.sort_unstable();
    tree
}

fn members(proc: &Path, scope: &Path) -> HashMap<u32, Stat> {
    let members = fs::read_to_string(scope.join("cgroup.procs")).unwrap_or_default();
    members.lines().filter_map(|line| line.trim().parse().ok()).filter_map(|pid| Some((pid, stat(proc, pid)?))).filter(|(_, stat)| stat.live).collect()
}

/// Whether `pid` still runs the process that started at `started`.
pub fn alive(proc: &Path, pid: u32, started: u64) -> bool {
    stat(proc, pid).is_some_and(|stat| stat.live && stat.started == started)
}

/// The runs in `before` that `now` no longer holds.
pub fn ended(before: &[Detached], now: &[Detached]) -> Vec<Detached> {
    before.iter().filter(|run| !now.iter().any(|live| live.same(run))).cloned().collect()
}

pub fn ended_message(runs: &[Detached]) -> String {
    let named: Vec<String> = runs.iter().map(|run| format!("pid {} `{}`", run.pid, run.command)).collect();
    format!("[north: a run you detached has ended] {}. Read its output and continue to a final report.", named.join("; "))
}

struct Stat {
    parent: u32,
    started: u64,
    live: bool,
}

// procfs stat: the command sits in parentheses and may hold spaces; field 3 (state) starts after the last ')'.
fn stat(proc: &Path, pid: u32) -> Option<Stat> {
    let text = fs::read_to_string(proc.join(pid.to_string()).join("stat")).ok()?;
    let fields: Vec<&str> = text.get(text.rfind(')')? + 1..)?.split_whitespace().collect();
    Some(Stat { live: !matches!(*fields.first()?, "Z" | "X"), parent: fields.get(1)?.parse().ok()?, started: fields.get(19)?.parse().ok()? })
}

fn descends(pid: u32, ancestor: u32, stats: &HashMap<u32, Stat>) -> bool {
    let mut current = pid;
    for _ in 0..=stats.len() {
        let Some(stat) = stats.get(&current) else { return false };
        if stat.parent == ancestor {
            return true;
        }
        current = stat.parent;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOST: u32 = 100;
    const SCOPE: &str = "/user.slice/user@1000.service/agent.slice/north-session-1.scope";

    fn tree(scope: &str, processes: &[(u32, u32, &str, u64, &str)]) -> (tempfile::TempDir, Procfs) {
        let root = tempfile::tempdir().unwrap();
        let (proc, cgroups) = (root.path().join("proc"), root.path().join("cgroup"));
        for (pid, parent, state, started, command) in processes {
            let dir = proc.join(pid.to_string());
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("stat"), format!("{pid} (a b) c) {state} {parent} {} {started} 0 0", ["0"; 17].join(" "))).unwrap();
            fs::write(dir.join("cmdline"), command.replace(' ', "\0")).unwrap();
            fs::write(dir.join("cgroup"), format!("0::{scope}\n")).unwrap();
        }
        let unit = cgroups.join(scope.trim_start_matches('/'));
        fs::create_dir_all(&unit).unwrap();
        fs::write(unit.join("cgroup.procs"), processes.iter().map(|(pid, ..)| format!("{pid}\n")).collect::<String>()).unwrap();
        let procfs = Procfs::new(proc, cgroups, HOST);
        (root, procfs)
    }

    #[test]
    fn a_detached_run_is_a_scope_process_outside_the_hosts_tree_named_by_its_root_and_a_zombie_or_a_watcher_of_the_host_is_not() {
        let processes = [
            (HOST, 1, "S", 10, "north session-host"),
            (101, HOST, "S", 11, "claude -p"),
            (102, 101, "S", 12, "bash -c cargo test"),
            (200, 892, "S", 20, "bash job-run land -- safe-push"),
            (201, 200, "S", 21, "safe-push --to main"),
            (300, 892, "Z", 30, ""),
            (400, 892, "S", 40, "bash -c reap safe-push-lane-reaper /repo /lane 101 100"),
        ];
        let (_root, procfs) = tree(SCOPE, &processes);
        assert_eq!(procfs.detached(), [Detached { pid: 200, started: 20, command: "bash job-run land -- safe-push".into() }]);
    }

    #[test]
    fn a_host_outside_a_session_scope_sees_no_detached_run() {
        let (_root, procfs) = tree("/user.slice/user@1000.service/app.slice/terminal.scope", &[(HOST, 1, "S", 10, "north"), (200, 1, "S", 20, "vim")]);
        assert!(procfs.detached().is_empty());
    }

    #[test]
    fn a_run_is_alive_only_while_its_pid_holds_the_process_that_started_then() {
        let (root, _) = tree(SCOPE, &[(200, 1, "S", 20, "sleep"), (300, 1, "Z", 30, "")]);
        let proc = root.path().join("proc");
        assert!(alive(&proc, 200, 20));
        assert!(!alive(&proc, 200, 21), "a reused pid is another process");
        assert!(!alive(&proc, 300, 30) && !alive(&proc, 400, 40));
    }
}
