use std::collections::BTreeSet;
use std::fs;
use std::os::unix::fs::symlink;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rustix::fs::{FlockOperation, flock};

use super::client::North;
use super::exec;
use super::notes::{Note, Notes};
use super::processes::{self, Process};
use super::report;
use super::scratch::{self, Scratch};

const CHANGED_FILE: &str = "worker.txt";
const HAND_IN_DEADLINE: Duration = Duration::from_secs(110);
const SERVE_WAIT: Duration = Duration::from_secs(30);
const QUERY_WAIT: Duration = Duration::from_secs(30);
const LAND_WAIT: Duration = Duration::from_mins(2);
const POLL: Duration = Duration::from_secs(1);
const LANDER_LOCK: &str = "lander.lock";
const BROKER_SOCKET_ENV: &str = "NORTH_OUTSIDE_BROKER";
const SHOWN_CHARS: usize = 160;
const WORKER_SCRIPT: &str = "set -eu\nprintf \"the worker's change\\n\" > worker.txt\ngit add worker.txt\ngit commit -qm \"Add the worker's change\"\nsafe-push --check-only\nnorth hand-in --message \"Add the worker's change\" -- \"$PWD/worker.txt\"\n";

type Step = fn(&mut Journey) -> Result<String, String>;

const STEPS: [(&str, &str, Step); 3] = [
    ("1", "`north land serve` serves a scratch repository's queue and the judged north's outside broker answers on its socket", serve_spine),
    ("2", "a confined worker command passes `safe-push --check-only` and `north hand-in` queues the lane without waiting on landing", worker_hand_in),
    ("3", "the lander landed the worker's commit on origin main", land),
];

pub struct Plan {
    pub stamp: u64,
    pub state: PathBuf,
}

struct Journey {
    north: North,
    notes: Notes,
    plan: Plan,
    scratch: Option<Scratch>,
    lander: Option<Process>,
    broker: Option<Process>,
}

pub fn walk(north: North, notes: Notes, plan: Plan) -> Option<&'static str> {
    let mut journey = Journey { north, notes, plan, scratch: None, lander: None, broker: None };
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

fn serve_spine(journey: &mut Journey) -> Result<String, String> {
    let scratch = Scratch::create()?;
    journey.notes.note(&Note::Scratch(scratch.root.clone()));
    let lane = scratch.lane.display().to_string();
    let lander = journey.serve(&scratch)?;
    journey.lander = Some(lander);
    let broker = journey.outside_broker(&scratch)?;
    journey.broker = Some(broker);
    journey.scratch = Some(scratch);
    Ok(lane)
}

fn worker_hand_in(journey: &mut Journey) -> Result<String, String> {
    let scratch = journey.scratch()?;
    let bin = scratch.root.join("bin");
    fs::create_dir_all(&bin).map_err(|error| format!("{}: {error}", bin.display()))?;
    for tool in ["north", "safe-push"] {
        let link = bin.join(tool);
        if !link.exists() {
            symlink(journey.north.program(), &link).map_err(|error| format!("{}: {error}", link.display()))?;
        }
    }
    let output = journey.north.run_in(
        &scratch.lane,
        &["--worker-sandbox", "--worker", WORKER_SCRIPT],
        &[(BROKER_SOCKET_ENV, &scratch.broker), ("NORTH_WORKER_SANDBOX", Path::new("1"))],
        Some(&bin),
        HAND_IN_DEADLINE,
    )?;
    if !output.lines().any(|line| line.starts_with("Done: safe-push --check-only passed")) {
        return Err(format!("missing check-only pass line in {:?}", shorten(&output)));
    }
    if !output.lines().any(|line| line.starts_with("Done: queued ")) {
        return Err(format!("missing `Done: queued` ticket in {:?}", shorten(&output)));
    }
    let head = scratch.lane_head(QUERY_WAIT)?;
    Ok(format!("{} at {}", scratch::BRANCH, short(&head)))
}

fn land(journey: &mut Journey) -> Result<String, String> {
    let waited = journey.scratch()?;
    let log = waited.root.join("lander.log");
    exec::poll_until(LAND_WAIT, POLL, || waited.origin_main(QUERY_WAIT).is_ok_and(|head| head != waited.base && fs::read_to_string(&log).is_ok_and(|said| said.contains(&format!("landed {head}")))));
    let served = journey.stop_lander();
    let scratch = journey.scratch()?;
    let landed = scratch.origin_main(QUERY_WAIT)?;
    let parent = scratch.origin(&["rev-parse", &format!("{landed}^")])?;
    let changed = scratch.origin(&["show", "--format=", "--name-only", &landed])?;
    if parent != scratch.base || changed != CHANGED_FILE || !served.contains(&format!("landed {landed}")) {
        return Err(format!("origin main is {landed} on {parent} changing {changed:?}; expected one commit changing {CHANGED_FILE} on {}; lander said: {}", scratch.base, shorten(&served)));
    }
    Ok(format!("landed {}", short(&landed)))
}

impl Journey {
    fn serve(&self, scratch: &Scratch) -> Result<Process, String> {
        let log = scratch.root.join("lander.log");
        let pushes = scratch.root.join("pushes.tsv");
        let lander = self.north.start_with(&["land", "serve", "--repo", &scratch.main.to_string_lossy()], &[("NORTH_PUSH_LOG", pushes.as_path())], &log)?;
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
        match scratch::keep_evidence(&scratch.root, &landing_queues(&self.plan.state), waiting, self.plan.stamp) {
            Ok(kept) => report::say(&format!("verify-north kept the scratch evidence in {}", kept.display())),
            Err(problem) => report::say(&format!("verify-north could not keep the scratch evidence: {problem}")),
        }
    }

    fn scratch(&self) -> Result<&Scratch, String> {
        self.scratch.as_ref().ok_or_else(|| "no scratch repository".to_owned())
    }
}

pub fn landing_queues(state: &Path) -> PathBuf {
    state.join("north/land-queue")
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(12)]
}

pub fn shorten(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= SHOWN_CHARS {
        return flat;
    }
    format!("{}…", flat.chars().take(SHOWN_CHARS).collect::<String>())
}

fn held_queue(queues: &Path) -> Option<PathBuf> {
    let entries = fs::read_dir(queues).ok()?;
    entries.filter_map(Result::ok).map(|entry| entry.path()).find(|queue| fs::File::open(queue.join(LANDER_LOCK)).is_ok_and(|file| flock(&file, FlockOperation::NonBlockingLockShared).is_err()))
}

#[cfg(test)]
mod tests {
    use super::{shorten, short};

    #[test]
    fn shorten_clips_long_output_and_short_prefixes_commit_ids() {
        assert_eq!(short("0123456789abcdef"), "0123456789ab");
        assert_eq!(shorten("  a\n  b "), "a b");
        assert!(shorten(&"x".repeat(200)).ends_with('…'));
    }
}
