use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::LazyLock;
use std::time::{Duration, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use toml_edit::DocumentMut;

use crate::capacity::agent_load::{Host, protected_pressure_basis_points};
use crate::error::{NorthError, NorthResult};
use crate::fleet::model::{LaunchRequest, Status};
use crate::fleet::registry::Fleet;
use crate::ops::dispatch::STAFFING_REFUSAL as REFUSE;
use crate::ops::flow::Places;
use crate::ops::pacing::placement::config;
use crate::platform::land_queue;
use crate::platform::process::run_within;

use super::admission;
use super::placement;

static DEFAULTS: LazyLock<DocumentMut> = LazyLock::new(|| include_str!("../../../harness/defaults.toml").parse().expect("harness/defaults.toml parses"));

const HOUR_SECS: f64 = 3600.0;
const DAY_SECS: f64 = 24.0 * HOUR_SECS;
const COLD_LANES: f64 = 4.0;
const QUEUE_FLOOR: f64 = 2.0;
const STALL_SLACK_MINUTES: f64 = 7.0;
const REPO_RATE_FLOOR: usize = 10;
const PENDING_ADMIT_SECS: f64 = 180.0;
const LOG_TAIL_BYTES: u64 = 256 * 1024;
const GH_LIMIT: Duration = Duration::from_secs(10);
const RED: &str = "main is red";

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub(super) struct Bounds {
    pub drain_minutes: f64,
    pub launch_to_queued_minutes: f64,
    pub lanes_per_session: f64,
    pub pressure_basis_points: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub(super) struct Landings {
    pub landed_last_hour: usize,
    pub landed_last_day: usize,
    pub busy_hours: usize,
}

impl Landings {
    pub fn per_hour(&self) -> f64 {
        let busy = if self.busy_hours == 0 { 0.0 } else { self.landed_last_day as f64 / self.busy_hours as f64 };
        busy.max(self.landed_last_hour as f64)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub(super) struct Lander {
    pub queued: usize,
    pub oldest_pending_minutes: Option<f64>,
    #[serde(flatten)]
    pub landings: Landings,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub(super) struct Load {
    pub working: usize,
    pub pending_admits: usize,
    pub pressure_basis_points: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub(super) struct Assessment {
    #[serde(flatten)]
    pub lander: Lander,
    pub all_repos: Landings,
    pub rate_basis: &'static str,
    pub landings_per_hour: f64,
    pub queue_ceiling: f64,
    pub stall_minutes: f64,
    #[serde(flatten)]
    pub load: Load,
    pub cold: bool,
    pub in_flight: f64,
    pub in_flight_ceiling: f64,
    pub bounds: Bounds,
    pub refusal: Option<String>,
    pub would_refuse: Option<String>,
}

#[derive(Serialize)]
pub(super) struct Decision<'a> {
    at: f64,
    repo: &'a str,
    session: &'a str,
    decision: &'a str,
    note: Option<&'a str>,
    staffing_ok: Option<&'a str>,
    #[serde(flatten)]
    assessment: Option<&'a Assessment>,
}

pub(super) struct Peer {
    pub name: String,
    pub status: Status,
    pub brief: String,
    pub origin: Option<String>,
}

#[derive(Deserialize)]
struct Decided {
    #[serde(default)]
    verdict: Option<String>,
}

pub(super) fn assess(repo: &str, lander: Lander, all_repos: Landings, load: Load, bounds: Bounds) -> Assessment {
    let (rate_basis, landings) = if lander.landings.landed_last_day < REPO_RATE_FLOOR { ("all-repos", all_repos) } else { ("repo", lander.landings) };
    let rate = landings.per_hour();
    let queue_ceiling = (rate * bounds.drain_minutes / 60.0).max(QUEUE_FLOOR);
    let stall_minutes = 2.0 * bounds.drain_minutes + STALL_SLACK_MINUTES;
    let wait = "wait for the queue to drain";
    let refusal = match lander.oldest_pending_minutes.filter(|oldest| *oldest > stall_minutes) {
        Some(oldest) => Some(format!("{REFUSE} {repo} lander stalled, oldest {oldest:.0} min; {wait}")),
        None => (lander.queued as f64 >= queue_ceiling).then(|| {
            format!("{REFUSE} {repo} has {} lanes queued to land, at or over the {queue_ceiling:.1} that {rate:.1} landings a busy hour drain in {:.0} minutes; {wait}", lander.queued, bounds.drain_minutes)
        }),
    };
    let cold = landings.landed_last_day == 0;
    let working = load.working + load.pending_admits;
    let in_flight = lander.queued as f64 + bounds.lanes_per_session * (working + 1) as f64;
    let in_flight_ceiling = if cold { COLD_LANES } else { rate * (bounds.drain_minutes + bounds.launch_to_queued_minutes) / 60.0 };
    let would_refuse = if in_flight > in_flight_ceiling {
        Some(format!("{REFUSE} {repo} would carry {in_flight:.1} lanes in flight with this one over the ceiling of {in_flight_ceiling:.1}"))
    } else {
        load.pressure_basis_points
            .filter(|pressure| *pressure >= bounds.pressure_basis_points)
            .map(|pressure| format!("{REFUSE} the chosen host's protected CPU pressure is {:.0}%, at or over the {:.0}% capacity limit", pressure / 100.0, bounds.pressure_basis_points / 100.0))
    };
    Assessment { lander, all_repos, rate_basis, landings_per_hour: rate, queue_ceiling, stall_minutes, load, cold, in_flight, in_flight_ceiling, bounds, refusal, would_refuse }
}

pub(super) fn read_landings(queues: &[PathBuf], now: f64) -> Landings {
    let mut landings = Landings::default();
    let mut hours = BTreeSet::new();
    for path in queues.iter().flat_map(|queue| tickets(&queue.join("done"))) {
        let Some(age) = modified(&path).map(|at| now - at).filter(|age| (0.0..DAY_SECS).contains(age)) else { continue };
        let landed = fs::read_to_string(&path).ok().and_then(|text| serde_json::from_str::<Decided>(&text).ok()).and_then(|ticket| ticket.verdict).is_some_and(|verdict| verdict.starts_with("landed"));
        if landed {
            landings.landed_last_day += 1;
            landings.landed_last_hour += usize::from(age < HOUR_SECS);
            hours.insert((age / HOUR_SECS) as usize);
        }
    }
    landings.busy_hours = hours.len();
    landings
}

pub(super) fn read_lander(queue: &Path, now: f64) -> Lander {
    let pending = tickets(queue);
    let queued_at = |path: &PathBuf| match north_landing::land::queue::queued_micros(path) {
        u128::MAX => modified(path),
        micros => Some(micros as f64 / 1e6),
    };
    let oldest_pending_minutes = pending.iter().filter_map(queued_at).map(|at| (now - at).max(0.0) / 60.0).reduce(f64::max);
    Lander { queued: pending.len(), oldest_pending_minutes, landings: read_landings(&[queue.to_path_buf()], now) }
}

pub(super) fn all_queues(queue: &Path) -> Vec<PathBuf> {
    queue.parent().map(|root| fs::read_dir(root).into_iter().flatten().flatten().map(|entry| entry.path()).filter(|path| path.is_dir()).collect()).unwrap_or_default()
}

pub(super) fn lands(name: &str, brief: &str) -> bool {
    let lead = name == "lead" || name.starts_with("lead-") || name.ends_with("-lead") || header(brief, "Delegation").is_some_and(|delegation| delegation.split_whitespace().any(|word| word == "role=lead"));
    !lead && !header(brief, "Category").is_some_and(|category| category.eq_ignore_ascii_case("research"))
}

pub(super) fn exemption(name: &str, brief: &str, red_task: impl FnOnce() -> bool) -> Option<&'static str> {
    if name.starts_with("lander-") {
        return Some("a lander-* launch repairs the landing path");
    }
    if !lands(name, brief) {
        return Some("a lead or research launch produces no lane");
    }
    (brief.to_lowercase().contains(RED) && red_task()).then_some("its task is the open main is red issue")
}

pub(super) fn working(peers: &[Peer], origin: &str) -> usize {
    peers.iter().filter(|peer| peer.status == Status::Working && peer.origin.as_deref() == Some(origin) && lands(&peer.name, &peer.brief)).count()
}

pub(super) fn pending_admits(log_tail: &str, repo: &str, running: &BTreeSet<String>, now: f64) -> usize {
    let admitted = |line: &Value| matches!(line["decision"].as_str(), Some("allow" | "staffing-ok")) && line["repo"] == repo;
    let recent = |line: &Value| line["at"].as_f64().is_some_and(|at| now - at < PENDING_ADMIT_SECS);
    let names: BTreeSet<String> = log_tail
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|line| admitted(line) && recent(line))
        .filter_map(|line| line["session"].as_str().map(str::to_owned))
        .filter(|name| !running.contains(name))
        .collect();
    names.len()
}

pub(super) fn record(log: &Path, entry: &Decision) -> std::io::Result<()> {
    let line = serde_json::to_string(entry).map_err(std::io::Error::other)?;
    if let Some(directory) = log.parent() {
        fs::create_dir_all(directory)?;
    }
    writeln!(OpenOptions::new().create(true).append(true).open(log)?, "{line}")
}

pub(super) fn guard(fleet: &Fleet, request: &LaunchRequest, excuse: Option<&str>) -> NorthResult<()> {
    if request.verification {
        return Ok(());
    }
    let places = Places::from_env();
    let directory = places.state.join("north/fleet-governor");
    let log = directory.join("decisions.jsonl");
    let now = crate::foundation::clock::Timestamp::now().unix_seconds_f64();
    let allow = |repo: &str, decision: &str, note: &str| {
        if let Err(error) = record(&log, &Decision { at: now, repo, session: &request.name, decision, note: Some(note), staffing_ok: None, assessment: None }) {
            eprintln!("governor: decision not recorded: {error}");
        }
        Ok(())
    };
    let brief = format!("{}\n{}", placement::brief_text(request.system_prompt.as_deref(), &request.prompt)?, request.named_brief().unwrap_or_default());
    let Some(origin) = origin(&request.cwd) else { return allow("", "allow", "no origin") };
    let repo = name(&origin);
    if let Some(why) = exemption(&request.name, &brief, || request.task().is_some_and(|task| red_issue(&origin, &task))) {
        return allow(&repo, "exempt", why);
    }
    if let Some(lander) = remote_lander(&request.cwd) {
        eprintln!("governor: no lander state on this host; {repo} lands on {lander}, so {} starts ungoverned", request.name);
        return allow(&repo, "allow", "governor: no lander state on this host");
    }
    fs::create_dir_all(&directory)?;
    let lock = File::create(directory.join("lock"))?;
    lock.lock()?;
    let queue = land_queue::directory(&places.state, &origin);
    let peers = peers(fleet, &request.name);
    let running: BTreeSet<String> = peers.iter().map(|peer| peer.name.clone()).collect();
    let local = request.machine.as_ref().is_none_or(|machine| short(&machine.name) == short(&crate::platform::host::name()));
    let load = Load {
        working: working(&peers, &origin),
        pending_admits: pending_admits(&tail(&log), &repo, &running, now),
        pressure_basis_points: local.then(|| protected_pressure_basis_points(&Host::from_env())),
    };
    let assessment = assess(&repo, read_lander(&queue, now), read_landings(&all_queues(&queue), now), load, bounds());
    let decision = match (&assessment.refusal, excuse) {
        (None, _) => "allow",
        (Some(_), Some(_)) => "staffing-ok",
        (Some(_), None) => "refuse",
    };
    let recorded = record(&log, &Decision { at: now, repo: &repo, session: &request.name, decision, note: None, staffing_ok: excuse.filter(|_| assessment.refusal.is_some()), assessment: Some(&assessment) });
    drop(lock);
    if let Err(error) = recorded {
        eprintln!("governor: decision not recorded: {error}");
    }
    let Some(refused) = assessment.refusal else { return Ok(()) };
    if let Some(reason) = excuse {
        eprintln!("{refused}");
        eprintln!("staffing-ok: {} starts anyway: {reason}", request.name);
        return Ok(());
    }
    admission::tell_parent(fleet, request.parent.as_deref(), &format!("Stop: landing. {} was not launched: {refused}", request.name));
    Err(NorthError::Refused(refused))
}

pub(super) fn noted(placement: Option<String>, excuse: Option<&str>) -> Option<String> {
    match (placement, excuse) {
        (note, None) => note,
        (None, Some(reason)) => Some(format!("staffing-ok: {reason}")),
        (Some(note), Some(reason)) => Some(format!("{note}; staffing-ok: {reason}")),
    }
}

fn peers(fleet: &Fleet, own: &str) -> Vec<Peer> {
    let mut origins: BTreeMap<PathBuf, Option<String>> = BTreeMap::new();
    fleet
        .sessions()
        .unwrap_or_default()
        .into_iter()
        .filter(|record| !record.stopped && !record.verification && record.name != own)
        .map(|record| {
            let origin = origins.entry(record.cwd.clone()).or_insert_with(|| origin(&record.cwd)).clone();
            let brief = placement::brief_text(record.system_prompt.as_deref(), "").unwrap_or_default();
            Peer { status: fleet.status(&record), name: record.name, brief, origin }
        })
        .collect()
}

fn red_issue(origin: &str, task: &str) -> bool {
    let Some(slug) = slug(origin) else { return false };
    let number = task.rsplit('#').next().unwrap_or(task);
    let Some(output) = run_within("gh", &["issue", "view", number, "-R", &slug, "--json", "title,state"], GH_LIMIT) else { return false };
    let issue: Value = serde_json::from_slice(&output.stdout).unwrap_or(Value::Null);
    output.status.success() && issue["state"] == "OPEN" && issue["title"].as_str().is_some_and(|title| title.trim().eq_ignore_ascii_case(RED))
}

pub(super) fn slug(origin: &str) -> Option<String> {
    let path = origin.trim_end_matches('/').trim_end_matches(".git");
    let path = path.split_once("github.com").map(|(_, path)| path.trim_start_matches([':', '/']))?;
    let mut parts = path.rsplitn(3, '/');
    let (repo, owner) = (parts.next()?, parts.next()?);
    Some(format!("{owner}/{repo}"))
}

fn tail(log: &Path) -> String {
    let Ok(mut file) = File::open(log) else { return String::new() };
    let length = file.metadata().map_or(0, |metadata| metadata.len());
    if file.seek(SeekFrom::Start(length.saturating_sub(LOG_TAIL_BYTES))).is_err() {
        return String::new();
    }
    let mut bytes = Vec::new();
    if file.read_to_end(&mut bytes).is_err() {
        return String::new();
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

fn tickets(directory: &Path) -> Vec<PathBuf> {
    fs::read_dir(directory).into_iter().flatten().flatten().map(|entry| entry.path()).filter(|path| path.is_file() && path.extension().is_some_and(|extension| extension == "json")).collect()
}

fn modified(path: &Path) -> Option<f64> {
    Some(fs::metadata(path).ok()?.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_secs_f64())
}

fn header<'a>(brief: &'a str, key: &str) -> Option<&'a str> {
    brief.lines().find_map(|line| {
        let (name, value) = line.trim().split_once(':')?;
        let value = value.trim().trim_matches('`');
        (name.trim().eq_ignore_ascii_case(key) && !value.is_empty()).then_some(value)
    })
}

fn default(name: &str) -> f64 {
    let value = &DEFAULTS[name]["value"];
    value.as_float().or_else(|| value.as_integer().map(|whole| whole as f64)).unwrap_or_else(|| panic!("harness/defaults.toml has no number for {name}"))
}

pub(super) fn default_bounds() -> Bounds {
    Bounds {
        drain_minutes: default("staffing_drain_minutes"),
        launch_to_queued_minutes: default("staffing_launch_to_queued_minutes"),
        lanes_per_session: default("staffing_lanes_per_session"),
        pressure_basis_points: default("admit_psi_attended_high") * 100.0,
    }
}

fn bounds() -> Bounds {
    let defaults = default_bounds();
    let drain_minutes = config()
        .ok()
        .and_then(|cfg| cfg.get("staffing").and_then(|table| table.get("drain-minutes")).and_then(|item| item.as_integer().map(|minutes| minutes as f64).or_else(|| item.as_float())))
        .filter(|minutes| *minutes > 0.0)
        .unwrap_or(defaults.drain_minutes);
    Bounds { drain_minutes, ..defaults }
}

fn short(host: &str) -> String {
    host.split('.').next().unwrap_or_default().to_owned()
}

fn remote_lander(directory: &Path) -> Option<String> {
    let lander = north_landing::land::setting::landing(directory, "origin/main", "landing.lander-host");
    (!lander.is_empty() && short(&lander) != short(&crate::platform::host::name())).then_some(lander)
}

fn origin(directory: &Path) -> Option<String> {
    let output = Command::new("git").arg("-C").arg(directory).args(["remote", "get-url", "--push", "origin"]).stdin(Stdio::null()).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned()).filter(|origin| !origin.is_empty())
}

fn name(origin: &str) -> String {
    let tail = origin.trim_end_matches('/').rsplit(['/', ':']).next().unwrap_or(origin);
    tail.strip_suffix(".git").unwrap_or(tail).to_owned()
}

#[cfg(test)]
#[path = "governor_tests.rs"]
mod tests;
