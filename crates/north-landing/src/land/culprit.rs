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

#[derive(Debug, PartialEq, Eq)]
pub enum Found {
    Decided,
    Innocent,
    Moved,
    Held(String),
}

struct Alone {
    verdict: Verdict,
    head: String,
    log: PathBuf,
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

fn wave_size(repo: &Path, lanes: usize) -> (usize, Option<(usize, usize)>) {
    let slots = free_slots(repo);
    let room = slots.map_or(MOST_AT_ONCE, |(free, _)| free.max(1));
    (room.min(lanes), slots)
}

enum Start {
    Running(Started, Alone),
    Ended(Alone),
}

fn start_alone(lander: &Lander, base: &str, offset: usize, entry: (&PathBuf, &Ticket), logs: &Path) -> Result<Start, String> {
    let (path, ticket) = entry;
    let worktree = candidate::lander_worktree(&lander.repo, &format!("lander-culprit-{offset}"))?;
    candidate::ensure(&lander.repo, &worktree)?;
    let name = path.file_stem().and_then(|stem| stem.to_str()).unwrap_or("lane");
    let log = logs.join(format!("{}-{name}-candidate.log", micros()));
    let built = candidate::build(&worktree, &format!("land-culprit-{offset}"), base, &[(path.clone(), ticket.clone())], 1, logs)?;
    let head = built.head;
    let reason = match built.picks.into_iter().next() {
        Some(Pick::Included) => None,
        Some(Pick::Decided(verdict) | Pick::Undecided(verdict)) => Some(verdict),
        None => Some("no pick".to_owned()),
    };
    if let Some(reason) = reason {
        return Ok(Start::Ended(Alone { verdict: Verdict::Transient(format!("{} cannot be built alone: {reason}", ticket.branch)), head, log }));
    }
    Ok(match checks::start(&lander.checker, &worktree, base, &log, true) {
        Ok(started) => Start::Running(started, Alone { verdict: Verdict::Green, head, log }),
        Err(verdict) => Start::Ended(Alone { verdict, head, log }),
    })
}

fn check_wave(lander: &Lander, base: &str, wave: &[(&PathBuf, &Ticket)], logs: &Path) -> Result<Vec<Alone>, String> {
    let mut finished: Vec<Option<Alone>> = wave.iter().map(|_| None).collect();
    let mut running: Vec<Option<(Started, Alone)>> = Vec::new();
    for (offset, entry) in wave.iter().enumerate() {
        match start_alone(lander, base, offset, *entry, logs) {
            Ok(Start::Running(started, alone)) => running.push(Some((started, alone))),
            Ok(Start::Ended(alone)) => {
                finished[offset] = Some(alone);
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
                finished[offset] = slot.take().map(|(_, alone)| Alone { verdict, ..alone });
            }
        }
        std::thread::sleep(POLL);
    }
    Ok(finished.into_iter().flatten().collect())
}

type Refused<'a> = ((&'a PathBuf, &'a Ticket), Alone, String);

fn sort<'a>(included: &[(&'a PathBuf, &'a Ticket)], alone: Vec<Alone>, strikes: &mut Strikes) -> (Vec<Refused<'a>>, Vec<String>) {
    let mut held = Vec::new();
    let mut refused = Vec::new();
    for (entry, alone) in included.iter().zip(alone) {
        let streak = strikes.record(&Strikes::key(&[*entry]), &alone.verdict);
        let branch = &entry.1.branch;
        match bisect::escalate(alone.verdict.clone(), streak, &alone.log) {
            Verdict::Green => {}
            Verdict::Refused(detail) => refused.push((*entry, alone, detail)),
            Verdict::RedMain(detail) => held.push(format!("{branch} alone: main is red ({detail})")),
            Verdict::Transient(detail) => held.push(format!("{branch} alone: transient {detail}")),
            Verdict::Deferred(detail) => held.push(format!("{branch} alone: deferred {detail}")),
        }
    }
    (refused, held)
}

pub fn search(lander: &Lander, base: &str, included: &[(&PathBuf, &Ticket)], logs: &Path, strikes: &mut Strikes, stages: &Stages) -> Result<Found, String> {
    let (width, slots) = wave_size(&lander.repo, included.len());
    match slots {
        Some((free, total)) if width < included.len() => println!("north land: {} lanes refused together; {free} of {total} offload slots are free, so they are checked alone in waves of {width}", included.len()),
        _ => println!("north land: {} lanes refused together; checking each alone at once to find the culprit", included.len()),
    }
    let mut alone = Vec::new();
    for wave in included.chunks(width) {
        alone.extend(check_wave(lander, base, wave, logs)?);
    }
    if alone.iter().any(|alone| alone.verdict != Verdict::Green) && lander.base_moved(base)? {
        println!("north land: main moved from {base} while the lanes were checked alone; no lane judged");
        return Ok(Found::Moved);
    }
    let (refused, held) = sort(included, alone, strikes);
    strikes.save(&lander.queue)?;
    if refused.is_empty() {
        return Ok(match held.into_iter().next() {
            Some(reason) => Found::Held(reason),
            None => {
                println!("north land: every lane passed alone, so the refusal is an interaction; falling back to halving the candidate");
                Found::Innocent
            }
        });
    }
    for ((path, ticket), alone, detail) in refused {
        let refusal = format!("failed {detail}");
        println!("north land: {} is the culprit: refused alone", ticket.branch);
        records::write(lander, path, ticket, &Candidate { head: &alone.head, batch_size: 1, stages }, &Decision::Refused { verdict: &refusal });
        bisect::decide(path, ticket, refusal, Some(&alone.log))?;
    }
    Ok(Found::Decided)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_repository_owns_its_slot_locks_and_no_other() {
        assert!(belongs("code-north-main.lock", "code-north-main"));
        assert!(belongs("code-north-main-3.lock", "code-north-main"));
        assert!(!belongs("code-north-main-3.holder", "code-north-main"));
        assert!(!belongs("code-north-main-x.lock", "code-north-main"));
        assert!(!belongs("code-north-mainly.lock", "code-north-main"));
        assert!(!belongs("code-makgora-main.lock", "code-north-main"));
    }
}
