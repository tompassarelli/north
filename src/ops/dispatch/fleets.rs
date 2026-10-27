//! Who holds which item across hosts: each host's `north` listings (here, over ssh, or the laptop's
//! copy), the running sessions in its fleet list, and the items those sessions and dispatch's fresh
//! launches hold, which `north fleet launch --task` refuses to staff twice.
use super::issues::{self, IssueRef};
use super::record;
use crate::ops::pacing::placement::{self, gate};
use crate::platform::process::run_within;
use regex::Regex;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::LazyLock;
use std::time::Duration;
use toml_edit::{DocumentMut, Item};

pub(crate) const PROBE: Duration = Duration::from_secs(30);
const MIRROR_FRESH_S: f64 = 10.0 * 60.0;
const RUNNING: [&str; 3] = ["starting", "working", "idle"];
static SESSION_ITEMS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(sc|smashcraft|w|wisp)-(\d+[a-z]?(?:-\d+[a-z]?)*)(?:-|$)").expect("session pattern"));

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Session {
    pub name: String,
    pub items: Vec<IssueRef>,
    /// The machine it runs on, when the registry that launched it names one.
    pub machine: Option<String>,
}

/// The issues a session's name starts with: `sc-277-d1435` holds smashcraft#277 and
/// `sc-560-562-illidan` holds smashcraft#560 and #562.
pub(crate) fn named_items(name: &str) -> Vec<IssueRef> {
    let Some(found) = SESSION_ITEMS.captures(name) else { return Vec::new() };
    let repo = if found[1].starts_with('s') { "smashcraft" } else { "wisp" };
    found[2].split('-').filter_map(|number| number.trim_end_matches(|c: char| c.is_ascii_lowercase()).parse().ok()).map(|number| IssueRef::new(repo, number)).collect()
}

/// The running sessions in one registry's `north fleet list --json`, each holding every item its
/// `--task` names, else the items its name starts with.
pub(crate) fn running_sessions(listing: &Value) -> Vec<Session> {
    listing
        .as_array()
        .into_iter()
        .flatten()
        .filter(|session| RUNNING.contains(&session["status"].as_str().unwrap_or_default()))
        .map(|session| {
            let name = session["name"].as_str().unwrap_or_default().to_owned();
            let tasked: Vec<IssueRef> = issues::references(session["task_ref"].as_str().unwrap_or_default(), "").into_iter().filter(|item| !item.repo.is_empty()).collect();
            let items = if tasked.is_empty() { named_items(&name) } else { tasked };
            Session { machine: session["machine"].as_str().map(str::to_owned), name, items }
        })
        .collect()
}

/// The names of fleet sessions that some listing shows and none shows running: a claim they made
/// holds nothing, as a session that has stopped cannot be working it.
pub(crate) fn stopped_names(fleets: &[(String, Value)]) -> BTreeSet<String> {
    let named = |running: bool| -> BTreeSet<String> {
        fleets
            .iter()
            .flat_map(|(_, listing)| listing.as_array().into_iter().flatten())
            .filter(|session| RUNNING.contains(&session["status"].as_str().unwrap_or_default()) == running)
            .filter_map(|session| session["name"].as_str().filter(|name| !name.is_empty()).map(str::to_owned))
            .collect()
    };
    let running = named(true);
    named(false).into_iter().filter(|name| !running.contains(name)).collect()
}

/// The sessions this registry runs on other machines, which their resident counts miss, and the
/// holds every running session makes.
pub(crate) fn tally(sessions: &[(Session, String)]) -> (BTreeMap<String, i64>, BTreeMap<IssueRef, String>) {
    let (mut elsewhere, mut holds) = (BTreeMap::new(), BTreeMap::new());
    for (session, registry) in sessions {
        if let Some(machine) = session.machine.as_ref().filter(|machine| *machine != registry) {
            *elsewhere.entry(machine.clone()).or_insert(0) += 1;
        }
        for item in &session.items {
            holds.insert(item.clone(), format!("fleet session {} on {registry}", session.name));
        }
    }
    (elsewhere, holds)
}

/// The first of `items` a running session or a fresh dispatch launch holds, with its holder.
pub(crate) fn held(items: &[IssueRef], sessions: &[(Session, String)], launching: &BTreeMap<IssueRef, String>) -> Option<(IssueRef, String)> {
    let (_, mut holds) = tally(sessions);
    holds.extend(launching.iter().map(|(item, holder)| (item.clone(), holder.clone())));
    items.iter().find_map(|item| holds.get(item).map(|holder| (item.clone(), holder.clone())))
}

