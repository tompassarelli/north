use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::after;
use super::candidate::{self, git};
use super::lander::{Lander, state_home};
use super::pause;
use super::precheck;
use super::queue;
use super::serve::Round;

const USAGE: &str = "usage: north land watchdog --once [--queue-root DIR]";
pub const LEAD: &str = "lander-lead";
pub const QUARANTINE: &str = "quarantine";
pub const ROUNDS: &str = "watchdog/rounds.json";
pub const PAGES: &str = "watchdog-pages.json";
pub const STALE_MICROS: u64 = 20 * 60 * 1_000_000;
pub const PAGE_GAP_MICROS: u64 = 15 * 60 * 1_000_000;
const CHECKING_EVERY_MICROS: u64 = 30 * 1_000_000;
const KEPT: usize = 3;
const REPEATS: usize = 3;
const IDLE_ROUNDS: usize = 2;
const PAGE_LIMIT: usize = 1500;

static PAGING: Mutex<()> = Mutex::new(());

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Observation {
    pub at_micros: u64,
    pub repo: PathBuf,
    pub worktree: PathBuf,
    pub slots: usize,
    pub busy: usize,
    pub waiting: usize,
    #[serde(default)]
    pub paused: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Invariant {
    IdleSlot,
    RepeatedError,
    StaleLane,
    SharedBranch,
}

impl Invariant {
    pub fn key(self) -> &'static str {
        match self {
            Invariant::IdleSlot => "idle-slot",
            Invariant::RepeatedError => "repeated-error",
            Invariant::StaleLane => "stale-lane",
            Invariant::SharedBranch => "shared-branch",
        }
    }

    pub fn statement(self) -> &'static str {
        match self {
            Invariant::IdleSlot => "(a) no check slot idles while lanes wait for more than one round",
            Invariant::RepeatedError => "(b) no repository repeats the same error three rounds in a row",
            Invariant::StaleLane => "(c) no lane waits in the queue longer than 20 minutes",
            Invariant::SharedBranch => "(d) no branch is claimed by two worktrees",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Repair {
    RecreatedWorktree(PathBuf),
    RemovedWorktree(PathBuf),
    QuarantinedLane(PathBuf),
    QuarantinedRepository(usize),
    RestartedLoop(String),
    AlreadyRestarted(String),
    Failed(String),
    Nothing(String),
}

impl fmt::Display for Repair {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Repair::RecreatedWorktree(path) => write!(formatter, "recreated the lander worktree {}", path.display()),
            Repair::RemovedWorktree(path) => write!(formatter, "removed the stray lander worktree {}", path.display()),
            Repair::QuarantinedLane(path) => write!(formatter, "quarantined the lane to {}", path.display()),
            Repair::QuarantinedRepository(lanes) => write!(formatter, "quarantined the repository: moved its {lanes} waiting lanes to {QUARANTINE}/"),
            Repair::RestartedLoop(unit) => write!(formatter, "restarted {unit}"),
            Repair::AlreadyRestarted(unit) => write!(formatter, "{unit} was already restarted this round"),
            Repair::Failed(error) => write!(formatter, "failed: {error}"),
            Repair::Nothing(reason) => write!(formatter, "none: {reason}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Violation {
    pub invariant: Invariant,
    pub queue: PathBuf,
    pub detail: String,
    pub repairs: Vec<Repair>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Page {
    Sent(String),
    Held { text: String, paged_micros: u64 },
    Failed(String),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub violations: Vec<Violation>,
    pub pages: Vec<(Invariant, Page)>,
}

impl Report {
    pub fn clean(&self) -> bool {
        self.violations.is_empty()
    }
}

impl fmt::Display for Report {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (invariant, page) in &self.pages {
            for violation in self.violations.iter().filter(|violation| violation.invariant == *invariant) {
                let key = violation.queue.file_name().unwrap_or_default().to_string_lossy();
                writeln!(formatter, "violated {}: queue {key}: {}", invariant.statement(), violation.detail)?;
                for repair in &violation.repairs {
                    writeln!(formatter, "  repair: {repair}")?;
                }
            }
            match page {
                Page::Sent(text) => writeln!(formatter, "  page: sent to {LEAD}: {text}")?,
                Page::Held { paged_micros, .. } => writeln!(formatter, "  page: held; {LEAD} was paged about {} at {paged_micros} micros, within 15 minutes", invariant.key())?,
                Page::Failed(text) => writeln!(formatter, "  page: failed to reach {LEAD}: {text}")?,
            }
        }
        Ok(())
    }
}

pub trait Hands {
    fn unit_exists(&self, unit: &str) -> bool;
    fn restart(&self, unit: &str) -> Result<(), String>;
    fn page(&self, session: &str, text: &str) -> bool;
}

pub struct Host;

impl Hands for Host {
    fn unit_exists(&self, unit: &str) -> bool {
        Command::new("systemctl").args(["--user", "cat", unit]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|status| status.success())
    }

    fn restart(&self, unit: &str) -> Result<(), String> {
        let output = Command::new("systemctl").args(["--user", "restart", unit]).stdin(Stdio::null()).output().map_err(|error| format!("systemctl: {error}"))?;
        if output.status.success() { Ok(()) } else { Err(format!("systemctl --user restart {unit}: {}", String::from_utf8_lossy(&output.stderr).trim())) }
    }

    fn page(&self, session: &str, text: &str) -> bool {
        after::send(session, text)
    }
}

pub fn now_micros() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| u64::try_from(elapsed.as_micros()).unwrap_or(u64::MAX))
}

pub fn rounds(queue: &Path) -> Vec<Observation> {
    fs::read_to_string(queue.join(ROUNDS)).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
}

fn save_rounds(queue: &Path, rounds: &[Observation]) -> Result<(), String> {
    let path = queue.join(ROUNDS);
    let text = serde_json::to_string_pretty(rounds).map_err(|error| error.to_string())? + "\n";
    queue::write_atomic(&path, &text).map_err(|error| format!("cannot write {}: {error}", path.display()))
}

pub fn observe(queue: &Path, observation: Observation, checking: bool) -> Result<bool, String> {
    let mut kept = rounds(queue);
    if checking && kept.last().is_some_and(|last| observation.at_micros.saturating_sub(last.at_micros) < CHECKING_EVERY_MICROS) {
        return Ok(false);
    }
    kept.push(observation);
    let surplus = kept.len().saturating_sub(KEPT);
    kept.drain(..surplus);
    save_rounds(queue, &kept)?;
    Ok(true)
}

pub fn queues(root: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = fs::read_dir(root).map(|entries| entries.filter_map(Result::ok).map(|entry| entry.path()).filter(|path| path.is_dir()).collect()).unwrap_or_default();
    found.sort();
    found
}

pub fn patrol(root: &Path, queues: &[PathBuf], now: u64, hands: &dyn Hands) -> Result<Report, String> {
    let mut violations = Vec::new();
    for queue in queues {
        violations.extend(inspect(queue, now, hands)?);
    }
    let pages = page(root, &violations, now, hands);
    Ok(Report { violations, pages })
}

fn inspect(queue: &Path, now: u64, hands: &dyn Hands) -> Result<Vec<Violation>, String> {
    let rounds = rounds(queue);
    let mut found = Vec::new();
    let mut restarted = BTreeSet::new();
    if let Some(latest) = rounds.last() {
        for (branch, claimants) in shared_branches(&latest.repo) {
            let repairs = claimants.iter().filter(|path| lander_worktree(path)).map(|path| recreate(&latest.repo, path, &latest.worktree)).collect::<Vec<_>>();
            let repairs = if repairs.is_empty() { vec![Repair::Nothing("no claimant is a lander worktree; a person must choose which worktree keeps the branch".to_owned())] } else { repairs };
            let listed = claimants.iter().map(|path| path.display().to_string()).collect::<Vec<_>>().join(", ");
            found.push(Violation { invariant: Invariant::SharedBranch, queue: queue.to_owned(), detail: format!("branch {branch} is claimed by {listed}"), repairs });
        }
        let mut reset = false;
        if let Some(problem) = repeated(&rounds) {
            let detail = format!("{} repeated \"{problem}\" for {REPEATS} rounds", latest.repo.display());
            let reason = format!("quarantined by the lander watchdog: {}: {detail}", Invariant::RepeatedError.statement());
            let repairs = repair_error(queue, latest, &problem, &reason, hands, &mut restarted)?;
            found.push(Violation { invariant: Invariant::RepeatedError, queue: queue.to_owned(), detail, repairs });
            reset = true;
        }
        if let Some(detail) = idle(&rounds) {
            let repairs = vec![restart(&latest.repo, hands, &mut restarted)];
            found.push(Violation { invariant: Invariant::IdleSlot, queue: queue.to_owned(), detail: format!("{}: {detail}", latest.repo.display()), repairs });
            reset = true;
        }
        if reset {
            save_rounds(queue, &[])?;
        }
    }
    for (path, ticket) in queue::pending(queue).map_err(|error| error.to_string())? {
        let age = now.saturating_sub(u64::try_from(queue::queued_micros(&path)).unwrap_or(u64::MAX));
        if age <= STALE_MICROS {
            continue;
        }
        let detail = format!("lane {} ({}) waited {} minutes", ticket.branch, path.file_name().unwrap_or_default().to_string_lossy(), age / 60_000_000);
        let reason = format!("quarantined by the lander watchdog: {}: {detail}", Invariant::StaleLane.statement());
        let repair = match queue::settle(&path, ticket, reason, QUARANTINE) {
            Ok(moved) => Repair::QuarantinedLane(moved),
            Err(error) => Repair::Failed(format!("cannot quarantine {}: {error}", path.display())),
        };
        found.push(Violation { invariant: Invariant::StaleLane, queue: queue.to_owned(), detail, repairs: vec![repair] });
    }
    Ok(found)
}

fn repeated(rounds: &[Observation]) -> Option<String> {
    let recent = rounds.get(rounds.len().checked_sub(REPEATS)?..)?;
    let first = recent.first()?.problem.clone()?;
    recent.iter().all(|round| round.problem.as_deref() == Some(first.as_str())).then_some(first)
}

fn idle(rounds: &[Observation]) -> Option<String> {
    let recent = rounds.get(rounds.len().checked_sub(IDLE_ROUNDS)?..)?;
    if !recent.iter().all(|round| !round.paused && round.busy < round.slots && round.waiting > 0) {
        return None;
    }
    let last = recent.last()?;
    Some(format!("{} of {} check slots idled while {} lanes waited for {IDLE_ROUNDS} rounds", last.slots - last.busy, last.slots, last.waiting))
}

fn repair_error(queue: &Path, latest: &Observation, problem: &str, reason: &str, hands: &dyn Hands, restarted: &mut BTreeSet<String>) -> Result<Vec<Repair>, String> {
    let mut broken: Vec<PathBuf> = named_worktrees(problem).into_iter().filter(|path| lander_worktree(path)).collect();
    let own = latest.worktree.display().to_string();
    if broken.is_empty() && !own.is_empty() && problem.contains(&own) {
        broken.push(latest.worktree.clone());
    }
    if !broken.is_empty() {
        return Ok(broken.iter().map(|path| recreate(&latest.repo, path, &latest.worktree)).collect());
    }
    let unit = unit(&latest.repo);
    if hands.unit_exists(&unit) {
        return Ok(vec![restart(&latest.repo, hands, restarted)]);
    }
    let pending = queue::pending(queue).map_err(|error| error.to_string())?;
    if pending.is_empty() {
        return Ok(vec![Repair::Nothing(format!("no lane waits to quarantine and no {unit} unit exists"))]);
    }
    let lanes = pending.len();
    for (path, ticket) in pending {
        queue::settle(&path, ticket, reason.to_owned(), QUARANTINE).map_err(|error| format!("cannot quarantine {}: {error}", path.display()))?;
    }
    Ok(vec![Repair::QuarantinedRepository(lanes)])
}

fn named_worktrees(problem: &str) -> Vec<PathBuf> {
    problem
        .match_indices("worktree at ")
        .filter_map(|(at, marker)| problem[at + marker.len()..].split_whitespace().next())
        .map(|path| PathBuf::from(path.trim_matches(|character| matches!(character, '\'' | '"' | ',' | ';' | ':' | ')' | '('))))
        .filter(|path| path.is_absolute())
        .collect()
}

pub fn unit(repo: &Path) -> String {
    let project = if repo.file_name().is_some_and(|name| name == "main") { repo.parent().and_then(Path::file_name) } else { repo.file_name() };
    format!("north-land@{}.service", project.unwrap_or_default().to_string_lossy())
}

fn restart(repo: &Path, hands: &dyn Hands, restarted: &mut BTreeSet<String>) -> Repair {
    let unit = unit(repo);
    if restarted.contains(&unit) {
        return Repair::AlreadyRestarted(unit);
    }
    if !hands.unit_exists(&unit) {
        return Repair::Nothing(format!("no {unit} unit to restart"));
    }
    match hands.restart(&unit) {
        Ok(()) => {
            restarted.insert(unit.clone());
            Repair::RestartedLoop(unit)
        }
        Err(error) => Repair::Failed(error),
    }
}

fn lander_worktree(path: &Path) -> bool {
    path.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.starts_with("lander")) && path.parent().and_then(Path::file_name).is_some_and(|name| name == "worktrees")
}

