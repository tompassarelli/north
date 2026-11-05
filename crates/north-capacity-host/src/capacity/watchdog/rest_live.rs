use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use regex::Regex;

use super::rest::{Observed, Unit};
use super::sampling::{Proc, round2};
use super::snapshot::{CGROOT, procs, read, user_root};

const SEARCH_DEPTH: usize = 8;
const AGENT_PREFIXES: [&str; 3] = ["claude", "codex", "tmux"];

struct Cost {
    usec: f64,
    switches: HashMap<String, f64>,
}

fn agent_named(candidate: &str) -> bool {
    AGENT_PREFIXES.iter().any(|prefix| candidate.starts_with(prefix))
}

fn unit_cgroups() -> HashMap<String, PathBuf> {
    let root = user_root();
    let unit = Regex::new(r"\.(service|scope)$").expect("unit pattern");
    let mut out = HashMap::new();
    let mut pending = vec![(PathBuf::from(CGROOT), 0usize)];
    while let Some((directory, depth)) = pending.pop() {
        for entry in fs::read_dir(&directory).into_iter().flatten().flatten().filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir())) {
            let (path, name) = (entry.path(), entry.file_name().to_string_lossy().into_owned());
            if path == root {
                out.insert(format!("system|{name}"), path.clone());
            } else if unit.is_match(&name) {
                let scope = if path.starts_with(&root) { "user" } else { "system" };
                out.insert(format!("{scope}|{name}"), path.clone());
            }
            if depth < SEARCH_DEPTH {
                pending.push((path, depth + 1));
            }
        }
    }
    out
}

fn switches_of(thread: &str) -> f64 {
    read(format!("/proc/{thread}/status")).unwrap_or_default().lines().filter(|line| line.starts_with("voluntary_ctxt_switches:") || line.starts_with("nonvoluntary_ctxt_switches:")).filter_map(|line| line.split_whitespace().nth(1)?.parse::<f64>().ok()).sum()
}

fn cost(path: &Path) -> Cost {
    let usec = read(path.join("cpu.stat")).and_then(|text| text.lines().find_map(|line| line.strip_prefix("usage_usec ")?.parse().ok())).unwrap_or(0.0);
    let mut switches = HashMap::new();
    let mut pending = vec![path.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for thread in read(directory.join("cgroup.threads")).unwrap_or_default().lines().filter(|line| !line.is_empty()) {
            switches.insert(thread.to_owned(), switches_of(thread));
        }
        pending.extend(fs::read_dir(&directory).into_iter().flatten().flatten().filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir())).map(|entry| entry.path()));
    }
    Cost { usec, switches }
}

fn listed(scope: &str, args: &[&str]) -> Vec<Unit> {
    let mut command = Command::new("systemctl");
    if scope == "user" {
        command.arg("--user");
    }
    command.args(["list-units", "--no-legend", "--plain"]).args(args).stdin(Stdio::null()).stderr(Stdio::null());
    let output = command.output().map(|out| String::from_utf8_lossy(&out.stdout).into_owned()).unwrap_or_default();
    output.lines().filter_map(|line| line.split_whitespace().next()).map(|name| Unit { scope: scope.to_owned(), name: name.to_owned(), cores: None, cs_per_sec: None, agent: None }).collect()
}

fn agent_of(process: &Proc) -> Option<String> {
    if agent_named(&process.comm) {
        return Some(process.comm.clone());
    }
    process.args.iter().take(2).map(|word| word.rsplit('/').next().unwrap_or_default()).find(|word| agent_named(word)).map(str::to_owned)
}

fn sockets() -> Vec<String> {
    let output = Command::new("ss").args(["-Htlnu", "--cgroup"]).stdin(Stdio::null()).stderr(Stdio::null()).output().map(|out| String::from_utf8_lossy(&out.stdout).into_owned()).unwrap_or_default();
    let zone = Regex::new(r"%[^:\]]+").expect("zone pattern");
    let cgroup = Regex::new(r"cgroup:(/\S*)").expect("cgroup pattern");
    output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            let owner = cgroup.captures(line).map_or("-".to_owned(), |found| found[1].rsplit('/').next().filter(|last| !last.is_empty()).unwrap_or("/").to_owned());
            Some(format!("{} {} {owner}", fields.first()?, zone.replace(fields.get(4)?, "")))
        })
        .collect()
}

pub(super) fn observe(seconds: f64) -> Observed {
    let mut units: Vec<Unit> = Vec::new();
    for scope in ["system", "user"] {
        units.extend(listed(scope, &["--type=service,scope", "--state=running"]));
        units.extend(listed(scope, &["--type=timer", "--state=active"]));
    }
    let paths = unit_cgroups();
    let measured: Vec<(usize, PathBuf)> = units.iter().enumerate().filter_map(|(index, unit)| Some((index, paths.get(&format!("{}|{}", unit.scope, unit.name))?.clone()))).collect();
    let leaves: Vec<&(usize, PathBuf)> = measured.iter().filter(|(_, path)| !measured.iter().any(|(_, other)| other != path && other.starts_with(path))).collect();
    let before: Vec<Cost> = leaves.iter().map(|(_, path)| cost(path)).collect();
    let start = Instant::now();
    std::thread::sleep(Duration::from_secs_f64(seconds));
    let elapsed = start.elapsed().as_secs_f64();
    let running = procs();
    for ((index, path), earlier) in leaves.iter().zip(&before) {
        let later = cost(path);
        let unit = &mut units[*index];
        unit.cores = Some(round2((later.usec - earlier.usec).max(0.0) / 1e6 / elapsed));
        let switched: f64 = later.switches.iter().map(|(thread, count)| (count - earlier.switches.get(thread).copied().unwrap_or(0.0)).max(0.0)).sum();
        unit.cs_per_sec = Some(round2(switched / elapsed));
    }
    let root = user_root();
    for (index, path) in measured.iter().filter(|(_, path)| *path != root) {
        let relative = path.strip_prefix(CGROOT).map(|rest| format!("/{}", rest.display())).unwrap_or_default();
        let inside = |process: &&Proc| process.cgroup == relative || process.cgroup.starts_with(&format!("{relative}/"));
        units[*index].agent = running.iter().filter(inside).find_map(agent_of);
    }
    Observed { seconds: round2(elapsed), units, sockets: sockets() }
}
