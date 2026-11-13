use std::collections::BTreeSet;
use std::fs;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use rustix::fs::{FlockOperation, flock};

use super::client::North;
use super::exec::{self, Clock};
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
Done when: worker.txt holds the line `the worker's change`, and the `hand-in` tool returns `Done: landed` for it, landing it on main.
In the current directory write the single line `the worker's change` to worker.txt. Then call the `mcp__north__hand-in` tool once with the message `Add the worker's change` and the paths set to the absolute path of worker.txt (the current directory followed by /worker.txt). Do not run git add or git commit first. The tool blocks until the lane lands and ends with the line `Done: landed SHA on main`; reply with that `Done: landed` line, copied exactly as the tool returned it, as your only reply.";
const CHANGED_FILE: &str = "worker.txt";
const HAND_IN_TOOL: &str = "mcp__north__hand-in";
const CLAUDE_MODEL: &str = "haiku";
const HAND_IN_DEADLINE: Duration = Duration::from_secs(110);
const SERVE_WAIT: Duration = Duration::from_secs(30);
const SETTLE_WAIT: Duration = Duration::from_mins(2);
const LEDGER_WAIT: Duration = Duration::from_mins(3);
const QUERY_WAIT: Duration = Duration::from_secs(30);
const POLL: Duration = Duration::from_secs(1);
const LANDER_LOCK: &str = "lander.lock";
const BROKER_SOCKET_ENV: &str = "NORTH_OUTSIDE_BROKER";

type Step = fn(&mut Journey) -> Result<String, String>;

mod hand_in;

const STEPS: [(&str, &str, Step); 4] = [
    ("1", "`north land serve` serves a scratch repository's queue, the judged north's outside broker serves the worker's sandbox, and a claude haiku worker takes the brief in its lane", take_brief),
    ("2", "the worker hands in with one `hand-in` tool call, which returns `Done: landed`", hand_in::hand_in),
    ("3", "the lander landed the worker's commit on origin main", land),
    ("4", "`north ledger worker` records the run done and landed at that commit", ledger_row),
];

pub struct Plan {
    pub stamp: u64,
    pub claude_account: Option<String>,
    pub state: PathBuf,
}

struct Journey {
    north: North,
    notes: Notes,
    plan: Plan,
    session: String,
    scratch: Option<Scratch>,
    lander: Option<Process>,
    broker: Option<Process>,
    landed: Option<String>,
    clock: Rc<dyn Clock>,
}

pub fn walk(north: North, notes: Notes, plan: Plan) -> Option<&'static str> {
    let session = format!("vn-{}", plan.stamp);
    let mut journey = Journey { north, notes, plan, session, scratch: None, lander: None, broker: None, landed: None, clock: Rc::new(exec::Wall) };
    for (index, (number, title, step)) in STEPS.iter().enumerate() {
        let started = Instant::now();
        let outcome = step(&mut journey);
        report::step(number, title, &outcome, started.elapsed());
        if outcome.is_err() {
            let waiting = journey.waiting();
            journey.stop_lander();
            journey.stop_broker();
            journey.keep_evidence(&waiting);
            STEPS[index + 1..].iter().for_each(|(number, title, _)| report::skipped(number, title));
            return Some(number);
        }
    }
    journey.stop_broker();
    None
}

fn take_brief(journey: &mut Journey) -> Result<String, String> {
    let scratch = Scratch::create()?;
    journey.notes.note(&Note::Scratch(scratch.root.clone()));
    let lane = scratch.lane.to_string_lossy().into_owned();
    let lander = journey.serve(&scratch)?;
    journey.lander = Some(lander);
    let broker = journey.outside_broker(&scratch)?;
    journey.broker = Some(broker);
    let socket = scratch.broker.clone();
    journey.scratch = Some(scratch);
    let account = match journey.plan.claude_account.clone() {
        Some(account) => account,
        None => journey.north.placed_claude_account()?,
    };
    let name = journey.session.clone();
    journey.notes.note(&Note::Session(name.clone()));
    let before = processes::session_hosts();
    let launch = ["fleet", "launch", "--provider", "claude", "--account", &account, "--model", CLAUDE_MODEL, "--name", &name, "--parent", "none", "--role", "worker", "--test", "--cwd", &lane, "--no-wait", BRIEF];
    journey.north.run_with(&launch, &[(BROKER_SOCKET_ENV, &socket)], QUERY_WAIT)?;
    journey.watch_host(&before)?;
    Ok(format!("{name} on claude/{account} in {lane}"))
}

fn land(journey: &mut Journey) -> Result<String, String> {
    let served = journey.stop_lander();
    let scratch = journey.scratch()?;
    let landed = scratch.origin_main(QUERY_WAIT)?;
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
    let recorded = journey.north.output_with(&["ledger", "worker", "--session", &conversation], &[("THREADS_DB", &ledger), ("LEDGER_REPOS", &scratch.main), ("NORTH_PUSH_LOG", &scratch.root.join("pushes.tsv"))], LEDGER_WAIT)?;
    let row = run_row(&ledger, &conversation)?;
    let settled = row.as_ref().is_some_and(|(_, outcome, landed_count, _, sha)| outcome == "done" && *landed_count == 1 && sha.as_deref() == Some(landed.as_str()));
    if !settled {
        let found = row.map_or_else(|| "no row".to_owned(), |(role, outcome, count, ended, sha)| format!("role {role}, outcome {outcome}, landed {count}, ended {ended:?}, landed_sha {sha:?}"));
        return Err(format!("runs holds {found} for {conversation}; expected outcome done, landed 1, landed_sha {landed}; `north ledger worker` said: {}", shorten(&recorded)));
    }
    Ok(format!("run {conversation} done, landed {}", short(&landed)))
}

