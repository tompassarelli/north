//! `north land adopt` (north#1575): lands a finished lane whose session has ended. A lane is finished when its last
//! finish call passed on the commit it holds now (the same commit, or the same change rebased or squashed) and its
//! worktree holds no uncommitted change; it is adopted once no session recorded in it or named by that finish record is
//! live and no ticket names its head. Adoption rebases it onto origin/main and hands it to the lander through the same
//! road a session's hand-in takes, waiting for the verdict; a conflict, a refusal before queueing or the lander's refusal
//! is filed on the lane's issue. Each head is tried once, so a refused lane stays put for a session to pick up.
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use north_base::platform::finish_record::{FinishRecords, Finished};
use north_base::platform::land_queue;
use serde::{Deserialize, Serialize};

pub const USAGE: &str = "usage: north land adopt [--dry-run]";
const LEDGER: &str = "north/land/adopted.jsonl";
const LOCK: &str = "north/land/adopt.lock";
const TICKET_AREAS: [&str; 6] = ["", "outbox", "done", "withdrawn", "outbox/decided", "decided"];

/// Whether a session still works in a lane: the fleet's records answer it, so the lander reads no fleet store itself.
pub trait Sessions {
    fn live(&self, session: &str, lane: &Path) -> bool;
}

/// Where an adopted lane goes: the lander, through the road `safe-push --to main --wait` takes, and the lane's issue for why it did not land.
pub trait Adopter {
    fn land(&self, lane: &Path) -> Result<String, String>;
    fn file(&self, lane: &Path, why: &str) -> Result<String, String>;
}

pub struct Inputs {
    pub code: PathBuf,
    pub state: PathBuf,
    pub host: String,
    pub records: FinishRecords,
}

