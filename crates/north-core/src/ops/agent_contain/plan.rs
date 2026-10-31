use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

const AGENT_COMMS: [&str; 2] = ["claude", "codex"];

#[derive(Clone, Debug)]
pub(super) struct Info {
    pub(super) comm: String,
    pub(super) ppid: i32,
    pub(super) cgroup: String,
    pub(super) cmd: String,
}

pub(super) type Moves = Vec<(i32, Info)>;
pub(super) type Scopes = Vec<(String, (i32, Info))>;

pub(super) fn processes(proc: &Path) -> BTreeMap<i32, Info> {
    let mut table = BTreeMap::new();
    for entry in fs::read_dir(proc).into_iter().flatten().flatten() {
        let Some(pid) = entry.file_name().to_str().and_then(|name| name.bytes().all(|byte| byte.is_ascii_digit()).then(|| name.parse::<i32>().ok()).flatten()) else {
            continue;
        };
        let read = |name: &str| fs::read(entry.path().join(name)).ok().map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
        let (Some(stat), Some(cgroup)) = (read("stat"), read("cgroup")) else {
            continue;
        };
        let (Some(open), Some(close)) = (stat.find('('), stat.rfind(')')) else {
            continue;
        };
        let comm = stat[open + 1..close].to_owned();
        let Some(ppid) = stat.get(close + 2..).and_then(|rest| rest.split_whitespace().nth(1)).and_then(|field| field.parse().ok()) else {
            continue;
        };
        let path = cgroup.lines().find_map(|line| line.strip_prefix("0::")).unwrap_or("").to_owned();
        let joined = read("cmdline").unwrap_or_default().split('\0').filter(|word| !word.is_empty()).collect::<Vec<_>>().join(" ");
        let cmd = if joined.is_empty() { comm.clone() } else { joined.chars().take(160).collect() };
        table.insert(pid, Info { comm, ppid, cgroup: path, cmd });
    }
    table
}

fn agent_started(pid: i32, table: &BTreeMap<i32, Info>) -> bool {
    let mut seen = BTreeSet::new();
    let mut pid = table[&pid].ppid;
    while let Some(info) = table.get(&pid) {
        if AGENT_COMMS.contains(&info.comm.as_str()) {
            return true;
        }
        if !seen.insert(pid) {
            break;
        }
        pid = info.ppid;
    }
    false
}

pub(super) fn plan(table: &BTreeMap<i32, Info>, uid: u32) -> (Moves, Scopes) {
    let user = format!("/user.slice/user-{uid}.slice/user@{uid}.service/");
    let (mut moves, mut scopes): (Moves, Scopes) = (Vec::new(), Vec::new());
    for (&pid, info) in table {
        let Some(rel) = info.cgroup.strip_prefix(&user) else {
            continue;
        };
        if rel == "agent.slice" || rel.starts_with("agent.slice/") || (rel.starts_with("native") && rel.contains("/agent-capacity-")) {
            continue;
        }
        let parts: Vec<&str> = rel.split('/').collect();
        let pod = parts.iter().position(|part| part.starts_with("libpod-") && part.ends_with(".scope"));
        if let Some(pod) = pod {
            let scope = format!("{user}{}", parts[..=pod].join("/"));
            if !scopes.iter().any(|(known, _)| *known == scope) {
                scopes.push((scope, (pid, info.clone())));
            }
        } else if !AGENT_COMMS.contains(&info.comm.as_str()) && agent_started(pid, table) {
            moves.push((pid, info.clone()));
        }
    }
    (moves, scopes)
}
