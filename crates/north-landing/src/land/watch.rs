use std::fs;
use std::path::{Path, PathBuf};
use std::thread::JoinHandle;

use serde::{Deserialize, Serialize};

use super::after;
use super::candidate::{self, git, output};
use super::checks::Verdict;
use super::lander::Lander;
use super::post_land::{self, Outcome};
use super::queue::{self, Ticket};
use super::sheriff;

const WORKTREE: &str = "lander-revert";
const JOURNAL: &str = "following";

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

struct Following {
    base: String,
    head: String,
    lanes: Vec<LandedLane>,
    run: JoinHandle<Outcome>,
}

#[derive(Default)]
pub struct Watch {
    following: Vec<Following>,
}

fn journal(lander: &Lander) -> PathBuf {
    lander.queue.join(JOURNAL)
}

fn entry_path(lander: &Lander, head: &str) -> PathBuf {
    journal(lander).join(format!("{head}.json"))
}

fn read_entry(path: &Path) -> Result<Entry, String> {
    let text = fs::read_to_string(path).map_err(|error| error.to_string())?;
    serde_json::from_str(&text).map_err(|error| error.to_string())
}

impl Watch {
    pub fn resume(lander: &Lander) -> Self {
        let mut watch = Self::default();
        let Ok(entries) = fs::read_dir(journal(lander)) else { return watch };
        let mut paths: Vec<PathBuf> = entries.filter_map(Result::ok).map(|entry| entry.path()).filter(|path| path.extension().is_some_and(|extension| extension == "json")).collect();
        paths.sort_by_key(|path| fs::metadata(path).and_then(|metadata| metadata.modified()).ok());
        for path in paths {
            match read_entry(&path) {
                Ok(entry) => {
                    println!("north land: rerunning the after-landing run of {}, which a stopped lander left without a verdict", entry.head);
                    watch.follow(lander, &entry.base, &entry.head, entry.lanes);
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
        self.following.len()
    }

    pub fn follow(&mut self, lander: &Lander, base: &str, head: &str, lanes: Vec<LandedLane>) {
        if self.following.iter().any(|following| following.head == head) {
            return;
        }
        let entry = Entry { base: base.to_owned(), head: head.to_owned(), lanes };
        let recorded = serde_json::to_string_pretty(&entry).map_err(|error| error.to_string()).and_then(|text| queue::write_atomic(&entry_path(lander, head), &text).map_err(|error| error.to_string()));
        if let Err(error) = recorded {
            println!("north land: could not journal the after-landing run of {head}, so a lander restart before its verdict loses it: {error}");
        }
        let Entry { lanes, .. } = entry;
        let names = lanes.iter().map(|lane| lane.ticket.branch.clone()).collect();
        let tips = lanes.iter().map(|lane| lane.tip.clone()).collect();
        let run = post_land::start(lander, base, head, names, tips);
        self.following.push(Following { base: base.to_owned(), head: head.to_owned(), lanes, run });
    }

    pub fn settle(&mut self, lander: &Lander) -> bool {
        let mut settled = false;
        let mut index = 0;
        while index < self.following.len() {
            if !self.following[index].run.is_finished() {
                index += 1;
                continue;
            }
            let Following { base, head, lanes, run } = self.following.swap_remove(index);
            settled = true;
            let outcome = run.join().unwrap_or_else(|_| Outcome { verdict: Verdict::Transient("the post-land thread panicked".to_owned()), culprit: None });
            if let Verdict::RedMain(detail) = outcome.verdict {
                blame(lander, &base, &lanes, outcome.culprit, &detail);
            }
            let _ = fs::remove_file(entry_path(lander, &head));
        }
        settled
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
