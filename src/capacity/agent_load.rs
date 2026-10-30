use std::collections::BTreeSet;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rustix::process::{Pid, Signal, kill_process};
use serde::Deserialize;
use serde_json::json;

use super::lease_deadline;

const WISP_DATA: &str = "/.local/share/wisp/";
const STEAM_PREFIXES: &str = "/steamapps/compatdata/";
const GAMING_MARKER: &str = "gaming.json";
const PREFIX_VARIABLES: [&str; 2] = ["STEAM_COMPAT_DATA_PATH", "WINEPREFIX"];
const KEPT_VARIABLES: [&str; 4] = ["XDG_RUNTIME_DIR", "PRIVATE_DESKTOP_RUN", "STEAM_COMPAT_DATA_PATH", "WINEPREFIX"];

pub(crate) struct Host {
    pub(crate) proc: PathBuf,
    pub(crate) cgroup: PathBuf,
    pub(crate) capacity: PathBuf,
    pub(crate) game_reserve: PathBuf,
    pub(crate) systemctl: String,
    pub(crate) uid: u32,
}

impl Host {
    pub(crate) fn from_env() -> Self {
        let uid = rustix::process::getuid().as_raw();
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
        let var = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from);
        Self {
            proc: var("AGENT_LOAD_PROC").unwrap_or_else(|| "/proc".into()),
            cgroup: var("AGENT_LOAD_CGROUP").unwrap_or_else(|| {
                PathBuf::from(format!("/sys/fs/cgroup/user.slice/user-{uid}.slice/user@{uid}.service"))
            }),
            capacity: capacity_root(),
            game_reserve: var("GAME_RESERVE_STATE").unwrap_or_else(|| home.join(".local/state/game-reserve/state.json")),
            systemctl: std::env::var("AGENT_LOAD_SYSTEMCTL").unwrap_or_else(|_| "systemctl".into()),
            uid,
        }
    }
}

pub(crate) fn capacity_root() -> PathBuf {
    let uid = rustix::process::getuid().as_raw();
    std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|value| !value.is_empty())
        .map_or_else(|| PathBuf::from(format!("/run/user/{uid}")), PathBuf::from)
        .join("agent-capacity-v1")
}

#[derive(Clone, Debug)]
pub(crate) struct Process {
    pub(crate) pid: i32,
    pub(crate) comm: String,
    pub(crate) executable: String,
    pub(crate) args: String,
    pub(crate) cgroup: String,
    pub(crate) env: Vec<(String, String)>,
}

impl Process {
    fn var(&self, name: &str) -> Option<&str> {
        self.env.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str())
    }

    fn segments(&self) -> impl Iterator<Item = &str> {
        self.cgroup.split('/')
    }

    fn private_desktop(&self) -> bool {
        self.var("PRIVATE_DESKTOP_RUN").is_some()
            || self.var("XDG_RUNTIME_DIR").is_some_and(|value| value.contains("/private-desktop."))
            || self.cgroup.contains("/private-desktop-client-")
    }

    fn wisp_prefix(&self) -> bool {
        self.args.contains(WISP_DATA) || PREFIX_VARIABLES.iter().any(|name| self.var(name).is_some_and(|value| value.contains(WISP_DATA)))
    }

    pub(crate) fn agent_cgroup(&self) -> bool {
        self.segments().any(|part| {
            part == "agent.slice" || (part.starts_with("native") && part.ends_with(".slice")) || part.starts_with("libpod-")
        })
    }

    pub(crate) fn agent_owned(&self) -> bool {
        self.private_desktop() || self.wisp_prefix() || self.agent_cgroup()
    }

    pub(crate) fn toms(&self) -> bool {
        self.segments().any(|part| part == "game-reserve.slice")
            || (!self.wisp_prefix()
                && PREFIX_VARIABLES.iter().any(|name| self.var(name).is_some_and(|value| value.contains(STEAM_PREFIXES))))
    }

    pub(crate) fn game_like(&self) -> bool {
        let text = format!("{} {}", self.comm, self.executable).to_lowercase();
        ["wine", "warcraft iii", "dolphin", "slippi"].iter().any(|word| text.contains(word)) || has_word(&text, "proton")
    }

    fn in_unit(&self, unit: &str) -> bool {
        self.segments().any(|part| part == unit)
    }
}

