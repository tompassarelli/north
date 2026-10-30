//! `north fleet wait RUN [NAME]` registers a long run and parks the worker; `north fleet waits`
//! revives each worker whose run is over with the run and its result.
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use serde_json::Value;

use crate::error::{NorthError, NorthResult};
use crate::fleet::model::Delivery;
use crate::fleet::registry::Fleet;
use crate::fleet::runtime::SESSION_ENV;
use crate::fleet::waits::{Probe, Registered, Run};
use crate::foundation::clock::Timestamp;
use crate::platform::process::run_command_within;

use super::fleet_placement::Overrides;
use super::fleet_revive;

const USAGE: &str = "Usage: north fleet wait RUN [NAME] | north fleet waits (RUN is job:NAME[@HOST], farm:RUN_ID, branch:BRANCH or capture:PATH)";
const PROBE: Duration = Duration::from_secs(30);
const JOB_STARTED_BEFORE_MS: f64 = 30.0 * 60_000.0;

pub(super) fn run(fleet: &Fleet, verb: &str, words: &[&str]) -> NorthResult<()> {
    match (verb, words) {
        ("wait", [run]) => register(fleet, run, std::env::var(SESSION_ENV).ok().as_deref()),
        ("wait", [run, name]) => register(fleet, run, Some(name)),
        ("waits", []) => settle(fleet),
        _ => Err(NorthError::Usage(USAGE.into())),
    }
}

fn register(fleet: &Fleet, run: &str, name: Option<&str>) -> NorthResult<()> {
    let name = name.filter(|name| !name.is_empty()).ok_or_else(|| NorthError::Usage(format!("{USAGE}; NAME defaults to ${SESSION_ENV}")))?;
    fleet.session(name)?;
    let run: Run = run.parse()?;
    fleet.waits().register(name, run.clone(), &std::env::current_dir()?)?;
    println!("{name} waits on {run}: end your turn now; it parks and {run}'s end revives it with the result");
    Ok(())
}

fn settle(fleet: &Fleet) -> NorthResult<()> {
    let tell = |name: &str, text: &str| -> NorthResult<()> {
        if fleet.session(name).map_or(true, |record| record.stopped) {
            return Ok(());
        }
        fleet_revive::revive(fleet, name, None, None, Overrides::default())?;
        fleet.send(name, text, Delivery::Queued)
    };
    for line in fleet.waits().settle(&Runs, &tell) {
        println!("{line}");
    }
    Ok(())
}

struct Runs;

impl Probe for Runs {
    fn result(&self, wait: &Registered) -> Option<String> {
        match &wait.run {
            Run::Job { name, host } => job(name, host.as_deref(), wait.since_ms),
            Run::Farm(id) => farm(id, &wait.cwd),
            Run::Branch(branch) => branch_state(branch, &wait.cwd),
            Run::Capture(path) => wait.cwd.join(path).exists().then(|| "written".to_owned()),
        }
    }
}

fn output(program: &str, args: &[&str], cwd: &Path) -> Option<Value> {
    let mut command = Command::new(program);
    command.args(args).current_dir(cwd);
    let out = run_command_within(command, None, PROBE).filter(|out| out.status.success())?;
    serde_json::from_slice(&out.stdout).ok()
}

fn job(name: &str, host: Option<&str>, since_ms: u64) -> Option<String> {
    let north = std::env::current_exe().ok()?;
    let rows = output(&north.to_string_lossy(), &["threads", "job-runs", "--json"], Path::new("."))?;
    let row = rows.as_array()?.iter().filter(|row| row["name"] == name && host.is_none_or(|host| row["host"] == host)).max_by_key(|row| row["last_started"].as_str().unwrap_or_default().to_owned())?;
    let started = Timestamp::parse(row["last_started"].as_str()?).ok()?.unix_seconds_f64() * 1000.0;
    let began_after = started + JOB_STARTED_BEFORE_MS >= since_ms as f64;
    (began_after && row["open_since"].is_null()).then(|| format!("exit {}", row["last_rc"].as_i64().unwrap_or(-1)))
}

fn farm(id: &str, cwd: &Path) -> Option<String> {
    let run = output("gh", &["run", "view", id, "--json", "status,conclusion"], cwd)?;
    (run["status"] == "completed").then(|| run["conclusion"].as_str().unwrap_or("completed").to_owned())
}

fn branch_state(branch: &str, cwd: &Path) -> Option<String> {
    let pull = output("gh", &["pr", "view", branch, "--json", "state,mergeCommit"], cwd)?;
    match pull["state"].as_str()? {
        "MERGED" => Some(format!("landed {}", pull["mergeCommit"]["oid"].as_str().unwrap_or_default()).trim_end().to_owned()),
        "CLOSED" => Some("closed unmerged".to_owned()),
        _ => None,
    }
}
