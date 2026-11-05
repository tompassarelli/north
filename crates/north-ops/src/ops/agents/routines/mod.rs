mod live;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use crate::foundation::clock::{local_today, parse_python_date};

pub use live::{live_timers, placement_findings, this_host};

const KINDS: [&str; 3] = ["session-cron", "systemd-timer", "cloud-routine"];
const PLACEMENTS: [&str; 4] = ["nexus", "gex45", "laptop", "any"];
const HOSTS: [&str; 3] = ["nexus", "gex45", "laptop"];
const FIELDS: [&str; 8] = ["name", "kind", "schedule", "owner", "purpose", "relates-to", "expires", "placement"];
static DURATION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A(\d+)(min|h|d)\z").expect("duration pattern"));
static POINTER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[routine:([a-z0-9][a-z0-9-]*)\]").expect("pointer pattern"));
pub const USAGE: &str = "usage: agents routines [list] [--no-live] [--crons FILE|-]\n       agents routines show NAME\n       agents routines pointer NAME\nLists the operator routines/*.md (south:agents/routines by default), compares them with live user timers and, given\nCronList output (--crons), with session crons; exits 1 on an unregistered live job,\na live timer on the wrong host, an expired or invalid entry.\nEach entry's `placement` (nexus, gex45, laptop or any) names the host it belongs on. A job that\nruns on both hosts has one entry per host, `name@nexus` and `name@laptop`, whose placement\nequals the suffix; the timer it matches is `name`. `deadline` (optional) is a duration such\nas 30min, 2d or 1h.";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Entry {
    pub fields: BTreeMap<String, String>,
    pub relates_to: Vec<String>,
    pub body: String,
}

impl Entry {
    pub fn get(&self, key: &str) -> &str {
        self.fields.get(key).map_or("", String::as_str)
    }
}

pub fn routines_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("AGENT_ROUTINES_DIR").filter(|dir| !dir.is_empty()) {
        return PathBuf::from(dir);
    }
    let operator = std::env::var_os("AGENTS_OPERATOR_DIR")
        .filter(|dir| !dir.is_empty())
        .map_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("code/south/main/agents"), PathBuf::from);
    operator.join("routines")
}

fn valid_duration(text: &str) -> bool {
    DURATION.is_match(text.trim())
}

fn repr(text: &str) -> String {
    let quote = if text.contains('\'') && !text.contains('"') { '"' } else { '\'' };
    let mut shown = String::from(quote);
    for c in text.chars() {
        match c {
            '\\' => shown.push_str("\\\\"),
            '\n' => shown.push_str("\\n"),
            '\t' => shown.push_str("\\t"),
            '\r' => shown.push_str("\\r"),
            c if c == quote => {
                shown.push('\\');
                shown.push(c);
            }
            c => shown.push(c),
        }
    }
    shown.push(quote);
    shown
}

pub fn parse(text: &str, stem: &str) -> Result<Entry, String> {
    let (head, body) = text.strip_prefix("---\n").and_then(|rest| rest.split_once("\n---\n")).ok_or("no front matter")?;
    let mut entry = Entry { body: body.trim().to_owned(), ..Entry::default() };
    for line in head.lines() {
        if let Some((key, value)) = line.split_once(':') {
            entry.fields.insert(key.trim().to_owned(), value.trim().to_owned());
        }
    }
    let relates = entry.get("relates-to");
    let relates = relates.strip_prefix('[').unwrap_or(relates);
    let relates = relates.strip_suffix(']').unwrap_or(relates);
    entry.relates_to = relates.split(',').map(str::trim).filter(|item| !item.is_empty()).map(str::to_owned).collect();
    let missing: Vec<&str> = FIELDS.into_iter().filter(|field| if *field == "relates-to" { entry.relates_to.is_empty() } else { entry.get(field).is_empty() }).collect();
    if !missing.is_empty() {
        return Err(format!("missing {}", missing.join(", ")));
    }
    check(&entry, stem)?;
    Ok(entry)
}

