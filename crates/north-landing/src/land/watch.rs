use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::after;
use super::after_landing_lifecycle::{self as lifecycle, AfterLandingRun, Run};
use super::candidate::{self, git, output};
use super::checks::Verdict;
use super::lander::Lander;
use super::after::owner::{self, Reverted};
use super::post_land::{self, Found, Hosts, Outcome};
use super::queue::{self, Ticket};
use super::jobs::Jobs;
use super::setting;
use super::slots::select::Config;
use super::sheriff;
use super::telemetry::epoch;
use super::trace::{self, AFTER, COVERED_BY, Category, PHASE, Span, VERDICT};

const WORKTREE: &str = "lander-revert";
const JOURNAL: &str = "following";
const RETRY: Duration = Duration::from_secs(30);
const UNSETTLED: usize = 3;
const AFTER_RUNS: &str = "landing.after-runs";

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

type Journaled = (f64, Entry);

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
    host: Option<String>,
    cancel: Arc<AtomicBool>,
}

struct Searching {
    batch: Batch,
    outcome: Outcome,
    search: JoinHandle<Found>,
    host: Option<String>,
    cancel: Arc<AtomicBool>,
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
        let ready = self.batches.iter().position(|batch| batch.retry_at <= now)?;
        Some(self.batches.remove(ready))
    }
}

#[derive(Default)]
pub struct Watch {
    running: Vec<Running>,
    searching: Vec<Searching>,
    pending: Pending,
    hosts: Hosts,
    pub jobs: Jobs,
    draining: bool,
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

fn heads(batch: &Batch) -> impl Iterator<Item = &str> {
    batch.heads.iter().map(|followed| followed.head.as_str())
}

/// Moves each head's after-landing run record, printing why when the store cannot be opened.
fn moved<'a>(heads: impl IntoIterator<Item = &'a str>, to: Run, cause: &str) {
    match lifecycle::host() {
        Ok(mut store) => heads.into_iter().for_each(|head| {
            lifecycle::moved::<AfterLandingRun>(&mut store, head, to, cause);
        }),
        Err(error) => println!("{error}"),
    }
}

/// Whether a stopped lander's journal entry still needs its run: a head whose record retired already has its verdict.
fn unsettled(head: &str) -> bool {
    lifecycle::host().ok().and_then(|store| lifecycle::current::<AfterLandingRun>(&store, head)).is_none_or(|record| record.state != Run::Retired)
}

impl Watch {
    pub fn resume(lander: &Lander) -> Self {
        let mut watch = Self::default();
        let Ok(entries) = fs::read_dir(journal(lander)) else { return watch };
        let mut paths: Vec<(f64, PathBuf)> = entries.filter_map(Result::ok).map(|entry| entry.path()).filter(|path| path.extension().is_some_and(|extension| extension == "json")).map(|path| (journaled_at(&path), path)).collect();
        paths.sort_by(|left, right| left.0.total_cmp(&right.0));
        let mut journaled: Vec<Journaled> = Vec::new();
        for (at, path) in paths {
            match read_entry(&path) {
                Ok(entry) if unsettled(&entry.head) => journaled.push((at, entry)),
                Ok(entry) => {
                    let _ = fs::remove_file(&path);
                    println!("north land: {} already has its after-landing verdict; dropping its journal entry", entry.head);
                }
                Err(error) => {
                    println!("north land: dropping the unreadable after-landing journal entry {}: {error}", path.display());
                    moved(path.file_stem().and_then(|stem| stem.to_str()), Run::Retired, &format!("its journal entry is unreadable: {error}"));
                    let _ = fs::remove_file(&path);
                }
            }
        }
        for (_, runner) in post_land::adoptable(&lander.queue) {
            let (mut covered, rest): (Vec<Journaled>, Vec<Journaled>) = journaled.into_iter().partition(|(_, entry)| runner.heads.contains(&entry.head));
            journaled = rest;
            if covered.len() == runner.heads.len() {
                covered.sort_by_key(|(_, entry)| runner.heads.iter().position(|head| *head == entry.head));
                watch.adopt(lander, runner, covered);
            } else {
                println!("north land: stopping the after-landing runner of {}, whose landings are no longer all journaled", runner.head);
                post_land::abandon(&runner);
                journaled.extend(covered);
                journaled.sort_by(|left, right| left.0.total_cmp(&right.0));
            }
        }
        let resumed = epoch();
        for (at, Entry { base, head, lanes }) in journaled {
            println!("north land: rerunning the after-landing run of {head}, which a stopped lander left without a verdict");
            record(lander, &[Span::new(&head, "after-landing run lost to a lander restart", Category::Lander, at, resumed).with(PHASE, AFTER)]);
            moved([head.as_str()], Run::Journaled, "rerun: a stopped lander left it without a verdict");
            if !watch.pending.covers(&head) {
                watch.pending.add(Batch::landed(base, head, lanes, resumed));
            }
        }
        watch
    }

