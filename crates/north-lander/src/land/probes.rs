use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{OnceLock, mpsc};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::candidate::{self, Pick};
use super::checks::{self, Verdict};
use super::lander::Lander;
use super::lane_gates;
use super::queue::Ticket;
use super::watch::Watch;

const POLL: Duration = Duration::from_millis(100);
const JOIN_WINDOW: Duration = Duration::from_secs(60);
const JOIN_LOOK: Duration = Duration::from_secs(1);

type Entry<'a> = (&'a PathBuf, &'a Ticket);

pub type Arrivals<'a> = &'a dyn Fn() -> Vec<(PathBuf, Ticket)>;

pub struct Placement<'a> {
    pub local: bool,
    pub arrivals: Option<Arrivals<'a>>,
}

pub enum End {
    Checked(Verdict),
    Unbuilt(String),
    Cancelled,
}

pub struct Probe {
    pub lanes: Vec<usize>,
    pub head: String,
    pub tips: Vec<String>,
    pub worktree: PathBuf,
    pub log: PathBuf,
    pub end: End,
}

impl Probe {
    pub fn verdict(&self) -> Option<&Verdict> {
        match &self.end {
            End::Checked(verdict) => Some(verdict),
            End::Unbuilt(_) | End::Cancelled => None,
        }
    }

    pub fn green(&self) -> bool {
        self.verdict() == Some(&Verdict::Green)
    }

    pub fn refused(&self) -> bool {
        matches!(self.verdict(), Some(Verdict::Refused(_)))
    }
}

fn micros() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_micros())
}

struct Round<'a> {
    lander: &'a Lander,
    base: &'a str,
    logs: &'a Path,
    entries: &'a [Entry<'a>],
    local: bool,
}

fn unbuilt(worktree: PathBuf, log: PathBuf, lanes: Vec<usize>, head: String, reason: String) -> Probe {
    Probe { lanes, head, tips: Vec::new(), worktree, log, end: End::Unbuilt(reason) }
}

fn refusal_of_build(picks: &[Pick], set: &[Entry]) -> Option<String> {
    picks
        .iter()
        .zip(set)
        .find_map(|(pick, (_, ticket))| match pick {
            Pick::Included => None,
            Pick::Decided(verdict) | Pick::Undecided(verdict) | Pick::Deferred(verdict) | Pick::Refused(lane_gates::Refused { verdict, .. }) => Some(format!("{}: {verdict}", ticket.branch)),
        })
        .or_else(|| (picks.len() < set.len()).then(|| "no pick".to_owned()))
}

impl Round<'_> {
    fn probe(&self, entries: &[Entry], offset: usize, lanes: Vec<usize>, cancel: &AtomicBool) -> Probe {
        let worktree = match candidate::lander_worktree(&self.lander.repo, &format!("lander-probe-{offset}")) {
            Ok(worktree) => worktree,
            Err(error) => return unbuilt(PathBuf::new(), PathBuf::new(), lanes, String::new(), error),
        };
        let set: Vec<Entry> = lanes.iter().map(|lane| entries[*lane]).collect();
        let name = set.last().and_then(|(path, _)| path.file_stem()).and_then(|stem| stem.to_str()).unwrap_or("lane");
        let log = self.logs.join(format!("{}-{name}-{}-probe{offset}-candidate.log", micros(), set.len()));
        let tickets: Vec<(PathBuf, Ticket)> = set.iter().map(|(path, ticket)| ((*path).clone(), (*ticket).clone())).collect();
        let built = candidate::ensure(&self.lander.repo, &worktree).and_then(|()| candidate::build(&worktree, self.base, &tickets, &[], set.len(), self.logs));
        let built = match built {
            Ok(built) => built,
            Err(error) => return unbuilt(worktree, log, lanes, String::new(), error),
        };
        if let Some(reason) = refusal_of_build(&built.picks, &set) {
            return unbuilt(worktree, log, lanes, built.head, format!("{} lanes cannot be built together: {reason}", set.len()));
        }
        let red_fix = set.iter().any(|(_, ticket)| ticket.red_fix.is_some());
        let launch = checks::Launch { red_fix, local: self.local, lanes: checks::lane_names(set.iter().map(|(_, ticket)| ticket.branch.as_str())), tips: built.tips.join(" "), ..checks::Launch::of(self.lander) };
        let end = match checks::start(&launch, &worktree, self.base, &log, true) {
            Ok(mut started) => loop {
                if cancel.load(Ordering::Relaxed) {
                    started.kill();
                    break End::Cancelled;
                }
                if let Some(verdict) = started.poll() {
                    break End::Checked(verdict);
                }
                std::thread::sleep(POLL);
            },
            Err(verdict) => End::Checked(verdict),
        };
        Probe { lanes, head: built.head, tips: built.tips, worktree, log, end }
    }
}

