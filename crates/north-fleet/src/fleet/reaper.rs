use std::collections::BTreeSet;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::LazyLock;
use std::time::{Duration, Instant, SystemTime};

use regex::Regex;
use serde::Serialize;
use toml_edit::DocumentMut;

use super::detached::{self, Prior, Scope};
use super::host::alive;
use super::model::{Entry, EntryKind, Role, SessionRecord, Status};
use super::receipts::is_final;
use super::registry::Fleet;
use super::runtime::binary;
use super::transcript::{session_stream_path, stream_path};

const STALE_KEY: &str = "fleet_stale_hours";
pub const RECORD: &str = ".reap.json";
const LAST_CHARS: usize = 4_000;
const LEASE_BATCH: usize = 16;
const BUDGET: Duration = Duration::from_secs(5);

static DEFAULTS: LazyLock<&DocumentMut> = LazyLock::new(|| north_base::platform::harness::document(north_base::platform::harness::DEFAULTS));
static STOP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?im)^\W*Stop:").expect("stop line pattern"));
static SHA: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:^|[^0-9A-Za-z_#/.-])([0-9a-f]{7,40})(?:$|[^0-9A-Za-z_-])").expect("sha pattern"));

pub fn stale_after() -> Duration {
    let hours = DEFAULTS[STALE_KEY]["value"].as_integer().and_then(|hours| u64::try_from(hours).ok()).unwrap_or_else(|| panic!("harness/defaults.toml has no whole hours for {STALE_KEY}"));
    Duration::from_secs(hours * 3600)
}

pub trait Evidence: Sync {
    fn landed(&self, cwd: &Path, sha: &str) -> bool;
    fn running(&self, records: &[&SessionRecord]) -> BTreeSet<String>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Report {
    pub text: String,
    pub taken: bool,
    pub unlanded: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Seen {
    pub status: Status,
    pub quiet: Duration,
    pub report: Option<Report>,
    pub live: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Verdict {
    Keep,
    Finished,
    Stale,
}

impl Verdict {
    fn why(&self, stale: Duration) -> String {
        match self {
            Self::Keep => "kept".into(),
            Self::Finished => "its parent took its final report".into(),
            Self::Stale => format!("quiet over {}h with nothing left to wake it", stale.as_secs() / 3600),
        }
    }
}

pub fn verdict(seen: &Seen, stale: Duration) -> Verdict {
    if !matches!(seen.status, Status::Idle | Status::Parked) || seen.live.is_some() {
        return Verdict::Keep;
    }
    if seen.report.as_ref().is_some_and(|report| report.taken && finishes(&report.text) && report.unlanded.is_empty()) {
        return Verdict::Finished;
    }
    if seen.quiet >= stale { Verdict::Stale } else { Verdict::Keep }
}

fn finishes(report: &str) -> bool {
    opens(report, "Done") || (opens(report, "Not done") || opens(report, "Split")) && STOP.is_match(report)
}

fn opens(report: &str, word: &str) -> bool {
    let opening = report.trim_start_matches(['*', '#', '`', ' ', '\n']);
    opening.strip_prefix(word).is_some_and(|rest| !rest.starts_with(|c: char| c.is_alphanumeric()))
}

pub fn named_shas(report: &str) -> Vec<String> {
    if !opens(report, "Done") {
        return Vec::new();
    }
    let mut shas: Vec<String> = Vec::new();
    for found in SHA.captures_iter(report) {
        if !shas.iter().any(|sha| sha == &found[1]) {
            shas.push(found[1].to_owned());
        }
    }
    shas
}

pub fn final_report(entries: &[Entry]) -> Option<&str> {
    let at = entries.iter().rposition(|entry| entry.kind == EntryKind::Assistant && is_final(&entry.text))?;
    let instructed = entries[at + 1..].iter().any(|entry| entry.kind == EntryKind::User && instruction(&entry.text));
    (!instructed).then(|| entries[at].text.as_str())
}

fn instruction(text: &str) -> bool {
    let text = text.trim_start();
    !(text.starts_with("<task-notification>") || text.starts_with("[from service:"))
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct Reaped {
    pub at_ms: u64,
    pub stale_hours: u64,
    pub retired: Vec<Retired>,
    pub failed: Vec<Retired>,
    pub kept: Vec<Retired>,
    pub deferred: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Retired {
    pub name: String,
    pub why: String,
}

impl Reaped {
    pub fn lines(&self) -> Vec<String> {
        let retired = self.retired.iter().map(|retired| format!("retired {}: {}", retired.name, retired.why));
        let failed = self.failed.iter().map(|failed| format!("not retired {}: {}", failed.name, failed.why));
        let deferred = (self.deferred > 0).then(|| format!("deferred {} sessions to the next tick: the reaper spent its {}s", self.deferred, BUDGET.as_secs()));
        retired.chain(failed).chain(deferred).collect()
    }
}

pub fn reap(fleet: &Fleet, evidence: &dyn Evidence, now_ms: u64, apply: bool) -> Reaped {
    reap_within(fleet, evidence, stale_after(), now_ms, apply, BUDGET)
}

pub fn reap_after(fleet: &Fleet, evidence: &dyn Evidence, stale: Duration, now_ms: u64, apply: bool) -> Reaped {
    reap_within(fleet, evidence, stale, now_ms, apply, Duration::MAX)
}

pub fn reap_within(fleet: &Fleet, evidence: &dyn Evidence, stale: Duration, now_ms: u64, apply: bool, budget: Duration) -> Reaped {
    let started = Instant::now();
    let mut reaped = Reaped { at_ms: now_ms, stale_hours: stale.as_secs() / 3600, ..Reaped::default() };
    let records = fleet.sessions().unwrap_or_default();
    let mut candidates: Vec<(&SessionRecord, Status, Duration)> = records.iter().filter(|record| record.role() != Role::Proxy && !record.adopted() && !record.verification && !record.stopped).map(|record| (record, fleet.status(record), quiet(record))).filter(|(_, status, _)| matches!(status, Status::Idle | Status::Parked)).collect();
    candidates.sort_by_key(|(_, _, quiet)| std::cmp::Reverse(*quiet));
    let running = evidence.running(&candidates.iter().map(|(record, _, _)| *record).collect::<Vec<_>>());
    let gated = fleet.open_gate_sessions();
    for (done, batch) in candidates.chunks(LEASE_BATCH).enumerate() {
        if started.elapsed() >= budget {
            reaped.deferred = candidates.len() - done * LEASE_BATCH;
            break;
        }
        let mut due: Vec<(&SessionRecord, String, Option<String>)> = Vec::new();
        for &(record, status, quiet) in batch {
            let live = fleet.live_wait(record, &records, &gated, running.contains(&record.name));
            let entries = if live.is_some() { Vec::new() } else { fleet.follow_recent(&record.name).map(|follower| follower.transcript().entries.clone()).unwrap_or_default() };
            let report = final_report(&entries).map(|text| Report { text: text.to_owned(), taken: fleet.report_taken(record), unlanded: named_shas(text).into_iter().filter(|sha| !evidence.landed(&record.cwd, sha)).collect() });
            let seen = Seen { status, quiet, report, live };
            let verdict = verdict(&seen, stale);
            if verdict == Verdict::Keep {
                let why = seen.live.or_else(|| seen.report.filter(|report| !report.unlanded.is_empty()).map(|report| format!("names {} not on origin/main", report.unlanded.join(", ")))).unwrap_or_else(|| "active within the stale window".into());
                reaped.kept.push(Retired { name: record.name.clone(), why });
                continue;
            }
            let last = entries.iter().rfind(|entry| entry.kind == EntryKind::Assistant).map(|entry| entry.text.chars().take(LAST_CHARS).collect::<String>());
            due.push((record, verdict.why(stale), last));
        }
        let leased = leased(fleet, &due.iter().map(|(record, _, _)| *record).collect::<Vec<_>>());
        for ((record, why, last), leased) in due.into_iter().zip(leased) {
            if leased {
                reaped.kept.push(Retired { name: record.name.clone(), why: "a job-run job still holds a lease on it".into() });
                continue;
            }
            let retired = if apply { fleet.retire(&record.name, &why, last.as_deref()).map(drop) } else { Ok(()) };
            match retired {
                Ok(()) => reaped.retired.push(Retired { name: record.name.clone(), why }),
                Err(error) => reaped.failed.push(Retired { name: record.name.clone(), why: error.user_message() }),
            }
        }
    }
    if apply && let Ok(text) = serde_json::to_vec_pretty(&reaped) {
        let _ = std::fs::write(fleet.root.join(RECORD), text);
    }
    reaped
}

fn leased(fleet: &Fleet, records: &[&SessionRecord]) -> Vec<bool> {
    std::thread::scope(|scope| {
        let asking: Vec<_> = records.iter().map(|record| scope.spawn(|| fleet.job_leased(record))).collect();
        asking.into_iter().map(|asked| asked.join().unwrap_or(true)).collect()
    })
}

fn quiet(record: &SessionRecord) -> Duration {
    let modified = |path: &Path| std::fs::metadata(path).and_then(|meta| meta.modified()).ok();
    let last = modified(&session_stream_path(&record.host_dir, &record.name)).or_else(|| modified(&stream_path(&record.host_dir)));
    last.and_then(|at| SystemTime::now().duration_since(at).ok()).unwrap_or(Duration::MAX)
}

pub struct Here;

impl Evidence for Here {
    fn landed(&self, cwd: &Path, sha: &str) -> bool {
        cwd.is_dir() && Command::new(binary("git")).arg("-C").arg(cwd).args(["merge-base", "--is-ancestor", sha, "origin/main"]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|status| status.success())
    }

    fn running(&self, records: &[&SessionRecord]) -> BTreeSet<String> {
        let (proc, cgroups) = (Path::new("/proc"), Path::new("/sys/fs/cgroup"));
        let mut running = BTreeSet::new();
        let mut units: Vec<(String, String)> = Vec::new();
        for record in records {
            if let Some(host) = record.host_pid.filter(|pid| !record.stopped && alive(*pid)) {
                let detached = detached::Procfs::new(proc.into(), cgroups.into(), host).after(Prior::all(&record.host_dir)).detached();
                if detached::cli_work(proc, cgroups, host) > 0 || !detached.is_empty() {
                    running.insert(record.name.clone());
                }
                continue;
            }
            let scope = std::fs::read_to_string(record.host_dir.join("host.scope")).ok().map(|unit| unit.trim().to_owned()).filter(|unit| !unit.is_empty());
            units.extend(scope.into_iter().chain(Prior::all(&record.host_dir).into_iter().map(|prior| prior.unit)).map(|unit| (record.name.clone(), unit)));
        }
        if units.is_empty() {
            return running;
        }
        let asked = Command::new(binary("systemctl")).args(["--user", "is-active"]).args(units.iter().map(|(_, unit)| unit)).stdin(Stdio::null()).stderr(Stdio::null()).output();
        let states: Vec<String> = asked.map(|answer| String::from_utf8_lossy(&answer.stdout).lines().map(str::to_owned).collect()).unwrap_or_default();
        for (index, (name, _)) in units.into_iter().enumerate() {
            if states.get(index).is_none_or(|state| state != "inactive" && state != "failed") {
                running.insert(name);
            }
        }
        running
    }
}

#[cfg(test)]
#[path = "reaper_tests.rs"]
mod tests;