    fn adopt(&mut self, lander: &Lander, runner: post_land::Record, covered: Vec<Journaled>) {
        let mut batch = Batch { base: runner.base.clone(), head: runner.head.clone(), lanes: Vec::new(), heads: Vec::new(), started: Some(runner.started), retry_at: 0.0 };
        for (at, entry) in covered {
            batch.lanes.extend(entry.lanes);
            batch.heads.push(Followed { head: entry.head, at });
        }
        if let Some(host) = &runner.host {
            self.hosts.hold(host);
        }
        let host = runner.host.clone();
        let cancel = Arc::default();
        moved(heads(&batch), Run::Running, "adopted the runner a stopped lander started");
        let run = post_land::adopt(lander, landed(lander, &batch), runner, Arc::clone(&cancel));
        self.running.push(Running { batch, run, host, cancel });
    }

    pub fn journaled(&self) -> usize {
        self.running.len() + self.searching.len() + self.pending.batches.len()
    }

    pub fn in_flight(&self) -> usize {
        self.journaled() + self.jobs.running().len()
    }

    pub fn drain(&mut self) {
        self.draining = true;
    }

    pub fn adoptable(&self, lander: &Lander) -> bool {
        let recorded = post_land::adoptable(&lander.queue);
        self.searching.is_empty() && self.running.iter().all(|running| recorded.iter().any(|(_, record)| record.head == running.batch.head))
    }

    pub fn follow(&mut self, lander: &Lander, base: &str, head: &str, lanes: Vec<LandedLane>) {
        if self.running.iter().any(|running| running.batch.covers(head)) || self.searching.iter().any(|searching| searching.batch.covers(head)) || self.pending.covers(head) {
            return;
        }
        let entry = Entry { base: base.to_owned(), head: head.to_owned(), lanes };
        let recorded = serde_json::to_string_pretty(&entry).map_err(|error| error.to_string()).and_then(|text| queue::write_atomic(&entry_path(lander, head), &text).map_err(|error| error.to_string()));
        if let Err(error) = recorded {
            println!("north land: could not journal the after-landing run of {head}, so a lander restart before its verdict loses it: {error}");
        }
        let Entry { base, head, lanes } = entry;
        moved([head.as_str()], Run::Journaled, &format!("landed on {base}"));
        self.supersede(lander, &head);
        self.pending.add(Batch::landed(base, head, lanes, epoch()));
        self.start_next(lander);
    }

    fn supersede(&self, lander: &Lander, newest: &str) {
        let current = Config::after_lines(&lander.repo, newest);
        let flights = self.running.iter().map(|running| (&running.batch, &running.cancel)).chain(self.searching.iter().map(|searching| (&searching.batch, &searching.cancel)));
        for (batch, cancel) in flights {
            if !cancel.load(Ordering::Relaxed) && superseded(&Config::after_lines(&lander.repo, &batch.head), &current) {
                println!("north land: {newest} changed the check lines of the after-landing run of {} in flight, so that run is cancelled as superseded and its landings are checked with {newest}", batch.head);
                cancel.store(true, Ordering::Relaxed);
            }
        }
    }

    fn start_next(&mut self, lander: &Lander) {
        if self.draining || self.pending.batches.is_empty() {
            return;
        }
        let limit = after_runs(lander);
        while self.running.len() < limit && self.start_one(lander) {}
    }

    fn start_one(&mut self, lander: &Lander) -> bool {
        let started = epoch();
        let Some(mut batch) = self.pending.next(started) else { return false };
        batch.started.get_or_insert(started);
        if batch.heads.len() > 1 {
            println!("north land: one after-landing run of {} covers {} landings since {}", batch.head, batch.heads.len(), batch.base);
        }
        let waits: Vec<Span> = batch.heads.iter().filter(|followed| started - followed.at >= 1.0).map(|followed| Span::new(&followed.head, "waiting for the after-landing run in flight", Category::Queue, followed.at, started).with(PHASE, AFTER)).collect();
        record(lander, &waits);
        let host = self.hosts.claim(&lander.repo, &batch.head);
        let cancel = Arc::default();
        moved(heads(&batch), Run::Running, &format!("its after-landing run of {} started{}", batch.head, host.as_deref().map_or_else(String::new, |host| format!(" on {host}"))));
        let run = post_land::start(lander, landed(lander, &batch), host.clone(), Arc::clone(&cancel));
        self.running.push(Running { batch, run, host, cancel });
        true
    }