impl Inputs {
    pub fn here() -> Self {
        let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
        Self { code: home.join("code"), state: super::lander::state_home(), host: land_queue::local_host(), records: FinishRecords::here() }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Orphan {
    pub root: PathBuf,
    pub head: String,
    pub session: String,
}

#[derive(Serialize, Deserialize)]
struct Tried {
    root: PathBuf,
    heads: Vec<String>,
    session: String,
    outcome: String,
}

struct Known {
    finished: Vec<Finished>,
    trees: BTreeMap<PathBuf, BTreeMap<String, usize>>,
    tried: BTreeSet<String>,
    ticketed: Vec<(PathBuf, String)>,
    queues: PathBuf,
}

pub fn orphans(inputs: &Inputs, sessions: &impl Sessions) -> Vec<Orphan> {
    let finished: Vec<Finished> = inputs.records.all().into_iter().filter(|record| record.passed).collect();
    if finished.is_empty() {
        return Vec::new();
    }
    let lanes = lanes(&inputs.code);
    let mut trees = BTreeMap::new();
    for lane in &lanes {
        if let Some(container) = container(lane).filter(|container| !trees.contains_key(container)) {
            trees.insert(container, record_trees(lane, &finished));
        }
    }
    let known = Known { finished, trees, tried: tried_heads(&inputs.state), ticketed: ticketed(&inputs.state, &inputs.host), queues: land_queue::queues(&inputs.state) };
    lanes.into_iter().filter_map(|root| orphan(inputs, sessions, &known, root)).collect()
}

fn orphan(inputs: &Inputs, sessions: &impl Sessions, known: &Known, root: PathBuf) -> Option<Orphan> {
    let (head, tree) = git(&root, &["log", "-1", "--format=%H %T"])?.split_once(' ').map(|(head, tree)| (head.to_owned(), tree.to_owned()))?;
    let record = &known.finished[*known.trees.get(&container(&root)?)?.get(&tree)?];
    let queued = known.ticketed.iter().any(|(named, held)| *held == head && same(named, &root)) || !land_queue::waiting_on(&known.queues, &inputs.host, &root).is_empty();
    let adoptable = !known.tried.contains(&head) && !queued && clean(&root) && !landed(&root, &head) && !sessions.live(&record.session, &root);
    adoptable.then(|| Orphan { session: record.session.clone(), root, head })
}

fn container(lane: &Path) -> Option<PathBuf> {
    lane.parent()?.parent().map(Path::to_path_buf)
}

fn record_trees(lane: &Path, finished: &[Finished]) -> BTreeMap<String, usize> {
    let heads: String = finished.iter().map(|record| format!("{}\n", record.head)).collect();
    let Ok(mut child) = Command::new("git").arg("-C").arg(lane).args(["log", "--no-walk", "--ignore-missing", "--stdin", "--format=%H %T"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn() else { return BTreeMap::new() };
    if let Some(mut input) = child.stdin.take() {
        let _ = input.write_all(heads.as_bytes());
    }
    let Ok(output) = child.wait_with_output() else { return BTreeMap::new() };
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines().filter_map(|line| line.split_once(' ')).filter_map(|(head, tree)| Some((tree.to_owned(), finished.iter().position(|record| record.head == head)?))).collect()
}
pub fn adopt(inputs: &Inputs, sessions: &impl Sessions, adopter: &impl Adopter, dry_run: bool) -> Vec<String> {
    if dry_run {
        return orphans(inputs, sessions).iter().map(|orphan| format!("north land adopt: would adopt {} at {} (session {} ended)", orphan.root.display(), orphan.head, orphan.session)).collect();
    }
    let Some(_held) = lock(&inputs.state) else {
        return vec!["north land adopt: another adoption is running".to_owned()];
    };
    orphans(inputs, sessions).iter().flat_map(|orphan| take(&inputs.state, adopter, orphan)).collect()
}

fn take(state: &Path, adopter: &impl Adopter, orphan: &Orphan) -> Vec<String> {
    let lane = orphan.root.display().to_string();
    let (outcome, line) = match rebase(&orphan.root).and_then(|()| adopter.land(&orphan.root)) {
        Ok(landed) => ("landed", format!("north land adopt: {lane} (session {} ended): {landed}", orphan.session)),
        Err(why) => {
            let why = format!("The lander adopted the finished lane {lane} at {} after its session {} ended and could not land it (north#1575):\n\n{why}", orphan.head, orphan.session);
            match adopter.file(&orphan.root, &why) {
                Ok(place) => ("filed", format!("north land adopt: {lane} did not land; filed on {place}")),
                Err(error) => ("refused", format!("north land adopt: {lane} did not land and the filing failed ({error}):\n{why}")),
            }
        }
    };
    let heads: Vec<String> = [Some(orphan.head.clone()), git(&orphan.root, &["rev-parse", "HEAD"])].into_iter().flatten().collect();
    let recorded = record(state, &Tried { root: orphan.root.clone(), heads, session: orphan.session.clone(), outcome: outcome.to_owned() });
    let mut lines = vec![line];
    if let Err(error) = recorded {
        lines.push(format!("north land adopt: could not record the adoption of {lane}: {error}"));
    }
    lines
}

fn rebase(root: &Path) -> Result<(), String> {
    git(root, &["fetch", "-q", "origin", "main"]).ok_or("git fetch origin main failed")?;
    if git(root, &["rebase", "-q", "origin/main"]).is_some() {
        return Ok(());
    }
    let conflicts = git(root, &["diff", "--name-only", "--diff-filter=U"]).unwrap_or_default();
    git(root, &["rebase", "--abort"]);
    Err(format!("the lane no longer rebases onto origin/main; conflicting paths:\n{conflicts}"))
}

fn clean(root: &Path) -> bool {
    git(root, &["status", "--porcelain", "--untracked-files=no"]).is_some_and(|status| status.is_empty())
}

fn landed(root: &Path, head: &str) -> bool {
    Command::new("git").arg("-C").arg(root).args(["merge-base", "--is-ancestor", head, "origin/main"]).stdin(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|status| status.success())
}

fn same(named: &Path, root: &Path) -> bool {
    named == root || fs::canonicalize(named).ok().is_some_and(|named| fs::canonicalize(root).ok() == Some(named))
}

fn ticketed(state: &Path, host: &str) -> Vec<(PathBuf, String)> {
    let ticket = |path: PathBuf| {
        let ticket: serde_json::Value = serde_json::from_str(&fs::read_to_string(path).ok()?).ok()?;
        ticket["host"].as_str().is_none_or(|named| named == host).then_some(())?;
        Some((PathBuf::from(ticket["root"].as_str()?), ticket["head"].as_str()?.to_owned()))
    };
    fs::read_dir(land_queue::queues(state))
        .into_iter()
        .flatten()
        .flatten()
        .flat_map(|queue| TICKET_AREAS.map(|area| queue.path().join(area)))
        .flat_map(|area| fs::read_dir(area).into_iter().flatten().flatten().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|extension| extension == "json"))
        .filter_map(ticket)
        .collect()
}

fn tried_heads(state: &Path) -> BTreeSet<String> {
    let text = fs::read_to_string(state.join(LEDGER)).unwrap_or_default();
    text.lines().filter_map(|line| serde_json::from_str::<Tried>(line).ok()).flat_map(|tried| tried.heads).collect()
}

fn record(state: &Path, tried: &Tried) -> std::io::Result<()> {
    let path = state.join(LEDGER);
    fs::create_dir_all(path.parent().unwrap_or(state))?;
    let mut file = fs::OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{}", serde_json::to_string(tried).map_err(std::io::Error::other)?)
}

fn lock(state: &Path) -> Option<fs::File> {
    let path = state.join(LOCK);
    fs::create_dir_all(path.parent().unwrap_or(state)).ok()?;
    let file = fs::OpenOptions::new().create(true).truncate(false).write(true).open(path).ok()?;
    rustix::fs::flock(&file, rustix::fs::FlockOperation::NonBlockingLockExclusive).ok()?;
    Some(file)
}

fn lanes(code: &Path) -> Vec<PathBuf> {
    let directories = |path: PathBuf| fs::read_dir(path).into_iter().flatten().flatten().filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir())).map(|entry| entry.path());
    let mut lanes: Vec<PathBuf> = directories(code.to_path_buf()).flat_map(|container| directories(container.join("worktrees"))).collect();
    lanes.sort();
    lanes
}

fn git(root: &Path, arguments: &[&str]) -> Option<String> {
    let output = Command::new("git").arg("-C").arg(root).args(arguments).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}