pub fn shared_branches(repo: &Path) -> BTreeMap<String, Vec<PathBuf>> {
    let Ok(listed) = git(repo, &["worktree", "list", "--porcelain"]) else { return BTreeMap::new() };
    let mut claims: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    for entry in listed.split("\n\n") {
        let path = entry.lines().find_map(|line| line.strip_prefix("worktree "));
        let branch = entry.lines().find_map(|line| line.strip_prefix("branch refs/heads/"));
        if let (Some(path), Some(branch)) = (path, branch) {
            claims.entry(branch.to_owned()).or_default().push(PathBuf::from(path));
        }
    }
    claims.retain(|_, claimants| claimants.len() > 1);
    claims
}

fn recreate(repo: &Path, path: &Path, canonical: &Path) -> Repair {
    let Some(text) = path.to_str() else { return Repair::Failed(format!("{} is not UTF-8", path.display())) };
    if git(repo, &["worktree", "remove", "--force", "--force", text]).is_err()
        && path.exists()
        && let Err(error) = fs::remove_dir_all(path)
    {
        return Repair::Failed(format!("cannot remove {}: {error}", path.display()));
    }
    if let Err(error) = git(repo, &["worktree", "prune"]) {
        return Repair::Failed(error);
    }
    if path != canonical && path.parent() != canonical.parent() {
        return Repair::RemovedWorktree(path.to_owned());
    }
    match candidate::ensure(repo, path) {
        Ok(()) => Repair::RecreatedWorktree(path.to_owned()),
        Err(error) => Repair::Failed(error),
    }
}

