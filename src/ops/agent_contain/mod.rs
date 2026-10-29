mod plan;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use self::plan::{Info, Moves, plan, processes};
use crate::foundation::clock::Timestamp;

const USAGE: &str = "usage: north agent-contain sweep [--dry-run]";
const SLICE: &str = "agent-capacity.slice";
const REPEAT_SECONDS: f64 = 3600.0;

pub(crate) struct Inputs {
    pub(crate) proc: PathBuf,
    pub(crate) cgroup: PathBuf,
    pub(crate) inbox: PathBuf,
    pub(crate) uid: u32,
    pub(crate) host: String,
    pub(crate) busctl: String,
    pub(crate) state: PathBuf,
    pub(crate) now: f64,
}

impl Inputs {
    fn from_env() -> Self {
        let set = |name: &str| std::env::var(name).ok().filter(|value| !value.is_empty());
        let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
        Self {
            proc: set("AGENT_CONTAIN_PROC").map_or_else(|| "/proc".into(), PathBuf::from),
            cgroup: set("AGENT_CONTAIN_CGROUP").map_or_else(|| "/sys/fs/cgroup".into(), PathBuf::from),
            inbox: set("AGENT_CONTAIN_INBOX").map_or_else(|| home.join(".local/state/agents/handoffs/proxy-inbox.md"), PathBuf::from),
            uid: set("AGENT_CONTAIN_UID").and_then(|uid| uid.parse().ok()).unwrap_or_else(|| rustix::process::getuid().as_raw()),
            host: set("AGENT_CONTAIN_HOST").unwrap_or_else(crate::ops::laptop::hostname),
            busctl: set("AGENT_CONTAIN_BUSCTL").unwrap_or_else(|| "busctl".into()),
            state: set("AGENT_CONTAIN_STATE").map_or_else(|| home.join(".local/state/agent-contain/reported.json"), PathBuf::from),
            now: Timestamp::now().unix_seconds_f64(),
        }
    }
}

pub(crate) fn sweep_from_env() -> u8 {
    sweep(&Inputs::from_env(), false, &mut std::io::stdout().lock(), &mut || {})
}

pub(crate) fn run(arguments: &[String]) -> u8 {
    match arguments {
        [verb, rest @ ..] if verb == "sweep" && rest.iter().all(|flag| flag == "--dry-run") => {
            let mut out = std::io::stdout().lock();
            sweep(&Inputs::from_env(), !rest.is_empty(), &mut out, &mut || {})
        }
        _ => {
            eprintln!("{USAGE}");
            2
        }
    }
}

