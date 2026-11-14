//! A landing-path commit that stops the pipe: once the newest commit on main crossing the trust_boundary_paths or two_accept_paths of main's own harness/defaults.toml is live, and every hand-in and after-landing run since reaches no verdict for ten minutes, or every lane refused since is refused by the same check while main has not moved, the sheriff reads the stalled check twice at main without that commit and twice at main through the shared pair comparison, reverts it only when the pair answers regressed, and lands that revert only through the review-accept scan an exact revert passes; any other answer reverts nothing and pages (north#1537).
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use north_base::platform::finish_record::FinishRecords;
use north_base::platform::records_mirror::Records;
use north_base::platform::review_accept::Accepts;
use north_base::platform::trust_boundary;
use north_base::foundation::pair::{self, Compared, Reading};
use crate::land::after::follow;
use crate::land::slots::runner::{confined_step, failing, first_error, under_capacity};
use crate::land::slots::select::Config;
use crate::landing::pipe_verdicts::{self, Outcome, Refused};

use super::{Lander, candidate, git, is_revert, lane_ticket, owner, post_land, queue, queue_revert_in, records, reverted, short, without};
use owner::Fleet;

pub const STALLED_SECONDS: f64 = 600.0;
const RECENT_SECONDS: f64 = 3600.0;
const SEEN: usize = 2;
const HISTORY: &str = "200";
const DEFAULTS: &str = "harness/defaults.toml";
const CAUSE_CHARS: usize = 200;
const BASE_WORKTREE: &str = "lander-sheriff-base";
const PROBE_SECONDS: &str = "1800";
const TIMED_OUT: i32 = 124;
const NO_VERDICT: i32 = 75;
const OFFLOAD_CHECK: &str = "the land offload of a check that exits 0";
const PAGED: &str = "proxy";

/// The newest commit on main that crosses the trust boundary, when it landed, and whether a landing other than a revert followed it.
#[derive(Clone, Debug, PartialEq)]
pub struct Live {
    pub commit: String,
    pub message: String,
    pub at: f64,
    pub moved_since: bool,
}

#[derive(Debug, PartialEq)]
pub enum Stop {
    NoVerdict { since: f64, runs: usize, newest: String, line: String },
    SameCheck { since: f64, check: String, command: Option<String>, sessions: BTreeSet<String> },
}

/// The check a stop names, run at one tree: the land offload of a passing check through the tree's own head-north on a build host,
/// written here, whose exit 75 is its red because reaching no verdict is the stall itself; or main's own land.check line that the
/// refusals name, run confined to the candidate checkout as the lander runs a land check, so no refusal text is ever run.
#[derive(Clone, Debug, PartialEq)]
pub enum Stalled {
    Offload { host: String },
    Listed(String),
}

impl Stalled {
    fn check(&self) -> &str {
        match self {
            Stalled::Offload { .. } => OFFLOAD_CHECK,
            Stalled::Listed(command) => command,
        }
    }

    fn argv(&self, north: &str) -> Vec<String> {
        match self {
            Stalled::Offload { host } => vec!["bash".to_owned(), "-c".to_owned(), format!("CARGO_TARGET_DIR=target ./bin/head-north capacity offload run --host {host} --base \"$SAFE_PUSH_LAND_HEAD\" --head \"$SAFE_PUSH_LAND_HEAD\" --timeout 600 -- true")],
            Stalled::Listed(command) => confined_step(north, command.clone()),
        }
    }
}

impl Stop {
    pub fn evidence(&self, live: &Live, now: f64) -> String {
        let subject = live.message.lines().next().unwrap_or_default();
        let went = format!("{} ({subject}) went live on main {:.0} min ago", short(&live.commit), (now - live.at) / 60.0);
        match self {
            Stop::NoVerdict { since, runs, newest, line } => {
                let line: String = line.chars().take(CAUSE_CHARS).collect();
                format!("{went}, and the {runs} hand-ins and after-landing runs since reached no verdict for {:.0} min; newest: {newest}: {line}", (now - since) / 60.0)
            }
            Stop::SameCheck { since, check, sessions, .. } => {
                let check: String = check.chars().take(CAUSE_CHARS).collect();
                format!("{went}, and every lane that the sessions {} handed in since was refused by the same check for {:.0} min while main did not move: {check}", sessions.iter().cloned().collect::<Vec<_>>().join(", "), (now - since) / 60.0)
            }
        }
    }

