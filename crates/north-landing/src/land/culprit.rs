use std::collections::BTreeMap;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rustix::fs::{FlockOperation, flock};

use super::bisect::{self, Strikes};
use super::candidate::{self, Pick};
use super::checks::{self, Started, Verdict};
use super::lander::Lander;
use super::queue::Ticket;
use super::records::{self, Candidate, Decision};
use super::telemetry::Stages;

const POLL: Duration = Duration::from_millis(100);
pub const MOST_AT_ONCE: usize = 6;

type Entry<'a> = (&'a PathBuf, &'a Ticket);

#[derive(Debug, PartialEq, Eq)]
pub enum Found {
    Decided,
    Interaction { lanes: usize, head: String },
    Moved,
    Held(String),
    Paused(String),
}

#[derive(Clone, Debug)]
pub struct Checked {
    pub verdict: Verdict,
    pub head: String,
    pub log: PathBuf,
}

type Refused<'a> = (Entry<'a>, Checked, String);

enum Outcome<'a> {
    Culprits(Vec<Refused<'a>>),
    Interaction { lanes: usize, head: String, partner: Refused<'a> },
    Moved,
    Held(String),
    Paused(String),
}

trait Bench {
    fn room(&self, wanted: usize) -> usize;
    fn check(&self, sets: &[&[Entry]]) -> Result<Vec<Checked>, String>;
    fn moved(&self) -> Result<bool, String>;
}

struct Offload<'a> {
    lander: &'a Lander,
    base: &'a str,
    logs: &'a Path,
}

fn micros() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_micros())
}

fn offload_root() -> Option<PathBuf> {
    std::env::var_os("NORTH_OFFLOAD_ROOT").filter(|value| !value.is_empty()).map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache/land-offload")))
}

fn slot_name(repo: &Path) -> Option<String> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    Some(repo.strip_prefix(home).ok()?.to_string_lossy().replace('/', "-"))
}

fn belongs(file: &str, name: &str) -> bool {
    let Some(stem) = file.strip_suffix(".lock") else { return false };
    stem == name || stem.strip_prefix(name).and_then(|rest| rest.strip_prefix('-')).is_some_and(|index| !index.is_empty() && index.bytes().all(|byte| byte.is_ascii_digit()))
}

fn free_slots(repo: &Path) -> Option<(usize, usize)> {
    let (root, name) = (offload_root()?, slot_name(repo)?);
    let locks: Vec<PathBuf> = fs::read_dir(root).ok()?.flatten().filter(|entry| belongs(&entry.file_name().to_string_lossy(), &name)).map(|entry| entry.path()).collect();
    if locks.is_empty() {
        return None;
    }
    let free = locks.iter().filter(|lock| File::open(lock).is_ok_and(|file| flock(&file, FlockOperation::NonBlockingLockExclusive).is_ok())).count();
    Some((free, locks.len()))
}

fn wave_size(repo: &Path, wanted: usize) -> (usize, Option<(usize, usize)>) {
    let slots = free_slots(repo);
    let room = slots.map_or(MOST_AT_ONCE, |(free, _)| free.max(1));
    (room.min(wanted), slots)
}

enum Start {
    Running(Started, Checked),
    Ended(Checked),
}

fn start_set(lander: &Lander, base: &str, offset: usize, set: &[Entry], logs: &Path) -> Result<Start, String> {
    let worktree = candidate::lander_worktree(&lander.repo, &format!("lander-culprit-{offset}"))?;
    candidate::ensure(&lander.repo, &worktree)?;
    let name = set.last().and_then(|(path, _)| path.file_stem()).and_then(|stem| stem.to_str()).unwrap_or("lane");
    let log = logs.join(format!("{}-{name}-{}-candidate.log", micros(), set.len()));
    let tickets: Vec<(PathBuf, Ticket)> = set.iter().map(|(path, ticket)| ((*path).clone(), (*ticket).clone())).collect();
    let built = candidate::build(&worktree, base, &tickets, &[], set.len(), logs)?;
    let head = built.head;
    let reason = built
        .picks
        .iter()
        .zip(set)
        .find_map(|(pick, (_, ticket))| match pick {
            Pick::Included => None,
            Pick::Decided(verdict) | Pick::Undecided(verdict) | Pick::Deferred(verdict) => Some(format!("{}: {verdict}", ticket.branch)),
        })
        .or_else(|| (built.picks.len() < set.len()).then(|| "no pick".to_owned()));
    if let Some(reason) = reason {
        return Ok(Start::Ended(Checked { verdict: Verdict::Transient(format!("{} lanes cannot be built together: {reason}", set.len())), head, log }));
    }
    Ok(match checks::start(&checks::Launch::of(lander), &worktree, base, &log, true) {
        Ok(started) => Start::Running(started, Checked { verdict: Verdict::Green, head, log }),
        Err(verdict) => Start::Ended(Checked { verdict, head, log }),
    })
}