    pub fn settle(&mut self, lander: &Lander) -> bool {
        let mut recovered = false;
        for landing in self.jobs.settle() {
            self.follow(lander, &landing.base, &landing.head, landing.lanes);
            recovered = true;
        }
        let mut settled = recovered;
        while let Some(at) = self.running.iter().position(|running| running.run.is_finished()) {
            let Running { batch, run, host, cancel } = self.running.remove(at);
            self.finish(lander, batch, run, host, cancel);
            settled = true;
        }
        while let Some(at) = self.searching.iter().position(|searching| searching.search.is_finished()) {
            let Searching { batch, outcome, search, host, .. } = self.searching.remove(at);
            if let Some(host) = host {
                self.hosts.release(&host);
            }
            let found = search.join().unwrap_or(Found { culprit: None, span: (epoch(), epoch()), superseded: false });
            conclude(lander, &batch, &outcome, &found);
            settled = true;
        }
        self.start_next(lander);
        settled
    }

    fn finish(&mut self, lander: &Lander, mut batch: Batch, run: JoinHandle<Outcome>, host: Option<String>, cancel: Arc<AtomicBool>) {
        let mut outcome = run.join().unwrap_or_else(|_| Outcome { verdict: Verdict::Transient("the post-land thread panicked".to_owned()), search: None, log: PathBuf::new(), failing: BTreeSet::new(), superseded: false });
        let ended = epoch();
        let release = |hosts: &mut Hosts, host: &Option<String>| {
            if let Some(host) = host {
                hosts.release(host);
            }
        };
        if outcome.superseded && matches!(outcome.verdict, Verdict::Transient(_)) {
            println!("north land: the after-landing run of {} was superseded; its landings run again now with the newest landing", batch.head);
            moved(heads(&batch), Run::Journaled, "superseded by a landing that changed its check lines");
            for followed in &mut batch.heads {
                followed.at = ended;
            }
            release(&mut self.hosts, &host);
            batch.retry_at = ended;
            self.pending.retry(batch);
            return;
        }
        if let Verdict::Transient(detail) = &outcome.verdict {
            println!("north land: the after-landing run of {} reached no verdict ({detail}); it runs again in {} s", batch.head, RETRY.as_secs());
            batch.retry_at = ended + RETRY.as_secs_f64();
            moved(heads(&batch), Run::Unsettled, detail);
            let misses = lifecycle::host().map_or(0, |store| heads(&batch).map(|head| lifecycle::misses(&store, head)).max().unwrap_or(0));
            if misses == UNSETTLED {
                after::send("proxy", &format!("north land: the after-landing run of {} reached no verdict {UNSETTLED} times in a row ({detail}); main is unverified until one does, and it keeps running every {} s", batch.head, RETRY.as_secs()));
            }
            release(&mut self.hosts, &host);
            for followed in &mut batch.heads {
                followed.at = ended;
            }
            self.pending.retry(batch);
            return;
        }
        let started = batch.started.unwrap_or(ended);
        let links: Vec<Span> = batch.heads.iter().filter(|followed| followed.head != batch.head).map(|followed| Span::new(&followed.head, format!("after-landing run of {}", batch.head), Category::Lander, started, ended).with(PHASE, AFTER).with(COVERED_BY, batch.head.clone())).collect();
        record(lander, &links);
        let (class, _) = outcome.verdict.class();
        let verdicts: Vec<Span> = batch.heads.iter().map(|followed| Span::new(&followed.head, format!("verdict {class}"), Category::Lander, ended, ended).with(PHASE, AFTER).with(VERDICT, class)).collect();
        record(lander, &verdicts);
        match outcome.search.take() {
            Some(search) if !outcome.superseded => {
                moved(heads(&batch), Run::Searching, &format!("verdict {class}, searching for the culprit lane"));
                self.searching.push(Searching { batch, outcome, search: post_land::search(search), host, cancel });
                return;
            }
            _ => release(&mut self.hosts, &host),
        }
        if outcome.superseded && matches!(outcome.verdict, Verdict::RedMain(_)) {
            println!("north land: the after-landing run of {} is red, but a later landing changed its check lines before its culprit search, so no lane is blamed and the newest landing's run checks the current lines", batch.head);
        }
        retire(lander, &batch, &format!("verdict {class}"));
    }
}

