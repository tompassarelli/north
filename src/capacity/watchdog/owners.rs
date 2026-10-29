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
    loaded: Option<BTreeMap<String, Owner>>,
}

impl Live {
    pub(super) fn new(home: PathBuf, north: PathBuf) -> Self {
        Self { home, north, loaded: None }
    }

    fn load(&self) -> BTreeMap<String, Owner> {
        let parents = self.parents();
        let mut owners = BTreeMap::new();
        let fleet = self.home.join(".local/state/north/fleet");
        for entry in fs::read_dir(&fleet).into_iter().flatten().flatten() {
            let Ok(unit) = fs::read_to_string(entry.path().join("host.scope")) else { continue };
            let session = entry.file_name().to_string_lossy().into_owned();
            let parent = parents.get(&session).cloned().flatten();
            owners.insert(unit.trim().to_owned(), Owner { session, parent });
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
        owners
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

impl Owners for Live {
    fn owner(&mut self, unit: &str) -> Option<Owner> {
        if self.loaded.is_none() {
            self.loaded = Some(self.load());
        }
        self.loaded.as_ref()?.get(unit).cloned()
    }
}
