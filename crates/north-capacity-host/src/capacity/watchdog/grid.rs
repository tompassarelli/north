use std::path::Path;
use std::process::Command;
use std::time::Duration;

use north_capacity::capacity::runner_alarm::{self, HostReading, LeaseUse, State};
use serde_json::Value;

use crate::platform::process::run_command_within;

const STATE_FILE: &str = "runner-alarm.json";
const SENDER: &str = "service:capacity-runners";
const COORDINATOR: &str = "proxy";
const INTERVAL: Duration = Duration::from_secs(60);
const READ_WAIT: Duration = Duration::from_secs(20);

pub(super) fn print_reading() -> Result<(), String> {
    println!("{}", serde_json::to_string(&reading()).map_err(|error| error.to_string())?);
    Ok(())
}

fn reading() -> HostReading {
    let mut systemctl = Command::new("systemctl");
    systemctl.args(["list-units", "--state=active", "--plain", "--no-legend", "--no-pager", "buildbarn-runner-*"]);
    let runners = run_command_within(systemctl, None, READ_WAIT).filter(|output| output.status.success()).map(|output| String::from_utf8_lossy(&output.stdout).lines().filter(|line| line.contains(".service")).count());
    HostReading { host: crate::platform::host::name(), runners, land_checks: land_checks(&north_capacity::capacity::local_machine::runtime_root().join("leases")) }
}

fn land_checks(leases: &Path) -> Vec<LeaseUse> {
    let mut names: Vec<String> = std::fs::read_dir(leases).into_iter().flatten().flatten().map(|entry| entry.file_name().to_string_lossy().into_owned()).filter(|name| name.ends_with(".json")).collect();
    names.sort();
    names
        .into_iter()
        .filter_map(|name| {
            let lease: Value = serde_json::from_str(&std::fs::read_to_string(leases.join(&name)).ok()?).ok()?;
            (lease["landing"].as_bool() == Some(true)).then_some(())?;
            let stem = name.trim_end_matches(".json").to_owned();
            let usage: Option<Value> = std::fs::read_to_string(leases.join(format!("{stem}.usage"))).ok().and_then(|text| serde_json::from_str(&text).ok());
            Some(LeaseUse { cores: usage.and_then(|usage| usage["recentCores"].as_f64()), lease: stem })
        })
        .collect()
}

fn remote(host: &str, destination: &str) -> HostReading {
    let mut ssh = Command::new("ssh");
    ssh.args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", "--", destination, "north", "capacity", "watchdog", "grid"]);
    let read = run_command_within(ssh, None, READ_WAIT).filter(|output| output.status.success()).and_then(|output| serde_json::from_slice::<HostReading>(&output.stdout).ok());
    read.unwrap_or_else(|| HostReading { host: host.to_owned(), runners: None, land_checks: Vec::new() })
}

fn grid() -> Option<Vec<HostReading>> {
    let cfg = crate::platform::orchestration::read().ok()?;
    let here = crate::platform::host::name();
    let mut hosts = vec![reading()];
    for name in crate::platform::orchestration::ssh_hosts(&cfg).into_iter().filter(|name| *name != here) {
        if let Some(destination) = crate::platform::orchestration::operator_login(&cfg, &name) {
            hosts.push(remote(&name, &destination));
        }
    }
    Some(hosts)
}

fn holds_the_ledger() -> bool {
    north_threads::threads::client::ledger_host_name().is_none() && north_threads::threads::client::threads_database().is_file()
}

pub(super) fn spawn(dir: &Path, north: &Path) {
    if !holds_the_ledger() {
        return;
    }
    let (dir, north) = (dir.to_owned(), north.to_owned());
    std::thread::spawn(move || {
        loop {
            if let Some(hosts) = grid() {
                watch(&dir, &north, &hosts);
            }
            std::thread::sleep(INTERVAL);
        }
    });
}

fn watch(dir: &Path, north: &Path, hosts: &[HostReading]) {
    let path = dir.join(STATE_FILE);
    let state: State = std::fs::read_to_string(&path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |age| i64::try_from(age.as_millis()).unwrap_or(i64::MAX));
    let (state, pages) = runner_alarm::step(state, now, hosts);
    let staged = path.with_extension("json.tmp");
    let saved = serde_json::to_vec(&state).map_err(std::io::Error::other).and_then(|bytes| std::fs::create_dir_all(dir).and_then(|()| std::fs::write(&staged, bytes))).and_then(|()| std::fs::rename(&staged, &path));
    if let Err(error) = saved {
        eprintln!("north capacity watchdog: {}: {error}", path.display());
    }
    for page in pages {
        let mut send = Command::new(north);
        send.args(["fleet", "send", "--now", "--from", SENDER, COORDINATOR, &page]);
        if !run_command_within(send, None, READ_WAIT).is_some_and(|output| output.status.success()) {
            eprintln!("north capacity watchdog: fleet send {COORDINATOR} did not deliver: {page}");
        }
    }
}
