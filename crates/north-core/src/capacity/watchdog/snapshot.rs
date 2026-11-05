use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{Value, json};

use super::sampling::{Proc, Snapshot, round2};

pub(super) const CGROOT: &str = "/sys/fs/cgroup";
const CGROUP_DEPTH: usize = 4;
const SYSTEM_SLICE_DEPTH: usize = 3;
const MAX_ARGS: usize = 24;
const PAGE_KIB: f64 = 4.0;

pub(super) fn read(path: impl AsRef<Path>) -> Option<String> {
    fs::read_to_string(path).ok()
}

pub(super) fn user_id() -> u32 {
    fs::metadata("/proc/self").map_or(1000, |meta| meta.uid())
}

pub(super) fn user_root() -> PathBuf {
    let uid = user_id();
    PathBuf::from(format!("{CGROOT}/user.slice/user-{uid}.slice/user@{uid}.service"))
}

fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR").map_or_else(|| PathBuf::from(format!("/run/user/{}", user_id())), PathBuf::from)
}

fn pressure(path: &str, kind: &str, field: &str) -> f64 {
    read(path)
        .and_then(|text| text.lines().find(|line| line.starts_with(kind)).and_then(|line| line.split_whitespace().find_map(|word| word.strip_prefix(field)?.parse().ok())))
        .unwrap_or(0.0)
}

fn usage_usec(directory: &Path) -> Option<f64> {
    read(directory.join("cpu.stat"))?.lines().find_map(|line| line.strip_prefix("usage_usec ")?.parse().ok())
}

fn child_directories(directory: &Path) -> Vec<(String, PathBuf)> {
    let entries = fs::read_dir(directory).into_iter().flatten().flatten();
    entries.filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir())).map(|entry| (entry.file_name().to_string_lossy().into_owned(), entry.path())).collect()
}

fn walk_usage(directory: &Path, relative: &str, depth: usize, out: &mut BTreeMap<String, f64>) {
    if let Some(usec) = usage_usec(directory).filter(|_| !relative.is_empty()) {
        out.insert(relative.to_owned(), usec);
    }
    if depth >= CGROUP_DEPTH {
        return;
    }
    for (name, path) in child_directories(directory) {
        let name = if relative.is_empty() { name } else { format!("{relative}/{name}") };
        walk_usage(&path, &name, depth + 1, out);
    }
}

fn cgroup_usage() -> BTreeMap<String, f64> {
    let mut out = BTreeMap::new();
    walk_usage(&user_root(), "", 0, &mut out);
    walk_usage(Path::new(&format!("{CGROOT}/system.slice")), "system.slice", SYSTEM_SLICE_DEPTH, &mut out);
    let login = format!("user.slice/user-{}.slice", user_id());
    for (name, path) in child_directories(Path::new(&format!("{CGROOT}/{login}"))) {
        if name.starts_with("session-") && name.ends_with(".scope") {
            walk_usage(&path, &format!("{login}/{name}"), CGROUP_DEPTH, &mut out);
        }
    }
    out
}

fn busy_ticks() -> Option<f64> {
    let stat = read("/proc/stat")?;
    let fields: Vec<f64> = stat.lines().next()?.split_whitespace().skip(1).filter_map(|word| word.parse().ok()).collect();
    (fields.len() >= 8).then(|| fields[0] + fields[1] + fields[2] + fields[5] + fields[6] + fields[7])
}

pub(super) fn procs() -> Vec<Proc> {
    fs::read_dir("/proc").into_iter().flatten().flatten().filter_map(|entry| process(&entry.file_name().to_string_lossy())).collect()
}

fn process(name: &str) -> Option<Proc> {
    let pid: i64 = name.parse().ok()?;
    let stat = read(format!("/proc/{name}/stat"))?;
    let close = stat.rfind(')')?;
    let fields: Vec<&str> = stat.get(close + 2..)?.split(' ').collect();
    if fields.len() < 20 || fields[0] == "Z" || fields[1] == "2" || pid == 2 {
        return None;
    }
    let cgroup = read(format!("/proc/{name}/cgroup")).unwrap_or_default().trim().trim_start_matches("0::").to_owned();
    let args = read(format!("/proc/{name}/cmdline")).unwrap_or_default().split('\0').filter(|word| !word.is_empty()).take(MAX_ARGS).map(str::to_owned).collect();
    let number = |index: usize| fields[index].parse::<f64>().unwrap_or(0.0);
    Some(Proc {
        pid,
        ppid: number(1) as i64,
        sid: number(3) as i64,
        comm: stat[stat.find('(')? + 1..close].to_owned(),
        args,
        cgroup,
        ticks: number(11) + number(12),
        start: fields[19].to_owned(),
        rss_kib: number(21) * PAGE_KIB,
        cwd: fs::read_link(format!("/proc/{name}/cwd")).ok().map(|path| path.to_string_lossy().into_owned()),
    })
}

struct FleetSession {
    conversation: String,
    host_pid: Option<i64>,
    parent: Option<String>,
}

