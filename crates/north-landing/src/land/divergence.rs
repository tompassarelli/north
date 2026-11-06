use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use super::candidate::{self, git};
use super::{after, queue, replay, sheriff};

pub const TRAILER: &str = "Replay-change:";
const KINDS: [&str; 8] = ["landed", "failed", "conflict", "undecided", "deferred", "transient", "red-main", "error"];
const LANDER_PATHS: [&str; 4] = ["crates/north-landing/", "crates/north-safe-push/", "crates/north-runtime/", "tests/it/land"];
const WORKTREE: &str = "lander-divergence";

pub fn declared_kind(line: &str) -> Option<Result<(String, String), String>> {
    let rest = line.strip_prefix(TRAILER)?.trim();
    let (name, reason) = rest.split_once(char::is_whitespace).map_or((rest, ""), |(name, reason)| (name, reason.trim()));
    let kind = name.strip_prefix("land.").filter(|kind| KINDS.contains(kind));
    Some(match kind {
        Some(kind) if !reason.is_empty() => Ok((kind.to_owned(), reason.to_owned())),
        Some(_) => Err(format!("'{line}' gives no reason after land.<kind>")),
        None => Err(format!("'{line}' names no replay decision; write {TRAILER} land.<kind> <reason>, kind one of {}", KINDS.join(", "))),
    })
}

pub fn refuse_malformed(bodies: &str) -> Result<(), String> {
    match bodies.lines().filter_map(declared_kind).find_map(Result::err) {
        Some(error) => Err(format!("{error}; NOT queued")),
        None => Ok(()),
    }
}

pub struct Divergence {
    pub checkout: PathBuf,
    pub tickets: Vec<PathBuf>,
    pub lanes: String,
    pub base: String,
    pub recorded: String,
    pub replayed: String,
}

impl Divergence {
    fn kind(&self) -> &str {
        self.replayed.split_whitespace().next().unwrap_or_default()
    }

    fn named(&self) -> String {
        format!("round {} on {} recorded {} but the candidate replayed {}", self.lanes, self.base, replay::recorded_class(&self.recorded).unwrap_or_default(), self.replayed)
    }

    fn diverges(&self, verdict: &str, declared: &BTreeSet<String>) -> bool {
        !replay::same(&self.recorded, verdict) && !replay::lands_what_was_refused(&self.recorded, verdict) && !verdict.split_whitespace().next().is_some_and(|kind| declared.contains(kind))
    }
}

pub struct Runtimes<'a> {
    pub home: &'a Path,
    pub source: &'a Path,
    pub queue: &'a Path,
    pub held: Option<&'a Path>,
    pub candidate: &'a Path,
    pub dry_run: bool,
    pub replay: &'a dyn Fn(&Path, &Divergence) -> Result<String, String>,
}

struct Point {
    sha: String,
    binary: PathBuf,
}

fn sha(target: &Path) -> String {
    target.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
}

fn binary(home: &Path, sha: &str) -> PathBuf {
    home.join("runtimes").join(sha).join(north_base::platform::harness::TREE).join("bin/north")
}

fn chain(checkout: &Path, held: &str, candidate: &str) -> Option<Vec<String>> {
    let listed = git(checkout, &["rev-list", "--first-parent", "--reverse", &format!("{held}..{candidate}")]).ok()?;
    Some(listed.lines().map(str::to_owned).collect())
}

fn declarations(checkout: &Path, commits: &[String]) -> Vec<(String, String, String)> {
    commits
        .iter()
        .filter_map(|commit| git(checkout, &["log", "-1", "--format=%B", commit]).ok().map(|body| (commit, body)))
        .flat_map(|(commit, body)| body.lines().filter_map(declared_kind).filter_map(Result::ok).map(|(kind, reason)| (commit.clone(), kind, reason)).collect::<Vec<_>>())
        .collect()
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(9)]
}

fn tell(runtimes: &Runtimes, text: &str) {
    if !runtimes.dry_run && !after::send("proxy", text) {
        println!("north land promote: could not tell the proxy: {text}");
    }
}

pub fn judge(runtimes: &Runtimes, divergences: &[Divergence]) -> Result<String, String> {
    let first = divergences.first().ok_or("no divergence to judge")?;
    let Some(held) = runtimes.held.map(sha) else { return Err(format!("{}; no held runtime to bisect from", first.named())) };
    let candidate = sha(runtimes.candidate);
    let Some(commits) = chain(runtimes.source, &held, &candidate) else { return Err(format!("{}; {}..{} is not a readable range in {}", first.named(), short(&held), short(&candidate), runtimes.source.display())) };
    let declared = declarations(runtimes.source, &commits);
    let kinds: BTreeSet<String> = declared.iter().map(|(_, kind, _)| kind.clone()).collect();
    let Some(undeclared) = divergences.iter().find(|divergence| !kinds.contains(divergence.kind())) else {
        let lines: Vec<String> = divergences
            .iter()
            .map(|divergence| {
                let (commit, kind, reason) = declared.iter().find(|(_, kind, _)| kind == divergence.kind()).expect("declared kind");
                format!("{}: declared by {} ({TRAILER} land.{kind} {reason})", divergence.named(), short(commit))
            })
            .collect();
        for line in &lines {
            println!("north land promote: {line}");
        }
        tell(runtimes, &format!("north land promote: the lander link follows {} over {} declared replay change(s): {}", short(&candidate), lines.len(), lines.join("; ")));
        return Ok(format!("{} declared replay change(s)", lines.len()));
    };
    bisected(runtimes, undeclared, &held, &commits)
}

fn points(home: &Path, held: &str, commits: &[String]) -> Vec<Point> {
    std::iter::once(held.to_owned()).chain(commits.iter().cloned()).map(|sha| Point { binary: binary(home, &sha), sha }).filter(|point| point.binary.is_file()).collect()
}

