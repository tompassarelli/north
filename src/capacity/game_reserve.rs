use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use rustix::process::{CpuSet, Pid, sched_getaffinity, sched_setaffinity};
use serde_json::{Value, json};

const USAGE: &str = "usage: north capacity game-reserve on [--auto] | off | status";
const GAME_SLICE: &str = "game-reserve.slice";
const GAME_ROOT: &str = "Warcraft III.exe";
const SETTLED_SECONDS: f64 = 60.0;
// /proc/PID/stat starttime is in USER_HZ clock ticks, which Linux fixes at 100 for userspace.
const TICKS_PER_SECOND: f64 = 100.0;

pub(crate) struct Settings {
    pub(crate) proc: PathBuf,
    pub(crate) state: PathBuf,
    pub(crate) systemctl: String,
    pub(crate) busctl: String,
    pub(crate) affinity_log: Option<PathBuf>,
    pub(crate) game_cpus: String,
    pub(crate) agent_cpus: String,
    pub(crate) agent_memory_max: String,
}

impl Settings {
    pub(crate) fn from_env() -> Self {
        let var = |name: &str| std::env::var(name).ok().filter(|value| !value.is_empty());
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
        Self {
            proc: var("GAME_RESERVE_PROC").map_or_else(|| "/proc".into(), PathBuf::from),
            state: var("GAME_RESERVE_STATE").map_or_else(|| home.join(".local/state/game-reserve/state.json"), PathBuf::from),
            systemctl: var("GAME_RESERVE_SYSTEMCTL").unwrap_or_else(|| "systemctl".into()),
            busctl: var("GAME_RESERVE_BUSCTL").unwrap_or_else(|| "busctl".into()),
            affinity_log: var("GAME_RESERVE_AFFINITY_LOG").map(PathBuf::from),
            game_cpus: var("GAME_RESERVE_GAME_CPUS").unwrap_or_else(|| "0-3,12-15".into()),
            agent_cpus: var("GAME_RESERVE_AGENT_CPUS").unwrap_or_else(|| "4-11,16-23".into()),
            agent_memory_max: var("GAME_RESERVE_AGENT_MEMORY_MAX").unwrap_or_else(|| "80G".into()),
        }
    }

    pub(crate) fn agent_cpu_count(&self) -> u32 {
        u32::try_from(cpu_set(&self.agent_cpus).len()).unwrap_or(u32::MAX)
    }

    fn game_properties(&self) -> Vec<String> {
        [format!("AllowedCPUs={}", self.game_cpus), "CPUWeight=1000".into(), "IOWeight=1000".into(), "MemoryMin=4G".into(), "MemoryLow=6G".into()]
            .into()
    }
}

#[derive(Debug)]
pub(crate) struct Entry {
    pub(crate) ppid: i32,
    pub(crate) cgroup: String,
    pub(crate) args: String,
    pub(crate) age: Option<f64>,
}

impl Entry {
    fn outside(&self) -> bool {
        !self.cgroup.contains("/native.slice/") && !self.cgroup.contains("/agent.slice/") && !self.cgroup.contains("libpod")
    }

    fn placed(&self) -> bool {
        self.cgroup.split('/').any(|part| part == GAME_SLICE)
    }

    fn agent(&self) -> bool {
        self.cgroup.contains("/agent.slice/") || self.cgroup.contains("/native.slice/")
    }
}

pub(crate) fn table(proc: &Path) -> BTreeMap<i32, Entry> {
    let uptime = fs::read_to_string(proc.join("uptime")).ok().and_then(|text| text.split_whitespace().next()?.parse::<f64>().ok());
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
            let cgroup = cgroup.lines().find_map(|line| line.strip_prefix("0::")).unwrap_or_default().to_owned();
            let args = String::from_utf8_lossy(&cmdline).replace('\0', " ");
            let started = fields.get(19).and_then(|ticks| ticks.trim().parse::<f64>().ok());
            let age = uptime.zip(started).map(|(up, ticks)| up - ticks / TICKS_PER_SECOND);
            Some((pid, Entry { ppid, cgroup, args, age }))
        })
        .collect()
}

