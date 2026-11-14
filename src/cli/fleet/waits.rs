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
use crate::fleet::model::{Delivery, Phase};
use crate::fleet::registry::{Fleet, Waiting, worktree_of};
use crate::fleet::runtime::{SESSION_ENV, now_ms};
use crate::fleet::sender::Sender;
use crate::fleet::store::LedgerHost;
use crate::fleet::tick::{ProbeJob, Probed, WaitProbes};
use crate::fleet::limit::Parked;
use crate::fleet::reaper;
use crate::fleet::resume::{self, Waker};
use crate::platform::attribution::{self, Cause};
use crate::fleet::spares;
use crate::fleet::waits::{Probe, Registered, Run, Settled, runtime_holds};
use crate::foundation::clock::Timestamp;
use crate::grounding::distill;
use crate::threads::incidents::corrections;
use crate::pacing::placement::{config, gate};
use crate::platform::process::{run_command_within, run_within};

use super::placement::Overrides;
use super::limit::{UsageTable, Unqueued, holds_unlanded_lane, lane_of, unqueued_lane};
use super::row;
use super::ledger::Ledger;
use super::revive;

const USAGE: &str = "Usage: north fleet wait RUN [NAME] | north fleet waits (RUN is job:NAME[@HOST], farm:RUN_ID, branch:BRANCH, capture:PATH, runtime:SHA, pid:PID@START or usage:ACCOUNT@TIME)";
const PROBE: Duration = Duration::from_secs(30);
const API_SPENT: Duration = Duration::from_secs(14);
const API_READING: Duration = Duration::from_secs(14);
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
    let probed = Probes::here(fleet).jobs().into_iter().map(|job| (job.run)(fleet.clone())).fold(said([]), |mut probed, more| {
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

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum Probes {
    Fleet { reports: bool },
    Registry,
}

impl Probes {
    pub(super) fn here(fleet: &Fleet) -> Self {
        match config() {
            Ok(cfg) if gate::spool_pulled(&cfg, fleet.host()) => Self::Registry,
            Ok(cfg) => Self::Fleet { reports: gate::fleet_reports(&cfg, fleet.host()) },
            Err(_) => Self::Fleet { reports: true },
        }
    }
}

impl WaitProbes for Probes {
    fn placement(&self) -> Option<Box<dyn FnOnce() + Send>> {
        if *self == Self::Registry {
            return None;
        }
        Some(Box::new(|| {
            if let Ok(cfg) = config() {
                gate::probed(&cfg, &std::collections::BTreeMap::new());
            }
        }))
    }

    fn jobs(&self) -> Vec<ProbeJob> {
        let registry = vec![
            ProbeJob { name: "probes.settle.waits", own: None, run: Box::new(|fleet| settle_waits(&fleet)) },
            ProbeJob { name: "probes.settle.park_limited", own: None, run: Box::new(|fleet| said(park_limited(&fleet, &UsageTable, now_ms()).iter().map(Parked::line))) },
            ProbeJob { name: "probes.settle.api_spent", own: Some(API_SPENT), run: Box::new(|fleet| said(api_spent(&fleet))) },
            ProbeJob { name: "probes.settle.reserve", own: None, run: Box::new(|fleet| said(fleet.park_on_reserve())) },
            ProbeJob { name: "probes.settle.api_reading", own: Some(API_READING), run: Box::new(|_| said(super::accounts::poll_tick())) },
            ProbeJob { name: "probes.settle.budget_notices", own: None, run: Box::new(|fleet| said(super::budget_notices::watch(&fleet, &tell_tom, now_ms()))) },
            ProbeJob { name: "probes.settle.promises", own: None, run: Box::new(|fleet| said(super::promises::watch(&fleet, now_ms()))) },
            ProbeJob {
                name: "probes.settle.hand_over",
                own: None,
                run: Box::new(|fleet| {
                    let lanes = reaper::Evidence::lanes(&reaper::Here);
                    let handed = hand_over(&fleet, &UsageTable, &lanes, &|record| holds_unlanded_lane(&record.name, &lane_of(&record.cwd, |worktrees| worktree_of(record, worktrees)), &lanes), &|request| super::placement::spend_admits(&fleet, request).and_then(|()| fleet.take_over(request)), &tell_tom, now_ms());
                    said(handed.lines.into_iter().chain(handed.alert))
                }),
            },
            ProbeJob { name: "probes.settle.resume", own: None, run: Box::new(|fleet| said(resume::resume(&fleet, &super::held::read(i64::try_from(now_ms() / 1000).unwrap_or_default()), &Revival(&fleet)))) },
            ProbeJob { name: "probes.settle.reap", own: Some(2 * reaper::BUDGET), run: Box::new(|fleet| said(reaper::reap(&fleet, &reaper::Here, now_ms(), true).lines())) },
            ProbeJob { name: "probes.settle.strays", own: None, run: Box::new(|fleet| said(crate::fleet::strays::stop_strays(&fleet, now_ms()))) },
        ];
        let reports = vec![
            ProbeJob { name: "probes.settle.goals", own: None, run: Box::new(|_| said(north_concerns::tick::lines(i64::try_from(now_ms() / 1000).unwrap_or_default(), &crate::cli::concern_history::LedgerHistory))) },
            ProbeJob { name: "probes.settle.scoreboard", own: None, run: Box::new(|_| said(crate::cli::scoreboard::tick(i64::try_from(now_ms() / 1000).unwrap_or_default()))) },
            ProbeJob { name: "probes.settle.corrections", own: None, run: Box::new(|_| said(recurred_corrections(i64::try_from(now_ms() / 1000).unwrap_or_default()))) },
        ];
        let published = vec![
            ProbeJob { name: "probes.records_mirror", own: None, run: Box::new(|_| said(crate::runtime::mirror::publish())) },
            ProbeJob { name: "probes.checkouts", own: None, run: Box::new(|_| said(crate::runtime::checkouts::publish())) },
            ProbeJob { name: "probes.runtime_lag", own: None, run: Box::new(|_| said(crate::runtime::lag::publish())) },
        ];
        match self {
            Self::Fleet { reports: true } => registry.into_iter().chain(reports).chain(published).collect(),
            Self::Fleet { reports: false } => registry.into_iter().chain(published).collect(),
            Self::Registry => registry,
        }
    }
}

pub(super) fn reap(fleet: &Fleet, words: &[&str]) -> NorthResult<()> {
    let apply = match words {
        [] => true,
        ["--dry-run"] => false,
        _ => return Err(NorthError::Usage("Usage: north fleet reap [--dry-run]".into())),
    };
    let reaped = reaper::reap_after(fleet, &reaper::Here, reaper::stale_after(), now_ms(), apply);
    for kept in &reaped.kept {
        println!("kept {}: {}", kept.name, kept.why);
    }
    reaped.lines().iter().for_each(|line| println!("{line}"));
    println!("{} {}, {} kept, {} failed", if apply { "retired" } else { "would retire" }, reaped.retired.len(), reaped.kept.len(), reaped.failed.len());
    if let Ok(told) = fleet.retirement() {
        println!("graph: {} idle or parked sessions told, {} dead letters in 24 h", told.verdicts.len(), told.dead_letters_24h.unwrap_or(0));
    }
    for lane in fleet.kept_lanes_since((now_ms() / 1000).saturating_sub(24 * 3600)) {
        println!("lane {} on {} kept in the last 24 h: {}", lane.lane, lane.machine, lane.reason);
    }
    Ok(())
}

fn recurred_corrections(now: i64) -> Vec<String> {
    north_concerns::tick::announce(&north_concerns::tick::state_file("north/grounding/recurrence-tick.json"), now, || {
        let seen = distill::corrections_seen();
        let (set, text) = corrections::red(&corrections::recurrences(&seen, corrections::classes(), now as f64));
        north_concerns::tick::Red { set, text }
    })
}

fn said(lines: impl IntoIterator<Item = String>) -> Probed {
    Probed { lines: lines.into_iter().collect(), settled: Vec::new() }
}

fn settle_waits(fleet: &Fleet) -> Probed {
    let usage_parked = |name: &str| fleet.waits().of(name).iter().any(|wait| matches!(wait.run, Run::Usage { .. }));
    let tell = |name: &str, text: &str| -> NorthResult<()> {
        let Ok(record) = fleet.session(name) else { return Ok(()) };
        if record.phase() == Some(Phase::Stopped) || (record.ended() && !usage_parked(name)) {
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
    lanes: OnceCell<std::collections::BTreeMap<String, Vec<crate::land_ticket::Lane>>>,
}

impl Runs {
    fn job_rows(&self, load: impl FnOnce() -> Option<Value>) -> Option<&Value> {
        self.job_rows.get_or_init(load).as_ref()
    }

    fn unqueued(&self, session: &str, wait: &Registered) -> Option<Unqueued> {
        let lanes = self.lanes.get_or_init(|| reaper::Evidence::lanes(&reaper::Here));
        unqueued_lane(session, &wait.cwd, &crate::watch::state_home(), lanes)
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
            Run::Runtime(sha) => runtime_holds(spares::current_runtime().as_deref(), sha, &wait.cwd),
            Run::Usage { .. } => UsageWait::of(wait)?.lifted(&self.observed.get_or_init(|| UsageTable.observed(now_ms())).readings, now_ms()),
            Run::Process { pid, started } => (!detached::alive(Path::new("/proc"), *pid, *started)).then(|| "ended".to_owned()),
        }
    }

    fn precedence(&self, session: &str, wait: &Registered) -> u8 {
        match wait.run {
            Run::Usage { .. } => match self.unqueued(session, wait) {
                Some(Unqueued::SentBack { .. }) => 0,
                Some(Unqueued::NeverEnqueued) => 1,
                None => 2,
            },
            _ => 2,
        }
    }

    fn note(&self, session: &str, wait: &Registered) -> Option<String> {
        if !matches!(wait.run, Run::Usage { .. }) {
            return None;
        }
        match self.unqueued(session, wait)? {
            Unqueued::SentBack { branch, cause } => {
                let verdict = cause.map(|cause| format!(" with the verdict {cause}")).unwrap_or_default();
                Some(format!("The lander sent back your lane `{branch}`{verdict}: fix what its verdict names before you re-enqueue it."))
            }
            Unqueued::NeverEnqueued => None,
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

fn api_spent(fleet: &Fleet) -> Vec<String> {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_default();
    crate::providers::claude::api_pool::move_spent(fleet, &crate::providers::claude::fleet::Claude, &crate::platform::orchestration::path(), &home, &UsageTable)
}

struct Revival<'a>(&'a Fleet);

impl Waker for Revival<'_> {
    fn revive(&self, name: &str) -> NorthResult<()> {
        revive::revive(self.0, name, None, None, Overrides::default())
    }

    fn attribute(&self, name: &str, cause: Cause, reason: &str) {
        attribution::record(name, "resume", cause, reason);
    }
}

#[cfg(test)]
#[path = "waits_tests.rs"]
mod tests;
