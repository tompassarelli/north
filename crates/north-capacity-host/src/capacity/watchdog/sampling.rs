use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::LazyLock;

use north_base::foundation::clock::Timestamp;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const TREE_CORES: f64 = 4.0;
const TREE_SECONDS: f64 = 120.0;
const MIN_CGROUP_CORES: f64 = 0.05;
const TOP_PROCESSES: usize = 8;
const UNCONDITIONAL_CGROUPS: usize = 15;
const CHAIN_DEPTH: usize = 8;
const COMMAND_WIDTH: usize = 160;
const SUMMARY_COMMAND_WIDTH: usize = 80;
const LISTED_PIDS: usize = 20;
const TICKS_PER_SECOND: f64 = 100.0;

static LEASED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"/agent-capacity-[0-9a-f]{32}\.scope(/|$)").expect("lease pattern"));
static NIGHTLY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"/git-maintenance-nightly\.service(/|$)").expect("nightly pattern"));
static CLIENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(Warcraft III|BlizzardBrows)").expect("client pattern"));
static USER_ROOT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^/user\.slice/user-\d+\.slice/user@\d+\.service/").expect("user root pattern"));
static UNIT_CGROUP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\.(scope|service)$").expect("unit pattern"));

#[derive(Clone, Debug, Deserialize)]
pub(super) struct Proc {
    pub(super) pid: i64,
    pub(super) ppid: i64,
    pub(super) sid: i64,
    pub(super) comm: String,
    pub(super) args: Vec<String>,
    pub(super) cgroup: String,
    pub(super) ticks: f64,
    pub(super) start: String,
    #[serde(rename = "rssKiB")]
    pub(super) rss_kib: f64,
    pub(super) cwd: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Snapshot {
    pub(super) at: i64,
    pub(super) load: Vec<f64>,
    pub(super) cpu_psi: f64,
    pub(super) cpu_psi60: f64,
    pub(super) mem_psi: f64,
    pub(super) mem_full_psi: f64,
    #[serde(rename = "memAvailableMiB")]
    pub(super) mem_available_mib: f64,
    #[serde(rename = "memTotalMiB")]
    pub(super) mem_total_mib: f64,
    pub(super) cgroups: BTreeMap<String, f64>,
    #[serde(default)]
    pub(super) busy_ticks: Option<f64>,
    pub(super) procs: Vec<Proc>,
    pub(super) leases: Vec<Value>,
}

#[derive(Clone, Copy, Debug)]
struct Hot {
    since: i64,
    reported: bool,
}

#[derive(Debug, Default)]
pub(super) struct State {
    prev: Option<Snapshot>,
    hot: HashMap<String, Hot>,
}

impl State {
    pub(super) fn has_previous(&self) -> bool {
        self.prev.is_some()
    }
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub(super) enum Kind {
    UnleasedTree,
    GitMaintenanceTerminated,
}

#[derive(Debug, Serialize)]
pub(super) struct Incident {
    pub(super) at: String,
    pub(super) kind: Kind,
    pub(super) summary: String,
    pub(super) cgroup: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) cores: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) seconds: Option<i64>,
    pub(super) cwd: Option<String>,
    pub(super) chain: Vec<String>,
    pub(super) pids: Vec<i64>,
}

#[derive(Debug, Clone)]
pub(super) struct Term {
    pub(super) pid: i64,
    pub(super) start: String,
}

#[derive(Debug, Serialize)]
struct Hog {
    pid: i64,
    cores: f64,
    #[serde(rename = "rssMiB")]
    rss_mib: i64,
    cgroup: String,
    command: String,
}

#[derive(Debug, Serialize)]
struct CgroupCores {
    cgroup: String,
    cores: f64,
}

