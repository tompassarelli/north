//! `north land adopt` (north#1575): lands a finished lane whose session has ended. A lane is finished when its last
//! finish call passed on the commit it holds now (the same commit, or the same change rebased or squashed) and its
//! worktree holds no uncommitted change; it is adopted once no session recorded in it or named by that finish record is
//! live and no ticket names its head. Adoption rebases it onto origin/main and hands it to the lander through the same
//! road a session's hand-in takes, waiting for the verdict; a conflict, a refusal before queueing or the lander's refusal
//! is filed on the lane's issue. Each head is tried once, so a refused lane stays put for a session to pick up.
//!
//! A worker writes its lane's gitfile, its git directory's `commondir` and what lies there, so a lane is taken only when
//! its git directory lies in its project's main repository under `worktrees/` and its common directory is that
//! repository; every git then runs with those directories forced, no fsmonitor and no hooks, in a worker's domain built
//! for that lane, and the finish records' heads resolve in the main repository.
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
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
/// The words after the north binary that run a program in a worker's domain built for the lane it starts in.
pub const WORKER_DOMAIN: [&str; 4] = ["--worker-sandbox", "--program", "--worker", "--"];
const UNHOOKED: [&str; 4] = ["-c", "core.fsmonitor=", "-c", "core.hooksPath=/dev/null"];

/// Whether a session still works in a lane: the fleet's records answer it, so the lander reads no fleet store itself.
pub trait Sessions {
    fn live(&self, session: &str, lane: &Path) -> bool;
}

/// Where an adopted lane goes: the lander, through the road `safe-push --to main --wait` takes, and the lane's issue for why it did not land.
pub trait Adopter {
    fn land(&self, lane: &Lane) -> Result<String, Refused>;
    fn file(&self, lane: &Lane, why: &str) -> Result<String, String>;
}

/// Why an adopted lane did not land: `summary` is fixed text of the adoption's own, which the lane's issue hears, and `detail` is what the lane's git and checks printed, which only the ledger keeps.
#[derive(Debug)]
pub struct Refused {
    pub summary: String,
    pub detail: String,
}

pub struct Inputs {
    pub code: PathBuf,
    pub state: PathBuf,
    pub host: String,
    pub records: FinishRecords,
    /// The words every program the adoption runs in a lane starts behind: a worker's domain, or none in a test.
    pub jail: Vec<OsString>,
}

impl Inputs {
    pub fn here() -> Self {
        let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
        let north = north_base::platform::north_binary::own_binary().into_os_string();
        let jail = std::iter::once(north).chain(WORKER_DOMAIN.map(OsString::from)).collect();
        Self { code: home.join("code"), state: super::lander::state_home(), host: land_queue::local_host(), records: FinishRecords::here(), jail }
    }
}

/// A checkout under `~/code/PROJECT/worktrees/` whose git directory is `~/code/PROJECT/main/.git/worktrees/NAME` and whose common directory is `~/code/PROJECT/main/.git`, both resolved once and canonical.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lane {
    pub root: PathBuf,
    pub git_dir: PathBuf,
    pub common: PathBuf,
}

impl Lane {
    fn of(root: PathBuf) -> Option<Self> {
        let repository = root.parent()?.parent()?.join("main/.git").canonicalize().ok()?;
        let pointer = fs::read_to_string(root.join(".git")).ok()?;
        let git_dir = root.join(pointer.trim().strip_prefix("gitdir:")?.trim()).canonicalize().ok()?;
        let common = git_dir.join(fs::read_to_string(git_dir.join("commondir")).ok()?.trim()).canonicalize().ok()?;
        let back = PathBuf::from(fs::read_to_string(git_dir.join("gitdir")).ok()?.trim()).canonicalize().ok()?;
        let own = back == root.join(".git").canonicalize().ok()?;
        (own && common == repository && git_dir.parent() == Some(repository.join("worktrees").as_path())).then_some(Self { root, git_dir, common })
    }

