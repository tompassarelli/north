//! `north fleet wait RUN [NAME]` registers a long run and parks the worker; `north fleet waits`
//! revives each worker whose run is over with the run and its result.
use std::cell::OnceCell;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use serde_json::Value;

use crate::error::{NorthError, NorthResult};
use crate::fleet::detached;
use crate::fleet::handover::{AccountReadings, Observed, hand_over};
use crate::fleet::limit::{UsageWait, park_limited};
use crate::fleet::model::{Delivery, Ending};
use crate::fleet::registry::{Fleet, Waiting};
use crate::fleet::runtime::{SESSION_ENV, now_ms};
use crate::fleet::sender::Sender;
use crate::fleet::store::LedgerHost;
use crate::fleet::tick::{ProbeJob, Probed, WaitProbes};
use crate::fleet::limit::Parked;
use crate::fleet::waits::{Probe, Registered, Run, Settled};
use crate::foundation::clock::Timestamp;
use crate::pacing::placement::{config, gate};
use crate::platform::process::{run_command_within, run_within};

use super::placement::Overrides;
use super::limit::{UsageTable, holds_unlanded_lane, holds_unqueued_lane};
use super::row;
use super::ledger::Ledger;
use super::revive;

const USAGE: &str = "Usage: north fleet wait RUN [NAME] | north fleet waits (RUN is job:NAME[@HOST], farm:RUN_ID, branch:BRANCH, capture:PATH, pid:PID@START or usage:ACCOUNT@TIME)";
const PROBE: Duration = Duration::from_secs(30);
const ALERT: Duration = Duration::from_secs(30);
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
    let run: Run = run.parse()?;
    let host = match fleet.register_wait(name, &run, &std::env::current_dir()?)? {
        Waiting::Here => None,
        Waiting::Spooled { host, .. } => Some(host),
    };
    if let Some(request) = run.request(name)
        && let Err(error) = row::record_wait(&request)
    {
        eprintln!("north fleet: {run} is registered but not in the fleet store: {}", error.user_message());
    }
    let settled_by = host.map(|host| format!(" on {host}")).unwrap_or_default();
    println!("{name} waits on {run}: end your turn now; it parks and {run}'s end revives it with the result, settled by `north fleet waits`{settled_by}");
    Ok(())
}

fn settle(fleet: &Fleet) -> NorthResult<()> {
    let probed = Probes.jobs().into_iter().map(|job| (job.run)(fleet.clone())).fold(said([]), |mut probed, more| {
        probed.lines.extend(more.lines);
        probed.settled.extend(more.settled);
        probed
    });
    probed.lines.iter().for_each(|line| println!("{line}"));
    if probed.settled.is_empty() {
        return Ok(());
    }
    Ledger.settle(&probed.settled).inspect_err(|_| {
        let left: Vec<_> = fleet.unsettled_waits().into_iter().chain(probed.settled.iter().cloned()).collect();
        if let Err(error) = fleet.set_unsettled_waits(&left) {
            eprintln!("north fleet: waits not kept for the next tick: {}", error.user_message());
        }
    })
}

pub(super) struct Probes;

impl WaitProbes for Probes {
    fn prepare(&self, _: &Fleet) {
        if let Ok(cfg) = config() {
            gate::probed(&cfg, &std::collections::BTreeMap::new());
        }
    }

    fn jobs(&self) -> Vec<ProbeJob> {
        vec![
            ProbeJob { name: "probes.settle.waits", run: Box::new(|fleet| settle_waits(&fleet)) },
            ProbeJob { name: "probes.settle.park_limited", run: Box::new(|fleet| said(park_limited(&fleet, &UsageTable, now_ms()).iter().map(Parked::line))) },
            ProbeJob { name: "probes.settle.breaker", run: Box::new(|fleet| said(super::breaker::watch(&fleet, &tell_tom, now_ms()))) },
            ProbeJob { name: "probes.settle.goals", run: Box::new(|_| said(north_goals::tick::lines(i64::try_from(now_ms() / 1000).unwrap_or_default()))) },
            ProbeJob {
                name: "probes.settle.hand_over",
                run: Box::new(|fleet| {
                    let handed = hand_over(&fleet, &UsageTable, &|record| holds_unlanded_lane(&record.cwd), &|request| fleet.take_over(request), &tell_tom, now_ms());
                    said(handed.lines.into_iter().chain(handed.alert))
                }),
            },
        ]
    }
}