fn check(entry: &Entry, stem: &str) -> Result<(), String> {
    let (name, kind, placement) = (entry.get("name"), entry.get("kind"), entry.get("placement"));
    if name != stem {
        return Err(format!("name {} differs from file {}", repr(name), repr(stem)));
    }
    if !KINDS.contains(&kind) {
        return Err(format!("kind {} is not one of {}", repr(kind), KINDS.join(", ")));
    }
    if !PLACEMENTS.contains(&placement) {
        return Err(format!("placement {} is not one of {}", repr(placement), PLACEMENTS.join(", ")));
    }
    if let Some((_, host)) = name.split_once('@') {
        if !HOSTS.contains(&host) {
            let hosts: Vec<String> = HOSTS.iter().map(|host| format!("@{host}")).collect();
            return Err(format!("name suffix @{host} is not one of {}", hosts.join(", ")));
        }
        if placement != host {
            return Err(format!("placement {} does not match name suffix @{host}", repr(placement)));
        }
    }
    let deadline = entry.get("deadline");
    if !deadline.is_empty() && !valid_duration(deadline) {
        return Err(format!("deadline {} is not a duration such as 2d, 30min or 1h", repr(deadline)));
    }
    let expires = entry.get("expires");
    if expires != "never" && parse_python_date(expires).is_none() {
        return Err(format!("expires {} is neither never nor YYYY-MM-DD", repr(expires)));
    }
    if entry.body.is_empty() {
        return Err("empty prompt or command".to_owned());
    }
    Ok(())
}

pub fn load(root: &Path) -> Result<(Vec<Entry>, Vec<String>), String> {
    let mut names: Vec<String> = std::fs::read_dir(root)
        .map_err(|error| format!("cannot read {}: {error}", root.display()))?
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .filter(|name| name.ends_with(".md"))
        .collect();
    names.sort();
    let (mut entries, mut problems) = (Vec::new(), Vec::new());
    for name in names {
        let text = std::fs::read_to_string(root.join(&name)).map_err(|error| format!("cannot read {name}: {error}"))?;
        match parse(&text, name.strip_suffix(".md").unwrap_or(&name)) {
            Ok(entry) => entries.push(entry),
            Err(error) => problems.push(format!("INVALID {name}: {error}")),
        }
    }
    Ok((entries, problems))
}

pub fn pointer(name: &str) -> String {
    format!("[routine:{name}] Run `agents routines show {name}` and follow it.")
}

fn truncate(text: &str, width: usize) -> String {
    text.chars().take(width).collect()
}

fn session_crons(entries: &[Entry], crons: &str, problems: &mut Vec<String>) -> usize {
    let mut count = 0;
    for line in crons.lines().filter(|line| !line.trim().is_empty()) {
        count += 1;
        match POINTER.captures(line) {
            None => problems.push(format!("UNREGISTERED session-cron: {}", truncate(line.trim(), 100))),
            Some(found) if !entries.iter().any(|entry| entry.get("kind") == "session-cron" && entry.get("name") == &found[1]) => {
                problems.push(format!("UNREGISTERED session-cron [routine:{}]: no such session-cron entry", &found[1]));
            }
            Some(_) => {}
        }
    }
    count
}

pub struct Listing {
    pub live: Option<Option<Vec<String>>>,
    pub crons: Option<String>,
    pub host: String,
}