fn diverges_at(runtimes: &Runtimes, divergence: &Divergence, commits: &[String], point: &Point) -> Option<bool> {
    let upto = commits.iter().position(|commit| *commit == point.sha).map_or(0, |index| index + 1);
    let declared: BTreeSet<String> = declarations(runtimes.source, &commits[..upto]).into_iter().map(|(_, kind, _)| kind).collect();
    match (runtimes.replay)(&point.binary, divergence) {
        Ok(verdict) => {
            println!("north land promote: bisect: runtime {} replays round {} {}", short(&point.sha), divergence.lanes, verdict.split_whitespace().next().unwrap_or_default());
            Some(divergence.diverges(&verdict, &declared))
        }
        Err(error) => {
            println!("north land promote: bisect: runtime {} cannot replay round {}: {error}; skipped", short(&point.sha), divergence.lanes);
            None
        }
    }
}

fn bisected(runtimes: &Runtimes, divergence: &Divergence, held: &str, commits: &[String]) -> Result<String, String> {
    let mut points = points(runtimes.home, held, commits);
    if points.first().is_none_or(|point| point.sha != held) || points.last().is_none_or(|point| commits.last() != Some(&point.sha)) {
        return Err(format!("{}; the held or candidate runtime has no binary to bisect with", divergence.named()));
    }
    if diverges_at(runtimes, divergence, commits, &points[0]) != Some(false) {
        let reason = format!("{}; the held runtime {} does not reproduce the recorded verdict either, so a bisect by verdict cannot name a commit between the runtimes", divergence.named(), short(held));
        tell(runtimes, &format!("north land promote: lander link held: {reason}"));
        return Err(reason);
    }
    let (mut good, mut bad) = (0, points.len() - 1);
    while bad - good > 1 {
        let middle = good + (bad - good) / 2;
        match diverges_at(runtimes, divergence, commits, &points[middle]) {
            Some(true) => bad = middle,
            Some(false) => good = middle,
            None => {
                points.remove(middle);
                bad -= 1;
            }
        }
    }
    let range = chain(runtimes.source, &points[good].sha, &points[bad].sha).unwrap_or_default();
    let suspects = suspects(runtimes.source, &range);
    let named = suspects.iter().map(|commit| short(commit)).collect::<Vec<_>>().join(", ");
    let found = format!("{}, an undeclared replay change; bisected between runtimes {} and {} to {named}", divergence.named(), short(&points[good].sha), short(&points[bad].sha));
    if runtimes.dry_run {
        return Err(format!("{found}; dry run: no revert queued"));
    }
    let queued: Vec<String> = suspects.iter().map(|commit| revert(runtimes, divergence, commit, &found).unwrap_or_else(|error| format!("revert of {} not queued: {error}", short(commit)))).collect();
    let reason = format!("{found}; {}", queued.join("; "));
    tell(runtimes, &format!("north land promote: lander link held: {reason}; the link follows once the revert lands, or the commit declares {TRAILER} land.{} <reason>", divergence.kind()));
    Err(reason)
}

fn suspects(checkout: &Path, range: &[String]) -> Vec<String> {
    let touches = |commit: &String| git(checkout, &["diff-tree", "--no-commit-id", "--name-only", "-r", commit]).is_ok_and(|files| files.lines().any(|file| LANDER_PATHS.iter().any(|prefix| file.starts_with(prefix))));
    let lander: Vec<String> = range.iter().filter(|commit| touches(commit)).cloned().collect();
    if lander.is_empty() { range.to_vec() } else { lander }
}

fn record(queue: &Path, commit: &str) -> PathBuf {
    queue.join("sheriff").join(format!("replay-{commit}"))
}

fn revert(runtimes: &Runtimes, divergence: &Divergence, commit: &str, found: &str) -> Result<String, String> {
    let (source, queue_dir) = (runtimes.source, runtimes.queue);
    let record = record(queue_dir, commit);
    if let Ok(text) = fs::read_to_string(&record) {
        return Ok(format!("{} already queued", text.trim()));
    }
    let worktree = candidate::lander_worktree(source, WORKTREE)?;
    candidate::ensure(source, &worktree)?;
    git(&worktree, &["fetch", "-q", "--no-tags", "origin", "refs/heads/main"])?;
    let tip = git(&worktree, &["rev-parse", "FETCH_HEAD^{commit}"])?;
    if !sheriff::without(&worktree, &tip, commit)? {
        return Err(format!("{} no longer reverts cleanly onto {}", short(commit), short(&tip)));
    }
    let branch = format!("sheriff-revert-{}", short(commit));
    git(&worktree, &["checkout", "-q", "-B", &branch])?;
    let subject = git(&worktree, &["log", "-1", "--format=%s", commit])?;
    let message = format!("Revert {subject}\n\nThe lander's promotion replay found {found}; nothing declared it with a {TRAILER} trailer, so the sheriff reverts it through the landing queue.\n\nReverts: {commit}\nSheriff-Reverts: {commit}\nReplay-divergence: {}", divergence.lanes);
    git(&worktree, &["-c", "core.hooksPath=/dev/null", "commit", "-q", "--no-verify", "-m", &message])?;
    let head = sheriff::queue_revert_in(source, queue_dir, &worktree, &branch)?;
    let done = format!("revert {commit} {head} queued as {branch}");
    fs::create_dir_all(record.parent().unwrap_or(queue_dir)).map_err(|error| error.to_string())?;
    queue::write_atomic(&record, &format!("{done}\n")).map_err(|error| format!("cannot write {}: {error}", record.display()))?;
    Ok(done)
}

#[cfg(test)]
#[path = "divergence_tests.rs"]
mod tests;
