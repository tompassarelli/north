use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::Value;

use super::client::North;
use super::exec;
use super::notes::{Note, Notes};
use super::processes::{self, Process};
use super::report;
use super::scratch::{self, Scratch};
use super::transcript::{Transcript, shorten};

const BRIEF: &str = "Category: tooling
Spec: exact
Scope: one-file
Surface: shell
Verify: local-test
Done when: worker.txt holds the line `the worker's change`, is committed on this branch and is queued with `safe-push --to main`.
In the current directory write the single line `the worker's change` to worker.txt, run `git add worker.txt && git commit -m \"Add the worker's change\"`, then run `safe-push --to main`. Once safe-push prints `Done: queued`, reply with one line that starts `Done:`.";
const CHANGED_FILE: &str = "worker.txt";
const CLAUDE_MODEL: &str = "haiku";
const WORKER_WAIT: Duration = Duration::from_mins(4);
const LAND_WAIT: Duration = Duration::from_mins(5);
const SETTLE_WAIT: Duration = Duration::from_mins(2);
const LEDGER_WAIT: Duration = Duration::from_mins(3);
const QUERY_WAIT: Duration = Duration::from_secs(30);
const POLL: Duration = Duration::from_secs(1);

type Step = fn(&mut Journey) -> Result<String, String>;

const STEPS: [(&str, &str, Step); 4] = [
    ("1", "a claude haiku worker takes the brief in a scratch repository's lane", take_brief),
    ("2", "the worker commits its change and queues it with `safe-push --to main`", queue_change),
    ("3", "`north land serve --once` lands the worker's commit on origin main", land),
    ("4", "`north ledger worker` records the run done and landed at that commit", ledger_row),
];

pub struct Plan {
    pub stamp: u64,
    pub claude_account: Option<String>,
}

struct Journey {
    north: North,
    notes: Notes,
    plan: Plan,
    session: String,
    scratch: Option<Scratch>,
    landed: Option<String>,
}

pub fn walk(north: North, notes: Notes, plan: Plan) -> Option<&'static str> {
    let session = format!("vn-{}", plan.stamp);
    let mut journey = Journey { north, notes, plan, session, scratch: None, landed: None };
    for (index, (number, title, step)) in STEPS.iter().enumerate() {
        let started = Instant::now();
        let outcome = step(&mut journey);
        report::step(number, title, &outcome, started.elapsed());
        if outcome.is_err() {
            STEPS[index + 1..].iter().for_each(|(number, title, _)| report::skipped(number, title));
            return Some(number);
        }
    }
    None
}

fn take_brief(journey: &mut Journey) -> Result<String, String> {
    let scratch = Scratch::create()?;
    journey.notes.note(&Note::Scratch(scratch.root.clone()));
    let lane = scratch.lane.to_string_lossy().into_owned();
    journey.scratch = Some(scratch);
    let account = match journey.plan.claude_account.clone() {
        Some(account) => account,
        None => journey.north.placed_claude_account()?,
    };
    let name = journey.session.clone();
    journey.notes.note(&Note::Session(name.clone()));
    let before = processes::session_hosts();
    let launch = ["fleet", "launch", "--provider", "claude", "--account", &account, "--model", CLAUDE_MODEL, "--name", &name, "--parent", "none", "--test", "--cwd", &lane, "--no-wait", BRIEF];
    journey.north.run(&launch, QUERY_WAIT)?;
    journey.watch_host(&before)?;
    Ok(format!("{name} on claude/{account} in {lane}"))
}

fn queue_change(journey: &mut Journey) -> Result<String, String> {
    let report = journey.settled_report(WORKER_WAIT, |_| true)?;
    if !report.starts_with("Done:") {
        return Err(format!("the worker reported {:?}", shorten(&report)));
    }
    let scratch = journey.scratch()?;
    let head = scratch.lane_head()?;
    let (queue, ticket) = queued_ticket(&scratch.origin).ok_or_else(|| format!("the worker reported {:?} but queued no ticket for {}", shorten(&report), scratch.origin.display()))?;
    journey.notes.note(&Note::Queue(queue));
    let expected = (scratch::BRANCH, head.as_str(), journey.session.as_str());
    let found = (ticket["branch"].as_str().unwrap_or_default(), ticket["head"].as_str().unwrap_or_default(), ticket["session"].as_str().unwrap_or_default());
    if found != expected {
        return Err(format!("the ticket holds branch, head and session {found:?}; expected {expected:?}"));
    }
    Ok(format!("{} queued {} at {}", journey.session, scratch::BRANCH, short(&head)))
}