    /// The check to read at both trees: for runs that reach no verdict the land offload of a passing check on `host`, and for a
    /// same-check stop main's own land.check command that `listed` finds for the refused one; `None` when there is neither.
    pub fn stalled(&self, host: Option<&str>, listed: &dyn Fn(&str) -> Option<String>) -> Option<Stalled> {
        match self {
            Stop::NoVerdict { .. } => host.map(|host| Stalled::Offload { host: host.to_owned() }),
            Stop::SameCheck { command, .. } => command.as_deref().and_then(listed).map(Stalled::Listed),
        }
    }
}

/// Whether the pipe stopped once `live` landed: the outcomes since are all without a verdict and span ten minutes to `now`,
/// or, with main not moved since, every refusal since names one check across two fleet sessions or more for ten minutes.
pub fn stopped(live: &Live, outcomes: &[Outcome], now: f64) -> Option<Stop> {
    if is_revert(&live.message) {
        return None;
    }
    let since: Vec<&Outcome> = outcomes.iter().filter(|outcome| outcome.at >= live.at).collect();
    let (first, newest) = (since.first()?, since.last()?);
    if now - newest.at > RECENT_SECONDS {
        return None;
    }
    if since.iter().all(|outcome| !outcome.verdict) {
        return (since.len() >= SEEN && now - first.at >= STALLED_SECONDS).then(|| Stop::NoVerdict { since: first.at, runs: since.len(), newest: newest.run.clone(), line: newest.line.clone() });
    }
    if live.moved_since {
        return None;
    }
    let refused: Vec<(f64, &Refused, &str)> = since.iter().filter_map(|outcome| outcome.refused.as_ref().map(|refused| (outcome.at, refused, outcome.line.as_str()))).collect();
    let ((started, _, _), (_, _, line)) = (refused.first()?, refused.last()?);
    let (started, line) = (*started, *line);
    let sessions: BTreeSet<String> = refused.iter().map(|(_, refused, _)| refused.session.clone()).filter(|session| !session.is_empty()).collect();
    let check = &refused[0].1.check;
    if refused.iter().any(|(_, other, _)| other.check != *check) {
        return None;
    }
    let command = failing(line).map(|(command, _)| command.to_owned());
    (sessions.len() >= SEEN && now - started >= STALLED_SECONDS).then(|| Stop::SameCheck { since: started, check: check.clone(), command, sessions })
}

/// The newest of the last commits on `main`'s first-parent line that changes a path the trust boundary lists of main's own
/// harness/defaults.toml name, `None` when main holds no such lists or no such commit.
pub fn newest(worktree: &Path, main: &str) -> Result<Option<Live>, String> {
    let Ok(defaults) = git(worktree, &["show", &format!("{main}:{DEFAULTS}")]) else { return Ok(None) };
    let Ok(document) = defaults.parse::<toml_edit::DocumentMut>() else { return Ok(None) };
    let log = git(worktree, &["log", "--first-parent", "--diff-merges=first-parent", "--name-only", "-n", HISTORY, "--format=%x1e%H%x1f%ct%x1f%B%x1f", main])?;
    let mut moved = false;
    for entry in log.split('\x1e') {
        let fields: Vec<&str> = entry.splitn(4, '\x1f').collect();
        let [commit, at, message, files] = fields[..] else { continue };
        let (message, files) = (message.trim(), files.lines().map(str::trim).filter(|file| !file.is_empty()).map(str::to_owned).collect::<Vec<_>>());
        let Some(crossing) = trust_boundary::crossing(&document, &files) else { return Ok(None) };
        if !crossing.is_empty() {
            return Ok(Some(Live { commit: commit.trim().to_owned(), message: message.to_owned(), at: at.trim().parse().unwrap_or(0.0), moved_since: moved }));
        }
        moved |= !is_revert(message);
    }
    Ok(None)
}