fn fleet_sessions(north: &Path) -> Vec<FleetSession> {
    let output = Command::new(north).args(["fleet", "list", "--json"]).stdin(Stdio::null()).stderr(Stdio::null()).output();
    let rows: Vec<Value> = output.ok().filter(|out| out.status.success()).and_then(|out| serde_json::from_slice(&out.stdout).ok()).unwrap_or_default();
    rows.iter()
        .map(|row| FleetSession { conversation: row["conversation"].as_str().unwrap_or_default().to_owned(), host_pid: row["host_pid"].as_i64(), parent: row["parent"].as_str().map(str::to_owned) })
        .collect()
}

fn lease_holder(pid: Option<i64>, by_pid: &HashMap<i64, &Proc>, sessions: &[FleetSession]) -> (Option<String>, Option<String>) {
    let mut chain: Vec<&Proc> = Vec::new();
    let mut next = pid.and_then(|pid| by_pid.get(&pid).copied());
    while let Some(process) = next.filter(|process| !chain.iter().any(|seen| seen.pid == process.pid)) {
        chain.push(process);
        next = by_pid.get(&process.ppid).copied();
    }
    let Some(agent) = chain.iter().find(|process| process.comm == "claude" || process.comm == "codex") else { return (None, None) };
    let session = agent.args.iter().enumerate().find_map(|(index, word)| {
        let id = agent.args.get(index + 1)?;
        (["--session-id", "--resume", "resume"].contains(&word.as_str()) && (32..=36).contains(&id.len()) && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-')).then(|| id.clone())
    });
    let pids: Vec<i64> = chain.iter().map(|process| process.pid).collect();
    let owner = session
        .as_ref()
        .and_then(|id| sessions.iter().find(|candidate| &candidate.conversation == id))
        .or_else(|| sessions.iter().find(|candidate| candidate.host_pid.is_some_and(|host| pids.contains(&host))));
    let named = session.or_else(|| owner.map(|owner| owner.conversation.clone()));
    (named, owner.and_then(|owner| owner.parent.clone()))
}

fn leases(by_pid: &HashMap<i64, &Proc>, sessions: &[FleetSession]) -> Vec<Value> {
    let directory = runtime_dir().join("agent-capacity-v1/leases");
    let mut names: Vec<String> = fs::read_dir(&directory).into_iter().flatten().flatten().map(|entry| entry.file_name().to_string_lossy().into_owned()).filter(|name| name.ends_with(".json")).collect();
    names.sort();
    names
        .iter()
        .filter_map(|name| {
            let lease: Value = serde_json::from_str(&read(directory.join(name))?).ok()?;
            let usage: Option<Value> = read(directory.join(name.replace(".json", ".usage"))).and_then(|text| serde_json::from_str(&text).ok());
            let (session, parent) = lease_holder(lease["wrapperPid"].as_i64(), by_pid, sessions);
            let declared = lease["declared"]["cpus"].clone();
            Some(json!({
                "owner": lease["owner"], "class": lease["class"], "cpus": lease["cpus"],
                "declaredCpus": if declared.is_null() { lease["cpus"].clone() } else { declared },
                "memoryMiB": lease["memoryMiB"],
                "recentCores": usage.as_ref().and_then(|u| u["recentCores"].as_f64()).map(round2),
                "usedMiB": usage.as_ref().and_then(|u| u["memoryMiB"].as_f64()).map(f64::round),
                "session": session, "parent": parent,
            }))
        })
        .collect()
}

pub(super) fn snapshot(north: &Path) -> Snapshot {
    let all = procs();
    let by_pid: HashMap<i64, &Proc> = all.iter().map(|process| (process.pid, process)).collect();
    let sessions = fleet_sessions(north);
    let memory = read("/proc/meminfo").unwrap_or_default();
    let leases = leases(&by_pid, &sessions);
    let kib = |key: &str| memory.lines().find_map(|line| line.strip_prefix(key)?.strip_prefix(':')?.split_whitespace().next()?.parse::<f64>().ok()).unwrap_or(0.0);
    let load = read("/proc/loadavg").unwrap_or_else(|| "0 0 0".to_owned()).split(' ').take(3).map(|word| word.parse().unwrap_or(0.0)).collect();
    Snapshot {
        at: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |age| age.as_millis() as i64),
        load,
        cpu_psi: pressure("/proc/pressure/cpu", "some", "avg10="),
        cpu_psi60: pressure("/proc/pressure/cpu", "some", "avg60="),
        mem_psi: pressure("/proc/pressure/memory", "some", "avg10="),
        mem_full_psi: pressure("/proc/pressure/memory", "full", "avg10="),
        mem_available_mib: (kib("MemAvailable") / 1024.0).round(),
        mem_total_mib: (kib("MemTotal") / 1024.0).round(),
        cgroups: cgroup_usage(),
        busy_ticks: busy_ticks(),
        leases,
        procs: all,
    }
}
