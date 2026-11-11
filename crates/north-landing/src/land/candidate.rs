use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError, RwLock};
use std::time::Duration;

use north_base::platform::process;

use super::landed_copy;
use super::queue::{self, Ticket};
use super::lane_gates::{self, Lane};
use super::reservation::{self, Attempt, Judged};
use crate::landing::{merge_continue, merge_set, stale_revert};
use crate::landing_refusals;
use crate::platform::decision_log::{self, Entry};
use north_strategy_types::Kind;
use north_strategy_types::land::{self, PickCase};

static WORKTREES: RwLock<()> = RwLock::new(());
pub const DEADLINE: Duration = Duration::from_secs(120);

fn run(directory: &Path, arguments: &[&str]) -> Result<Output, String> {
    let mut command = Command::new("git");
    command.arg("-C").arg(directory).args(merge_set::driver_arguments()).args(arguments).env("GIT_EDITOR", "true");
    match process::try_run_group_traced_within(command, DEADLINE) {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(stalled)) => Err(format!("git {} gave no answer within {} s and was killed: {stalled}", arguments.join(" "), DEADLINE.as_secs())),
        Err(error) => Err(format!("git {}: {error}", arguments.join(" "))),
    }
}

fn stdout(arguments: &[&str], output: Output) -> Result<String, String> {
    if !output.status.success() {
        return Err(format!("git {} failed: {}", arguments.join(" "), String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

pub fn output(directory: &Path, arguments: &[&str]) -> Result<Output, String> {
    let _shared = WORKTREES.read().unwrap_or_else(PoisonError::into_inner);
    run(directory, arguments)
}

pub fn git(directory: &Path, arguments: &[&str]) -> Result<String, String> {
    let attempt = || stdout(arguments, output(directory, arguments)?);
    match arguments.first() {
        Some(&"fetch") => north_git_locks::retry_after_clearing(directory, attempt),
        _ => attempt(),
    }
}

pub fn change_worktrees_alone(repo: &Path, arguments: &[&str]) -> Result<String, String> {
    let _sole = WORKTREES.write().unwrap_or_else(PoisonError::into_inner);
    stdout(arguments, run(repo, arguments)?)
}

fn succeeds(directory: &Path, arguments: &[&str]) -> bool {
    output(directory, arguments).is_ok_and(|output| output.status.success())
}

pub fn lander_worktree(repo: &Path, name: &str) -> Result<PathBuf, String> {
    let checkout = super::repositories::checkout(repo)?;
    let container = checkout.parent().ok_or_else(|| format!("{} has no project container", checkout.display()))?;
    Ok(container.join("worktrees").join(name))
}

pub fn ensure(repo: &Path, worktree: &Path) -> Result<(), String> {
    if !worktree.join(".git").exists() {
        let path = worktree.to_str().ok_or_else(|| format!("{} is not UTF-8", worktree.display()))?;
        git(repo, &["worktree", "add", "-q", "-f", "--detach", path, "HEAD"])?;
    }
    if let Ok(lock) = git(worktree, &["rev-parse", "--path-format=absolute", "--git-path", "index.lock"]) {
        let _ = fs::remove_file(lock);
    }
    if worktree.join(".git").is_file() && succeeds(worktree, &["rev-parse", "-q", "--verify", "CHERRY_PICK_HEAD"]) {
        git(worktree, &["cherry-pick", "--abort"])?;
    }
    Ok(())
}

pub fn prepare(repo: &Path, worktree: &Path) -> Result<String, String> {
    ensure(repo, worktree)?;
    git(worktree, &["fetch", "-q", "--no-tags", "origin", "refs/heads/main"])?;
    git(worktree, &["rev-parse", "FETCH_HEAD^{commit}"])
}

pub enum Pick {
    Included,
    Decided(String),
    Refused(lane_gates::Refused),
    Undecided(String),
    Deferred(String),
}

pub struct Built {
    pub head: String,
    pub picks: Vec<Pick>,
    pub tips: Vec<String>,
}

impl Built {
    pub fn included(&self) -> usize {
        self.picks.iter().filter(|pick| matches!(pick, Pick::Included)).count()
    }
}

fn has_commit(worktree: &Path, sha: &str) -> bool {
    succeeds(worktree, &["cat-file", "-e", &format!("{sha}^{{commit}}")])
}

fn dropped_refusal(base: &str, commits: &str) -> String {
    format!("lane commits missing from the candidate after the pick onto main@{base}: {commits}; NOT landed.")
}

fn refuse(logs: &Path, name: &str, kind: &str, text: &str) -> Result<Pick, String> {
    fs::create_dir_all(logs).map_err(|error| format!("{}: {error}", logs.display()))?;
    let log = logs.join(format!("{name}.{kind}.log"));
    fs::write(&log, format!("{text}\n")).map_err(|error| format!("{}: {error}", log.display()))?;
    Ok(Pick::Decided(format!("failed {}", log.display())))
}

fn readable(worktree: &Path, ticket: &Ticket) -> bool {
    if has_commit(worktree, &ticket.head) {
        return true;
    }
    let root = ticket.root.to_str().unwrap_or_default();
    let _ = output(worktree, &["fetch", "-q", "--no-tags", root, &format!("refs/heads/{}", ticket.branch)]);
    has_commit(worktree, &ticket.head)
}

fn files(worktree: &Path, base: &str, ticket: &Ticket) -> HashSet<String> {
    let Ok(fork) = git(worktree, &["merge-base", base, &ticket.head]) else { return HashSet::new() };
    let changed: Vec<String> = git(worktree, &["diff", "--name-only", "--no-renames", &fork, &ticket.head]).unwrap_or_default().lines().map(str::to_owned).collect();
    if changed.is_empty() {
        return HashSet::new();
    }
    let mut arguments = vec!["check-attr", "merge", "--"];
    arguments.extend(changed.iter().map(String::as_str));
    let resolved: HashSet<String> = git(worktree, &arguments).unwrap_or_default().lines().filter_map(|line| line.strip_suffix(": merge: set")).map(str::to_owned).collect();
    changed.into_iter().filter(|path| !resolved.contains(path)).collect()
}

fn queued_ahead<'a>(worktree: &Path, base: &str, ticket: &Ticket, earlier: &[&'a Ticket]) -> Option<&'a str> {
    let own = files(worktree, base, ticket);
    let lane = queue::lane(ticket);
    earlier.iter().find(|ahead| queue::lane(ahead) != lane && readable(worktree, ahead) && !files(worktree, base, ahead).is_disjoint(&own)).map(|ahead| ahead.branch.as_str())
}

struct Turn<'a> {
    ticket: &'a Ticket,
    earlier: &'a [&'a Ticket],
}

