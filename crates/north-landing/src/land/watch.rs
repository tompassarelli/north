use std::fs;
use std::path::{Path, PathBuf};
use std::thread::JoinHandle;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::after;
use super::candidate::{self, git, output};
use super::checks::Verdict;
use super::lander::Lander;
use super::post_land::{self, Outcome};
use super::queue::{self, Ticket};
use super::sheriff;
use super::telemetry::epoch;
use super::trace::{self, AFTER, COVERED_BY, Category, PHASE, Span};

const WORKTREE: &str = "lander-revert";
const JOURNAL: &str = "following";
const RETRY: Duration = Duration::from_secs(30);

#[derive(Clone, Serialize, Deserialize)]
pub struct LandedLane {
    pub ticket: Ticket,
    pub tip: String,
}

#[derive(Serialize, Deserialize)]
struct Entry {
    base: String,
    head: String,
    lanes: Vec<LandedLane>,
}

struct Followed {
    head: String,
    at: f64,
}

struct Batch {
    base: String,
    head: String,
    lanes: Vec<LandedLane>,
    heads: Vec<Followed>,
    started: Option<f64>,
    retry_at: f64,
}

impl Batch {
    fn landed(base: String, head: String, lanes: Vec<LandedLane>, at: f64) -> Self {
        Self { base, heads: vec![Followed { head: head.clone(), at }], head, lanes, started: None, retry_at: 0.0 }
    }

    fn covers(&self, head: &str) -> bool {
        self.heads.iter().any(|followed| followed.head == head)
    }

    fn absorb(&mut self, next: Self) {
        self.head = next.head;
        self.lanes.extend(next.lanes);
        self.heads.extend(next.heads);
    }
}

struct Running {
    batch: Batch,
    run: JoinHandle<Outcome>,
}

#[derive(Default)]
struct Pending {
    batches: Vec<Batch>,
}

impl Pending {
    fn covers(&self, head: &str) -> bool {
        self.batches.iter().any(|batch| batch.covers(head))
    }

    fn add(&mut self, batch: Batch) {
        match self.batches.last_mut() {
            Some(last) if last.head == batch.base => last.absorb(batch),
            _ => self.batches.push(batch),
        }
    }

    fn retry(&mut self, mut batch: Batch) {
        if self.batches.first().is_some_and(|next| next.base == batch.head) {
            batch.absorb(self.batches.remove(0));
        }
        self.batches.insert(0, batch);
    }

    fn next(&mut self, now: f64) -> Option<Batch> {
        self.batches.first().is_some_and(|batch| batch.retry_at <= now).then(|| self.batches.remove(0))
    }
}

#[derive(Default)]
pub struct Watch {
    running: Option<Running>,
    pending: Pending,
}

fn journal(lander: &Lander) -> PathBuf {
    lander.queue.join(JOURNAL)
}

fn entry_path(lander: &Lander, head: &str) -> PathBuf {
    journal(lander).join(format!("{head}.json"))
}

fn journaled_at(path: &Path) -> f64 {
    fs::metadata(path).and_then(|metadata| metadata.modified()).ok().and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok()).map_or_else(epoch, |since| since.as_secs_f64())
}

fn read_entry(path: &Path) -> Result<Entry, String> {
    let text = fs::read_to_string(path).map_err(|error| error.to_string())?;
    serde_json::from_str(&text).map_err(|error| error.to_string())
}

impl Watch {
    pub fn resume(lander: &Lander) -> Self {
        let mut watch = Self::default();
        let Ok(entries) = fs::read_dir(journal(lander)) else { return watch };
        let mut paths: Vec<(f64, PathBuf)> = entries.filter_map(Result::ok).map(|entry| entry.path()).filter(|path| path.extension().is_some_and(|extension| extension == "json")).map(|path| (journaled_at(&path), path)).collect();
        paths.sort_by(|left, right| left.0.total_cmp(&right.0));
        for (at, path) in paths {
            match read_entry(&path) {
                Ok(Entry { base, head, lanes }) => {
                    println!("north land: rerunning the after-landing run of {head}, which a stopped lander left without a verdict");
                    if !watch.pending.covers(&head) {
                        watch.pending.add(Batch::landed(base, head, lanes, at));
                    }
                }
                Err(error) => {
                    println!("north land: dropping the unreadable after-landing journal entry {}: {error}", path.display());
                    let _ = fs::remove_file(&path);
                }
            }
        }
        watch
    }

    pub fn in_flight(&self) -> usize {
        usize::from(self.running.is_some()) + self.pending.batches.len()
    }

    pub fn follow(&mut self, lander: &Lander, base: &str, head: &str, lanes: Vec<LandedLane>) {
        if self.running.as_ref().is_some_and(|running| running.batch.covers(head)) || self.pending.covers(head) {
            return;
        }
        let entry = Entry { base: base.to_owned(), head: head.to_owned(), lanes };
        let recorded = serde_json::to_string_pretty(&entry).map_err(|error| error.to_string()).and_then(|text| queue::write_atomic(&entry_path(lander, head), &text).map_err(|error| error.to_string()));
        if let Err(error) = recorded {
            println!("north land: could not journal the after-landing run of {head}, so a lander restart before its verdict loses it: {error}");
        }
        let Entry { base, head, lanes } = entry;
        self.pending.add(Batch::landed(base, head, lanes, epoch()));
        self.start_next(lander);
    }