#[derive(Debug, Serialize)]
struct Client {
    pid: i64,
    comm: String,
    cores: f64,
    #[serde(rename = "rssMiB")]
    rss_mib: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Reading {
    pub(super) at: String,
    pub(super) load: Vec<f64>,
    pub(super) cpu_psi: f64,
    pub(super) cpu_psi60: f64,
    pub(super) mem_psi: f64,
    pub(super) mem_full_psi: f64,
    #[serde(rename = "memAvailableMiB")]
    pub(super) mem_available_mib: f64,
    #[serde(rename = "memTotalMiB")]
    pub(super) mem_total_mib: f64,
    pub(super) interval_seconds: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    busy_cores: Option<f64>,
    cgroups: Vec<CgroupCores>,
    top: Vec<Hog>,
    clients: Vec<Client>,
    leases: Vec<Value>,
    hot_trees: usize,
}

pub(super) struct Stepped {
    pub(super) state: State,
    pub(super) reading: Reading,
    pub(super) incidents: Vec<Incident>,
    pub(super) terms: Vec<Term>,
}

pub(super) fn round2(number: f64) -> f64 {
    (number * 100.0 + 0.5).floor() / 100.0
}

fn command(process: &Proc) -> String {
    let text = if process.args.is_empty() { format!("[{}]", process.comm) } else { process.args.join(" ") };
    text.chars().take(COMMAND_WIDTH).collect()
}

fn leased(cgroup: &str) -> bool {
    LEASED.is_match(cgroup)
}

fn short(cgroup: &str) -> String {
    USER_ROOT.replace(cgroup, "").into_owned()
}

fn iso(at: i64) -> String {
    Timestamp::from_unix_millis(at).iso_millis()
}

pub(super) fn git_maintenance_verb(args: &[String]) -> Option<&str> {
    let program = args.first()?.rsplit('/').next()?;
    if program != "git" {
        return None;
    }
    let mut words = args[1..].iter();
    while let Some(word) = words.next() {
        match word.as_str() {
            "-C" | "-c" | "--git-dir" | "--work-tree" => {
                words.next();
            }
            flag if flag.starts_with('-') => {}
            verb => return ["maintenance", "gc", "repack"].contains(&verb).then_some(verb),
        }
    }
    None
}

fn chain_of(pid: i64, by_pid: &HashMap<i64, &Proc>) -> Vec<String> {
    let mut chain = Vec::new();
    let mut next = by_pid.get(&pid).copied();
    while let Some(process) = next.filter(|_| chain.len() < CHAIN_DEPTH) {
        chain.push(format!("{} {}", process.pid, command(process)));
        next = by_pid.get(&process.ppid).copied();
    }
    chain
}

fn tree_incident(procs: &[&Proc], cores: &HashMap<i64, f64>, by_pid: &HashMap<i64, &Proc>, total: f64, seconds: f64, at: &str) -> Incident {
    let rate = |process: &Proc| cores.get(&process.pid).copied().unwrap_or(0.0);
    let lead = procs.iter().copied().reduce(|best, next| if rate(next) > rate(best) { next } else { best }).expect("a tree has processes");
    let lead_command: String = command(lead).chars().take(SUMMARY_COMMAND_WIDTH).collect();
    Incident {
        at: at.to_owned(),
        kind: Kind::UnleasedTree,
        cgroup: short(&procs[0].cgroup),
        cores: Some(round2(total)),
        seconds: Some(seconds.round() as i64),
        cwd: lead.cwd.clone(),
        chain: chain_of(lead.pid, by_pid),
        pids: procs.iter().map(|process| process.pid).take(LISTED_PIDS).collect(),
        summary: format!("unleased tree using {} cores for {} s: {lead_command}", round2(total), seconds.round() as i64),
    }
}

fn terminations(snap: &Snapshot, by_pid: &HashMap<i64, &Proc>, at: &str) -> (Vec<Incident>, Vec<Term>) {
    let roots = snap.procs.iter().filter(|process| !leased(&process.cgroup) && !NIGHTLY.is_match(&process.cgroup) && git_maintenance_verb(&process.args).is_some());
    let (mut incidents, mut terms, mut killed) = (Vec::new(), Vec::new(), HashSet::new());
    for root in roots {
        let prefix = format!("{} ", root.pid);
        let mut group = vec![root];
        group.extend(snap.procs.iter().filter(|process| !killed.contains(&process.pid) && process.pid != root.pid && chain_of(process.pid, by_pid).iter().any(|link| link.starts_with(&prefix))));
        let fresh: Vec<&Proc> = group.into_iter().filter(|process| !killed.contains(&process.pid)).collect();
        if fresh.is_empty() {
            continue;
        }
        for process in &fresh {
            killed.insert(process.pid);
            terms.push(Term { pid: process.pid, start: process.start.clone() });
        }
        let verb = git_maintenance_verb(&root.args).unwrap_or_default();
        incidents.push(Incident {
            at: at.to_owned(),
            kind: Kind::GitMaintenanceTerminated,
            cgroup: short(&root.cgroup),
            cwd: root.cwd.clone(),
            chain: chain_of(root.pid, by_pid),
            pids: fresh.iter().map(|process| process.pid).collect(),
            summary: format!("SIGTERM git {verb} outside the nightly timer ({} processes) in {}", fresh.len(), root.cwd.as_deref().unwrap_or("?")),
            cores: None,
            seconds: None,
        });
    }
    (incidents, terms)
}

fn process_cores(prev: Option<&Snapshot>, snap: &Snapshot, dt: f64) -> HashMap<i64, f64> {
    let Some(prev) = prev.filter(|_| dt > 0.0) else { return HashMap::new() };
    let before: HashMap<i64, &Proc> = prev.procs.iter().map(|process| (process.pid, process)).collect();
    snap.procs
        .iter()
        .filter_map(|process| before.get(&process.pid).filter(|old| old.start == process.start).map(|old| (process.pid, ((process.ticks - old.ticks) / TICKS_PER_SECOND / dt).max(0.0))))
        .collect()
}

fn cgroup_cores(prev: Option<&Snapshot>, snap: &Snapshot, dt: f64) -> Vec<CgroupCores> {
    let Some(prev) = prev.filter(|_| dt > 0.0) else { return Vec::new() };
    let mut busy: Vec<CgroupCores> = snap
        .cgroups
        .iter()
        .filter_map(|(cgroup, usec)| prev.cgroups.get(cgroup).map(|before| CgroupCores { cgroup: cgroup.clone(), cores: round2((usec - before).max(0.0) / 1e6 / dt) }))
        .filter(|entry| entry.cores >= MIN_CGROUP_CORES)
        .collect();
    busy.sort_by(|left, right| right.cores.total_cmp(&left.cores));
    busy.into_iter().enumerate().filter(|(index, entry)| *index < UNCONDITIONAL_CGROUPS || UNIT_CGROUP.is_match(&entry.cgroup)).map(|(_, entry)| entry).collect()
}

pub(super) fn step(state: State, snap: Snapshot) -> Stepped {
    let prev = state.prev.as_ref();
    let dt = prev.map_or(0.0, |prev| (snap.at - prev.at) as f64 / 1000.0);
    let cores = process_cores(prev, &snap, dt);
    let rate = |process: &Proc| cores.get(&process.pid).copied().unwrap_or(0.0);
    let by_pid: HashMap<i64, &Proc> = snap.procs.iter().map(|process| (process.pid, process)).collect();
    let unleased: Vec<&Proc> = snap.procs.iter().filter(|process| !leased(&process.cgroup) && process.cgroup != "/").collect();
    let mut hogs: Vec<&Proc> = unleased.iter().copied().filter(|process| rate(process) >= MIN_CGROUP_CORES).collect();
    hogs.sort_by(|left, right| rate(right).total_cmp(&rate(left)));
    let top = hogs.into_iter().take(TOP_PROCESSES).map(|process| Hog { pid: process.pid, cores: round2(rate(process)), rss_mib: (process.rss_kib / 1024.0).round() as i64, cgroup: short(&process.cgroup), command: command(process) }).collect();
    let clients = snap.procs.iter().filter(|process| CLIENT.is_match(&process.comm)).map(|process| Client { pid: process.pid, comm: process.comm.clone(), cores: round2(rate(process)), rss_mib: (process.rss_kib / 1024.0).round() as i64 }).collect();

    let at = iso(snap.at);
    let mut trees: Vec<(String, Vec<&Proc>)> = Vec::new();
    for process in &unleased {
        let key = format!("{}|{}", process.cgroup, process.sid);
        match trees.iter_mut().find(|(name, _)| *name == key) {
            Some((_, members)) => members.push(process),
            None => trees.push((key, vec![process])),
        }
    }
    let mut incidents = Vec::new();
    let mut hot = HashMap::new();
    for (key, members) in &trees {
        let total: f64 = members.iter().map(|process| rate(process)).sum();
        if total <= TREE_CORES {
            continue;
        }
        let mut entry = state.hot.get(key).copied().unwrap_or(Hot { since: prev.map_or(snap.at, |prev| prev.at), reported: false });
        let seconds = (snap.at - entry.since) as f64 / 1000.0;
        if !entry.reported && seconds >= TREE_SECONDS {
            entry.reported = true;
            incidents.push(tree_incident(members, &cores, &by_pid, total, seconds, &at));
        }
        hot.insert(key.clone(), entry);
    }
    let (git_incidents, terms) = terminations(&snap, &by_pid, &at);
    incidents.extend(git_incidents);

    let busy_cores = prev.zip(snap.busy_ticks).filter(|_| dt > 0.0).and_then(|(prev, now)| prev.busy_ticks.map(|before| round2((now - before).max(0.0) / TICKS_PER_SECOND / dt)));
    let reading = Reading {
        at,
        load: snap.load.clone(),
        cpu_psi: snap.cpu_psi,
        cpu_psi60: snap.cpu_psi60,
        mem_psi: snap.mem_psi,
        mem_full_psi: snap.mem_full_psi,
        mem_available_mib: snap.mem_available_mib,
        mem_total_mib: snap.mem_total_mib,
        interval_seconds: round2(dt),
        busy_cores,
        cgroups: cgroup_cores(prev, &snap, dt),
        top,
        clients,
        leases: snap.leases.clone(),
        hot_trees: hot.len(),
    };
    Stepped { state: State { prev: Some(snap), hot }, reading, incidents, terms }
}