fn carrier(worktree: &Path, base: &str, head: &str) -> Option<String> {
    let range = format!("{base}...{head}");
    git(worktree, &["log", "--cherry-mark", "--left-only", "--no-merges", "--format=%m%H", &range]).ok()?.lines().find_map(|line| line.strip_prefix('=').map(str::to_owned))
}

fn landed_twice(worktree: &Path, base: &str, head: &str, unlanded: &HashSet<&str>) -> Result<Option<landed_copy::Landed>, String> {
    if unlanded.is_empty() {
        return Ok(None);
    }
    let fork = git(worktree, &["merge-base", base, head])?;
    let main = git(worktree, &["log", "--no-merges", landed_copy::FIELDS, &format!("{fork}..{base}")])?;
    let lane = git(worktree, &["log", "--no-merges", landed_copy::FIELDS, &format!("{base}..{head}")])?;
    Ok(landed_copy::amended_after_landing(&main, &lane, unlanded))
}

fn pick(worktree: &Path, base: &str, turn: &Turn, logs: &Path, name: &str, case: &mut PickCase, gating: &mut Option<Gating>) -> Result<Pick, String> {
    let (ticket, earlier) = (turn.ticket, turn.earlier);
    case.readable = Some(readable(worktree, ticket));
    if case.readable == Some(false) {
        return Ok(Pick::Decided("conflict unreadable".to_owned()));
    }
    let cherry = git(worktree, &["cherry", base, &ticket.head])?;
    case.unlanded = Some(cherry.lines().any(|line| line.starts_with('+')));
    if case.unlanded == Some(false) {
        return Ok(Pick::Decided(format!("landed {}", carrier(worktree, base, &ticket.head).unwrap_or_else(|| base.to_owned()))));
    }
    let plus: HashSet<&str> = cherry.lines().filter_map(|line| line.strip_prefix("+ ")).collect();
    if let Some(landed) = landed_twice(worktree, base, &ticket.head, &plus)? {
        return refuse(logs, name, "landed-twice", &landed_copy::refusal(&landed, base));
    }
    let picked_onto = git(worktree, &["rev-parse", "HEAD"])?;
    let stack_cherry = git(worktree, &["cherry", &picked_onto, &ticket.head])?;
    let unlanded: Vec<&str> = stack_cherry.lines().filter_map(|line| line.strip_prefix("+ ")).collect();
    let picked = output(worktree, &["cherry-pick", "--allow-empty", "--empty=drop", &format!("{base}..{}", ticket.head)])?;
    let resolved = picked.status.success() || merge_continue::finish(worktree, &worktree.join(merge_set::DEFAULT_CONFIG)).is_ok();
    case.picked_clean = Some(resolved);
    if !resolved {
        let files = git(worktree, &["diff", "--name-only", "--diff-filter=U"]).unwrap_or_default().lines().collect::<Vec<_>>().join(",");
        let dropped = if files.is_empty() && succeeds(worktree, &["diff", "--cached", "--quiet", "HEAD"]) { git(worktree, &["rev-parse", "-q", "--verify", "CHERRY_PICK_HEAD"]).ok() } else { None };
        if !succeeds(worktree, &["cherry-pick", "--abort"]) {
            git(worktree, &["reset", "-q", "--hard", &picked_onto])?;
        }
        if let Some(commit) = dropped {
            case.dropped = Some(commit.clone());
            return refuse(logs, name, "dropped", &dropped_refusal(base, &commit));
        }
        case.conflict_files = Some(files.clone());
        let files = if files.is_empty() { "cherry-pick" } else { files.as_str() };
        let bare_main = picked_onto == base;
        let with_main = bare_main || !succeeds(worktree, &["merge-tree", "--write-tree", "--no-messages", base, &ticket.head]);
        let red_fix = ticket.red_fix.is_some();
        let queued_ahead = if with_main && !red_fix { queued_ahead(worktree, base, ticket, earlier) } else { None };
        (case.bare_main, case.with_main, case.queued_ahead) = (Some(bare_main), Some(with_main), queued_ahead.map(str::to_owned));
        return Ok(reservation::judge(&Judged { branch: &ticket.branch, red_fix, bare_main, queued_ahead }, &Attempt::Conflict { files, with_main }));
    }
    let picked_count: usize = git(worktree, &["rev-list", "--count", &format!("{picked_onto}..HEAD")])?.parse().map_err(|error| format!("rev-list --count: {error}"))?;
    if picked_count < unlanded.len() {
        git(worktree, &["reset", "-q", "--hard", &picked_onto])?;
        let commits = unlanded.join(" ");
        case.dropped = Some(commits.clone());
        return refuse(logs, name, "dropped", &dropped_refusal(base, &commits));
    }
    match merge_continue::generator_moved(worktree, &worktree.join(merge_set::DEFAULT_CONFIG), base, &picked_onto) {
        Ok(None) => {}
        Ok(Some(reason)) => {
            case.generator_moved = Some(reason.clone());
            git(worktree, &["reset", "-q", "--hard", &picked_onto])?;
            return Ok(Pick::Deferred(format!("deferred: {}: {reason}; it stays queued for the next candidate", ticket.branch)));
        }
        Err(error) => {
            case.generator_moved = Some(format!("unreadable: {error}"));
            git(worktree, &["reset", "-q", "--hard", &picked_onto])?;
            return refuse(logs, name, "regenerate", &format!("could not read the regenerate rules: {error}; NOT landed."));
        }
    }
    if let Err(error) = merge_continue::refresh(worktree, &worktree.join(merge_set::DEFAULT_CONFIG), &picked_onto) {
        git(worktree, &["reset", "-q", "--hard", &picked_onto])?;
        return refuse(logs, name, "regenerate", &format!("could not regenerate an index the lane leaves stale: {error}; NOT landed."));
    }
    let found = match stale_revert::reverted(worktree, &picked_onto, "HEAD", &ticket.head) {
        Ok(found) => found,
        Err(error) => {
            git(worktree, &["reset", "-q", "--hard", &picked_onto])?;
            return Ok(Pick::Undecided(format!("cannot check whether {} reverts recent main: {error}", ticket.branch)));
        }
    };
    case.stale_reverts = Some(found.iter().map(|reverted| reverted.commit.clone()).collect());
    let (enforcing, state_home, root) = (landing_refusals::enforcing(), crate::platform::records_mirror::state_home(), ticket.root.to_string_lossy());
    let shadowed = landing_refusals::Lane { at: "pick", root: &root, branch: &ticket.branch, head: &ticket.head };
    if !found.is_empty() {
        if enforcing {
            git(worktree, &["reset", "-q", "--hard", &picked_onto])?;
            return refuse(logs, name, "stale-revert", &stale_revert::refusal(&found));
        }
        shadowed.record(&state_home, "revert guard", &stale_revert::refusal(&found));
    }
    let head = git(worktree, &["rev-parse", "HEAD"])?;
    *gating = (!lane_gates::judged_before_queueing(worktree, ticket.gated_from.as_deref(), &ticket.head, &picked_onto)).then_some(Gating { picked_onto, head });
    Ok(Pick::Included)
}