fn said(lines: impl IntoIterator<Item = String>) -> Probed {
    Probed { lines: lines.into_iter().collect(), settled: Vec::new() }
}

fn settle_waits(fleet: &Fleet) -> Probed {
    let usage_parked = |name: &str| fleet.waits().of(name).iter().any(|wait| matches!(wait.run, Run::Usage { .. }));
    let tell = |name: &str, text: &str| -> NorthResult<()> {
        let Ok(record) = fleet.session(name) else { return Ok(()) };
        if record.ended == Some(Ending::Stopped) || (record.stopped && !usage_parked(name)) {
            return Ok(());
        }
        revive::revive(fleet, name, None, None, Overrides::default())?;
        fleet.send_signed(&Sender::fleet(), name, text, Delivery::Queued)
    };
    let exists = |name: &str| fleet.session(name).is_ok();
    let told = fleet.waits().settle(&Runs::default(), &exists, &tell);
    Probed { lines: told.iter().map(Settled::line).collect(), settled: told.iter().filter_map(|settled| settled.run.request(&settled.session)).map(|request| request.key()).collect() }
}

fn tell_tom(line: &str) -> Result<(), String> {
    match run_within("nexus-alert", &["normal", line, "north fleet tick"], ALERT) {
        Some(out) if out.status.success() => Ok(()),
        Some(out) => Err(format!("nexus-alert exited {}: {}", out.status, String::from_utf8_lossy(&out.stderr).trim())),
        None => Err(format!("nexus-alert did not run or did not answer within {}s", ALERT.as_secs())),
    }
}

#[derive(Default)]
struct Runs {
    job_rows: OnceCell<Option<Value>>,
    observed: OnceCell<Observed>,
}

impl Runs {
    fn job_rows(&self, load: impl FnOnce() -> Option<Value>) -> Option<&Value> {
        self.job_rows.get_or_init(load).as_ref()
    }
}

fn load_job_rows() -> Option<Value> {
    let north = std::env::current_exe().ok()?;
    output(&north.to_string_lossy(), &["threads", "job-runs", "--json"], Path::new("."))
}

impl Probe for Runs {
    fn result(&self, wait: &Registered) -> Option<String> {
        match &wait.run {
            Run::Job { name, host } => job(self.job_rows(load_job_rows)?, name, host.as_deref(), wait.since_ms),
            Run::Farm(id) => farm(id, &wait.cwd),
            Run::Branch(branch) => branch_state(branch, &wait.cwd),
            Run::Capture(path) => wait.cwd.join(path).exists().then(|| "written".to_owned()),
            Run::Usage { .. } => UsageWait::of(wait)?.lifted(&self.observed.get_or_init(|| UsageTable.observed(now_ms())).readings, now_ms()),
            Run::Process { pid, started } => (!detached::alive(Path::new("/proc"), *pid, *started)).then(|| "ended".to_owned()),
        }
    }

    fn precedence(&self, session: &str, wait: &Registered) -> u8 {
        match wait.run {
            Run::Usage { .. } if holds_unqueued_lane(session, &wait.cwd) => 0,
            _ => 1,
        }
    }
}

fn output(program: &str, args: &[&str], cwd: &Path) -> Option<Value> {
    let mut command = Command::new(program);
    command.args(args).current_dir(cwd);
    let out = run_command_within(command, None, PROBE).filter(|out| out.status.success())?;
    serde_json::from_slice(&out.stdout).ok()
}

fn job(rows: &Value, name: &str, host: Option<&str>, since_ms: u64) -> Option<String> {
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

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    #[test]
    fn the_job_ledger_is_fetched_once_for_every_job_wait_of_a_pass() {
        let runs = Runs::default();
        let fetched = Cell::new(0);
        let load = || {
            fetched.set(fetched.get() + 1);
            Some(serde_json::json!([]))
        };
        assert!(runs.job_rows(load).is_some() && runs.job_rows(load).is_some() && runs.job_rows(load).is_some());
        assert_eq!(fetched.get(), 1);
        let failed = Runs::default();
        let tries = Cell::new(0);
        let load = || {
            tries.set(tries.get() + 1);
            None
        };
        assert!(failed.job_rows(load).is_none() && failed.job_rows(load).is_none());
        assert_eq!(tries.get(), 1, "a failed fetch is not retried within the pass");
    }

}