/// Runs every probe at once on its own worktree and returns them in plan order. When a probe comes back refused, the
/// probes still running that contain its red stride are cancelled, since they would be refused for the same lanes.
pub fn run(lander: &Lander, watch: &mut Watch, base: &str, logs: &Path, entries: &[Entry], plan: &[Vec<usize>], placement: Placement) -> (Vec<Probe>, Vec<(PathBuf, Ticket)>) {
    let Placement { local, arrivals } = placement;
    let round = Round { lander, base, logs, entries, local };
    let mut plan = plan.to_vec();
    let cancels: Vec<AtomicBool> = (0..=plan.len()).map(|_| AtomicBool::new(false)).collect();
    let (sender, receiver) = mpsc::channel();
    let mut finished: Vec<Option<Probe>> = (0..=plan.len()).map(|_| None).collect();
    let joined: OnceLock<Vec<(PathBuf, Ticket)>> = OnceLock::new();
    let extended: OnceLock<Vec<Entry>> = OnceLock::new();
    let (started, mut looked) = (Instant::now(), Instant::now());
    std::thread::scope(|scope| {
        for (offset, lanes) in plan.clone().into_iter().enumerate() {
            let (round, cancel, sender) = (&round, &cancels[offset], sender.clone());
            scope.spawn(move || {
                let _ = sender.send((offset, round.probe(round.entries, offset, lanes, cancel)));
            });
        }
        let mut waiting = plan.len();
        while waiting > 0 {
            match receiver.recv_timeout(POLL) {
                Ok((offset, probe)) => {
                    waiting -= 1;
                    let running: Vec<bool> = finished.iter().zip(&cancels).map(|(done, cancel)| done.is_none() && !cancel.load(Ordering::Relaxed)).collect();
                    let stopping = if probe.refused() {
                        north_land_probes::cancelled(&plan, offset, &running)
                    } else if probe.green() {
                        north_land_probes::superseded(&plan, offset, &running)
                    } else {
                        Vec::new()
                    };
                    for stopped in stopping {
                        println!("north land: the probe of {} lanes is {}; cancelling the probe of {} lanes", plan[offset].len(), if probe.green() { "green and carries it" } else { "red and holds its stride" }, plan[stopped].len());
                        cancels[stopped].store(true, Ordering::Relaxed);
                    }
                    finished[offset] = Some(probe);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    watch.settle(lander);
                    let Some(arrivals) = arrivals.filter(|_| joined.get().is_none() && started.elapsed() < JOIN_WINDOW && looked.elapsed() >= JOIN_LOOK) else { continue };
                    looked = Instant::now();
                    if finished.iter().flatten().any(Probe::refused) {
                        continue;
                    }
                    let arrived = arrivals();
                    if arrived.is_empty() {
                        continue;
                    }
                    let tickets = joined.get_or_init(|| arrived);
                    let set = extended.get_or_init(|| entries.iter().copied().chain(tickets.iter().map(|(path, ticket)| (path, ticket))).collect());
                    let (offset, lanes) = (plan.len(), (0..set.len()).collect::<Vec<usize>>());
                    println!("north land: {} ticket(s) queued while this round runs join it: a probe of {} lanes checks them with the round's {}", tickets.len(), set.len(), entries.len());
                    plan.push(lanes.clone());
                    waiting += 1;
                    let (round, cancel, sender) = (&round, &cancels[offset], sender.clone());
                    scope.spawn(move || {
                        let _ = sender.send((offset, round.probe(set, offset, lanes, cancel)));
                    });
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    });
    drop(extended);
    (finished.into_iter().flatten().collect(), joined.into_inner().unwrap_or_default())
}