impl Journey {
    fn serve(&self, scratch: &Scratch) -> Result<Process, String> {
        let log = scratch.root.join("lander.log");
        let variables = [("THREADS_DB", scratch.root.join("threads.db")), ("NORTH_PUSH_LOG", scratch.root.join("pushes.tsv"))];
        let variables: Vec<(&str, &Path)> = variables.iter().map(|(key, path)| (*key, path.as_path())).collect();
        let lander = self.north.start_with(&["land", "serve", "--repo", &scratch.main.to_string_lossy()], &variables, &log)?;
        let queues = landing_queues(&self.plan.state);
        let mut served = None;
        exec::poll_until(SERVE_WAIT, POLL, || {
            served = held_queue(&queues);
            served.is_some() || !lander.alive()
        });
        if let (Some(queue), true) = (served, lander.alive()) {
            self.notes.note(&Note::Queue(queue));
            return Ok(lander);
        }
        lander.terminate();
        Err(format!("`north land serve` held no queue under {} within {} s; it said: {}", queues.display(), SERVE_WAIT.as_secs(), shorten(&fs::read_to_string(&log).unwrap_or_default())))
    }

    fn outside_broker(&self, scratch: &Scratch) -> Result<Process, String> {
        let log = scratch.root.join("broker.log");
        let broker = self.north.start_with(&["--worker-sandbox", "--broker", "--code", &scratch.root.to_string_lossy()], &[(BROKER_SOCKET_ENV, &scratch.broker)], &log)?;
        let answers = || UnixStream::connect(&scratch.broker).is_ok();
        exec::poll_until(SERVE_WAIT, POLL, || answers() || !broker.alive());
        if answers() {
            return Ok(broker);
        }
        broker.terminate();
        Err(format!("the judged north's outside broker did not answer on {} within {} s; it said: {}", scratch.broker.display(), SERVE_WAIT.as_secs(), shorten(&fs::read_to_string(&log).unwrap_or_default())))
    }

    fn stop_broker(&mut self) {
        if let Some(broker) = self.broker.take() {
            broker.terminate();
        }
    }

    fn stop_lander(&mut self) -> String {
        let Some(lander) = self.lander.take() else { return String::new() };
        lander.terminate();
        exec::poll_until(SERVE_WAIT, POLL, || !lander.alive());
        self.scratch.as_ref().and_then(|scratch| fs::read_to_string(scratch.root.join("lander.log")).ok()).unwrap_or_default()
    }

    fn waiting(&self) -> String {
        let roots: Vec<Process> = self.scratch.iter().map(|scratch| scratch.root.as_path()).chain([self.plan.state.as_path()]).flat_map(processes::within).collect::<BTreeSet<_>>().into_iter().collect();
        let every: BTreeSet<Process> = roots.iter().copied().chain(processes::descendants(&roots)).collect();
        every.into_iter().map(|process| format!("{}\n", process.waiting())).collect()
    }

    fn keep_evidence(&self, waiting: &str) {
        let Some(scratch) = &self.scratch else { return };
        match scratch::keep_ledger(&scratch.root, &landing_queues(&self.plan.state), waiting, self.plan.stamp) {
            Ok(kept) => report::say(&format!("verify-north kept the scratch ledger in {}", kept.display())),
            Err(problem) => report::say(&format!("verify-north could not keep the scratch ledger: {problem}")),
        }
    }

    fn scratch(&self) -> Result<&Scratch, String> {
        self.scratch.as_ref().ok_or_else(|| "no scratch repository".to_owned())
    }

    fn transcript(&self) -> Result<Transcript, String> {
        self.north.transcript(&self.session)
    }

    fn settled_report(&self, limit: Duration, ready: impl Fn(&Transcript) -> bool) -> Result<String, String> {
        let mut last = format!("{} has no transcript yet", self.session);
        let deadline = self.clock.now() + limit;
        let report = exec::wait_for(self.clock.as_ref(), limit, POLL, || {
            let left = deadline.saturating_duration_since(self.clock.now()).min(QUERY_WAIT);
            if left.is_zero() {
                return Ok(None);
            }
            let transcript = match self.north.transcript_within(&self.session, left) {
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

type RunRow = (String, String, i64, Option<String>, Option<String>);

fn run_row(ledger: &Path, conversation: &str) -> Result<Option<RunRow>, String> {
    let connection = rusqlite::Connection::open_with_flags(ledger, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|error| format!("{}: {error}", ledger.display()))?;
    let mut statement = connection.prepare("SELECT role, outcome, landed, ended, landed_sha FROM runs WHERE agent = ?1").map_err(|error| format!("{}: {error}", ledger.display()))?;
    let mut rows = statement.query_map([conversation], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?))).map_err(|error| format!("{}: {error}", ledger.display()))?;
    rows.next().transpose().map_err(|error| format!("{}: {error}", ledger.display()))
}

pub fn landing_queues(state: &Path) -> PathBuf {
    state.join("north/land-queue")
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(12)]
}

fn held_queue(queues: &Path) -> Option<PathBuf> {
    let entries = fs::read_dir(queues).ok()?;
    entries.filter_map(Result::ok).map(|entry| entry.path()).find(|queue| fs::File::open(queue.join(LANDER_LOCK)).is_ok_and(|file| flock(&file, FlockOperation::NonBlockingLockShared).is_err()))
}