/// Where the sheriff works: the repository and queue it lands into, the worktree it reads main in and builds its revert in, and the
/// worktree it reads main without the commit in, each tree in its own checkout so the four runs share one build per tree.
pub struct Trees<'a> {
    pub repo: &'a Path,
    pub queue: &'a Path,
    pub candidate: &'a Path,
    pub base: &'a Path,
}

/// What the sheriff did with a stop: queued the revert on this branch, built it but the review-accept scan refused it for the
/// reason given with the evidence its commit message carries, or reverted nothing because the pair did not answer regressed.
#[derive(Debug, PartialEq)]
pub enum Acted {
    Queued(String),
    Unreviewed { refusal: String, evidence: String },
    Unconfirmed(Compared),
}

/// The review-accept scan of a revert `head` over `main` in a worktree, which an exact revert passes with one reviewer's accept.
pub type Review<'a> = &'a dyn Fn(&Path, &str, &str) -> Result<(), String>;

/// Reads `check` at main without `live` and at `main` through the shared pair comparison and, only on a regression, commits the
/// revert and queues it once `review` passes it.
pub fn act(trees: &Trees, main: &str, live: &Live, evidence: &str, check: &dyn Fn(&Path) -> Reading, review: Review) -> Result<Acted, String> {
    if !without(trees.base, main, &live.commit)? {
        return Err(format!("{} no longer reverts cleanly onto {}", short(&live.commit), short(main)));
    }
    git(trees.base, &["-c", "core.hooksPath=/dev/null", "commit", "-q", "--no-verify", "-m", &format!("sheriff probe: main {} without {}", short(main), short(&live.commit))])?;
    git(trees.candidate, &["reset", "-q", "--hard"])?;
    git(trees.candidate, &["checkout", "-q", "--detach", main])?;
    let compared = pair::compare(check, trees.base, trees.candidate);
    if !compared.regressed() {
        return Ok(Acted::Unconfirmed(compared));
    }
    queue_in(trees, main, live, &format!("{evidence}; {}", readings(&compared)), review)
}

/// Commits the revert of `live` onto `main` in the candidate worktree and queues it ahead of the queue once `review` passes it.
pub fn queue_in(trees: &Trees, main: &str, live: &Live, evidence: &str, review: Review) -> Result<Acted, String> {
    let branch = format!("sheriff-revert-{}", short(&live.commit));
    if !without(trees.candidate, main, &live.commit)? {
        return Err(format!("{} no longer reverts cleanly onto {}", short(&live.commit), short(main)));
    }
    git(trees.candidate, &["checkout", "-q", "-B", &branch])?;
    let subject = live.message.lines().next().unwrap_or_default();
    let message = format!("Revert {subject}\n\n{evidence}, so the sheriff reverts it through the landing queue.\n\nReverts: {0}\nSheriff-Reverts: {0}", live.commit);
    git(trees.candidate, &["-c", "core.hooksPath=/dev/null", "commit", "-q", "--no-verify", "-m", &message])?;
    let head = git(trees.candidate, &["rev-parse", "HEAD"])?;
    if let Err(refusal) = review(trees.candidate, main, &head) {
        return Ok(Acted::Unreviewed { refusal, evidence: evidence.to_owned() });
    }
    queue_revert_in(trees.repo, trees.queue, trees.candidate, &branch)?;
    Ok(Acted::Queued(branch))
}

fn readings(compared: &Compared) -> String {
    let words = |runs: &[Reading; 2]| runs.iter().map(Reading::word).collect::<Vec<_>>().join(" then ");
    format!("the stalled check read {:?} at main without it ({}) and at main ({})", compared.pair, words(&compared.base), words(&compared.candidate))
}