fn landed(lander: &Lander, batch: &Batch) -> post_land::Landed {
    let heads = batch.heads.iter().map(|followed| followed.head.clone()).collect();
    let names = batch.lanes.iter().map(|lane| lane.ticket.branch.clone()).collect();
    let tips = batch.lanes.iter().map(|lane| lane.tip.clone()).collect();
    post_land::Landed::on(lander, &batch.base, &batch.head, heads, names, tips)
}

fn conclude(lander: &Lander, batch: &Batch, outcome: &Outcome, found: &Found) {
    let (from, to) = found.span;
    record(lander, &[Span::new(&batch.head, format!("culprit search over {} landed lanes", batch.lanes.len()), Category::Lander, from, to).with(PHASE, AFTER)]);
    if found.superseded {
        println!("north land: the after-landing run of {} is red, but a later landing changed its check lines during the culprit search, so no lane is blamed and the newest landing's run checks the current lines", batch.head);
    } else if let Some(verdict) = north_check_environment::credential::incident(&outcome.log) {
        println!("north land: the post-land checks of {} are red: {verdict}", batch.head);
    } else if let Verdict::RedMain(detail) = &outcome.verdict {
        blame(lander, batch, found.culprit, detail, outcome);
    }
    retire(lander, batch, "its culprit search ended");
}

fn retire(lander: &Lander, batch: &Batch, cause: &str) {
    moved(heads(batch), Run::Retired, cause);
    for followed in &batch.heads {
        let _ = fs::remove_file(entry_path(lander, &followed.head));
    }
}

fn superseded(running: &[String], current: &[String]) -> bool {
    running.iter().any(|line| !current.contains(line))
}

fn after_runs(lander: &Lander) -> usize {
    setting::landing(&lander.repo, "refs/remotes/origin/main", AFTER_RUNS).parse().ok().filter(|runs| *runs > 0).unwrap_or(1)
}

fn record(lander: &Lander, spans: &[Span]) {
    if let Err(error) = trace::Spans::beside(&lander.queue.join("logs")).append(spans) {
        println!("north land: could not record after-landing spans: {error}");
    }
}

fn blame(lander: &Lander, batch: &Batch, culprit: Option<usize>, detail: &str, outcome: &Outcome) {
    let Some(at) = culprit else {
        inherited(lander, &batch.head, detail, outcome);
        return;
    };
    let (base, lanes) = (&batch.base, &batch.lanes);
    let lane = &lanes[at];
    let since = if at == 0 { base } else { &lanes[at - 1].tip };
    if lane.ticket.red_fix.is_some() {
        let message = format!("north land: the post-land checks failed ({detail}) with {}, a red fix; it is not reverted again", lane.ticket.branch);
        println!("{message}");
        after::send("proxy", &message);
        return;
    }
    match revert(lander, lane, since, detail, outcome) {
        Ok(line) => println!("north land: {line}"),
        Err(error) => {
            let message = format!("north land: the post-land checks of {} failed ({detail}) and its revert could not be queued: {error}", lane.ticket.branch);
            println!("{message}");
            after::send("proxy", &message);
        }
    }
}

fn inherited(lander: &Lander, head: &str, detail: &str, outcome: &Outcome) {
    match sheriff::chase(lander, head, &outcome.failing, &outcome.log) {
        Ok(line) => println!("north land: sheriff: {line}"),
        Err(error) => {
            let message = format!("north land: the post-land checks of {head} are red ({detail}) with no culprit among the batch, and the sheriff could not be asked for the earlier one: {error}");
            println!("{message}");
            after::send("proxy", &message);
        }
    }
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(9)]
}

fn revert(lander: &Lander, lane: &LandedLane, since: &str, detail: &str, outcome: &Outcome) -> Result<String, String> {
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
    let failing = if outcome.failing.is_empty() { detail.to_owned() } else { sheriff::names(&outcome.failing.iter().cloned().collect::<Vec<_>>()) };
    let reverted = Reverted { branch: &lane.ticket.branch, commit: &lane.tip, revert: &branch, red: &lane.tip, failing: &failing, log: &outcome.log };
    println!("north land: {}", owner::tell(&owner::Cli, lane.ticket.session.as_deref(), &reverted));
    Ok(format!("revert of {} {head} queued as {branch}", lane.ticket.branch))
}

#[cfg(test)]
#[path = "watch_tests.rs"]
mod tests;