pub(crate) fn game_tree(table: &BTreeMap<i32, Entry>) -> Vec<i32> {
    let mut children: BTreeMap<i32, Vec<i32>> = BTreeMap::new();
    for (pid, entry) in table {
        children.entry(entry.ppid).or_default().push(*pid);
    }
    let mut stack = table
        .iter()
        .filter(|(_, entry)| entry.outside() && entry.args.contains(GAME_ROOT) && entry.age.is_some_and(|age| age >= SETTLED_SECONDS))
        .map(|(pid, _)| *pid)
        .collect::<Vec<_>>();
    let mut tree = BTreeSet::new();
    while let Some(pid) = stack.pop() {
        if tree.insert(pid) {
            stack.extend(children.get(&pid).into_iter().flatten());
        }
    }
    tree.into_iter().filter(|pid| table.get(pid).is_some_and(|entry| entry.outside() && !entry.placed())).collect()
}

fn cpu_set(spec: &str) -> BTreeSet<usize> {
    spec.split(',')
        .filter_map(|part| {
            let (lo, hi) = part.split_once('-').unwrap_or((part, part));
            Some(lo.trim().parse::<usize>().ok()?..=hi.trim().parse::<usize>().ok()?)
        })
        .flatten()
        .collect()
}

fn pin(settings: &Settings, pids: &[i32], cpus: &BTreeSet<usize>) -> usize {
    let (Some(low), Some(high)) = (cpus.first(), cpus.last()) else { return 0 };
    if let Some(log) = &settings.affinity_log {
        let lines = pids.iter().map(|pid| format!("pin {pid} {low}-{high}:{}\n", cpus.len())).collect::<String>();
        let appended = fs::OpenOptions::new().create(true).append(true).open(log).and_then(|mut file| file.write_all(lines.as_bytes()));
        return if appended.is_ok() { pids.len() } else { 0 };
    }
    let mut wanted = CpuSet::new();
    cpus.iter().filter(|cpu| **cpu < CpuSet::MAX_CPU).for_each(|cpu| wanted.set(*cpu));
    let same = |current: &CpuSet| (0..CpuSet::MAX_CPU).all(|cpu| current.is_set(cpu) == wanted.is_set(cpu));
    let mut changed = 0;
    for pid in pids {
        let Ok(tasks) = fs::read_dir(settings.proc.join(pid.to_string()).join("task")) else { continue };
        for tid in tasks.flatten().filter_map(|task| task.file_name().to_str()?.parse::<i32>().ok()).filter_map(Pid::from_raw) {
            if sched_getaffinity(Some(tid)).is_ok_and(|current| !same(&current)) && sched_setaffinity(Some(tid), &wanted).is_ok() {
                changed += 1;
            }
        }
    }
    changed
}

fn call(program: &str, arguments: &[String]) -> Result<Output, String> {
    Command::new(program).args(arguments).output().map_err(|error| format!("{program}: {error}"))
}

fn checked(program: &str, arguments: &[String]) -> Result<(), String> {
    let output = call(program, arguments)?;
    if output.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&output.stderr).trim().to_owned()) }
}

fn systemctl(settings: &Settings, arguments: &[String]) -> Result<(), String> {
    let mut all = vec!["--user".to_owned()];
    all.extend_from_slice(arguments);
    checked(&settings.systemctl, &all)
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs())
}

fn move_game(settings: &Settings, pids: &[i32]) -> Result<(), String> {
    let unit = format!("game-reserve-{}-{}.scope", now(), pids[0]);
    let mut arguments = [
        "--user", "call", "org.freedesktop.systemd1", "/org/freedesktop/systemd1", "org.freedesktop.systemd1.Manager", "StartTransientUnit",
        "ssa(sv)a(sa(sv))", &unit, "fail", "2", "PIDs", "au",
    ]
    .map(String::from)
    .to_vec();
    arguments.push(pids.len().to_string());
    arguments.extend(pids.iter().map(i32::to_string));
    arguments.extend(["Slice", "s", GAME_SLICE, "0"].map(String::from));
    checked(&settings.busctl, &arguments).map_err(|error| format!("moving {} game processes failed: {error}", pids.len()))
}