struct Gating {
    picked_onto: String,
    head: String,
}

struct GateJob {
    name: String,
    gating: Gating,
}

type Gated = Result<Option<lane_gates::Refused>, String>;

fn gate_lane(worktree: &Path, job: &GateJob, logs: &Path) -> Gated {
    let scratch = tempfile::tempdir_in(worktree.parent().unwrap_or(worktree)).map_err(|error| error.to_string())?;
    let tree = scratch.path().join("tree");
    let path = tree.to_str().ok_or_else(|| format!("{} is not UTF-8", tree.display()))?;
    change_worktrees_alone(worktree, &["worktree", "add", "-q", "-f", "--detach", "--no-checkout", path, &job.gating.head])?;
    let judged = git(&tree, &["reset", "-q", "--hard", &job.gating.head]).and_then(|_| {
        let (state_home, code_health) = (crate::platform::records_mirror::state_home(), super::after::gates::code_health_program());
        let advisory_db = crate::advisory_gate::database(&state_home);
        let lane = Lane { worktree: &tree, base: &job.gating.picked_onto, head: &job.gating.head, state_home: &state_home, code_health: &code_health, registry: &crate::dep_gate::PublicRegistries, advisory_db: &advisory_db };
        lane_gates::judge(&lane, logs, &job.name)
    });
    let _ = change_worktrees_alone(worktree, &["worktree", "remove", "--force", path]);
    judged
}