fn check_wave(lander: &Lander, base: &str, wave: &[&[Entry]], logs: &Path) -> Result<Vec<Checked>, String> {
    let mut finished: Vec<Option<Checked>> = wave.iter().map(|_| None).collect();
    let mut running: Vec<Option<(Started, Checked)>> = Vec::new();
    for (offset, set) in wave.iter().enumerate() {
        match start_set(lander, base, offset, set, logs) {
            Ok(Start::Running(started, checked)) => running.push(Some((started, checked))),
            Ok(Start::Ended(checked)) => {
                finished[offset] = Some(checked);
                running.push(None);
            }
            Err(error) => {
                for (started, _) in running.into_iter().flatten() {
                    started.kill();
                }
                return Err(error);
            }
        }
    }
    while running.iter().any(Option::is_some) {
        for (offset, slot) in running.iter_mut().enumerate() {
            if let Some((started, _)) = slot.as_mut()
                && let Some(verdict) = started.poll()
            {
                finished[offset] = slot.take().map(|(_, checked)| Checked { verdict, ..checked });
            }
        }
        std::thread::sleep(POLL);
    }
    Ok(finished.into_iter().flatten().collect())
}

impl Bench for Offload<'_> {
    fn room(&self, wanted: usize) -> usize {
        let (width, slots) = wave_size(&self.lander.repo, wanted);
        if let Some((free, total)) = slots.filter(|_| width < wanted) {
            println!("north land: {free} of {total} offload slots are free, so {wanted} checks run in waves of {width}");
        }
        width
    }

    fn check(&self, sets: &[&[Entry]]) -> Result<Vec<Checked>, String> {
        check_wave(self.lander, self.base, sets, self.logs)
    }

    fn moved(&self) -> Result<bool, String> {
        self.lander.base_moved(self.base)
    }
}

fn held(subject: &str, verdict: &Verdict) -> Option<String> {
    match verdict {
        Verdict::Green | Verdict::Refused(_) | Verdict::BrokenBase(_) => None,
        Verdict::RedMain(detail) => Some(format!("{subject}: main is red ({detail})")),
        Verdict::Transient(detail) => Some(format!("{subject}: transient {detail}")),
        Verdict::Deferred(detail) => Some(format!("{subject}: deferred {detail}")),
    }
}

fn broken(checked: &[Checked]) -> Option<String> {
    checked.iter().find_map(|checked| match &checked.verdict {
        Verdict::BrokenBase(detail) => Some(detail.clone()),
        _ => None,
    })
}

fn judged(set: &[Entry], mut checked: Checked, strikes: &mut Strikes) -> Checked {
    let streak = strikes.record(&Strikes::key(set), &checked.verdict);
    checked.verdict = bisect::escalate(checked.verdict, streak, &checked.log);
    checked
}

fn sort<'a>(included: &[Entry<'a>], alone: Vec<Checked>, strikes: &mut Strikes) -> (Vec<Refused<'a>>, Vec<String>) {
    let mut reasons = Vec::new();
    let mut refused = Vec::new();
    for (entry, checked) in included.iter().zip(alone) {
        let checked = judged(std::slice::from_ref(entry), checked, strikes);
        match checked.verdict.clone() {
            Verdict::Refused(detail) => refused.push((*entry, checked, detail)),
            verdict => reasons.extend(held(&format!("{} alone", entry.1.branch), &verdict)),
        }
    }
    (refused, reasons)
}

fn boundary<'a>(included: &[Entry<'a>], lanes: usize, known: &mut BTreeMap<usize, Checked>) -> Outcome<'a> {
    let partner = included[lanes];
    let subject = format!("{} after the {lanes} lanes before it", partner.1.branch);
    match (known.remove(&lanes), known.remove(&(lanes + 1))) {
        (Some(before), Some(after)) => match after.verdict.clone() {
            Verdict::Refused(detail) => Outcome::Interaction { lanes, head: before.head, partner: (partner, after, detail) },
            verdict => Outcome::Held(held(&subject, &verdict).unwrap_or(subject)),
        },
        _ => Outcome::Held(format!("{subject}: no verdict")),
    }
}

