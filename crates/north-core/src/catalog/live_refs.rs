use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::{NorthError, NorthResult};
use crate::platform::hook_commands;

use super::configuration;
use super::live_refs_transport as transport;
use super::paths::home;

pub const SCHEMA: &str = "north.host-config-facts/v1";
pub(super) const DEFAULT_MAX_AGE_MS: u64 = 24 * 60 * 60 * 1000;
const USAGE: &str = "usage: north config agents live-refs --record [--facts-dir DIR] [--repo DIR [--remote NAME]]\n       north config agents live-refs --path NAME [--facts-dir DIR | --repo DIR [--remote NAME]] [--max-age 24h] [--hosts a,b,c]\n       north config agents live-refs --dropped-by-catalog --south-root DIR --north-root DIR --repo DIR [--remote NAME] [--max-age 24h] [--hosts a,b,c]";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Facts {
    pub schema: String,
    pub host: String,
    #[serde(rename = "recordedAtMs")]
    pub recorded_at_ms: u64,
    pub hooks: Vec<HookFact>,
    #[serde(rename = "unitExecStarts")]
    pub unit_exec_starts: Vec<UnitFact>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookFact {
    pub path: String,
    #[serde(rename = "registeredIn")]
    pub registered_in: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnitFact {
    pub unit: String,
    pub path: String,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub naming: Vec<String>,
    pub unknown: Vec<String>,
    pub ages: Vec<String>,
}

impl Report {
    pub fn is_clean(&self) -> bool {
        self.naming.is_empty() && self.unknown.is_empty()
    }
}

pub fn collect(host: &str, now_ms: u64, home: &Path, state: &Path, units: &str) -> Facts {
    let mut hooks = Vec::new();
    for file in hook_commands::registration_files(home, state) {
        let Some(settings) = fs::read_to_string(&file).ok().and_then(|text| serde_json::from_str(&text).ok()) else {
            continue;
        };
        for path in hook_commands::paths(&settings) {
            hooks.push(HookFact { path, registered_in: file.display().to_string() });
        }
    }
    hooks.sort_by(|a, b| (&a.path, &a.registered_in).cmp(&(&b.path, &b.registered_in)));
    hooks.dedup();
    Facts {
        schema: SCHEMA.into(),
        host: host.into(),
        recorded_at_ms: now_ms,
        hooks,
        unit_exec_starts: unit_exec_starts(units),
    }
}

pub fn unit_exec_starts(units: &str) -> Vec<UnitFact> {
    let mut unit = String::new();
    let mut found = Vec::new();
    for line in units.lines() {
        if let Some(path) = line.strip_prefix("# ") {
            unit = path.split(" -> ").next().unwrap_or(path).trim().to_owned();
            continue;
        }
        let Some((key, command)) = line.split_once('=') else { continue };
        if !key.starts_with("Exec") || key.contains(' ') {
            continue;
        }
        for word in command.trim_start_matches(['@', '-', ':', '+', '!']).split_whitespace() {
            let word = word.trim_matches(['"', '\'']);
            if word.starts_with('/') {
                found.push(UnitFact { unit: unit.clone(), path: word.to_owned() });
            }
        }
    }
    found.sort_by(|a, b| (&a.unit, &a.path).cmp(&(&b.unit, &b.path)));
    found.dedup();
    found
}

fn names(path: &str, name: &str) -> bool {
    let name = name.trim_matches('/');
    !name.is_empty() && (path == name || path.ends_with(&format!("/{name}")))
}

pub fn judge(all: &[Facts], name: &str, hosts: &[String], now_ms: u64, max_age_ms: u64) -> Report {
    let mut report = Report::default();
    for fact in all {
        report.ages.push(format!("{} {} s", fact.host, now_ms.saturating_sub(fact.recorded_at_ms) / 1000));
        if now_ms.saturating_sub(fact.recorded_at_ms) > max_age_ms {
            report.unknown.push(format!("{} (facts older than {} s)", fact.host, max_age_ms / 1000));
            continue;
        }
        for hook in fact.hooks.iter().filter(|hook| names(&hook.path, name)) {
            report.naming.push(format!("{}: {} registered in {}", fact.host, hook.path, hook.registered_in));
        }
        for unit in fact.unit_exec_starts.iter().filter(|unit| names(&unit.path, name)) {
            report.naming.push(format!("{}: {} runs {}", fact.host, unit.unit, unit.path));
        }
    }
    for host in hosts {
        if !all.iter().any(|fact| &fact.host == host) {
            report.unknown.push(format!("{host} (no facts recorded)"));
        }
    }
    if all.is_empty() && hosts.is_empty() {
        report.unknown.push("no host has recorded facts".into());
    }
    report
}

pub fn read_all(directory: &Path) -> NorthResult<Vec<Facts>> {
    let mut all = Vec::new();
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(all),
        Err(error) => return Err(error.into()),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|extension| extension != "json") {
            continue;
        }
        let facts: Facts = serde_json::from_slice(&fs::read(&path)?)
            .map_err(|error| NorthError::Configuration(format!("cannot parse {}: {error}", path.display())))?;
        if facts.schema == SCHEMA {
            all.push(facts);
        }
    }
    all.sort_by(|a, b| a.host.cmp(&b.host));
    Ok(all)
}

pub fn hosts_naming(directory: &Path, name: &str, hosts: &[String], now_ms: u64, max_age_ms: u64) -> NorthResult<Report> {
    Ok(judge(&read_all(directory)?, name, hosts, now_ms, max_age_ms))
}