const GATE_THREADS: usize = 8;

fn gate_lanes(worktree: &Path, jobs: &[GateJob], logs: &Path) -> Vec<Gated> {
    gate_lanes_with(jobs, std::thread::available_parallelism().map_or(1, usize::from), |job| gate_lane(worktree, job, logs))
}

fn gate_lanes_with(jobs: &[GateJob], parallelism: usize, judge: impl Fn(&GateJob) -> Gated + Sync) -> Vec<Gated> {
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<Gated>>> = Mutex::new(jobs.iter().map(|_| None).collect());
    let threads = jobs.len().min(GATE_THREADS).min(parallelism).max(1);
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let at = next.fetch_add(1, Ordering::Relaxed);
                    let Some(job) = jobs.get(at) else { break };
                    let judged = judge(job);
                    results.lock().unwrap_or_else(PoisonError::into_inner)[at] = Some(judged);
                }
            });
        }
    });
    results.into_inner().unwrap_or_else(PoisonError::into_inner).into_iter().map(|judged| judged.unwrap_or_else(|| Err("a lane gate thread ended without a verdict".to_owned()))).collect()
}

/// The pick as the decision log records it.
pub fn recorded(pick: &Pick) -> land::Pick {
    let (pick, text) = match pick {
        Pick::Included => (land::PickKind::Included, String::new()),
        Pick::Decided(text) => (land::PickKind::Decided, text.clone()),
        Pick::Refused(refused) => (land::PickKind::Refused, refused.verdict.clone()),
        Pick::Undecided(text) => (land::PickKind::Undecided, text.clone()),
        Pick::Deferred(text) => (land::PickKind::Deferred, text.clone()),
    };
    land::Pick { pick, text }
}

pub fn build(worktree: &Path, base: &str, tickets: &[(PathBuf, Ticket)], queued: &[(PathBuf, Ticket)], limit: usize, logs: &Path) -> Result<Built, String> {
    assemble(worktree, base, tickets, queued, limit, logs)
}

pub fn rebuild_recorded(worktree: &Path, base: &str, tickets: &[(PathBuf, Ticket)], logs: &Path) -> Result<Built, String> {
    assemble(worktree, base, tickets, &[], usize::MAX, logs)
}

struct Stacked {
    pick: Pick,
    case: PickCase,
    decide_us: u64,
    gating: Option<Gating>,
    name: String,
}

struct Stack {
    head: String,
    tips: Vec<String>,
    lanes: Vec<Stacked>,
}

