use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

use super::probes::SILENT_SECS;
use super::{CapacityView, CloudSession, CloudView, HostView, RuntimeView, SessionView, SubagentView, Ticket, WaitView, WatchContext};
use crate::capacity::admission::default_mib;
use crate::fleet::waits::Waits;

const SUBAGENT_HORIZON_SECS: u64 = 6 * 3600;

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs())
}

fn epoch(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs())
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

fn state() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME").map_or_else(|| home().join(".local/state"), PathBuf::from).join("north")
}

fn north() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("north"))
}

fn bounded(seconds: u32, program: &Path, arguments: &[&str]) -> Option<Output> {
    Command::new("timeout").arg(seconds.to_string()).arg(program).args(arguments).stdin(std::process::Stdio::null()).output().ok()
}

pub(super) fn repo_of(cwd: &str) -> Option<String> {
    let rest = cwd.split_once(":").map_or(cwd, |(_, path)| path);
    let after = rest.split_once("/code/")?.1;
    after.split('/').next().filter(|name| !name.is_empty()).map(str::to_owned)
}

struct Since {
    path: PathBuf,
    marks: BTreeMap<String, u64>,
    seen: BTreeSet<String>,
}

impl Since {
    fn load() -> Self {
        let path = state().join("watch/probe-since.json");
        let marks = fs::read(&path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
        Self { path, marks, seen: BTreeSet::new() }
    }

    fn mark(&mut self, key: &str, holds: bool, now: u64) -> Option<u64> {
        if !holds {
            return None;
        }
        self.seen.insert(key.to_owned());
        Some(*self.marks.entry(key.to_owned()).or_insert(now))
    }

    fn save(mut self) {
        let seen = std::mem::take(&mut self.seen);
        self.marks.retain(|key, _| seen.contains(key));
        if let Some(dir) = self.path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        let _ = serde_json::to_vec(&self.marks).map(|bytes| fs::write(&self.path, bytes));
    }
}

pub(super) fn context() -> WatchContext {
    let now = now();
    let mut since = Since::load();
    let fleet = fleet_list();
    let (hosts, cloud, subagents, runtime) = std::thread::scope(|scope| {
        let hosts = scope.spawn(host_reads);
        let cloud = scope.spawn(cloud);
        let subagents = scope.spawn(|| subagents(now, &fleet));
        let runtime = scope.spawn(|| runtime(&state(), &home().join("code/north/main")));
        (hosts.join().unwrap_or_default(), cloud.join().unwrap_or_default(), subagents.join().unwrap_or_default(), runtime.join().unwrap_or_default())
    });
    let hosts = hosts.into_iter().map(|read| read.view(now, &mut since)).collect();
    since.save();
    WatchContext { now, hosts, sessions: sessions(&fleet), cloud, subagents, waits: waits(&fleet), runtime }
}

struct HostRead {
    name: String,
    local: bool,
    reachable: bool,
    status: Option<Value>,
    facts: Option<String>,
}

impl HostRead {
    fn view(self, now: u64, since: &mut Since) -> HostView {
        let down_since = since.mark(&format!("host:{}:down", self.name), !self.reachable, now);
        let waste = self.facts.as_deref().map(|facts| super::host_facts::parse(facts, self.status.as_ref()));
        let capacity = self.status.map(|status| {
            let mut view = capacity_view(&status, now);
            view.pressure_since = since.mark(&format!("host:{}:pressure", self.name), view.cpu_some_avg10 > view.pressure_target && !view.queued.is_empty(), now);
            view
        });
        HostView { name: self.name, local: self.local, down_since, capacity, waste }
    }
}

pub(super) fn capacity_view(status: &Value, now: u64) -> CapacityView {
    let queued = status["queued"].as_array().into_iter().flatten().map(|ticket| Ticket {
        owner: ticket["owner"].as_str().unwrap_or("unknown").to_owned(),
        class: ticket["class"].as_str().unwrap_or("unknown").to_owned(),
        queued_since: now.saturating_sub(ticket["waitingSeconds"].as_f64().unwrap_or(0.0) as u64),
        wait_p90_secs: ticket["waitP90Seconds"].as_f64().map(|seconds| seconds as u64),
    });
    let target = if status["profile"].as_str() == Some("unattended") { "admit_psi_unattended_high" } else { "admit_psi_attended_high" };
    CapacityView { queued: queued.collect(), cpu_some_avg10: status["cpuSomeAvg10"].as_f64().unwrap_or(0.0), pressure_target: default_mib(target), pressure_since: None }
}

pub(super) fn remote_hosts(orchestration: &str, local: &str) -> Vec<(String, String)> {
    let Ok(document) = orchestration.parse::<toml_edit::DocumentMut>() else { return Vec::new() };
    let Some(hosts) = document.get("hosts").and_then(|hosts| hosts.as_table_like()) else { return Vec::new() };
    hosts
        .iter()
        .filter(|(name, _)| *name != local)
        .filter_map(|(name, table)| {
            let destination = table.get("ssh").or_else(|| table.get("deploy_ssh")).and_then(|value| value.as_str())?;
            Some((name.to_owned(), destination.to_owned()))
        })
        .collect()
}

fn host_reads() -> Vec<HostRead> {
    let local = fs::read_to_string("/etc/hostname").map(|name| name.trim().to_owned()).unwrap_or_default();
    let operator = std::env::var_os("AGENTS_OPERATOR_DIR").map_or_else(|| home().join("code/south/main/agents"), PathBuf::from);
    let remote = remote_hosts(&fs::read_to_string(operator.join("orchestration.toml")).unwrap_or_default(), &local);
    std::thread::scope(|scope| {
        let here = scope.spawn(|| {
            let status = bounded(20, &north(), &["capacity", "host", "status"]).filter(|output| output.status.success()).and_then(|output| serde_json::from_slice(&output.stdout).ok());
            HostRead { name: local.clone(), local: true, reachable: true, status, facts: super::host_facts::read(None) }
        });
        let there: Vec<_> = remote.iter().map(|(name, destination)| scope.spawn(move || ssh_read(name, destination))).collect();
        std::iter::once(here).chain(there).filter_map(|handle| handle.join().ok()).collect()
    })
}

fn ssh_read(name: &str, destination: &str) -> HostRead {
    let output = bounded(20, Path::new("ssh"), &["-o", "ConnectTimeout=5", "-o", "BatchMode=yes", destination, "north", "capacity", "host", "status"]);
    let code = output.as_ref().and_then(|output| output.status.code());
    let reachable = !matches!(code, None | Some(255 | 124));
    let status = output.filter(|output| output.status.success()).and_then(|output| serde_json::from_slice(&output.stdout).ok());
    let facts = if reachable { super::host_facts::read(Some(destination)) } else { None };
    HostRead { name: name.to_owned(), local: false, reachable, status, facts }
}

fn fleet_list() -> Vec<Value> {
    bounded(30, &north(), &["fleet", "list", "--json"]).and_then(|output| serde_json::from_slice(&output.stdout).ok()).unwrap_or_default()
}

fn text(value: &Value, key: &str) -> Option<String> {
    value[key].as_str().map(str::to_owned)
}

fn newest_stream(host_dir: &Path) -> Option<u64> {
    let generations = fs::read_dir(host_dir).ok()?.flatten();
    generations.filter_map(|generation| fs::metadata(generation.path().join("stream.jsonl")).and_then(|meta| meta.modified()).ok()).map(epoch).max()
}

fn sessions(fleet: &[Value]) -> Vec<SessionView> {
    let root = state().join("fleet");
    let waits = Waits::in_root(&root).all();
    fleet
        .iter()
        .filter(|entry| entry["status"].as_str() == Some("working"))
        .filter_map(|entry| {
            let name = text(entry, "name")?;
            let record: Value = fs::read(root.join(&name).join("session.json")).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
            let host_dir = record["host_dir"].as_str().map_or_else(|| root.join(&name), PathBuf::from);
            let started = record["addressed_since_ms"].as_u64().or_else(|| entry["created_ms"].as_u64()).unwrap_or(0) / 1000;
            let waiting = entry["waiting"].as_array().is_some_and(|waiting| !waiting.is_empty()) || waits.contains_key(&name);
            Some(SessionView { repo: repo_of(entry["cwd"].as_str().unwrap_or("")), parent: text(entry, "parent"), status: "working".into(), working_since: started, last_event: newest_stream(&host_dir), eta_min: record["eta_min"].as_u64(), waiting, name })
        })
        .collect()
}

fn waits(fleet: &[Value]) -> Vec<WaitView> {
    let live: BTreeMap<String, &Value> = fleet.iter().filter(|entry| entry["status"].as_str() != Some("exited")).filter_map(|entry| Some((text(entry, "name")?, entry))).collect();
    let all = Waits::in_root(&state().join("fleet")).all();
    all.into_iter()
        .filter(|(session, _)| live.contains_key(session))
        .flat_map(|(session, registered)| registered.into_iter().map(move |wait| WaitView { repo: repo_of(&wait.cwd.to_string_lossy()), run: wait.run.to_string(), since: wait.since_ms / 1000, session: session.clone() }))
        .collect()
}

pub(super) fn cloud_rows(listing: &str) -> Vec<CloudSession> {
    listing
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            let [id, status, _, updated, title, ..] = fields[..] else { return None };
            let updated = crate::foundation::clock::Timestamp::parse_rfc3339(updated)?.unix_seconds();
            Some(CloudSession { id: id.into(), status: status.into(), updated: u64::try_from(updated).ok()?, title: title.into() })
        })
        .collect()
}

