use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

use super::detect::{Owner, Owners};

const SESSION_PREFIX: &str = "north-session-";

pub(super) struct Live {
    home: PathBuf,
    north: PathBuf,
    loaded: Option<Loaded>,
}

struct Loaded {
    owners: BTreeMap<String, Owner>,
    parents: BTreeMap<String, Option<String>>,
}

struct Socket {
    local: String,
    peer: String,
    pids: Vec<u32>,
}

impl Live {
    pub(super) fn new(home: PathBuf, north: PathBuf) -> Self {
        Self { home, north, loaded: None }
    }

    fn load(&self) -> Loaded {
        let parents = self.parents();
        let mut owners = BTreeMap::new();
        let fleet = self.home.join(".local/state/north/fleet");
        for entry in fs::read_dir(&fleet).into_iter().flatten().flatten() {
            let Ok(unit) = fs::read_to_string(entry.path().join("host.scope")) else { continue };
            let session = entry.file_name().to_string_lossy().into_owned();
            let parent = parents.get(&session).cloned().flatten();
            let owner = Owner { session, parent };
            for prior in crate::fleet::detached::Prior::all(&entry.path()) {
                owners.insert(prior.unit, owner.clone());
            }
            owners.insert(unit.trim().to_owned(), owner);
        }
        let runtime = std::env::var_os("XDG_RUNTIME_DIR").map_or_else(|| PathBuf::from("/run/user/1000"), PathBuf::from);
        for entry in fs::read_dir(runtime.join("agent-capacity-v1/leases")).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|extension| extension != "json") {
                continue;
            }
            let Some(lease) = fs::read_to_string(&path).ok().and_then(|text| serde_json::from_str::<Value>(&text).ok()) else { continue };
            let (Some(id), Some(pid)) = (lease["id"].as_str(), lease["wrapperPid"].as_u64()) else { continue };
            if let Some(owner) = session_of(pid).and_then(|unit| owners.get(&unit)).cloned() {
                owners.insert(format!("agent-capacity-{}.scope", id.replace('-', "")), owner);
            }
        }
        let connected = sockets();
        for (unit, owner) in ssh_owners(&connected, &owners, &|pid| unit_of(pid, ".scope")) {
            owners.insert(unit, owner);
        }
        Loaded { owners, parents }
    }

    fn parents(&self) -> BTreeMap<String, Option<String>> {
        let listed = Command::new(&self.north).args(["fleet", "list", "--json"]).output().ok().filter(|output| output.status.success());
        let rows = listed.and_then(|output| serde_json::from_slice::<Value>(&output.stdout).ok()).and_then(|rows| rows.as_array().cloned()).unwrap_or_default();
        rows.iter().filter_map(|row| Some((row["name"].as_str()?.to_owned(), row["parent"].as_str().map(str::to_owned)))).collect()
    }
}

fn session_of(pid: u64) -> Option<String> {
    let cgroup = fs::read_to_string(format!("/proc/{pid}/cgroup")).ok()?;
    cgroup.split('/').find(|segment| segment.starts_with(SESSION_PREFIX) && segment.trim_end().ends_with(".scope")).map(|segment| segment.trim_end().to_owned())
}

fn unit_of(pid: u32, suffix: &str) -> Option<String> {
    let cgroup = fs::read_to_string(format!("/proc/{pid}/cgroup")).ok()?;
    cgroup.trim_end().rsplit('/').next().filter(|unit| unit.ends_with(suffix)).map(str::to_owned)
}

fn sockets() -> Vec<Socket> {
    let listed = Command::new("ss").args(["-H", "-t", "-n", "-p", "state", "established"]).output().ok().filter(|output| output.status.success());
    listed.map_or_else(Vec::new, |output| String::from_utf8_lossy(&output.stdout).lines().filter_map(socket).collect())
}

fn socket(line: &str) -> Option<Socket> {
    let mut columns = line.split_whitespace();
    let (local, peer) = (columns.nth(2)?.to_owned(), columns.next()?.to_owned());
    let pids = line.split("pid=").skip(1).filter_map(|rest| rest.split(|character: char| !character.is_ascii_digit()).next()?.parse().ok()).collect();
    Some(Socket { local, peer, pids })
}

fn ssh_owners(connected: &[Socket], owners: &BTreeMap<String, Owner>, unit_of_pid: &dyn Fn(u32) -> Option<String>) -> Vec<(String, Owner)> {
    connected
        .iter()
        .flat_map(|server| {
            let scope = server.pids.iter().find_map(|pid| unit_of_pid(*pid).filter(|unit| unit.starts_with("session-")));
            let client = connected.iter().filter(|other| other.local == server.peer && other.peer == server.local).flat_map(|other| other.pids.iter()).find_map(|pid| owners.get(&unit_of_pid(*pid)?).cloned());
            scope.zip(client)
        })
        .collect()
}

impl Live {
    fn loaded(&mut self) -> &Loaded {
        if self.loaded.is_none() {
            self.loaded = Some(self.load());
        }
        self.loaded.as_ref().expect("loaded above")
    }
}

impl Owners for Live {
    fn owner(&mut self, unit: &str) -> Option<Owner> {
        self.loaded().owners.get(unit).cloned()
    }

    fn parent(&mut self, session: &str) -> Option<String> {
        self.loaded().parents.get(session).cloned().flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ssh_scope_is_owned_by_the_session_of_the_ssh_client_on_the_other_end_of_its_connection() {
        let line = "0 0 127.0.0.1:22 127.0.0.1:51514 users:((\"sshd-session\",pid=900,fd=4),(\"sshd-session\",pid=901,fd=4))";
        let server = socket(line).unwrap();
        assert_eq!((server.local.as_str(), server.peer.as_str(), server.pids.as_slice()), ("127.0.0.1:22", "127.0.0.1:51514", [900, 901].as_slice()));
        let client = socket("0 0 127.0.0.1:51514 127.0.0.1:22 users:((\"ssh\",pid=77,fd=3))").unwrap();
        let owners = BTreeMap::from([("north-session-a.scope".to_owned(), Owner { session: "sc-extract".into(), parent: None })]);
        let units = |pid: u32| Some(if pid == 77 { "north-session-a.scope" } else { "session-12.scope" }.to_owned());
        let owned = ssh_owners(&[server, client], &owners, &units);
        assert_eq!(owned, [("session-12.scope".to_owned(), Owner { session: "sc-extract".into(), parent: None })]);
    }
}