fn stack(worktree: &Path, base: &str, tickets: &[(PathBuf, Ticket)], queued: &[(PathBuf, Ticket)], limit: usize, logs: &Path, refused: &BTreeMap<usize, lane_gates::Refused>) -> Result<Stack, String> {
    git(worktree, &["reset", "-q", "--hard"])?;
    git(worktree, &["checkout", "-q", "--detach", base])?;
    let mut lanes = Vec::new();
    let mut tips = Vec::new();
    let mut included = 0;
    let mut decided = HashSet::new();
    for (index, (path, ticket)) in tickets.iter().enumerate() {
        if included >= limit {
            break;
        }
        let name = path.file_stem().and_then(|stem| stem.to_str()).unwrap_or("lane").to_owned();
        let earlier: Vec<&Ticket> = match queued.iter().position(|(queued, _)| queued == path) {
            Some(at) => queued[..at].iter().filter(|(queued, _)| !decided.contains(queued)).map(|(_, ticket)| ticket).collect(),
            None => Vec::new(),
        };
        let mut case = PickCase { base: base.to_owned(), branch: ticket.branch.clone(), head: ticket.head.clone(), red_fix: ticket.red_fix.is_some(), earlier: earlier.iter().map(|ahead| ahead.branch.clone()).collect(), ..PickCase::default() };
        let mut gating = None;
        let (pick, decide_us) = match refused.get(&index) {
            Some(gate) => {
                case.lane_gate = Some(gate.verdict.clone());
                (Ok(Pick::Refused(gate.clone())), 0)
            }
            None => decision_log::timed(|| pick(worktree, base, &Turn { ticket, earlier: &earlier }, logs, &name, &mut case, &mut gating)),
        };
        let pick = pick?;
        included += usize::from(matches!(pick, Pick::Included));
        if matches!(pick, Pick::Included) {
            tips.push(git(worktree, &["rev-parse", "HEAD"])?);
        }
        if matches!(pick, Pick::Decided(_) | Pick::Refused(_)) {
            decided.insert(path);
        }
        lanes.push(Stacked { pick, case, decide_us, gating, name });
    }
    Ok(Stack { head: git(worktree, &["rev-parse", "HEAD"])?, tips, lanes })
}

fn assemble(worktree: &Path, base: &str, tickets: &[(PathBuf, Ticket)], queued: &[(PathBuf, Ticket)], limit: usize, logs: &Path) -> Result<Built, String> {
    let enforcing = landing_refusals::enforcing();
    let mut refused = BTreeMap::new();
    let mut gated: BTreeMap<usize, Option<lane_gates::Refused>> = BTreeMap::new();
    loop {
        let mut built = stack(worktree, base, tickets, queued, limit, logs, &refused)?;
        let todo: Vec<usize> = (0..built.lanes.len()).filter(|at| built.lanes[*at].gating.is_some() && !gated.contains_key(at)).collect();
        if todo.is_empty() {
            return Ok(finish(built, &gated, tickets, enforcing));
        }
        let jobs: Vec<GateJob> = todo.iter().filter_map(|at| built.lanes[*at].gating.take().map(|gating| GateJob { name: built.lanes[*at].name.clone(), gating })).collect();
        let mut refusals = false;
        for (at, judged) in todo.into_iter().zip(gate_lanes(worktree, &jobs, logs)) {
            let judged = judged?;
            if enforcing && let Some(gate) = &judged {
                refused.insert(at, gate.clone());
                refusals = true;
            }
            gated.insert(at, judged);
        }
        if !refusals {
            return Ok(finish(built, &gated, tickets, enforcing));
        }
    }
}

fn finish(built: Stack, gated: &BTreeMap<usize, Option<lane_gates::Refused>>, tickets: &[(PathBuf, Ticket)], enforcing: bool) -> Built {
    let state_home = crate::platform::records_mirror::state_home();
    let mut picks = Vec::new();
    for (at, lane) in built.lanes.into_iter().enumerate() {
        let Stacked { pick, mut case, decide_us, .. } = lane;
        if let Some(gate) = gated.get(&at).and_then(Option::as_ref) {
            case.lane_gate = Some(gate.verdict.clone());
            if !enforcing && let Some((_, ticket)) = tickets.get(at) {
                let root = ticket.root.to_string_lossy();
                landing_refusals::Lane { at: "pick", root: &root, branch: &ticket.branch, head: &ticket.head }.record(&state_home, &gate.gates, &fs::read_to_string(&gate.log).unwrap_or_default());
            }
        }
        decision_log::record(Entry { kind: Kind::LandPick, at: decision_log::now(), follows: None, case: &case, decision: &recorded(&pick), decide_us });
        picks.push(pick);
    }
    Built { head: built.head, picks, tips: built.tips }
}

#[cfg(test)]
#[path = "candidate_tests.rs"]
mod tests;
