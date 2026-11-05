use std::path::Path;
use std::process::Command;
use std::time::Duration;

use serde_json::Value;

use super::{Entry, base};
use crate::platform::process::run_command_within;

pub fn this_host() -> String {
    let name = crate::platform::host::name();
    if ["nexus", "gex45"].contains(&name.as_str()) { name } else { "laptop".to_owned() }
}

fn systemctl(arguments: &[&str]) -> Option<std::process::Output> {
    let mut command = Command::new("systemctl");
    command.arg("--user").args(arguments);
    run_command_within(command, None, Duration::from_secs(20))
}

pub fn live_timers() -> Option<Vec<String>> {
    let listed = systemctl(&["list-timers", "--all", "--output=json", "--no-pager"]).filter(|output| output.status.success())?;
    let rows: Vec<Value> = serde_json::from_slice(&listed.stdout).ok()?;
    let units: Vec<String> = rows.iter().map(|row| row.get("unit").and_then(Value::as_str).map(str::to_owned)).collect::<Option<_>>()?;
    if units.is_empty() {
        return Some(Vec::new());
    }
    let mut arguments = vec!["show", "-p", "Id", "-p", "FragmentPath"];
    arguments.extend(units.iter().map(String::as_str));
    let shown = systemctl(&arguments).map(|output| String::from_utf8_lossy(&output.stdout).into_owned()).unwrap_or_default();
    let own = format!("{}/.config/systemd/user/", std::env::var("HOME").unwrap_or_default());
    let mut timers = Vec::new();
    for block in shown.trim().split("\n\n") {
        let property = |key: &str| block.lines().find_map(|line| line.strip_prefix(key)?.strip_prefix('='));
        if property("FragmentPath").is_some_and(|path| path.starts_with(&own))
            && let Some(id) = property("Id")
        {
            timers.push(id.strip_suffix(".timer").unwrap_or(id).to_owned());
        }
    }
    Some(timers)
}

pub fn placement_findings(root: &Path, entries: &[Entry], timers: &[String], host: &str) -> (Vec<String>, Vec<String>, Vec<String>) {
    let systemd: Vec<&Entry> = entries.iter().filter(|entry| entry.get("kind") == "systemd-timer").collect();
    let named = |name: &str| entries.iter().find(|entry| entry.get("name") == name);
    let (mut problems, mut not_live, mut elsewhere) = (Vec::new(), Vec::new(), Vec::new());
    for unit in timers {
        match named(&format!("{unit}@{host}")).or_else(|| named(unit)) {
            None => match systemd.iter().find(|entry| base(entry.get("name")) == unit) {
                None => problems.push(format!("UNREGISTERED systemd-timer {unit}: add {}/{unit}.md", root.display())),
                Some(other) => problems.push(format!("WRONG HOST: {unit} runs here but is placed on {}", other.get("placement"))),
            },
            Some(entry) if !["any", host].contains(&entry.get("placement")) => {
                problems.push(format!("WRONG HOST: {unit} runs here but is placed on {}", entry.get("placement")));
            }
            Some(_) => {}
        }
    }
    let mut sorted = systemd;
    sorted.sort_by_key(|entry| entry.get("name"));
    for entry in sorted {
        let name = entry.get("name");
        if timers.iter().any(|timer| timer == base(name)) {
            continue;
        }
        let placement = entry.get("placement");
        if ["any", host].contains(&placement) {
            not_live.push(format!("not live: {name}"));
        } else {
            elsewhere.push(format!("elsewhere: {name} (placed on {placement})"));
        }
    }
    (problems, not_live, elsewhere)
}
