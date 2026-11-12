use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::LazyLock;
use std::time::{Duration, Instant, SystemTime};

use north_base::land_ticket::{Lane, lanes_by_session};
use north_base::platform::{land_queue, records_mirror};
use north_session_host::held::{self, unit};
use regex::Regex;
use toml_edit::DocumentMut;

use super::detached::{self, Prior, Scope};
use super::jobs::Lease;
use super::model::{Entry, EntryKind, Phase, Role, SessionRecord, Status};
use super::receipts::is_final;
use super::registry::{Fleet, worktree_of};
use super::resume::{Landing, ended_ms, landing};
use super::runtime::binary;
use super::transcript::last_written;

const STALE_KEY: &str = "fleet_stale_hours";
const LAST_CHARS: usize = 4_000;
const LEASE_BATCH: usize = 16;
pub const BUDGET: Duration = Duration::from_secs(5);
const PROBE_SECS: u64 = 15;

static DEFAULTS: LazyLock<&DocumentMut> = LazyLock::new(|| north_base::platform::harness::document(north_base::platform::harness::DEFAULTS));
static STOP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?im)^\W*Stop:").expect("stop line pattern"));
static SHA: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:^|[^0-9A-Za-z_#/.-])([0-9a-f]{7,40})(?:$|[^0-9A-Za-z_-])").expect("sha pattern"));

pub fn stale_after() -> Duration {
    let hours = DEFAULTS[STALE_KEY]["value"].as_integer().and_then(|hours| u64::try_from(hours).ok()).unwrap_or_else(|| panic!("harness/defaults.toml has no whole hours for {STALE_KEY}"));
    Duration::from_secs(hours * 3600)
}

pub trait Evidence: Sync {
    fn landed(&self, cwd: &Path, sha: &str) -> bool;
    fn running(&self, records: &[&SessionRecord]) -> BTreeMap<String, Work>;
    fn lanes(&self) -> BTreeMap<String, Vec<Lane>>;
    fn lane(&self, record: &SessionRecord) -> Option<Unlanded>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Ahead {
    pub path: PathBuf,
    pub tip: String,
    pub commits: usize,
}

type Digests = BTreeMap<String, Vec<(String, Unlanded)>>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Unlanded {
    Ahead(Ahead),
    Unread(PathBuf),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Work {
    Cli,
    Detached,
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
    pub background: bool,
    pub ended: bool,
    pub lane_holds: bool,
    pub lane_ahead: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Verdict {
    Keep,
    Finished,
    Ended,
    Stale,
}

impl Verdict {
    fn why(&self, stale: Duration) -> String {
        match self {
            Self::Keep => "kept".into(),
            Self::Finished => "its parent took its final report".into(),
            Self::Ended => "its host ended it at its idle exit as finished".into(),
            Self::Stale => format!("quiet over {}h with nothing left to wake it", stale.as_secs() / 3600),
        }
    }
}

pub fn verdict(seen: &Seen, stale: Duration) -> Verdict {
    if seen.live.is_some() {
        return Verdict::Keep;
    }
    if seen.lane_holds && seen.quiet < stale {
        return Verdict::Keep;
    }
    if seen.ended {
        return Verdict::Ended;
    }
    if seen.status == Status::Exited {
        return if seen.quiet >= stale && !seen.background { Verdict::Stale } else { Verdict::Keep };
    }
    if !matches!(seen.status, Status::Idle | Status::Parked) {
        return Verdict::Keep;
    }
    if seen.report.as_ref().is_some_and(|report| finished(seen, report)) {
        return Verdict::Finished;
    }
    if seen.quiet >= stale && !seen.background { Verdict::Stale } else { Verdict::Keep }
}

fn finishes(report: &str) -> bool {
    opens(report, "Done") || (opens(report, "Not done") || opens(report, "Split") || opens(report, "Blocked")) && STOP.is_match(report)
}

fn finished(seen: &Seen, report: &Report) -> bool {
    report.taken && finishes(&report.text) && !(seen.lane_ahead && waits(&report.text)) && report.unlanded.is_empty() && (opens(&report.text, "Done") || !seen.background)
}

fn waits(report: &str) -> bool {
    opens(report, "Not done") || opens(report, "Blocked")
}

fn owes_landing(lanes: &[Lane], ended_ms: u64) -> bool {
    landing(lanes, ended_ms) != Landing::Settled
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

pub fn reported_done(entries: &[Entry]) -> bool {
    final_report(entries).is_some_and(|text| opens(text, "Done"))
}

fn instruction(text: &str) -> bool {
    let text = text.trim_start();
    !(text.starts_with("<task-notification>") || text.starts_with("[from service:"))
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Reaped {
    pub at_ms: u64,
    pub stale_hours: u64,
    pub retired: Vec<Retired>,
    pub failed: Vec<Retired>,
    pub kept: Vec<Retired>,
    pub deferred: usize,
    pub unread: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Retired {
    pub name: String,
    pub why: String,
}

impl Reaped {
    pub fn lines(&self) -> Vec<String> {
        let retired = self.retired.iter().map(|retired| format!("retired {}: {}", retired.name, retired.why));
        let failed = self.failed.iter().map(|failed| format!("not retired {}: {}", failed.name, failed.why));
        let deferred = (self.deferred > 0).then(|| format!("deferred {} sessions to the next tick: the reaper spent its {}s", self.deferred, BUDGET.as_secs()));
        let unread = (self.unread > 0).then(|| format!("deferred {} sessions to the next tick: their job leases did not answer within the reaper's budget", self.unread));
        retired.chain(failed).chain(deferred).chain(unread).collect()
    }
}

pub fn reap(fleet: &Fleet, evidence: &dyn Evidence, now_ms: u64, apply: bool) -> Reaped {
    let started = Instant::now();
    reap_within(fleet, evidence, stale_after(), now_ms, apply, BUDGET, &|| started.elapsed())
}

pub fn reap_after(fleet: &Fleet, evidence: &dyn Evidence, stale: Duration, now_ms: u64, apply: bool) -> Reaped {
    reap_within(fleet, evidence, stale, now_ms, apply, Duration::MAX, &|| Duration::ZERO)
}

pub fn reap_within(fleet: &Fleet, evidence: &dyn Evidence, stale: Duration, now_ms: u64, apply: bool, budget: Duration, spent: &dyn Fn() -> Duration) -> Reaped {
    let mut reaped = Reaped { at_ms: now_ms, stale_hours: stale.as_secs() / 3600, ..Reaped::default() };
    let records = fleet.sessions().unwrap_or_default();
    let mut candidates: Vec<(&SessionRecord, Status, Duration)> = records.iter().filter(|record| record.role() != Role::Proxy && !record.adopted() && !record.verification).map(|record| (record, fleet.status(record), quiet(record))).filter(|(record, status, quiet)| ended_by_host(record) || !record.ended() && matches!(status, Status::Idle | Status::Parked) || *status == Status::Exited && *quiet >= stale).collect();
    candidates.sort_by_key(|(_, _, quiet)| std::cmp::Reverse(*quiet));
    let running = evidence.running(&candidates.iter().map(|(record, _, _)| *record).collect::<Vec<_>>());
    let gated = fleet.open_gate_sessions();
    let mut verdicts: BTreeMap<String, String> = BTreeMap::new();
    let lanes = evidence.lanes();
    let mut unlanded: Digests = BTreeMap::new();
    for (done, batch) in candidates.chunks(LEASE_BATCH).enumerate() {
        if spent() >= budget {
            reaped.deferred = candidates.len() - done * LEASE_BATCH;
            break;
        }
        let mut due: Vec<(&SessionRecord, String, Option<String>, Option<Ahead>)> = Vec::new();
        for &(record, status, quiet) in batch {
            let (ended, work) = (ended_by_host(record), running.get(&record.name).copied());
            let live = if ended { fleet.live_hold(record, &records, &gated) } else { fleet.live_wait(record, &records, &gated, work == Some(Work::Detached)) };
            let entries = if live.is_some() { Vec::new() } else { fleet.follow_recent(&record.name).map(|follower| follower.transcript().entries.clone()).unwrap_or_default() };
            let report = final_report(&entries).map(|text| Report { text: text.to_owned(), taken: fleet.report_taken(record), unlanded: named_shas(text).into_iter().filter(|sha| !evidence.landed(&record.cwd, sha)).collect() });
            let lane_holds = lanes.get(&record.name).is_some_and(|held| owes_landing(held, ended_ms(record).unwrap_or(u64::MAX)));
            let mut seen = Seen { status, quiet, report, live, background: work == Some(Work::Cli), ended, lane_holds, lane_ahead: false };
            let mut judged = verdict(&seen, stale);
            let probed = if judged == Verdict::Keep { None } else { evidence.lane(record) };
            if let Some(Unlanded::Unread(path)) = &probed {
                reaped.kept.push(Retired { name: record.name.clone(), why: format!("its lane {} did not answer within {PROBE_SECS} s", path.display()) });
                if let Some(parent) = record.parent.as_deref() {
                    unlanded.entry(parent.to_owned()).or_default().push((record.name.clone(), Unlanded::Unread(path.clone())));
                }
                continue;
            }
            let ahead = match probed {
                Some(Unlanded::Ahead(ahead)) => Some(ahead),
                _ => None,
            };
            if judged == Verdict::Finished && ahead.is_some() {
                seen.lane_ahead = true;
                judged = verdict(&seen, stale);
            }
            if let Some(label) = told(&judged, status) {
                verdicts.insert(record.name.clone(), label.to_owned());
            }
            if judged == Verdict::Keep {
                let background = seen.background.then(|| "its CLI still runs a background task".to_owned());
                let why = seen.live.or_else(|| seen.report.as_ref().filter(|report| !report.unlanded.is_empty()).map(|report| format!("names {} not on origin/main", report.unlanded.join(", ")))).or_else(|| (seen.lane_holds && quiet < stale).then(|| "its queued lane waits on the lander, or the lander's verdict has not woken it yet".to_owned())).or(background).unwrap_or_else(|| "active within the stale window".into());
                reaped.kept.push(Retired { name: record.name.clone(), why });
                continue;
            }
            let last = entries.iter().rfind(|entry| entry.kind == EntryKind::Assistant && is_final(&entry.text)).or_else(|| entries.iter().rfind(|entry| entry.kind == EntryKind::Assistant)).map(|entry| entry.text.chars().take(LAST_CHARS).collect::<String>());
            due.push((record, judged.why(stale), last, ahead));
        }
        let leases = fleet.job_leases(&due.iter().map(|(record, _, _, _)| *record).collect::<Vec<_>>(), now_ms, budget.saturating_sub(spent()));
        for ((record, why, last, ahead), lease) in due.into_iter().zip(leases) {
            retire_unleased(fleet, (&mut reaped, &mut unlanded), apply, (record, why, last, ahead), lease);
        }
    }
    if apply {
        fleet.tell_retirement_digests(unlanded.iter().map(|(parent, lanes)| (parent.clone(), lanes.iter().map(digest_line).collect())).collect(), now_ms);
    }
    if apply && let Err(why) = fleet.tell_retirement(verdicts, now_ms) {
        eprintln!("fleet reaper: graph tick failed: {why}");
    }
    reaped
}

fn told(verdict: &Verdict, status: Status) -> Option<&'static str> {
    match (verdict, status) {
        (Verdict::Stale, _) => Some("stale"),
        (_, Status::Idle) => Some("idle"),
        (_, Status::Parked) => Some("parked"),
        _ => None,
    }
}

fn retire_unleased(fleet: &Fleet, (reaped, unlanded): (&mut Reaped, &mut Digests), apply: bool, (record, why, last, ahead): (&SessionRecord, String, Option<String>, Option<Ahead>), lease: Lease) {
    let name = record.name.clone();
    match lease {
        Lease::Held => reaped.kept.push(Retired { name, why: "a job-run job still holds a lease on it".into() }),
        Lease::Unknown => {
            reaped.unread += 1;
            reaped.kept.push(Retired { name, why: "its job lease did not answer in time; the next tick reads it again".into() });
        }
        Lease::Free => {
            let retired = if apply { fleet.retire(&name, &why, last.as_deref()).map(drop) } else { Ok(()) };
            match retired {
                Ok(()) => {
                    if apply && let (Some(ahead), Some(parent)) = (ahead, record.parent.as_deref()) {
                        unlanded.entry(parent.to_owned()).or_default().push((name.clone(), Unlanded::Ahead(ahead)));
                    }
                    reaped.retired.push(Retired { name, why });
                }
                Err(error) => reaped.failed.push(Retired { name, why: error.user_message() }),
            }
        }
    }
}

fn digest_line((name, lane): &(String, Unlanded)) -> String {
    match lane {
        Unlanded::Ahead(ahead) => format!("\n- {name}: retired; {} at {}, {} commits ahead of origin/main", ahead.path.display(), ahead.tip, ahead.commits),
        Unlanded::Unread(path) => format!("\n- {name}: kept; its lane {} did not answer within {PROBE_SECS} s", path.display()),
    }
}

fn ended_by_host(record: &SessionRecord) -> bool {
    record.phase() == Some(Phase::IdleExited)
}

fn quiet(record: &SessionRecord) -> Duration {
    last_written(record).and_then(|at| SystemTime::now().duration_since(at).ok()).unwrap_or(Duration::MAX)
}

pub struct Here;

impl Evidence for Here {
    fn landed(&self, cwd: &Path, sha: &str) -> bool {
        cwd.is_dir() && Command::new(binary("git")).arg("-C").arg(cwd).args(["merge-base", "--is-ancestor", sha, "origin/main"]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|status| status.success())
    }

    fn lanes(&self) -> BTreeMap<String, Vec<Lane>> {
        lanes_by_session(&land_queue::queues(&records_mirror::state_home())).unwrap_or_default()
    }

    fn lane(&self, record: &SessionRecord) -> Option<Unlanded> {
        let parent = record.cwd.parent()?;
        let path = if parent.file_name().is_some_and(|name| name == "worktrees") { record.cwd.clone() } else { worktree_of(record, &parent.join("worktrees"))? };
        let common = path.parent()?.parent()?.join("main/.git");
        let git_dir = common.join("worktrees").join(path.file_name()?);
        let git = format!("cd / && GIT_DIR='{}' GIT_COMMON_DIR='{}' git", git_dir.display(), common.display());
        let probe = format!("{git} rev-list --count origin/main..HEAD && {git} rev-parse --short=10 HEAD");
        let bound = PROBE_SECS.to_string();
        let output = match &record.machine {
            Some(machine) => Command::new(binary("timeout")).args([bound.as_str(), "ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=5", &machine.ssh, &probe]).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?,
            None => Command::new(binary("timeout")).args([bound.as_str(), "sh", "-c", &probe]).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?,
        };
        if output.status.code() == Some(124) {
            return Some(Unlanded::Unread(path));
        }
        let text = String::from_utf8(output.stdout).ok().filter(|_| output.status.success())?;
        let mut lines = text.lines();
        let commits = lines.next()?.trim().parse::<usize>().ok().filter(|commits| *commits > 0)?;
        Some(Unlanded::Ahead(Ahead { path, tip: lines.next()?.trim().to_owned(), commits }))
    }

    fn running(&self, records: &[&SessionRecord]) -> BTreeMap<String, Work> {
        let (proc, cgroups) = (Path::new("/proc"), Path::new("/sys/fs/cgroup"));
        let mut running = BTreeMap::new();
        let mut units: Vec<(String, String)> = Vec::new();
        let mut placed: BTreeMap<&str, BTreeMap<String, String>> = BTreeMap::new();
        for record in records {
            if let Some(machine) = record.machine.as_ref().filter(|_| !record.ended()) {
                placed.entry(machine.ssh.as_str()).or_default().insert(record.held_unit.clone().unwrap_or_else(|| unit(&record.name)), record.name.clone());
                continue;
            }
            if let Some(host) = record.host_pid.filter(|_| record.live()) {
                let detached = detached::Procfs::new(proc.into(), cgroups.into(), host).after(Prior::all(&record.host_dir)).detached();
                if !detached.is_empty() {
                    running.insert(record.name.clone(), Work::Detached);
                } else if detached::cli_work(proc, cgroups, host) > 0 {
                    running.insert(record.name.clone(), Work::Cli);
                }
                continue;
            }
            let scope = crate::platform::session_host_files::scope(&record.host_dir);
            units.extend(scope.into_iter().chain(Prior::all(&record.host_dir).into_iter().map(|prior| prior.unit)).map(|unit| (record.name.clone(), unit)));
        }
        let ssh = &binary("ssh");
        for (work, named) in std::thread::scope(|scope| placed.into_iter().map(|(destination, named)| scope.spawn(move || (held::work(ssh, destination, &named.keys().cloned().collect::<Vec<_>>()), named))).collect::<Vec<_>>().into_iter().filter_map(|reading| reading.join().ok()).collect::<Vec<_>>()) {
            running.extend(named.into_iter().filter(|(unit, _)| work.as_ref().is_none_or(|work| work.get(unit).is_some_and(|count| *count > 0))).map(|(_, name)| (name, Work::Cli)));
        }
        if units.is_empty() {
            return running;
        }
        let asked = Command::new(binary("systemctl")).args(["--user", "is-active"]).args(units.iter().map(|(_, unit)| unit)).stdin(Stdio::null()).stderr(Stdio::null()).output();
        let states: Vec<String> = asked.map(|answer| String::from_utf8_lossy(&answer.stdout).lines().map(str::to_owned).collect()).unwrap_or_default();
        for (index, (name, _)) in units.into_iter().enumerate() {
            if states.get(index).is_none_or(|state| state != "inactive" && state != "failed") {
                running.insert(name, Work::Detached);
            }
        }
        running
    }
}

#[cfg(test)]
#[path = "reaper_tests.rs"]
mod tests;