/// Who already holds an item `task` names, read from `local` (this host's fleet list), every other
/// host's and dispatch's fresh launches here; unread hosts go to `notes`.
pub(crate) fn holder(task: &str, local: Value, notes: &mut Vec<String>) -> Option<(IssueRef, String)> {
    let items: Vec<IssueRef> = issues::references(task, "").into_iter().filter(|item| !item.repo.is_empty()).collect();
    if items.is_empty() {
        return None;
    }
    let here = crate::platform::host::name();
    let at = placement::now();
    let mut fleets = vec![(here.clone(), local)];
    match placement::config() {
        Ok(cfg) => fleets.extend(others(&cfg, &here, &["fleet", "list", "--json"], "fleet", at, notes)),
        Err(why) => notes.push(format!("no placement config, so only this host's sessions count: {why}")),
    }
    let sessions: Vec<(Session, String)> = fleets.iter().flat_map(|(host, listing)| running_sessions(listing).into_iter().map(|session| (session, host.clone()))).collect();
    held(&items, &sessions, &record::launching(&record::read(&record::directory()), at))
}

pub(crate) fn remote_json(ssh: Option<&str>, command: &[&str]) -> Result<Value, String> {
    let north = std::env::current_exe().map_err(|error| error.to_string())?;
    let north = north.to_string_lossy();
    let output = match ssh {
        None => run_within(&north, command, PROBE),
        Some(destination) => {
            let mut words = vec!["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", destination, "north"];
            words.extend(command);
            run_within("ssh", &words, PROBE)
        }
    };
    let output = output.ok_or("timed out")?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).lines().last().unwrap_or("failed").to_owned());
    }
    serde_json::from_slice(&output.stdout).map_err(|error| error.to_string())
}

fn strings(item: Option<&Item>) -> Vec<String> {
    item.and_then(Item::as_array).map(|values| values.iter().filter_map(|value| value.as_str().map(str::to_owned)).collect()).unwrap_or_default()
}

/// The host other than `here` reached through the laptop job queue: no ssh destination, serves `laptop`.
pub(crate) fn laptop(cfg: &DocumentMut, here: &str) -> Option<String> {
    let hosts = cfg.get("hosts").and_then(Item::as_table_like)?;
    hosts.iter().find(|(name, table)| *name != here && table.get("ssh").is_none() && strings(table.get("serves")).iter().any(|need| need == "laptop")).map(|(name, _)| name.to_owned())
}

/// One `north` JSON listing from every host but `here`: an ssh host over its destination, the laptop
/// from the copy it leaves in the dispatch record as `KIND-HOST.json`, any other host over ssh by name.
pub(crate) fn others(cfg: &DocumentMut, here: &str, command: &[&str], kind: &str, at: f64, notes: &mut Vec<String>) -> Vec<(String, Value)> {
    let laptop = laptop(cfg, here);
    let names: Vec<String> = cfg.get("hosts").and_then(Item::as_table_like).into_iter().flat_map(|hosts| hosts.iter().map(|(name, _)| name.to_owned())).filter(|name| name != here).collect();
    let machines = gate::machines(cfg);
    let mut found = Vec::new();
    for host in names {
        if laptop.as_deref() == Some(host.as_str()) {
            found.push((host.clone(), mirrored(&record::directory(), kind, &host, at, notes)));
            continue;
        }
        let ssh = machines.iter().find(|machine| machine.name == host).map_or_else(|| host.clone(), |machine| machine.ssh.clone());
        match remote_json(Some(&ssh), command) {
            Ok(listing) => found.push((host, listing)),
            Err(why) => notes.push(format!("{host} north {} unread: {why}", command[..2].join(" "))),
        }
    }
    found
}

/// A listing the laptop copies to `KIND-HOST.json` in `directory`, since no host here reaches it
/// over ssh; an old copy still holds its issues.
pub(crate) fn mirrored(directory: &Path, kind: &str, host: &str, at: f64, notes: &mut Vec<String>) -> Value {
    let path = directory.join(format!("{kind}-{host}.json"));
    let listing = std::fs::read_to_string(&path).map_err(|error| error.to_string()).and_then(|text| serde_json::from_str::<Value>(&text).map_err(|error| error.to_string()));
    let copied = std::fs::metadata(&path).and_then(|meta| meta.modified()).ok().and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok());
    match listing {
        Ok(listing) => {
            if let Some(age) = copied.map(|copied| at - copied.as_secs_f64()).filter(|age| *age > MIRROR_FRESH_S) {
                notes.push(format!("{host} {kind} list copied {:.0} min ago", age / 60.0));
            }
            listing
        }
        Err(why) => {
            notes.push(format!("{host} {kind} list unread: {}: {why}", path.display()));
            Value::Null
        }
    }
}

/// One `north` JSON listing from this host and from every other, each with the host it came from.
pub(crate) fn everywhere(cfg: &DocumentMut, here: &str, command: &[&str], kind: &str, at: f64, notes: &mut Vec<String>) -> Vec<(String, Value)> {
    let mut found = Vec::new();
    match remote_json(None, command) {
        Ok(listing) => found.push((here.to_owned(), listing)),
        Err(why) => notes.push(format!("{here} north {} unread: {why}", command[..2].join(" "))),
    }
    found.extend(others(cfg, here, command, kind, at, notes));
    found
}