fn has_word(text: &str, word: &str) -> bool {
    let boundary = |c: Option<char>| c.is_none_or(|c| !(c.is_alphanumeric() || c == '_'));
    text.match_indices(word).any(|(at, _)| boundary(text[..at].chars().next_back()) && boundary(text[at + word.len()..].chars().next()))
}

pub(crate) fn processes(host: &Host) -> Vec<Process> {
    let Ok(entries) = fs::read_dir(&host.proc) else { return Vec::new() };
    let mut table = entries
        .flatten()
        .filter_map(|entry| {
            let pid = entry.file_name().to_str()?.parse::<i32>().ok()?;
            (entry.metadata().ok()?.uid() == host.uid).then(|| read_process(&entry.path(), pid)).flatten()
        })
        .collect::<Vec<_>>();
    table.sort_by_key(|process| process.pid);
    table
}

fn read_process(dir: &Path, pid: i32) -> Option<Process> {
    let stat = fs::read_to_string(dir.join("stat")).ok()?;
    let comm = stat.get(stat.find('(')? + 1..stat.rfind(')')?)?.to_owned();
    let cgroup = fs::read_to_string(dir.join("cgroup")).ok()?;
    let cgroup = cgroup.lines().find_map(|line| line.strip_prefix("0::")).unwrap_or_default().to_owned();
    let cmdline = fs::read(dir.join("cmdline")).unwrap_or_default();
    let words = cmdline.split(|byte| *byte == 0).filter(|word| !word.is_empty()).map(String::from_utf8_lossy).collect::<Vec<_>>();
    let executable = words.first().map(|word| word.rsplit('/').next().unwrap_or_default().to_owned()).unwrap_or_default();
    let environ = fs::read(dir.join("environ")).unwrap_or_default();
    let env = environ
        .split(|byte| *byte == 0)
        .filter_map(|entry| {
            let entry = String::from_utf8_lossy(entry);
            let (key, value) = entry.split_once('=')?;
            KEPT_VARIABLES.contains(&key).then(|| (key.to_owned(), value.to_owned()))
        })
        .collect();
    Some(Process { pid, comm, executable, args: words.join(" "), cgroup, env })
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Lease {
    pub(crate) id: String,
    pub(crate) kind: String,
    pub(crate) class: String,
    #[serde(default)]
    pub(crate) aggregate_slice: Option<String>,
    #[serde(default)]
    pub(crate) wrapper_pid: Option<i32>,
    #[serde(default)]
    pub(crate) wrapper_start: Option<String>,
}

impl Lease {
    pub(crate) fn unit(&self) -> String {
        format!("agent-capacity-{}.scope", self.id.replace('-', ""))
    }

    fn scope_dir(&self, host: &Host) -> PathBuf {
        let slice = self.aggregate_slice.as_deref().unwrap_or(if self.class == "native" { "native.slice" } else { "agent-capacity.slice" });
        let parts = slice.trim_end_matches(".slice").split('-').collect::<Vec<_>>();
        let mut dir = host.cgroup.clone();
        for index in 0..parts.len() {
            dir.push(format!("{}.slice", parts[..=index].join("-")));
        }
        dir.join(self.unit())
    }
}

pub(crate) fn leases(host: &Host) -> Vec<Lease> {
    let Ok(entries) = fs::read_dir(host.capacity.join("leases")) else { return Vec::new() };
    entries
        .flatten()
        .filter(|entry| entry.path().extension().is_some_and(|extension| extension == "json"))
        .filter_map(|entry| serde_json::from_slice::<Lease>(&fs::read(entry.path()).ok()?).ok())
        .filter(|lease| lease.kind == "run")
        .collect()
}

pub(crate) fn process_start(host: &Host, pid: i32) -> Option<String> {
    let stat = fs::read_to_string(host.proc.join(pid.to_string()).join("stat")).ok()?;
    let fields = stat.get(stat.rfind(')')? + 2..)?.split(' ').collect::<Vec<_>>();
    (fields.first() != Some(&"Z")).then(|| fields.get(19).map(|field| field.to_string())).flatten()
}

#[derive(Debug, Default, PartialEq)]
pub(crate) struct Plan {
    pub(crate) stop: BTreeSet<String>,
    pub(crate) freeze: BTreeSet<String>,
    pub(crate) thaw: BTreeSet<String>,
    pub(crate) kill: BTreeSet<i32>,
}

pub(crate) struct Observed<'a> {
    pub(crate) gaming: bool,
    pub(crate) game_reserve: bool,
    pub(crate) processes: &'a [Process],
    pub(crate) leases: &'a [Lease],
    pub(crate) frozen: &'a BTreeSet<String>,
}