fn repr(text: &str) -> String {
    let quote = if text.contains('\'') && !text.contains('"') { '"' } else { '\'' };
    let mut out = String::from(quote);
    for character in text.chars() {
        match character {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if c.is_control() => out.push_str(&format!("\\x{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

fn incident(inputs: &Inputs, action: &str, pid: i32, info: &Info, count: usize) -> String {
    let stamp = Timestamp::from_unix_seconds(inputs.now as i64).iso_minute();
    let more = if count > 1 { format!(" processes={count}") } else { String::new() };
    let line = format!("INCIDENT {stamp} agent-contain host={} action={action} pid={pid} cmd={} cgroup={}{more}\n", inputs.host, repr(&info.cmd), info.cgroup);
    if let Some(parent) = inputs.inbox.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let written = OpenOptions::new().create(true).append(true).open(&inputs.inbox).and_then(|mut file| file.write_all(line.as_bytes()));
    if let Err(error) = written {
        eprintln!("agent-contain: {}: {error}", inputs.inbox.display());
    }
    line
}

fn live(proc: &Path, pids: &[i32]) -> Vec<i32> {
    pids.iter().copied().filter(|pid| proc.join(pid.to_string()).exists()).collect()
}

fn move_into_slice(inputs: &Inputs, pids: &[i32]) -> (bool, Vec<i32>) {
    let pids = live(&inputs.proc, pids);
    let Some(first) = pids.first() else {
        return (true, pids);
    };
    let unit = format!("agent-contain-{first}-{}.scope", inputs.now as i64);
    let mut command = Command::new(&inputs.busctl);
    command.args(["--user", "call", "org.freedesktop.systemd1", "/org/freedesktop/systemd1", "org.freedesktop.systemd1.Manager", "StartTransientUnit", "ssa(sv)a(sa(sv))", &unit, "fail", "2", "PIDs", "au"]);
    command.arg(pids.len().to_string()).args(pids.iter().map(i32::to_string)).args(["Slice", "s", SLICE, "0"]);
    let moved = command.output().is_ok_and(|output| output.status.success());
    (moved, pids)
}

fn freeze(inputs: &Inputs, scopes: &[(String, (i32, Info))], dry: bool, out: &mut dyn Write) {
    for (cg, (pid, info)) in scopes {
        let file = inputs.cgroup.join(cg.trim_start_matches('/')).join("cgroup.freeze");
        if fs::read_to_string(&file).is_ok_and(|text| text.trim() == "1") {
            continue;
        }
        if dry {
            let _ = writeln!(out, "freeze {pid} {cg}");
            continue;
        }
        match fs::write(&file, "1") {
            Ok(()) => {
                let _ = out.write_all(incident(inputs, "frozen", *pid, info, 1).as_bytes());
            }
            Err(error) => eprintln!("agent-contain: cannot freeze {cg}: {error}"),
        }
    }
}

fn report(inputs: &Inputs, moves: &Moves, remaining: &[i32], moved: bool, out: &mut dyn Write) {
    let mut last: BTreeMap<String, f64> = fs::read_to_string(&inputs.state).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
    let mut groups: Vec<(String, Vec<&(i32, Info)>)> = Vec::new();
    for entry in moves.iter().filter(|(pid, _)| remaining.contains(pid)) {
        let key = entry.1.cgroup.split("/agent-contain-").next().unwrap_or_default().to_owned();
        match groups.iter_mut().find(|(known, _)| *known == key) {
            Some((_, members)) => members.push(entry),
            None => groups.push((key, vec![entry])),
        }
    }
    for (cg, members) in groups {
        if inputs.now - last.get(&cg).copied().unwrap_or(0.0) < REPEAT_SECONDS {
            continue;
        }
        let (pid, info) = members[0];
        let action = if moved { "moved" } else { "move-failed" };
        let _ = out.write_all(incident(inputs, action, *pid, info, members.len()).as_bytes());
        last.insert(cg, inputs.now);
    }
    last.retain(|_, at| inputs.now - *at < REPEAT_SECONDS);
    if let Some(parent) = inputs.state.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Err(error) = fs::write(&inputs.state, serde_json::to_string(&last).unwrap_or_default()) {
        eprintln!("agent-contain: {}: {error}", inputs.state.display());
    }
}

pub(crate) fn sweep(inputs: &Inputs, dry: bool, out: &mut dyn Write, after_scan: &mut dyn FnMut()) -> u8 {
    let table = processes(&inputs.proc);
    let (moves, scopes) = plan(&table, inputs.uid);
    after_scan();
    freeze(inputs, &scopes, dry, out);
    if moves.is_empty() {
        return 0;
    }
    if dry {
        for (pid, info) in &moves {
            let _ = writeln!(out, "move {pid} {} {}", info.cgroup, info.cmd);
        }
        return 0;
    }
    let all: Vec<i32> = moves.iter().map(|(pid, _)| *pid).collect();
    let (mut moved, mut pids) = move_into_slice(inputs, &all);
    if !moved {
        (moved, pids) = move_into_slice(inputs, &pids);
    }
    if !moved {
        pids = live(&inputs.proc, &pids);
    }
    report(inputs, &moves, &pids, moved, out);
    0
}
