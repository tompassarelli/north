//! What holds tasks (processes and threads) on this dispatch host, read when a launch here starves or the host is overloaded: the user slice's and the launcher's own cgroup counts against their limits, the user's process limit, the ten units holding the most tasks, and the session hosts whose session no registry lists live. Only names and numbers are read out.
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const CGROUPS: &str = "/sys/fs/cgroup";
const TOP: usize = 10;
const NAMED: usize = 10;
pub(super) const SLICES: [&str; 2] = ["system.slice", "user.slice"];
pub(super) const DEPTH: usize = 6;

/// Whether a launch failure's cause is a start that ran out of time or was refused a resource on this host.
pub(super) fn starved(cause: &str) -> bool {
    ["os error 11", "did not open its socket", "did not answer a frame"].iter().any(|said| cause.contains(said))
}

pub(super) fn lines(host: &str, live: &BTreeSet<String>) -> Vec<String> {
    let mut lines = Vec::new();
    let uid = own_uid();
    let slice = format!("user.slice/user-{}.slice", uid.map_or_else(|| "unread".to_owned(), |uid| uid.to_string()));
    lines.push(format!("user slice {slice}: {}", counted(&Path::new(CGROUPS).join(&slice))));
    match own_cgroup() {
        Some(own) => {
            let mut dir = Path::new(CGROUPS).join(own.trim_start_matches('/'));
            loop {
                if dir.as_path() == Path::new(CGROUPS) || dir.ends_with("user.slice") {
                    break;
                }
                let name = dir.file_name().map_or_else(String::new, |name| name.to_string_lossy().into_owned());
                lines.push(format!("launcher cgroup {name}: {}", counted(&dir)));
                if name.ends_with(".slice") || !dir.pop() {
                    break;
                }
            }
        }
        None => lines.push("launcher cgroup: unread".to_owned()),
    }
    let limit = process_limit().unwrap_or_else(|| "unread".to_owned());
    lines.push(format!("user process limit: {} tasks of {limit}", uid.map_or_else(|| "unread".to_owned(), |uid| user_tasks(uid).to_string())));
    for (unit, tasks) in busiest() {
        lines.push(format!("unit {unit}: {tasks} tasks"));
    }
    let orphans = orphan_hosts(live);
    let named = orphans.iter().take(NAMED).cloned().collect::<Vec<_>>().join(", ");
    lines.push(format!("session hosts no registry lists live: {}{}", orphans.len(), if named.is_empty() { String::new() } else { format!(" ({named})") }));
    lines.into_iter().map(|line| format!("host {host} tasks: {line}")).collect()
}

fn counted(dir: &Path) -> String {
    let read = |file: &str| std::fs::read_to_string(dir.join(file)).map_or_else(|_| "unread".to_owned(), |text| text.trim().to_owned());
    format!("{} of {}", read("pids.current"), read("pids.max"))
}

fn own_uid() -> Option<u32> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status.lines().find_map(|line| line.strip_prefix("Uid:")?.split_whitespace().next()?.parse().ok())
}

fn own_cgroup() -> Option<String> {
    std::fs::read_to_string("/proc/self/cgroup").ok()?.lines().find_map(|line| line.strip_prefix("0::").map(str::to_owned))
}

fn process_limit() -> Option<String> {
    let limits = std::fs::read_to_string("/proc/self/limits").ok()?;
    let line = limits.lines().find(|line| line.starts_with("Max processes"))?;
    line.trim_start_matches("Max processes").split_whitespace().next().map(str::to_owned)
}

fn processes() -> impl Iterator<Item = PathBuf> {
    std::fs::read_dir("/proc").into_iter().flatten().filter_map(|entry| entry.ok()).filter(|entry| entry.file_name().to_string_lossy().bytes().all(|byte| byte.is_ascii_digit())).map(|entry| entry.path())
}

fn user_tasks(uid: u32) -> usize {
    processes()
        .filter_map(|dir| {
            let status = std::fs::read_to_string(dir.join("status")).ok()?;
            let owner: u32 = status.lines().find_map(|line| line.strip_prefix("Uid:")?.split_whitespace().next()?.parse().ok())?;
            let threads: usize = status.lines().find_map(|line| line.strip_prefix("Threads:")?.trim().parse().ok())?;
            (owner == uid).then_some(threads)
        })
        .sum()
}

fn busiest() -> Vec<(String, u64)> {
    let mut units = Vec::new();
    for slice in SLICES {
        leaves(&Path::new(CGROUPS).join(slice), DEPTH, &mut units);
    }
    let mut rows: Vec<(String, u64)> = units
        .iter()
        .filter_map(|unit| Some((unit.file_name()?.to_string_lossy().into_owned(), std::fs::read_to_string(unit.join("pids.current")).ok()?.trim().parse().ok()?)))
        .collect();
    rows.sort_by_key(|row| std::cmp::Reverse(row.1));
    rows.truncate(TOP);
    rows
}

fn orphan_hosts(live: &BTreeSet<String>) -> Vec<String> {
    let mut names: Vec<String> = processes()
        .filter_map(|dir| {
            let cmdline = std::fs::read(dir.join("cmdline")).ok()?;
            let words: Vec<String> = cmdline.split(|byte| *byte == 0).map(|word| String::from_utf8_lossy(word).into_owned()).collect();
            if words.get(1).map(String::as_str) != Some("session-host") {
                return None;
            }
            let name = words.iter().skip_while(|word| *word != "--name").nth(1)?.clone();
            (!live.contains(&name)).then_some(name)
        })
        .collect();
    names.sort();
    names
}

/// The leaf cgroups under `dir` down to `depth` levels: services, session scopes and transient jobs each one leaf.
pub(super) fn leaves(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let children: Vec<PathBuf> = std::fs::read_dir(dir).into_iter().flatten().filter_map(|entry| entry.ok().map(|entry| entry.path())).filter(|path| path.is_dir()).collect();
    if children.is_empty() || depth == 0 {
        out.push(dir.to_path_buf());
    } else {
        for child in &children {
            leaves(child, depth - 1, out);
        }
    }
}