pub(crate) fn plan(observed: &Observed, live: impl Fn(&Lease) -> bool, wrapper_alive: impl Fn(&Lease) -> bool) -> Plan {
    let mut plan = Plan::default();
    for lease in observed.leases.iter().filter(|lease| live(lease)) {
        let unit = lease.unit();
        let vetoed = observed.processes.iter().any(|process| process.in_unit(&unit) && process.toms());
        let orphan = lease.wrapper_start.is_some() && !wrapper_alive(lease);
        if !vetoed && (orphan || (observed.gaming && lease.class == "native")) {
            plan.stop.insert(unit);
        } else if observed.gaming {
            plan.hold_while_gaming(observed, lease, unit);
        }
    }
    if observed.gaming {
        plan.kill = observed
            .processes
            .iter()
            .filter(|process| process.agent_owned() && !process.toms() && (process.game_like() || process.private_desktop()))
            .filter(|process| !plan.stop.iter().any(|unit| process.in_unit(unit)))
            .map(|process| process.pid)
            .collect();
    } else {
        plan.thaw = observed.frozen.clone();
    }
    plan
}

impl Plan {
    fn hold_while_gaming(&mut self, observed: &Observed, lease: &Lease, unit: String) {
        let held = frozen_while_gaming(&lease.class, observed.game_reserve);
        let frozen = observed.frozen.contains(&unit);
        if held && !frozen {
            self.freeze.insert(unit);
        } else if !held && frozen {
            self.thaw.insert(unit);
        }
    }
}

fn frozen_while_gaming(class: &str, game_reserve: bool) -> bool {
    match class {
        "exclusive" | "gpu" => true,
        "heavy" => !game_reserve,
        _ => false,
    }
}

pub(crate) fn reap_orphans(host: &Host, processes: &[Process]) -> Plan {
    let leases = leases(host);
    let observed = Observed { gaming: false, game_reserve: false, processes, leases: &leases, frozen: &BTreeSet::new() };
    let plan = plan(&observed, |lease| lease.scope_dir(host).is_dir(), |lease| wrapper_alive(host, lease));
    plan.stop.iter().for_each(|unit| {
        systemctl_user(host, "stop", unit);
    });
    plan
}

pub(crate) fn reap_orphans_run() -> u8 {
    let host = Host::from_env();
    let plan = reap_orphans(&host, &processes(&host));
    if !plan.stop.is_empty() {
        println!("agent-load: stopped {:?}", plan.stop);
    }
    0
}

fn wrapper_alive(host: &Host, lease: &Lease) -> bool {
    lease.wrapper_pid.and_then(|pid| process_start(host, pid)) == lease.wrapper_start
}

fn systemctl_user(host: &Host, verb: &str, unit: &str) -> bool {
    Command::new(&host.systemctl).args(["--user", verb, unit]).status().is_ok_and(|status| status.success())
}