/// How the sheriff runs a probe: the north that confines it, main's own landing.capacity gate it runs under, and the offload owner.
struct Runner<'a> {
    north: &'a str,
    capacity: &'a str,
    owner: &'a str,
}

/// One run of `stalled` in the checkout `tree` at its HEAD under the capacity gate, as a land check runs: green on exit 0, red with the check's name and first failing line on
/// another exit, and no verdict when it cannot start, outlives its bound or, unless its exit 75 is its red, exits 75.
fn read(tree: &Path, stalled: &Stalled, runner: &Runner) -> Reading {
    let Ok(head) = git(tree, &["rev-parse", "HEAD"]) else { return Reading::NoVerdict(format!("{} holds no HEAD", tree.display())) };
    let ran = Command::new("timeout")
        .arg(PROBE_SECONDS)
        .args(under_capacity(runner.capacity, stalled.argv(runner.north)))
        .current_dir(tree)
        .env("SAFE_PUSH_LAND_BASE", &head)
        .env("SAFE_PUSH_LAND_HEAD", &head)
        .env(north_base::foundation::offload::OWNER_ENV, runner.owner)
        .stdin(Stdio::null())
        .output();
    let Ok(ran) = ran else { return Reading::NoVerdict(format!("{} could not start", stalled.check())) };
    let log = format!("{}\n{}", String::from_utf8_lossy(&ran.stdout), String::from_utf8_lossy(&ran.stderr)).replace(&tree.display().to_string(), "TREE");
    let first = first_error(&log).unwrap_or_default();
    match ran.status.code() {
        Some(0) => Reading::Green(first),
        Some(NO_VERDICT) if matches!(stalled, Stalled::Listed(_)) => Reading::NoVerdict(format!("{}: {first}", stalled.check())),
        Some(TIMED_OUT) | None => Reading::NoVerdict(format!("{} outlived {PROBE_SECONDS} s or was killed", stalled.check())),
        Some(_) => Reading::Red(format!("{}: {first}", stalled.check())),
    }
}

/// Whether a revert or red fix of `commit` is already queued or recorded, so the sheriff queues none.
fn held(lander: &Lander, commit: &str) -> bool {
    if reverted(lander).contains(commit) {
        return true;
    }
    let named = short(commit);
    let queued = queue::pending(&lander.queue).unwrap_or_default();
    queued.iter().filter(|(_, ticket)| ticket.red_fix.is_some()).any(|(_, ticket)| ticket.branch.contains(named) || git(&lander.repo, &["log", "-1", "--format=%B", &ticket.head]).is_ok_and(|message| message.contains(named)))
}

fn reviewed(directory: &Path, base: &str, head: &str) -> Result<(), String> {
    north_land_scans::review_accept::check(directory, base, head, &Accepts::counted(&Records::here()), &FinishRecords::here().all())
}

fn page(author: Option<&str>, text: &str) -> String {
    let told: Vec<&str> = [Some(PAGED), author].into_iter().flatten().filter(|session| !session.is_empty() && owner::Cli.send(session, text)).collect();
    if told.is_empty() { "paged nobody: no fleet session took the message".to_owned() } else { format!("paged {}", told.join(", ")) }
}

