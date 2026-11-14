use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::Path;

use serde::Serialize;

use super::cpu::{self, CpuRow};
use crate::foundation::clock::Timestamp;

const MIB_KIB: i64 = 1024;
const AGENTS: [&str; 2] = ["claude", "codex"];
const NON_RESIDENT_ARGS: [&str; 7] = ["-p", "--print", "--cloud", "exec", "app-server", "mcp-server", "cloud"];

#[derive(Debug, PartialEq, Serialize)]
pub(super) struct Row {
    pub(super) ts: String,
    pub(super) mem_available_mib: i64,
    pub(super) swap_used_mib: i64,
    pub(super) zram_used_mib: i64,
    pub(super) zram_compressed_mib: i64,
    pub(super) psi_some_avg60: f64,
    pub(super) psi_full_avg60: f64,
    pub(super) agents: usize,
    pub(super) agent_pss_mib: i64,
    #[serde(flatten)]
    pub(super) cpu: CpuRow,
}

struct Process {
    comm: String,
    ppid: i64,
    argv: Vec<String>,
}

pub(super) fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

fn meminfo_kib(proc_root: &Path) -> HashMap<String, i64> {
    read(&proc_root.join("meminfo"))
        .lines()
        .filter_map(|line| {
            let (key, rest) = line.split_once(':')?;
            Some((key.to_owned(), rest.split_whitespace().next()?.parse().ok()?))
        })
        .collect()
}

fn zram(path: &Path) -> (i64, i64) {
    let fields = read(path).split_whitespace().map(|field| field.parse().unwrap_or(0)).collect::<Vec<i64>>();
    if fields.len() < 3 { (0, 0) } else { (fields[0], fields[1]) }
}

fn process(entry: &Path) -> Option<Process> {
    let stat = fs::read_to_string(entry.join("stat")).ok()?;
    let cmdline = fs::read(entry.join("cmdline")).ok()?;
    let mut argv = cmdline.split(|byte| *byte == 0).map(|arg| String::from_utf8_lossy(arg).into_owned()).collect::<Vec<_>>();
    argv.pop();
    let (head, tail) = stat.rsplit_once(')').unwrap_or(("", &stat));
    let comm = head.split_once('(').map_or("", |(_, comm)| comm).to_owned();
    let ppid = tail.split_whitespace().nth(1)?.parse().ok()?;
    Some(Process { comm, ppid, argv })
}

fn process_table(proc_root: &Path) -> BTreeMap<i64, Process> {
    let Ok(entries) = fs::read_dir(proc_root) else {
        return BTreeMap::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.is_empty() || !name.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            Some((name.parse().ok()?, process(&entry.path())?))
        })
        .collect()
}

fn is_agent(info: &Process) -> bool {
    let Some(first) = info.argv.first() else {
        return false;
    };
    let name = first.rsplit('/').next().unwrap_or(first);
    AGENTS.contains(&name)
        && info.comm == name
        && !info.argv[1..].iter().any(|arg| NON_RESIDENT_ARGS.contains(&arg.as_str()))
}

fn resident_agents(table: &BTreeMap<i64, Process>) -> Vec<i64> {
    let agents = table.iter().filter(|(_, info)| is_agent(info)).map(|(pid, _)| *pid).collect::<HashSet<_>>();
    let mut resident = agents
        .iter()
        .copied()
        .filter(|pid| {
            let (mut parent, mut seen) = (table[pid].ppid, HashSet::new());
            while table.contains_key(&parent) && seen.insert(parent) {
                if agents.contains(&parent) {
                    return false;
                }
                parent = table[&parent].ppid;
            }
            true
        })
        .collect::<Vec<_>>();
    resident.sort_unstable();
    resident
}

fn descendants(table: &BTreeMap<i64, Process>, root: i64) -> Vec<i64> {
    let mut children = HashMap::<i64, Vec<i64>>::new();
    for (pid, info) in table {
        children.entry(info.ppid).or_default().push(*pid);
    }
    let (mut tree, mut stack) = (Vec::new(), vec![root]);
    while let Some(pid) = stack.pop() {
        tree.push(pid);
        stack.extend(children.get(&pid).into_iter().flatten());
    }
    tree
}

fn pss_kib(proc_root: &Path, pid: i64) -> i64 {
    read(&proc_root.join(pid.to_string()).join("smaps_rollup"))
        .lines()
        .find_map(|line| line.strip_prefix("Pss:")?.split_whitespace().next()?.parse().ok())
        .unwrap_or(0)
}

pub(super) fn sample(proc_root: &Path, zram_path: &Path, at: f64) -> Row {
    let mem = meminfo_kib(proc_root);
    let pressure = cpu::psi(&read(&proc_root.join("pressure/memory")));
    let avg60 = |kind: &str| pressure.get(kind).and_then(|fields| fields.get("avg60")).and_then(|value| value.parse().ok()).unwrap_or(0.0);
    let (orig, compr) = zram(zram_path);
    let table = process_table(proc_root);
    let agents = resident_agents(&table);
    let total_kib = agents.iter().flat_map(|root| descendants(&table, *root)).map(|pid| pss_kib(proc_root, pid)).sum::<i64>();
    let mem_kib = |key: &str| mem.get(key).copied().unwrap_or(0);
    Row {
        ts: Timestamp::from_unix_seconds_f64(at).iso(),
        mem_available_mib: mem_kib("MemAvailable").div_euclid(MIB_KIB),
        swap_used_mib: (mem_kib("SwapTotal") - mem_kib("SwapFree")).div_euclid(MIB_KIB),
        zram_used_mib: orig / MIB_KIB / MIB_KIB,
        zram_compressed_mib: compr / MIB_KIB / MIB_KIB,
        psi_some_avg60: avg60("some"),
        psi_full_avg60: avg60("full"),
        agents: agents.len(),
        agent_pss_mib: total_kib.div_euclid(MIB_KIB),
        cpu: CpuRow {
            gap_s: None,
            steal_pct: None,
            cpu_stat: cpu::cpu_stat(&read(&proc_root.join("stat"))),
            cpu_psi: cpu::pressure(&read(&proc_root.join("pressure/cpu"))),
            io_psi: cpu::pressure(&read(&proc_root.join("pressure/io"))),
        },
    }
}