pub fn list(root: &Path, listing: &Listing) -> Result<(u8, String), String> {
    let (entries, mut problems) = load(root)?;
    let today = local_today();
    let mut out = format!("{:28} {:14} {:34} {:9} OWNER\n", "NAME", "KIND", "SCHEDULE", "PLACEMENT");
    for entry in &entries {
        let schedule = truncate(entry.get("schedule"), 34);
        let _ = writeln!(out, "{:28} {:14} {schedule:34} {:9} {}", entry.get("name"), entry.get("kind"), entry.get("placement"), entry.get("owner"));
        if parse_python_date(entry.get("expires")).is_some_and(|expires| expires < today) {
            problems.push(format!("EXPIRED {}: expired {}", entry.get("name"), entry.get("expires")));
        }
    }
    match &listing.live {
        None => {}
        Some(None) => out.push_str("live timers: systemctl --user unavailable\n"),
        Some(Some(timers)) => {
            let registered: std::collections::BTreeSet<&str> = entries.iter().filter(|entry| entry.get("kind") == "systemd-timer").map(|entry| base(entry.get("name"))).filter(|name| timers.iter().any(|timer| timer == name)).collect();
            let _ = writeln!(out, "live timers: {} ({} registered)", timers.len(), registered.len());
            let (found, not_live, elsewhere) = placement_findings(root, &entries, timers, &listing.host);
            problems.extend(found);
            for line in not_live.iter().chain(&elsewhere) {
                let _ = writeln!(out, "{line}");
            }
        }
    }
    if let Some(crons) = &listing.crons {
        let count = session_crons(&entries, crons, &mut problems);
        let _ = writeln!(out, "session crons: {count} checked");
    }
    for problem in &problems {
        let _ = writeln!(out, "{problem}");
    }
    let _ = writeln!(out, "routines: {} entries, {} problem(s)", entries.len(), problems.len());
    Ok((u8::from(!problems.is_empty()), out))
}

pub fn base(name: &str) -> &str {
    name.split_once('@').map_or(name, |(base, _)| base)
}

fn list_command(arguments: &[String]) -> u8 {
    let (mut live, mut crons) = (true, None);
    let mut rest = arguments.iter();
    while let Some(argument) = rest.next() {
        match (argument.as_str(), rest.clone().next()) {
            ("--no-live", _) => live = false,
            ("--crons", Some(source)) => {
                rest.next();
                let text = if source == "-" { std::io::read_to_string(std::io::stdin()) } else { std::fs::read_to_string(source) };
                match text {
                    Ok(text) => crons = Some(text),
                    Err(error) => {
                        eprintln!("agents routines: cannot read {source}: {error}");
                        return 2;
                    }
                }
            }
            _ => {
                eprintln!("{USAGE}");
                return 2;
            }
        }
    }
    let listing = Listing { live: live.then(live_timers), crons, host: this_host() };
    match list(&routines_dir(), &listing) {
        Ok((code, text)) => {
            print!("{text}");
            code
        }
        Err(error) => {
            eprintln!("agents routines: {error}");
            1
        }
    }
}

pub fn show(entry: &Entry) -> String {
    let deadline = entry.get("deadline");
    let deadline = if deadline.is_empty() { String::new() } else { format!(", deadline {deadline}") };
    format!(
        "# routine {} ({}, {}, placed on {}{deadline}): {}\n{}",
        entry.get("name"),
        entry.get("kind"),
        entry.get("schedule"),
        entry.get("placement"),
        entry.get("purpose"),
        entry.body
    )
}

pub fn run(arguments: &[String]) -> u8 {
    if arguments.first().is_some_and(|first| first == "-h" || first == "--help") {
        println!("{USAGE}");
        return 0;
    }
    let (command, rest) = match arguments.first() {
        Some(first) if !first.starts_with('-') => (first.as_str(), &arguments[1..]),
        _ => ("list", arguments),
    };
    match (command, rest) {
        ("list", _) => list_command(rest),
        ("show" | "pointer", [name]) => {
            let entries = load(&routines_dir()).map(|(entries, _)| entries).unwrap_or_default();
            let Some(entry) = entries.iter().find(|entry| entry.get("name") == name) else {
                eprintln!("agents routines: no valid entry named {}", repr(name));
                return 1;
            };
            println!("{}", if command == "pointer" { pointer(entry.get("name")) } else { show(entry) });
            0
        }
        _ => {
            eprintln!("{USAGE}");
            2
        }
    }
}