    fn start_next(&mut self, lander: &Lander) {
        if self.running.is_some() {
            return;
        }
        let started = epoch();
        let Some(mut batch) = self.pending.next(started) else { return };
        batch.started.get_or_insert(started);
        if batch.heads.len() > 1 {
            println!("north land: one after-landing run of {} covers {} landings since {}", batch.head, batch.heads.len(), batch.base);
        }
        let waits: Vec<Span> = batch.heads.iter().filter(|followed| started - followed.at >= 1.0).map(|followed| Span::new(&followed.head, "waiting for the after-landing run in flight", Category::Queue, followed.at, started).with(PHASE, AFTER)).collect();
        record(lander, &waits);
        let names = batch.lanes.iter().map(|lane| lane.ticket.branch.clone()).collect();
        let tips = batch.lanes.iter().map(|lane| lane.tip.clone()).collect();
        let run = post_land::start(lander, &batch.base, &batch.head, names, tips);
        self.running = Some(Running { batch, run });
    }

    pub fn settle(&mut self, lander: &Lander) -> bool {
        if !self.running.as_ref().is_some_and(|running| running.run.is_finished()) {
            self.start_next(lander);
            return false;
        }
        let Some(Running { mut batch, run }) = self.running.take() else { return false };
        let outcome = run.join().unwrap_or_else(|_| Outcome { verdict: Verdict::Transient("the post-land thread panicked".to_owned()), culprit: None });
        let ended = epoch();
        if let Verdict::Transient(detail) = &outcome.verdict {
            println!("north land: the after-landing run of {} reached no verdict ({detail}); it runs again in {} s", batch.head, RETRY.as_secs());
            batch.retry_at = ended + RETRY.as_secs_f64();
            for followed in &mut batch.heads {
                followed.at = ended;
            }
            self.pending.retry(batch);
            return true;
        }
        let started = batch.started.unwrap_or(ended);
        let links: Vec<Span> = batch.heads.iter().filter(|followed| followed.head != batch.head).map(|followed| Span::new(&followed.head, format!("after-landing run of {}", batch.head), Category::Lander, started, ended).with(PHASE, AFTER).with(COVERED_BY, batch.head.clone())).collect();
        record(lander, &links);
        if let Verdict::RedMain(detail) = outcome.verdict {
            blame(lander, &batch.base, &batch.lanes, outcome.culprit, &detail);
        }
        for followed in &batch.heads {
            let _ = fs::remove_file(entry_path(lander, &followed.head));
        }
        self.start_next(lander);
        true
    }
}

fn record(lander: &Lander, spans: &[Span]) {
    if let Err(error) = trace::Spans::beside(&lander.queue.join("logs")).append(spans) {
        println!("north land: could not record after-landing spans: {error}");
    }
}

fn blame(lander: &Lander, base: &str, lanes: &[LandedLane], culprit: Option<usize>, detail: &str) {
    let Some(at) = culprit else { return };
    let lane = &lanes[at];
    let since = if at == 0 { base } else { &lanes[at - 1].tip };
    if lane.ticket.red_fix.is_some() {
        let message = format!("north land: the post-land checks failed ({detail}) with {}, a red fix; it is not reverted again", lane.ticket.branch);
        println!("{message}");
        after::send("proxy", &message);
        return;
    }
    match revert(lander, lane, since, detail) {
        Ok(line) => println!("north land: {line}"),
        Err(error) => {
            let message = format!("north land: the post-land checks of {} failed ({detail}) and its revert could not be queued: {error}", lane.ticket.branch);
            println!("{message}");
            after::send("proxy", &message);
        }
    }
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(9)]
}

fn revert(lander: &Lander, lane: &LandedLane, since: &str, detail: &str) -> Result<String, String> {
    let worktree: PathBuf = candidate::lander_worktree(&lander.repo, WORKTREE)?;
    let main = candidate::prepare(&lander.repo, &worktree)?;
    git(&worktree, &["reset", "-q", "--hard"])?;
    git(&worktree, &["checkout", "-q", "--detach", &main])?;
    let range = format!("{since}..{}", lane.tip);
    let commits = git(&worktree, &["rev-list", "--reverse", &range])?;
    if !output(&worktree, &["revert", "--no-commit", &range])?.status.success() {
        let _ = output(&worktree, &["revert", "--abort"]);
        git(&worktree, &["reset", "-q", "--hard", &main])?;
        return Err(format!("{range} no longer reverts cleanly onto main {}", short(&main)));
    }
    let branch = format!("revert-{}", short(&lane.tip));
    git(&worktree, &["checkout", "-q", "-B", &branch])?;
    let subject = git(&worktree, &["log", "-1", "--format=%s", &lane.tip])?;
    let trailers: Vec<String> = commits.lines().map(|commit| format!("Reverts: {commit}")).collect();
    let message = format!("Revert {subject}\n\n{} landed and its post-land checks failed on {} ({detail}), so the lander reverts it through the landing queue.\n\n{}", lane.ticket.branch, short(&lane.tip), trailers.join("\n"));
    git(&worktree, &["-c", "core.hooksPath=/dev/null", "commit", "-q", "--no-verify", "-m", &message])?;
    let head = sheriff::queue_revert(lander, &worktree, &branch)?;
    if let Some(session) = lane.ticket.session.as_deref().filter(|session| !session.is_empty()) {
        let text = format!("north land: {} landed, but its post-land checks failed on {} ({detail}); the lander queued its revert {branch} ahead of the queue; fix the lane and queue it again with Follows: {}", lane.ticket.branch, short(&lane.tip), short(&lane.tip));
        if !after::send(session, &text) {
            println!("north land: could not tell session {session} that {} was reverted", lane.ticket.branch);
        }
    }
    Ok(format!("revert of {} {head} queued as {branch}", lane.ticket.branch))
}

#[cfg(test)]
#[path = "watch_tests.rs"]
mod tests;