fn read_state(path: &Path) -> Value {
    fs::read(path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or(Value::Null)
}

fn write_state(path: &Path, state: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    fs::write(path, state.to_string()).map_err(|error| format!("{}: {error}", path.display()))
}

pub(crate) fn on(settings: &Settings, auto: bool) -> Result<String, String> {
    systemctl(
        settings,
        &["set-property", "--runtime", "agent.slice", &format!("AllowedCPUs={}", settings.agent_cpus), &format!("MemoryMax={}", settings.agent_memory_max)]
            .map(String::from),
    )
    .map_err(|error| format!("confining agent.slice failed: {error}"))?;
    let table = table(&settings.proc);
    let pids = game_tree(&table);
    if !pids.is_empty() {
        move_game(settings, &pids)?;
    }
    let mut game = table.iter().filter(|(_, entry)| entry.placed()).map(|(pid, _)| *pid).collect::<Vec<_>>();
    game.extend(&pids);
    let agents = table.iter().filter(|(_, entry)| entry.agent()).map(|(pid, _)| *pid).collect::<Vec<_>>();
    let pinned = pin(settings, &game, &cpu_set(&settings.game_cpus)) + pin(settings, &agents, &cpu_set(&settings.agent_cpus));
    let mut reserve = vec!["set-property".to_owned(), "--runtime".to_owned(), GAME_SLICE.to_owned()];
    reserve.extend(settings.game_properties());
    if let Err(error) = systemctl(settings, &reserve)
        && !pids.is_empty()
    {
        return Err(format!("reserving {GAME_SLICE} failed: {error}"));
    }
    let previous = read_state(&settings.state);
    let was_on = previous["on"] == true;
    let state = json!({
        "on": true,
        "auto": if was_on { previous.get("auto").cloned().unwrap_or(Value::Bool(auto)) } else { Value::Bool(auto) },
        "since": if was_on { previous.get("since").cloned().unwrap_or(Value::Null) } else { json!(now()) },
    });
    write_state(&settings.state, &state)?;
    Ok(format!(
        "game-reserve on: game CPUs {}, agents CPUs {}; moved {} game processes, pinned {pinned} threads",
        settings.game_cpus,
        settings.agent_cpus,
        pids.len()
    ))
}

pub(crate) fn off(settings: &Settings) -> Result<String, String> {
    let table = table(&settings.proc);
    let every = cpu_set(&settings.game_cpus).union(&cpu_set(&settings.agent_cpus)).copied().collect();
    let members = table.iter().filter(|(_, entry)| entry.agent() || entry.placed()).map(|(pid, _)| *pid).collect::<Vec<_>>();
    pin(settings, &members, &every);
    systemctl(settings, &["set-property", "--runtime", "agent.slice", "AllowedCPUs=", "MemoryMax=infinity"].map(String::from))
        .map_err(|error| format!("releasing agent.slice failed: {error}"))?;
    write_state(&settings.state, &json!({ "on": false }))?;
    Ok("game-reserve off: agents may use every CPU".to_owned())
}

pub(crate) fn status(settings: &Settings) -> (bool, String) {
    let current = read_state(&settings.state);
    let table = table(&settings.proc);
    let games = table.values().filter(|entry| entry.args.contains(GAME_ROOT) && !entry.cgroup.contains("/native.slice/")).collect::<Vec<_>>();
    let placed = games.iter().filter(|entry| entry.placed()).count();
    let place = if games.is_empty() { "Warcraft III not running".to_owned() } else { format!("Warcraft III {placed}/{} in {GAME_SLICE}", games.len()) };
    if current["on"] == true {
        let auto = if current["auto"] == true { " (auto)" } else { "" };
        return (true, format!("game-reserve on{auto}: game CPUs {}, agents CPUs {}; {place}", settings.game_cpus, settings.agent_cpus));
    }
    (false, format!("game-reserve off; {place}"))
}

pub(crate) fn run(arguments: &[String]) -> u8 {
    let settings = Settings::from_env();
    let outcome = match arguments.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["on"] => on(&settings, false),
        ["on", "--auto"] => on(&settings, true),
        ["off"] => off(&settings),
        ["status"] => {
            let (live, line) = status(&settings);
            println!("{line}");
            return u8::from(!live);
        }
        _ => {
            eprintln!("{USAGE}");
            return 2;
        }
    };
    match outcome {
        Ok(line) => {
            println!("{line}");
            0
        }
        Err(message) => {
            eprintln!("game-reserve: {message}");
            1
        }
    }
}