fn interaction<'a>(bench: &impl Bench, included: &[Entry<'a>], first: Checked, together: Checked, strikes: &mut Strikes) -> Result<Outcome<'a>, String> {
    let lanes = included.len();
    let mut known = BTreeMap::from([(1, first), (lanes, together)]);
    let mut widest = lanes - 1;
    loop {
        let green = (1..lanes).rev().take_while(|width| known.contains_key(width)).find(|width| known[width].verdict == Verdict::Green);
        if let Some(green) = green {
            return Ok(if bench.moved()? { Outcome::Moved } else { boundary(included, green, &mut known) });
        }
        let room = bench.room(widest - 1);
        let widths: Vec<usize> = (widest + 1 - room..=widest).rev().collect();
        let sets: Vec<&[Entry]> = widths.iter().map(|width| &included[..*width]).collect();
        println!("north land: every lane passed alone, so the refusal is an interaction; checking the first {} lanes together at once", widths.iter().map(usize::to_string).collect::<Vec<_>>().join(", "));
        let checked = bench.check(&sets)?;
        if checked.iter().any(|checked| checked.verdict != Verdict::Green) && bench.moved()? {
            return Ok(Outcome::Moved);
        }
        if let Some(detail) = broken(&checked) {
            return Ok(Outcome::Paused(detail));
        }
        for ((width, set), checked) in widths.iter().zip(&sets).zip(checked) {
            known.insert(*width, judged(set, checked, strikes));
        }
        widest -= room;
    }
}

fn find<'a>(bench: &impl Bench, included: &[Entry<'a>], together: Checked, strikes: &mut Strikes) -> Result<Outcome<'a>, String> {
    println!("north land: {} lanes refused together; checking each alone at once to find the culprit", included.len());
    let singles: Vec<&[Entry]> = included.chunks(1).collect();
    let mut alone = Vec::new();
    for wave in singles.chunks(bench.room(singles.len())) {
        alone.extend(bench.check(wave)?);
    }
    if alone.iter().any(|checked| checked.verdict != Verdict::Green) && bench.moved()? {
        return Ok(Outcome::Moved);
    }
    let first = alone.first().cloned();
    let broken = broken(&alone);
    let (refused, reasons) = sort(included, alone, strikes);
    if !refused.is_empty() {
        return Ok(Outcome::Culprits(refused));
    }
    if let Some(detail) = broken {
        return Ok(Outcome::Paused(detail));
    }
    if let Some(reason) = reasons.into_iter().next() {
        return Ok(Outcome::Held(reason));
    }
    let Some(first) = first else { return Ok(Outcome::Held("no lane was checked alone".to_owned())) };
    interaction(bench, included, first, together, strikes)
}

fn refuse(lander: &Lander, refused: Refused, refusal: String, batch_size: usize, stages: &Stages, main: &str) -> Result<(), String> {
    let ((path, ticket), checked, _) = refused;
    records::write(lander, path, ticket, &Candidate { head: &checked.head, batch_size, stages }, &Decision::Refused { verdict: &refusal });
    bisect::decide(path, ticket, refusal, Some(&checked.log), main)
}

pub fn search(lander: &Lander, base: &str, included: &[Entry], together: Checked, logs: &Path, strikes: &mut Strikes, stages: &Stages) -> Result<Found, String> {
    let outcome = find(&Offload { lander, base, logs }, included, together, strikes)?;
    strikes.save(&lander.queue)?;
    match outcome {
        Outcome::Moved => {
            println!("north land: main moved from {base} while the lanes were checked apart; no lane judged");
            Ok(Found::Moved)
        }
        Outcome::Held(reason) => Ok(Found::Held(reason)),
        Outcome::Paused(detail) => Ok(Found::Paused(detail)),
        Outcome::Culprits(refused) => {
            for culprit in refused {
                let refusal = format!("failed {}", culprit.2);
                println!("north land: {} is the culprit: refused alone", culprit.0.1.branch);
                refuse(lander, culprit, refusal, 1, stages, base)?;
            }
            Ok(Found::Decided)
        }
        Outcome::Interaction { lanes, head, partner } => {
            let with = included[..lanes].iter().map(|(_, ticket)| ticket.branch.as_str()).collect::<Vec<_>>().join(", ");
            let refusal = format!("failed interaction with {with}: {}", partner.2);
            println!("north land: the first {lanes} lanes pass together and {} breaks them; landing them as {head} and refusing it", partner.0.1.branch);
            refuse(lander, partner, refusal, lanes + 1, stages, &head)?;
            Ok(Found::Interaction { lanes, head })
        }
    }
}