fn land(journey: &mut Journey) -> Result<String, String> {
    let scratch = journey.scratch()?;
    let ledger = scratch.root.join("threads.db");
    let served = journey.north.run_with(&["land", "serve", "--once", "--depth", "1", "--repo", &scratch.main.to_string_lossy()], &[("THREADS_DB", &ledger)], LAND_WAIT)?;
    let landed = scratch.origin_main()?;
    let parent = scratch.origin(&["rev-parse", &format!("{landed}^")])?;
    let changed = scratch.origin(&["show", "--format=", "--name-only", &landed])?;
    if parent != scratch.base || changed != CHANGED_FILE || !served.contains(&format!("landed {landed}")) {
        return Err(format!("origin main is {landed} on {parent} changing {changed:?}; expected one commit changing {CHANGED_FILE} on {}; lander said: {}", scratch.base, shorten(&served)));
    }
    journey.landed = Some(landed.clone());
    Ok(format!("landed {}", short(&landed)))
}

fn ledger_row(journey: &mut Journey) -> Result<String, String> {
    let landed = journey.landed.clone().ok_or("no landed commit")?;
    let notice = format!("landed {} as {landed}", scratch::BRANCH);
    journey.settled_report(SETTLE_WAIT, |transcript| transcript.entries.iter().any(|entry| entry.kind == "user" && entry.text.contains(&notice)))?;
    let conversation = journey.transcript()?.conversation;
    let scratch = journey.scratch()?;
    let ledger = scratch.root.join("threads.db");
    let recorded = journey.north.output_with(&["ledger", "worker", "--session", &conversation], &[("THREADS_DB", &ledger), ("LEDGER_REPOS", &scratch.main)], LEDGER_WAIT)?;
    let row = run_row(&ledger, &conversation)?;
    let expected = ("done".to_owned(), 1, landed.clone());
    if row.as_ref() != Some(&expected) {
        return Err(format!("runs holds {row:?} for {conversation}; expected {expected:?}; `north ledger worker` said: {}", shorten(&recorded)));
    }
    Ok(format!("run {conversation} done, landed {}", short(&landed)))
}

impl Journey {
    fn scratch(&self) -> Result<&Scratch, String> {
        self.scratch.as_ref().ok_or_else(|| "no scratch repository".to_owned())
    }

    fn transcript(&self) -> Result<Transcript, String> {
        self.north.transcript(&self.session)
    }

    fn settled_report(&self, limit: Duration, ready: impl Fn(&Transcript) -> bool) -> Result<String, String> {
        let mut last = format!("{} has no transcript yet", self.session);
        let report = exec::wait_for(limit, POLL, || {
            let transcript = match self.north.transcript(&self.session) {
                Ok(transcript) => transcript,
                Err(error) if error.contains("no session named") => return Ok(None),
                Err(error) => return Err(error),
            };
            last = transcript.state();
            Ok(if transcript.settled() && ready(&transcript) { transcript.report() } else { None })
        })?;
        report.ok_or_else(|| format!("{} did not settle within {} s; {last}", self.session, limit.as_secs()))
    }

    fn watch_host(&self, before: &BTreeSet<Process>) -> Result<(), String> {
        let pid = self.north.host_pid(&self.session)?.ok_or_else(|| format!("{} reports no session host pid", self.session))?;
        let host = Process::of(pid).ok_or_else(|| format!("{}'s session host {pid} is not running", self.session))?;
        if !before.contains(&host) {
            self.notes.watch_all(std::iter::once(host).chain(processes::descendants(&[host])));
        }
        Ok(())
    }
}

fn run_row(ledger: &Path, conversation: &str) -> Result<Option<(String, i64, String)>, String> {
    let connection = rusqlite::Connection::open_with_flags(ledger, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|error| format!("{}: {error}", ledger.display()))?;
    let mut statement = connection.prepare("SELECT outcome, landed, landed_sha FROM runs WHERE agent = ?1").map_err(|error| format!("{}: {error}", ledger.display()))?;
    let mut rows = statement.query_map([conversation], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).map_err(|error| format!("{}: {error}", ledger.display()))?;
    rows.next().transpose().map_err(|error| format!("{}: {error}", ledger.display()))
}

fn queued_ticket(origin: &Path) -> Option<(PathBuf, Value)> {
    let origin = origin.to_string_lossy();
    let queues = fs::read_dir(landing_queues()).ok()?;
    queues.filter_map(Result::ok).map(|queue| queue.path()).find_map(|queue| {
        let tickets = fs::read_dir(&queue).ok()?;
        let ticket = tickets
            .filter_map(Result::ok)
            .map(|ticket| ticket.path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "json"))
            .filter_map(|path| serde_json::from_str::<Value>(&fs::read_to_string(path).ok()?).ok())
            .find(|ticket| ticket["origin"] == *origin)?;
        Some((queue, ticket))
    })
}

pub fn landing_queues() -> PathBuf {
    let state = std::env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()).map_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state"), PathBuf::from);
    state.join("north/land-queue")
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(12)]
}