    /// The project's main checkout, as a lane of its own repository.
    pub fn main(&self) -> Self {
        Self { root: self.common.parent().unwrap_or(&self.common).to_path_buf(), git_dir: self.common.clone(), common: self.common.clone() }
    }

    /// `program` started in the lane behind `jail`.
    pub fn command(&self, jail: &[OsString], program: impl AsRef<OsStr>) -> Command {
        let mut command = match jail.split_first() {
            Some((first, rest)) => {
                let mut command = Command::new(first);
                command.args(rest).arg(program);
                command
            }
            None => Command::new(program),
        };
        command.current_dir(&self.root).stdin(Stdio::null());
        command
    }

    /// git in the lane behind `jail`, reading only the resolved git directories and running no program their configuration names.
    pub fn git_command(&self, jail: &[OsString]) -> Command {
        let mut command = self.command(jail, "git");
        command.args(UNHOOKED).env("GIT_DIR", &self.git_dir).env("GIT_COMMON_DIR", &self.common).env("GIT_WORK_TREE", &self.root);
        command
    }

    /// Whether the repository's own settings turn on per-worktree config, which lets a lane's own settings file name what the lander reads from it.
    pub fn per_worktree_config(&self, jail: &[OsString]) -> bool {
        self.git(jail, &["config", "--type=bool", "--get", "extensions.worktreeConfig"]).is_some_and(|value| value == "true")
    }