pub(crate) fn enforce(host: &Host, gaming: bool, game_reserve: bool, processes: &[Process]) -> Plan {
    let leases = leases(host);
    let frozen_path = host.capacity.join("gaming-frozen.json");
    let frozen: BTreeSet<String> = fs::read(&frozen_path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
    let observed = Observed { gaming, game_reserve, processes, leases: &leases, frozen: &frozen };
    let plan = plan(
        &observed,
        |lease| lease.scope_dir(host).is_dir(),
        |lease| wrapper_alive(host, lease),
    );
    let _ = fs::create_dir_all(&host.capacity);
    let (mut unfrozen, mut unthawed) = (BTreeSet::new(), BTreeSet::new());
    for (verb, units) in [("stop", &plan.stop), ("freeze", &plan.freeze), ("thaw", &plan.thaw)] {
        for unit in units {
            let done = systemctl_user(host, verb, unit);
            if !done && verb == "freeze" {
                unfrozen.insert(unit.clone());
            }
            if !done && verb == "thaw" {
                unthawed.insert(unit.clone());
            }
        }
    }
    let live = leases.iter().map(Lease::unit).collect::<BTreeSet<_>>();
    let still: BTreeSet<String> = if gaming {
        frozen.union(&plan.freeze).filter(|unit| live.contains(*unit) && !unfrozen.contains(*unit) && (!plan.thaw.contains(*unit) || unthawed.contains(*unit))).cloned().collect()
    } else {
        unthawed.into_iter().filter(|unit| live.contains(unit)).collect()
    };
    let (held, released): (Vec<&Lease>, Vec<&Lease>) = leases.iter().partition(|lease| still.contains(&lease.unit()));
    let ids = |leases: Vec<&Lease>| leases.into_iter().map(|lease| lease.id.clone()).collect::<BTreeSet<_>>();
    lease_deadline::reconcile(&host.capacity, &ids(held), &ids(released));
    signal_all(&plan.kill);
    let _ = write_atomic(&frozen_path, &serde_json::to_string(&still).unwrap_or_default());
    plan
}

fn signal_all(pids: &BTreeSet<i32>) {
    let send = |pid: i32, signal: Signal| Pid::from_raw(pid).map(|pid| kill_process(pid, signal));
    let running = |pid: &i32| {
        fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| stat.rsplit_once(") ").is_some_and(|(_, rest)| !rest.starts_with('Z')))
    };
    pids.iter().for_each(|pid| {
        let _ = send(*pid, Signal::Term);
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline && pids.iter().any(running) {
        std::thread::sleep(Duration::from_millis(100));
    }
    pids.iter().filter(|pid| running(pid)).for_each(|pid| {
        let _ = send(*pid, Signal::Kill);
    });
}

fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    let temporary = path.with_extension(format!("tmp{}", std::process::id()));
    fs::write(&temporary, text)?;
    fs::rename(temporary, path)
}

fn now_seconds() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs())
}

pub(crate) fn gaming_marked(capacity: &Path) -> bool {
    #[derive(Deserialize)]
    struct Marker {
        gaming: bool,
    }
    fs::read(capacity.join(GAMING_MARKER))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Marker>(&bytes).ok())
        .is_some_and(|marker| marker.gaming)
}

pub(crate) fn declare_gaming(capacity: &Path, gaming: bool) -> std::io::Result<()> {
    fs::create_dir_all(capacity)?;
    write_atomic(&capacity.join(GAMING_MARKER), &json!({ "gaming": gaming, "at": now_seconds() }).to_string())
}

pub(crate) fn gaming_declared(host: &Host) -> bool {
    gaming_marked(&host.capacity) || game_reserve_on(host)
}

pub(crate) fn game_reserve_on(host: &Host) -> bool {
    let booted = fs::read_to_string(host.proc.join("stat")).ok().and_then(|stat| {
        stat.lines().find_map(|line| line.strip_prefix("btime ")).and_then(|value| value.trim().parse::<u64>().ok())
    });
    fs::read(&host.game_reserve)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .is_some_and(|state| state["on"] == true && booted.is_none_or(|booted| state["since"].as_u64().is_none_or(|since| since >= booted)))
}