pub fn patrol(lander: &Lander, worktree: &Path) -> Result<Option<String>, String> {
    let outcomes = pipe_verdicts::gather(&crate::land::lander::state_home());
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |since| since.as_secs_f64());
    if outcomes.last().is_none_or(|newest| now - newest.at > RECENT_SECONDS) {
        return Ok(None);
    }
    let main = git(&lander.repo, &["ls-remote", "origin", "refs/heads/main"])?.split_whitespace().next().unwrap_or_default().to_owned();
    if git(worktree, &["cat-file", "-e", &format!("{main}^{{commit}}")]).is_err() {
        git(worktree, &["fetch", "-q", "--no-tags", "origin", "refs/heads/main"])?;
    }
    let Some(live) = newest(worktree, &main)? else { return Ok(None) };
    let Some(stop) = stopped(&live, &outcomes, now) else { return Ok(None) };
    if held(lander, &live.commit) {
        return Ok(Some(format!("the pipe stopped after {} went live, and its revert or red fix is already queued, so none is queued again", short(&live.commit))));
    }
    let record = records(lander).join(format!("pipe-{}", live.commit));
    let judged = fs::read_to_string(&record).unwrap_or_default();
    if judged.starts_with(&format!("unconfirmed {main}")) {
        return Ok(None);
    }
    let write = |text: String| queue::write_atomic(&record, &text).map_err(|error| format!("cannot write {}: {error}", record.display()));
    let evidence = stop.evidence(&live, now);
    let author = lane_ticket(lander, &format!("{}^", live.commit), &live.commit, &live.commit);
    let session = author.as_ref().and_then(|author| author.session.as_deref());
    let acted = if let Some(confirmed) = judged.strip_prefix(&format!("unreviewed {main} confirmed: ")) {
        queue_in(&Trees { repo: &lander.repo, queue: &lander.queue, candidate: worktree, base: worktree }, &main, &live, confirmed.trim(), &reviewed)?
    } else {
        let base = candidate::lander_worktree(&lander.repo, BASE_WORKTREE)?;
        candidate::ensure(&lander.repo, &base)?;
        let host = post_land::choose_host(&lander.repo, None);
        let config = Config::load(worktree, &main);
        let Some(stalled) = stop.stalled(host.as_deref(), &|failed| config.listed(failed).map(str::to_owned)) else {
            write(format!("unconfirmed {main}: no check to read\n"))?;
            return Ok(Some(format!("{evidence}; the sheriff has no check to read at main without {}: the refused check is no land.check line of main's own .safe-push, or no build host takes the offload, so nothing is reverted; {}", short(&live.commit), page(session, &format!("north land: sheriff: {evidence}; the stall names no check to confirm it by, so {} stays live; judge it by hand", short(&live.commit))))));
        };
        let trees = Trees { repo: &lander.repo, queue: &lander.queue, candidate: worktree, base: &base };
        let (north, capacity) = (follow::program(), config.capacity.unwrap_or_default());
        let runner = Runner { north: &north, capacity: &capacity, owner: &lander.offload_owner };
        act(&trees, &main, &live, &evidence, &|tree| read(tree, &stalled, &runner), &reviewed)?
    };
    match acted {
        Acted::Queued(branch) => {
            write(format!("revert {} queued as {branch}\n", live.commit))?;
            let reverted = owner::Reverted { branch: author.as_ref().map_or(live.commit.as_str(), |author| author.branch.as_str()), commit: &live.commit, revert: &branch, cause: owner::Cause::PipeStopped(&evidence) };
            let told = owner::tell(&owner::Cli, session, &reverted);
            Ok(Some(format!("{evidence}; confirmed regressed; revert queued as {branch}; {told}")))
        }
        Acted::Unreviewed { refusal, .. } if judged.starts_with(&format!("unreviewed {main} ")) => Ok(Some(format!("the revert of {} still waits on the review-accept scan: {refusal}", short(&live.commit)))),
        Acted::Unreviewed { refusal, evidence: confirmed } => {
            write(format!("unreviewed {main} confirmed: {confirmed}\n"))?;
            Ok(Some(format!("{evidence}; confirmed regressed, but its revert waits on the review-accept scan: {refusal}; {}", page(session, &format!("north land: sheriff: {evidence}; the revert of {} is confirmed and built, and waits on the review-accept scan: {refusal}", short(&live.commit))))))
        }
        Acted::Unconfirmed(compared) => {
            let why = readings(&compared);
            write(format!("unconfirmed {main}: {why}\n"))?;
            Ok(Some(format!("{evidence}; {why}, so nothing is reverted; {}", page(session, &format!("north land: sheriff: {evidence}; {why}, so {} stays live and the stall is not blamed on it", short(&live.commit))))))
        }
    }
}

#[cfg(test)]
#[path = "pipe_break_tests.rs"]
mod tests;