    pub fn git(&self, jail: &[OsString], arguments: &[&str]) -> Option<String> {
        let output = self.git_command(jail).args(arguments).stderr(Stdio::null()).output().ok()?;
        output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Orphan {
    pub lane: Lane,
    pub head: String,
    pub session: String,
}

#[derive(Serialize, Deserialize)]
struct Tried {
    root: PathBuf,
    heads: Vec<String>,
    session: String,
    outcome: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    detail: String,
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
    let lanes: Vec<Lane> = lanes(&inputs.code).into_iter().filter(|lane| !lane.main().per_worktree_config(&inputs.jail)).collect();
    let mut trees = BTreeMap::new();
    for lane in &lanes {
        if !trees.contains_key(&lane.common) {
            trees.insert(lane.common.clone(), record_trees(&lane.main(), &inputs.jail, &finished));
        }
    }
    let known = Known { finished, trees, tried: tried_heads(&inputs.state), ticketed: ticketed(&inputs.state, &inputs.host), queues: land_queue::queues(&inputs.state) };
    lanes.into_iter().filter_map(|lane| orphan(inputs, sessions, &known, lane)).collect()
}

fn orphan(inputs: &Inputs, sessions: &impl Sessions, known: &Known, lane: Lane) -> Option<Orphan> {
    let jail = &inputs.jail;
    let (head, tree) = lane.git(jail, &["log", "-1", "--format=%H %T"])?.split_once(' ').map(|(head, tree)| (head.to_owned(), tree.to_owned()))?;
    let record = &known.finished[*known.trees.get(&lane.common)?.get(&tree)?];
    let queued = known.ticketed.iter().any(|(named, held)| *held == head && same(named, &lane.root)) || !land_queue::waiting_on(&known.queues, &inputs.host, &lane.root).is_empty();
    let adoptable = !known.tried.contains(&head) && !queued && clean(&lane, jail) && !landed(&lane, jail, &head) && !sessions.live(&record.session, &lane.root);
    adoptable.then(|| Orphan { session: record.session.clone(), lane, head })
}

fn record_trees(main: &Lane, jail: &[OsString], finished: &[Finished]) -> BTreeMap<String, usize> {
    let heads: String = finished.iter().map(|record| format!("{}\n", record.head)).collect();
    let Ok(mut child) = main.git_command(jail).args(["log", "--no-walk", "--ignore-missing", "--stdin", "--format=%H %T"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn() else { return BTreeMap::new() };
    if let Some(mut input) = child.stdin.take() {
        let _ = input.write_all(heads.as_bytes());
    }
    let Ok(output) = child.wait_with_output() else { return BTreeMap::new() };
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines().filter_map(|line| line.split_once(' ')).filter_map(|(head, tree)| Some((tree.to_owned(), finished.iter().position(|record| record.head == head)?))).collect()
}

pub fn adopt(inputs: &Inputs, sessions: &impl Sessions, adopter: &impl Adopter, dry_run: bool) -> Vec<String> {
    if dry_run {
        return orphans(inputs, sessions).iter().map(|orphan| format!("north land adopt: would adopt {} at {} (session {} ended)", orphan.lane.root.display(), orphan.head, orphan.session)).collect();
    }
    let Some(_held) = lock(&inputs.state) else {
        return vec!["north land adopt: another adoption is running".to_owned()];
    };
    orphans(inputs, sessions).iter().flat_map(|orphan| take(inputs, adopter, orphan)).collect()
}

fn take(inputs: &Inputs, adopter: &impl Adopter, orphan: &Orphan) -> Vec<String> {
    let lane = orphan.lane.root.display().to_string();
    let (outcome, line, detail) = match rebase(&orphan.lane, &inputs.jail).and_then(|()| adopter.land(&orphan.lane)) {
        Ok(landed) => ("landed", format!("north land adopt: {lane} (session {} ended): {landed}", orphan.session), String::new()),
        Err(refused) => {
            let why = format!("The lander adopted the finished lane {lane} at {} after its session {} ended and could not land it: {} (north#1575). What it printed is in {LEDGER} on {}.", orphan.head, orphan.session, refused.summary, inputs.host);
            match adopter.file(&orphan.lane, &why) {
                Ok(place) => ("filed", format!("north land adopt: {lane} did not land; filed on {place}"), refused.detail),
                Err(error) => ("refused", format!("north land adopt: {lane} did not land and the filing failed ({error}): {}", refused.summary), refused.detail),
            }
        }
    };
    let heads: Vec<String> = [Some(orphan.head.clone()), orphan.lane.git(&inputs.jail, &["rev-parse", "HEAD"])].into_iter().flatten().collect();
    let recorded = record(&inputs.state, &Tried { root: orphan.lane.root.clone(), heads, session: orphan.session.clone(), outcome: outcome.to_owned(), detail });
    let mut lines = vec![line];
    if let Err(error) = recorded {
        lines.push(format!("north land adopt: could not record the adoption of {lane}: {error}"));
    }
    lines
}

fn rebase(lane: &Lane, jail: &[OsString]) -> Result<(), Refused> {
    lane.git(jail, &["fetch", "-q", "origin", "main"]).ok_or_else(|| Refused { summary: "git fetch origin main failed".to_owned(), detail: String::new() })?;
    if lane.git(jail, &["rebase", "-q", "origin/main"]).is_some() {
        return Ok(());
    }
    let conflicts = lane.git(jail, &["diff", "--name-only", "--diff-filter=U"]).unwrap_or_default();
    lane.git(jail, &["rebase", "--abort"]);
    Err(Refused { summary: "the lane no longer rebases onto origin/main".to_owned(), detail: format!("conflicting paths:\n{conflicts}") })
}

fn clean(lane: &Lane, jail: &[OsString]) -> bool {
    lane.git(jail, &["status", "--porcelain", "--untracked-files=no"]).is_some_and(|status| status.is_empty())
}

fn landed(lane: &Lane, jail: &[OsString], head: &str) -> bool {
    lane.git_command(jail).args(["merge-base", "--is-ancestor", head, "origin/main"]).stderr(Stdio::null()).status().is_ok_and(|status| status.success())
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

fn lanes(code: &Path) -> Vec<Lane> {
    let directories = |path: PathBuf| fs::read_dir(path).into_iter().flatten().flatten().filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir())).map(|entry| entry.path());
    let mut roots: Vec<PathBuf> = directories(code.to_path_buf()).flat_map(|container| directories(container.join("worktrees"))).collect();
    roots.sort();
    roots.into_iter().filter_map(Lane::of).collect()
}
