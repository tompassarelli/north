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
static SESSION_ITEMS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(sc|smashcraft|w|wisp|north)-(\d+[a-z]?(?:-\d+[a-z]?)*)(?:-|$)").expect("session pattern"));

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
    let repo = match &found[1] {
        "sc" | "smashcraft" => "smashcraft",
        "north" => "north",
        _ => "wisp",
    };
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
            let task_text = match session["tasks"].as_array() {
                Some(tasks) => tasks.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" "),
                None => session["task_ref"].as_str().unwrap_or_default().to_owned(),
            };
            let tasked: Vec<IssueRef> = issues::references(&task_text, "").into_iter().filter(|item| !item.repo.is_empty()).collect();
            let items = if tasked.is_empty() { named_items(&name) } else { tasked };
            Session { machine: session["machine"].as_str().map(str::to_owned), name, items }
        })
        .collect()
}

/// The names of fleet sessions that some listing shows and none shows running: their launches no
/// longer hold their items.
pub(crate) fn stopped_names(fleets: &[(String, Value)]) -> BTreeSet<String> {
    names_outside(fleets, &RUNNING)
}

/// The names of fleet sessions that some listing shows and none shows running or parked: a claim
/// they made holds nothing, while a parked owner's claim holds until it is stale, as `threads
/// claim` refuses on it.
pub(crate) fn exited_names(fleets: &[(String, Value)]) -> BTreeSet<String> {
    names_outside(fleets, &[RUNNING.as_slice(), &["parked"]].concat())
}

fn names_outside(fleets: &[(String, Value)], holding: &[&str]) -> BTreeSet<String> {
    let named = |inside: bool| -> BTreeSet<String> {
        fleets
            .iter()
            .flat_map(|(_, listing)| listing.as_array().into_iter().flatten())
            .filter(|session| holding.contains(&session["status"].as_str().unwrap_or_default()) == inside)
            .filter_map(|session| session["name"].as_str().filter(|name| !name.is_empty()).map(str::to_owned))
            .collect()
    };
    let held = named(true);
    named(false).into_iter().filter(|name| !held.contains(name)).collect()
}

/// The names of dispatch launches whose sessions no longer hold their items: sessions some listing
/// shows and none shows running, and sessions absent from every listing though the listing of the
/// host they launched on was read and taken late enough after the launch for it to show. A listing
/// was taken at `at` unless `copies` holds the time its host copied it.
pub(crate) fn gone(fleets: &[(String, Value)], copies: &BTreeMap<String, f64>, records: &[record::Record], at: f64) -> BTreeSet<String> {
    let listed: BTreeSet<&str> = fleets.iter().flat_map(|(_, listing)| listing.as_array().into_iter().flatten()).filter_map(|session| session["name"].as_str()).collect();
    let forgotten = records.iter().filter(|launch| launch.launched && !launch.cloud && !launch.hand && copies.get(&launch.host).copied().unwrap_or(at) - launch.at >= record::LAPTOP_SHOW_S).filter(|launch| !listed.contains(launch.name.as_str())).filter(|launch| fleets.iter().any(|(host, listing)| *host == launch.host && listing.is_array()));
    stopped_names(fleets).into_iter().chain(forgotten.map(|launch| launch.name.clone())).collect()
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
    let mut copies = BTreeMap::new();
    match placement::config() {
        Ok(cfg) => {
            fleets.extend(others(&cfg, &here, &["fleet", "list", "--json"], "fleet", at, PROBE, notes));
            copies = copy_times(&record::directory(), "fleet", laptop(&cfg, &here).as_deref());
        }
        Err(why) => notes.push(format!("no placement config, so only this host's sessions count: {why}")),
    }
    let sessions: Vec<(Session, String)> = fleets.iter().flat_map(|(host, listing)| running_sessions(listing).into_iter().map(|session| (session, host.clone()))).collect();
    let records = record::read(&record::directory());
    held(&items, &sessions, &record::launching(&records, at, &gone(&fleets, &copies, &records, at)))
}

pub(crate) fn remote_json(ssh: Option<&str>, command: &[&str]) -> Result<Value, String> {
    remote_json_within(ssh, command, PROBE)
}

pub(crate) fn remote_json_within(ssh: Option<&str>, command: &[&str], limit: Duration) -> Result<Value, String> {
    let north = std::env::current_exe().map_err(|error| error.to_string())?;
    let north = north.to_string_lossy();
    let output = match ssh {
        None => run_within(&north, command, limit),
        Some(destination) => {
            let mut words = vec!["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", destination, "north"];
            words.extend(command);
            run_within("ssh", &words, limit)
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
pub(crate) fn others(cfg: &DocumentMut, here: &str, command: &[&str], kind: &str, at: f64, limit: Duration, notes: &mut Vec<String>) -> Vec<(String, Value)> {
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
        match remote_json_within(Some(&ssh), command, limit) {
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
    match listing {
        Ok(listing) => {
            if let Some(age) = copied(directory, kind, host).map(|copied| at - copied).filter(|age| *age > MIRROR_FRESH_S) {
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

/// When the laptop last copied its `KIND-HOST.json` listing to `directory`.
pub(crate) fn copied(directory: &Path, kind: &str, host: &str) -> Option<f64> {
    let modified = std::fs::metadata(directory.join(format!("{kind}-{host}.json"))).and_then(|meta| meta.modified()).ok()?;
    modified.duration_since(std::time::UNIX_EPOCH).ok().map(|since| since.as_secs_f64())
}

/// The time `laptop` copied its listing of `kind`, by host, which [`gone`] takes for when the listing was taken.
pub(crate) fn copy_times(directory: &Path, kind: &str, laptop: Option<&str>) -> BTreeMap<String, f64> {
    laptop.and_then(|host| copied(directory, kind, host).map(|at| (host.to_owned(), at))).into_iter().collect()
}

/// One `north` JSON listing from this host and from every other, each with the host it came from.
pub(crate) fn everywhere(cfg: &DocumentMut, here: &str, command: &[&str], kind: &str, at: f64, limit: Duration, notes: &mut Vec<String>) -> Vec<(String, Value)> {
    let mut found = Vec::new();
    match remote_json_within(None, command, limit) {
        Ok(listing) => found.push((here.to_owned(), listing)),
        Err(why) => notes.push(format!("{here} north {} unread: {why}", command[..2].join(" "))),
    }
    found.extend(others(cfg, here, command, kind, at, limit, notes));
    found
}
