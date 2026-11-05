use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use toml_edit::{DocumentMut, Item};

pub const REGISTRY: &str = include_str!("../../../../harness/loops.toml");
const USAGE: &str = "usage: north loops check";

#[derive(Clone, Debug, PartialEq)]
pub struct Loop {
    pub name: String,
    pub decision: String,
    pub sites: Vec<String>,
    pub signal: Option<String>,
    pub updated_by: String,
    pub cadence_hours: u64,
    pub record: Option<String>,
    pub owner: Option<String>,
}

pub fn parse(text: &str) -> Result<Vec<Loop>, String> {
    let document: DocumentMut = text.parse().map_err(|error| format!("loops registry: {error}"))?;
    let tables = document.get("loop").and_then(Item::as_array_of_tables).ok_or("loops registry has no [[loop]]")?;
    tables.iter().map(|table| {
        let text = |key: &str| table.get(key).and_then(Item::as_str).map(str::to_owned);
        let name = text("name").ok_or("a loop has no name")?;
        let needs = |key: &str| text(key).ok_or_else(|| format!("loop {name} has no {key}"));
        let sites = table.get("sites").and_then(Item::as_array).map(|sites| sites.iter().filter_map(|site| site.as_str().map(str::to_owned)).collect::<Vec<_>>()).unwrap_or_default();
        if sites.is_empty() {
            return Err(format!("loop {name} names no sites"));
        }
        let cadence_hours = table.get("cadence_hours").and_then(Item::as_integer).filter(|hours| *hours > 0).ok_or_else(|| format!("loop {name} has no positive cadence_hours"))?;
        let (signal, record, owner) = (text("signal"), text("record"), text("owner"));
        if signal.is_some() != record.is_some() {
            return Err(format!("loop {name} names a signal and a record together or neither"));
        }
        if signal.is_none() && owner.is_none() {
            return Err(format!("loop {name} has no signal and no owner to close it"));
        }
        Ok(Loop { decision: needs("decision")?, updated_by: needs("updated_by")?, name, sites, signal, cadence_hours: cadence_hours.unsigned_abs(), record, owner })
    }).collect()
}

#[derive(Debug, PartialEq)]
pub struct Checked {
    pub lines: Vec<String>,
    pub failing: Vec<String>,
}

pub fn check(loops: &[Loop], updated: &dyn Fn(&str) -> Option<SystemTime>, now: SystemTime) -> Checked {
    let mut lines = Vec::new();
    let mut failing = Vec::new();
    for found in loops {
        let verdict = verdict(found, updated, now);
        if verdict.starts_with("FAIL") {
            failing.push(found.name.clone());
        }
        lines.push(format!("{}: {verdict}", found.name));
    }
    lines.push(format!("loops: {} of {} failing", failing.len(), loops.len()));
    Checked { lines, failing }
}

fn verdict(found: &Loop, updated: &dyn Fn(&str) -> Option<SystemTime>, now: SystemTime) -> String {
    let (Some(signal), Some(record)) = (&found.signal, &found.record) else {
        return format!("FAIL no outcome signal; {} sets it; owner {}", found.updated_by, found.owner.as_deref().unwrap_or("none"));
    };
    let limit = Duration::from_secs(2 * found.cadence_hours * 3_600);
    match updated(record) {
        None => format!("FAIL never updated: no {record}"),
        Some(at) => {
            let age = now.duration_since(at).unwrap_or_default();
            let hours = age.as_secs() / 3_600;
            if age > limit {
                format!("FAIL stale: {} last updated {hours} h ago, over twice its {} h cadence", found.updated_by, found.cadence_hours)
            } else {
                format!("ok: {} updated {hours} h ago from {signal}", found.updated_by)
            }
        }
    }
}

pub fn state_root() -> PathBuf {
    let var = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from);
    var("XDG_STATE_HOME").unwrap_or_else(|| var("HOME").unwrap_or_default().join(".local/state"))
}

pub fn last_written(root: &Path, record: &str) -> Option<SystemTime> {
    let path = root.join(record);
    let own = std::fs::metadata(&path).and_then(|meta| meta.modified()).ok()?;
    let entries = std::fs::read_dir(&path).into_iter().flatten().flatten().filter_map(|entry| entry.metadata().and_then(|meta| meta.modified()).ok());
    entries.chain([own]).max()
}

pub fn here() -> Result<Checked, String> {
    let root = state_root();
    Ok(check(&parse(REGISTRY)?, &|record| last_written(&root, record), SystemTime::now()))
}

pub fn run(arguments: &[String]) -> u8 {
    if !matches!(arguments, [command] if command == "check") {
        eprintln!("{USAGE}");
        return 2;
    }
    match here() {
        Ok(checked) => {
            checked.lines.iter().for_each(|line| println!("{line}"));
            u8::from(!checked.failing.is_empty())
        }
        Err(error) => {
            eprintln!("north loops: {error}");
            1
        }
    }
}