fn page(root: &Path, violations: &[Violation], now: u64, hands: &dyn Hands) -> Vec<(Invariant, Page)> {
    let _guard = PAGING.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let path = root.join(PAGES);
    let mut paged: BTreeMap<String, u64> = fs::read_to_string(&path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
    let mut invariants: Vec<Invariant> = violations.iter().map(|violation| violation.invariant).collect();
    invariants.sort();
    invariants.dedup();
    let mut pages = Vec::new();
    for invariant in invariants {
        let text = page_text(invariant, violations);
        if let Some(&at) = paged.get(invariant.key())
            && now.saturating_sub(at) < PAGE_GAP_MICROS
        {
            pages.push((invariant, Page::Held { text, paged_micros: at }));
            continue;
        }
        if hands.page(LEAD, &text) {
            paged.insert(invariant.key().to_owned(), now);
            pages.push((invariant, Page::Sent(text)));
        } else {
            pages.push((invariant, Page::Failed(text)));
        }
    }
    if let Ok(text) = serde_json::to_string_pretty(&paged)
        && let Err(error) = queue::write_atomic(&path, &(text + "\n"))
    {
        eprintln!("north land watchdog: cannot record pages in {}: {error}", path.display());
    }
    pages
}

fn page_text(invariant: Invariant, violations: &[Violation]) -> String {
    let details: Vec<String> = violations
        .iter()
        .filter(|violation| violation.invariant == invariant)
        .map(|violation| format!("{} (repair: {})", violation.detail, violation.repairs.iter().map(ToString::to_string).collect::<Vec<_>>().join("; ")))
        .collect();
    let text = format!("north land watchdog: violated {}: {}", invariant.statement(), details.join(" | "));
    if text.chars().count() <= PAGE_LIMIT { text } else { text.chars().take(PAGE_LIMIT).collect::<String>() + "..." }
}

pub fn after_round(lander: &Lander, state: &precheck::State, round: &Result<Round, String>) {
    let problem = match round {
        Err(error) => Some(error.clone()),
        Ok(Round::Waiting(reason)) if reason.starts_with("transient:") => Some(reason.clone()),
        Ok(_) => None,
    };
    let queued = queue::pending(&lander.queue).unwrap_or_default();
    let observation = Observation {
        at_micros: now_micros(),
        repo: lander.repo.clone(),
        worktree: lander.worktree.clone(),
        slots: state.slots(),
        busy: state.in_flight(),
        waiting: state.waiting(&queued),
        paused: state.draining() || pause::held(&lander.queue).is_some(),
        problem,
    };
    let at = observation.at_micros;
    match observe(&lander.queue, observation, matches!(round, Ok(Round::Checking))) {
        Ok(false) => return,
        Ok(true) => {}
        Err(error) => eprintln!("north land watchdog: {error}"),
    }
    let root = lander.queue.parent().unwrap_or(&lander.queue);
    match patrol(root, std::slice::from_ref(&lander.queue), at, &Host) {
        Ok(report) => print_report(&report),
        Err(error) => eprintln!("north land watchdog: {error}"),
    }
}

fn print_report(report: &Report) {
    for line in report.to_string().lines() {
        println!("north land watchdog: {line}");
    }
}

pub fn run(arguments: &[String]) -> u8 {
    let mut once = false;
    let mut root = None;
    let mut rest = arguments.iter();
    while let Some(argument) = rest.next() {
        match argument.as_str() {
            "--once" => once = true,
            "--queue-root" => match rest.next() {
                Some(value) => root = Some(PathBuf::from(value)),
                None => {
                    eprintln!("{USAGE}");
                    return 64;
                }
            },
            "-h" | "--help" => {
                println!("{USAGE}");
                return 0;
            }
            _ => {
                eprintln!("{USAGE}");
                return 64;
            }
        }
    }
    if !once {
        eprintln!("{USAGE}\nthe lander runs the watchdog every round; standalone it runs --once");
        return 64;
    }
    let root = root.unwrap_or_else(|| state_home().join("north/land-queue"));
    match patrol(&root, &queues(&root), now_micros(), &Host) {
        Ok(report) if report.clean() => {
            println!("north land watchdog: {} queues under {} hold every invariant", queues(&root).len(), root.display());
            0
        }
        Ok(report) => {
            print_report(&report);
            1
        }
        Err(error) => {
            eprintln!("north land watchdog: {error}");
            1
        }
    }
}