pub fn write(directory: &Path, facts: &Facts) -> NorthResult<PathBuf> {
    fs::create_dir_all(directory)?;
    let target = directory.join(format!("{}.json", facts.host));
    let temporary = directory.join(format!(".{}.json.tmp-{}", facts.host, std::process::id()));
    fs::write(&temporary, serde_json::to_vec_pretty(facts)?)?;
    fs::rename(&temporary, &target)?;
    Ok(target)
}

pub(super) fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_millis() as u64)
}

fn host_name() -> NorthResult<String> {
    let name = fs::read_to_string("/proc/sys/kernel/hostname")?.trim().to_owned();
    if name.is_empty() {
        return configuration("hostname printed nothing");
    }
    Ok(name)
}

fn user_units() -> NorthResult<String> {
    let systemctl = |args: &[&str]| -> NorthResult<String> {
        let output = Command::new("systemctl").arg("--user").args(args).output()?;
        if !output.status.success() {
            return configuration(format!("systemctl --user {} failed: {}", args.join(" "), String::from_utf8_lossy(&output.stderr).trim()));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    };
    let listing = systemctl(&["list-unit-files", "--no-legend", "--no-pager"])?;
    let units: Vec<&str> = listing
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .filter(|name| [".service", ".timer", ".path", ".socket"].iter().any(|suffix| name.ends_with(suffix)) && !name.ends_with("@.service"))
        .collect();
    if units.is_empty() {
        return Ok(String::new());
    }
    let mut arguments = vec!["cat", "--no-pager", "--"];
    arguments.extend(units);
    systemctl(&arguments)
}

pub(super) fn parse_duration_ms(text: &str) -> NorthResult<u64> {
    let (number, unit) = text.split_at(text.find(|c: char| !c.is_ascii_digit()).unwrap_or(text.len()));
    let scale = match unit {
        "" | "s" => 1000,
        "m" => 60_000,
        "h" => 3_600_000,
        "d" => 86_400_000,
        _ => return Err(NorthError::Usage(format!("bad duration {text}"))),
    };
    number.parse::<u64>().map(|number| number * scale).map_err(|_| NorthError::Usage(format!("bad duration {text}")))
}

pub struct Recorded {
    pub facts: Facts,
    pub path: PathBuf,
    pub published: Option<String>,
}

pub fn default_facts_dir() -> NorthResult<PathBuf> {
    let (_, state) = home_and_state()?;
    Ok(state.join("north/host-facts"))
}

fn home_and_state() -> NorthResult<(PathBuf, PathBuf)> {
    let home = home()?;
    let state = std::env::var_os("XDG_STATE_HOME").map_or_else(|| home.join(".local/state"), PathBuf::from);
    Ok((home, state))
}

pub fn record(directory: &Path, publish: Option<(&Path, &str)>) -> NorthResult<Recorded> {
    let (home, state) = home_and_state()?;
    let facts = collect(&host_name()?, now_ms(), &home, &state, &user_units()?);
    let path = write(directory, &facts)?;
    let published = match publish {
        Some((repo, remote)) => Some(transport::publish(repo, remote, &facts)?),
        None => None,
    };
    Ok(Recorded { facts, path, published })
}

pub(super) fn run(arguments: &[String]) -> NorthResult<()> {
    let mut recording = false;
    let mut name = None;
    let mut directory = None;
    let mut repo = None;
    let mut remote = "origin".to_owned();
    let mut max_age = DEFAULT_MAX_AGE_MS;
    let mut hosts = Vec::new();
    let mut entries = arguments.iter();
    while let Some(flag) = entries.next() {
        let mut value = || entries.next().cloned().ok_or_else(|| NorthError::Usage(format!("{flag} needs a value")));
        match flag.as_str() {
            "--help" | "-h" => {
                println!("{USAGE}");
                return Ok(());
            }
            "--record" => recording = true,
            "--path" => name = Some(value()?),
            "--facts-dir" => directory = Some(PathBuf::from(value()?)),
            "--repo" => repo = Some(PathBuf::from(value()?)),
            "--remote" => remote = value()?,
            "--max-age" => max_age = parse_duration_ms(&value()?)?,
            "--hosts" => hosts = value()?.split(',').filter(|host| !host.is_empty()).map(str::to_owned).collect(),
            _ => return Err(NorthError::Usage(format!("unknown option {flag}\n{USAGE}"))),
        }
    }
    let directory = match directory {
        Some(directory) => directory,
        None => default_facts_dir()?,
    };
    match (recording, name) {
        (true, None) => {
            let recorded = record(&directory, repo.as_deref().map(|repo| (repo, remote.as_str())))?;
            println!("recorded {} hook paths and {} unit paths in {}", recorded.facts.hooks.len(), recorded.facts.unit_exec_starts.len(), recorded.path.display());
            if let Some(commit) = recorded.published {
                println!("published {commit} to {remote} {}", transport::reference(&recorded.facts.host));
            }
            Ok(())
        }
        (false, Some(name)) => {
            let all = match &repo {
                Some(repo) => transport::fetch_all(repo, &remote)?,
                None => read_all(&directory)?,
            };
            let report = judge(&all, &name, &hosts, now_ms(), max_age);
            if report.is_clean() {
                println!("no recorded host names {name}; fact ages: {}", report.ages.join(", "));
                return Ok(());
            }
            let mut lines = Vec::new();
            if !report.naming.is_empty() {
                lines.push(format!("still named: {}", report.naming.join("; ")));
            }
            if !report.unknown.is_empty() {
                lines.push(format!("unknown, so refused: {}", report.unknown.join("; ")));
            }
            lines.push(format!("fact ages: {}", report.ages.join(", ")));
            configuration(format!("{name} is not safe to remove: {}", lines.join("; ")))
        }
        _ => Err(NorthError::Usage(USAGE.into())),
    }
}

#[cfg(test)]
#[path = "live_refs_tests.rs"]
mod tests;
