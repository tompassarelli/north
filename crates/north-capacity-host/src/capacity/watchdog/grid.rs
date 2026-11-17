use std::path::Path;
use std::process::Command;
use std::time::Duration;

use north_capacity::capacity::runner_alarm::{HostReading, LeaseUse};
use serde_json::Value;

use north_base::platform::process::run_command_within;

const READ_WAIT: Duration = Duration::from_secs(20);

pub(super) fn print_reading() -> Result<(), String> {
    println!("{}", serde_json::to_string(&reading()).map_err(|error| error.to_string())?);
    Ok(())
}

fn reading() -> HostReading {
    let mut systemctl = Command::new("systemctl");
    systemctl.args(["list-units", "--state=active", "--plain", "--no-legend", "--no-pager", "buildbarn-runner-*"]);
    let runners = run_command_within(systemctl, None, READ_WAIT).filter(|output| output.status.success()).map(|output| String::from_utf8_lossy(&output.stdout).lines().filter(|line| line.contains(".service")).count());
    HostReading { host: north_base::platform::host::name(), runners, land_checks: land_checks(&north_capacity::capacity::local_machine::runtime_root().join("leases")) }
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
