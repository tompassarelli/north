use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

const PREFIX_VARIABLES: [&str; 2] = ["STEAM_COMPAT_DATA_PATH", "WINEPREFIX"];
const CLIENT_COMM: &str = "Warcraft III";
const LAUNCH_COMM: &str = "steam.exe";
const CLIENT_EXECUTABLE: &str = "Warcraft III.exe";

pub fn run() -> u8 {
    println!("{}", count());
    0
}

pub fn count() -> usize {
    clients(&proc_root()).len()
}

/// The cgroup each live client runs in.
pub fn cgroups() -> Vec<String> {
    clients(&proc_root()).into_iter().map(|client| client.cgroup).collect()
}

/// The live clients as `north capacity host status` lists them.
pub fn view() -> Vec<Value> {
    clients(&proc_root())
        .iter()
        .map(|client| json!({ "pid": client.pid, "prefix": client.prefix, "state": client.state.name(), "privateDesktop": client.private_desktop, "cgroup": client.cgroup }))
        .collect()
}

fn proc_root() -> PathBuf {
    std::env::var_os("AGENT_CAPACITY_PROC").unwrap_or_else(|| "/proc".into()).into()
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum State {
    Launching,
    Running,
}

impl State {
    fn name(self) -> &'static str {
        match self {
            State::Launching => "launching",
            State::Running => "running",
        }
    }
}

struct Client {
    pid: u32,
    prefix: Option<String>,
    identity: String,
    state: State,
    private_desktop: bool,
    cgroup: String,
}

/// Live Warcraft III clients: one per Wine prefix, so a client running two processes counts once.
/// A process whose environment this one can't read, such as another pair's client seen from inside a pair agent's user namespace, belongs to the client of its cgroup.
/// A client counts from its Proton launch shim on, so one still loading counts against the limit like one in game.
fn clients(proc_root: &Path) -> Vec<Client> {
    let Ok(entries) = fs::read_dir(proc_root) else {
        return Vec::new();
    };
    let mut by_client: BTreeMap<String, Client> = BTreeMap::new();
    for entry in entries.flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else { continue };
        let Some(client) = inspect(&entry.path(), pid) else { continue };
        let key = client.identity.clone();
        match by_client.get_mut(&key) {
            Some(known) => merge(known, client),
            None => {
                by_client.insert(key, client);
            }
        }
    }
    by_client.into_values().collect()
}

fn merge(known: &mut Client, other: Client) {
    let private_desktop = known.private_desktop || other.private_desktop;
    if other.state > known.state {
        *known = other;
    }
    known.private_desktop = private_desktop;
}

fn inspect(process: &Path, pid: u32) -> Option<Client> {
    let comm = fs::read_to_string(process.join("comm")).ok()?;
    let state = if comm.starts_with(CLIENT_COMM) {
        State::Running
    } else if comm.trim_end() == LAUNCH_COMM && launching(process) {
        State::Launching
    } else {
        return None;
    };
    let environment = fs::read(process.join("environ")).ok();
    let variable = |name: &str| environment.iter().flat_map(|environment| environment.split(|byte| *byte == 0)).find_map(|entry| entry.strip_prefix(format!("{name}=").as_bytes()).filter(|value| !value.is_empty()));
    let cgroup = fs::read_to_string(process.join("cgroup")).ok().and_then(|text| text.lines().next().and_then(|line| line.rsplit(':').next()).map(str::to_owned)).unwrap_or_default();
    let private_desktop = variable("PRIVATE_DESKTOP_RUN").is_some()
        || variable("XDG_RUNTIME_DIR").is_some_and(|value| String::from_utf8_lossy(value).contains("/private-desktop."))
        || cgroup.contains("/private-desktop-client-");
    let prefix = PREFIX_VARIABLES.iter().find_map(|name| variable(name)).map(|value| format!("prefix:{}", String::from_utf8_lossy(value)));
    let identity = match (&prefix, &environment) {
        (Some(prefix), _) => prefix.clone(),
        (None, None) if !cgroup.is_empty() => format!("cgroup:{cgroup}"),
        (None, _) => format!("pid:{pid}"),
    };
    Some(Client { pid, prefix, identity, state, private_desktop, cgroup })
}

fn launching(process: &Path) -> bool {
    fs::read(process.join("cmdline")).is_ok_and(|bytes| String::from_utf8_lossy(&bytes).contains(CLIENT_EXECUTABLE))
}