fn cloud() -> CloudView {
    match bounded(30, &north(), &["cloud", "list"]) {
        Some(output) if output.status.success() => CloudView { sessions: cloud_rows(&String::from_utf8_lossy(&output.stdout)), failure: None },
        Some(output) => {
            let error = String::from_utf8_lossy(&output.stderr);
            let first = error.lines().next().unwrap_or("no output");
            let cut = first.find(": {").map_or(first, |end| &first[..end]);
            CloudView { sessions: Vec::new(), failure: Some(cut.chars().take(120).collect()) }
        }
        None => CloudView { sessions: Vec::new(), failure: Some("could not run".into()) },
    }
}

fn subagents(now: u64, fleet: &[Value]) -> Vec<SubagentView> {
    let owners: BTreeMap<String, String> = fleet.iter().filter_map(|entry| Some((text(entry, "conversation")?, text(entry, "name")?))).collect();
    let uid = rustix::process::getuid().as_raw();
    let scratch = PathBuf::from(format!("/tmp/claude-{uid}"));
    let candidates = super::shells::candidates(&state().join("profiles"), &scratch, now, SUBAGENT_HORIZON_SECS, SILENT_SECS);
    let open = if candidates.is_empty() { BTreeSet::new() } else { super::shells::open_files(&scratch) };
    candidates
        .into_iter()
        .filter_map(|candidate| {
            let finished = candidate.shells.iter().filter(|shell| !open.contains(&shell.path)).map(|shell| shell.modified).max()?;
            Some(SubagentView { owner: owners.get(&candidate.session).cloned(), repo: candidate.repo, agent: candidate.agent, session: candidate.session, last_event: candidate.last_event, shell_finished: Some(finished) })
        })
        .collect()
}

pub(super) fn abandoned(journal: &str) -> Vec<u64> {
    journal.lines().filter_map(|line| line.split_whitespace().next()?.parse().ok()).collect()
}

fn runtime(state: &Path, checkout: &Path) -> RuntimeView {
    let abandoned = abandoned(&fs::read_to_string(state.join("runtime-abandoned")).unwrap_or_default());
    let live = fs::read_link(state.join("current")).ok().and_then(|link| Some(link.file_name()?.to_str()?.to_owned()));
    let rust_change_since = live.as_deref().and_then(|live| {
        let range = format!("{live}..origin/main");
        let output = crate::platform::git::run(checkout, &["log", "--format=%ct", &range, "--", ":(glob)**/*.rs", ":(glob)**/Cargo.toml", "Cargo.lock", "rust-toolchain.toml"]).ok()?;
        String::from_utf8_lossy(&output.stdout).lines().filter_map(|line| line.trim().parse().ok()).min()
    });
    RuntimeView { abandoned, live, rust_change_since }
}
