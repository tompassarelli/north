//! `north fleet on`: enables the units `north fleet off` disabled, then starts on each host exactly what `north fleet off` recorded stopping there and lifts the landing
//! freeze and the dispatch pause it placed. A service its recorded timer starts is left to that timer; a transient unit, a session among
//! them, is gone once stopped, so it is named and not restarted. Only lines it could not put back stay recorded.

use std::process::ExitCode;

use crate::config::Config;
use crate::host::{Host, each};
use crate::paths::Paths;
use crate::record::{self, Entry};
use crate::units::{self, Unit};

const LIFT: &str = "f=\"${XDG_STATE_HOME:-$HOME/.local/state}/north/land-freeze.json\"; if grep -q '\"cause\":\"north fleet off' \"$f\" 2>/dev/null; then rm -f \"$f\"; fi";

const UNPAUSE: &str = "f=\"$HOME/.local/state/north/dispatch-paused\"; if grep -q '\"set_by\":\"north fleet off\"' \"$f\" 2>/dev/null; then rm -f \"$f\"; fi";

struct Restore {
    started: Vec<String>,
    gone: Vec<String>,
    unfrozen: bool,
    unpaused: bool,
    kept: Vec<Entry>,
    problems: Vec<String>,
}

pub fn on() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("north fleet on: {error}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<ExitCode, String> {
    let paths = Paths::here()?;
    Config::load(&paths.config_home)?;
    let entries = record::read(&paths.state_home)?;
    if entries.is_empty() {
        println!("north fleet on: north fleet off recorded nothing as stopped at {}", record::path(&paths.state_home).display());
        return Ok(ExitCode::SUCCESS);
    }
    let names: Vec<String> = entries.iter().map(|entry| entry.host().to_owned()).collect::<std::collections::BTreeSet<_>>().into_iter().collect();
    let hosts = Host::fleet(&names)?;
    let restores = each(&hosts, |host| restore(host, entries.iter().filter(|entry| entry.host() == host.name).cloned().collect()));
    let mut kept = Vec::new();
    for (host, restore) in hosts.iter().zip(restores) {
        report(&host.name, &restore);
        kept.extend(restore.kept);
    }
    record::write(&paths.state_home, &kept)?;
    if kept.is_empty() {
        println!("north fleet on: everything north fleet off stopped is back");
        return Ok(ExitCode::SUCCESS);
    }
    println!("north fleet on: {} recorded lines could not be put back and stay in {}", kept.len(), record::path(&paths.state_home).display());
    Ok(ExitCode::from(1))
}

fn restore(host: &Host, entries: Vec<Entry>) -> Restore {
    let stopped: Vec<Unit> = entries.iter().filter_map(|entry| if let Entry::Unit { name, transient, disabled, .. } = entry { Some(Unit { name: name.clone(), transient: *transient, enabled: *disabled }) } else { None }).collect();
    let gone: Vec<String> = stopped.iter().filter(|unit| unit.transient).map(|unit| unit.name.clone()).collect();
    let startable: Vec<String> = stopped.iter().filter(|unit| !unit.transient && units::triggered_by(&unit.name, &stopped).is_none()).map(|unit| unit.name.clone()).collect();
    let mut restore = Restore { started: Vec::new(), gone, unfrozen: false, unpaused: false, kept: Vec::new(), problems: Vec::new() };
    let disabled: Vec<String> = stopped.iter().filter(|unit| unit.enabled).map(|unit| unit.name.clone()).collect();
    if let Err(error) = units::enable(host, &disabled) {
        restore.problems.push(format!("enable: {error}"));
        restore.kept.extend(entries.iter().filter(|entry| matches!(entry, Entry::Unit { disabled: true, .. })).cloned());
    }
    if let Err(error) = units::start(host, &startable) {
        restore.problems.push(format!("start: {error}"));
        let active = units::list(host).map(|listing| listing.active).unwrap_or_default();
        restore.kept.extend(entries.iter().filter(|entry| matches!(entry, Entry::Unit { name, .. } if startable.contains(name) && !active.contains(name))).cloned());
    }
    restore.started = startable.into_iter().filter(|name| !restore.kept.iter().any(|entry| matches!(entry, Entry::Unit { name: kept, .. } if kept == name))).collect();
    if entries.iter().any(|entry| matches!(entry, Entry::Freeze { .. })) {
        match host.run(LIFT) {
            Ok(_) => restore.unfrozen = true,
            Err(error) => {
                restore.problems.push(format!("landing freeze not lifted: {error}"));
                restore.kept.push(Entry::Freeze { host: host.name.clone() });
            }
        }
    }
    if entries.iter().any(|entry| matches!(entry, Entry::Pause { .. })) {
        match host.run(UNPAUSE) {
            Ok(_) => restore.unpaused = true,
            Err(error) => {
                restore.problems.push(format!("dispatch pause not lifted: {error}"));
                restore.kept.push(Entry::Pause { host: host.name.clone() });
            }
        }
    }
    restore
}

fn report(host: &str, restore: &Restore) {
    if !restore.started.is_empty() {
        println!("north fleet on: {host}: started {}: {}", restore.started.len(), restore.started.join(" "));
    }
    if !restore.gone.is_empty() {
        println!("north fleet on: {host}: transient, so gone once stopped (sessions come back with `north fleet revive NAME`): {}", restore.gone.join(" "));
    }
    if restore.unfrozen {
        println!("north fleet on: {host}: landing freeze lifted");
    }
    if restore.unpaused {
        println!("north fleet on: {host}: dispatch pause lifted");
    }
    for problem in &restore.problems {
        println!("north fleet on: {host}: {problem}");
    }
}
