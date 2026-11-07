//! One-time adoption of the marker files the promotion machines replace: `runtime-failed` and its causes,
//! `runtime-previous`, `runtime-canary` and `runtime-settle-judged`, with `current` for the live runtime.
use super::{Canary, Canarying, Judge, Judgement, Promoting, Promotion};
use crate::runtime::failure::Failure;
use crate::runtime::layout::{Layout, is_sha, linked_sha};
use north_base::foundation::clock::Timestamp;
use north_base::lifecycle::{Lifecycle, State, Store};
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

const HOLD: Duration = Duration::from_secs(60 * 60);
const FAILED: &str = "runtime-failed";
const CAUSES: &str = "runtime-failed-causes";
const PREVIOUS: &str = "runtime-previous";
const CANARIED: &str = "runtime-canary";
const JUDGED: &str = "runtime-settle-judged";

pub(super) fn files(layout: &Layout) -> [PathBuf; 5] {
    [FAILED, CAUSES, PREVIOUS, CANARIED, JUDGED].map(|name| layout.state.join(name))
}

pub(super) struct Old {
    current: Option<String>,
    previous: Option<String>,
    failed: Vec<(String, Promotion, String)>,
    held_at: Option<SystemTime>,
    canaried: Vec<(String, String)>,
    judged: Option<String>,
}

pub(super) enum Adopted {
    Promotion(String, Promotion, String),
    Canary(String, Canary, String),
    Judgement(String, Judgement, String),
}

pub(super) struct Impossible(pub(super) String);

impl Old {
    pub(super) fn read(layout: &Layout) -> Self {
        let read = |name: &str| fs::read_to_string(layout.state.join(name)).unwrap_or_default();
        let causes: Vec<(String, Promotion, String)> = read(CAUSES).lines().filter_map(cause).collect();
        let held_at = fs::metadata(layout.state.join(FAILED)).and_then(|metadata| metadata.modified()).ok();
        let fresh = held_at.is_some_and(|at| at.elapsed().is_ok_and(|age| age < HOLD));
        let failed = read(FAILED)
            .lines()
            .map(str::trim)
            .filter(|sha| fresh && is_sha(sha))
            .map(|sha| causes.iter().rev().find(|(known, _, _)| known == sha).cloned().unwrap_or_else(|| (sha.to_owned(), Promotion::InstallFailed, String::new())))
            .collect();
        Self {
            current: fs::read_link(layout.current()).ok().and_then(|link| linked_sha(&link).map(str::to_owned)),
            previous: Some(read(PREVIOUS).trim().to_owned()).filter(|sha| is_sha(sha)),
            failed,
            held_at,
            canaried: read(CANARIED).lines().filter_map(|line| line.split_once(' ')).filter(|(_, sha)| is_sha(sha)).map(|(host, sha)| (host.to_owned(), sha.to_owned())).collect(),
            judged: Some(read(JUDGED).trim().to_owned()).filter(|sha| is_sha(sha)),
        }
    }

    pub(super) fn held_at(&self) -> Option<SystemTime> {
        self.held_at
    }
}

fn cause(line: &str) -> Option<(String, Promotion, String)> {
    let mut words = line.trim().splitn(3, ' ');
    let sha = words.next().filter(|sha| is_sha(sha))?.to_owned();
    let state = match words.next()? {
        "build" => Promotion::BuildFailed,
        "promote" => Promotion::InstallFailed,
        "rollback" => Promotion::RolledBack,
        _ => return None,
    };
    Some((sha, state, words.next().unwrap_or("").trim().to_owned()))
}

/// Every instance the old files describe in the state the new machines name for it; a combination no state names is
/// `Impossible`: a hold on the runtime `current` points at, which only a first install that failed its check leaves.
pub(super) fn classify(old: &Old) -> Vec<Result<Adopted, Impossible>> {
    let mut adopted = Vec::new();
    if let Some(sha) = &old.current {
        adopted.push(Ok(Adopted::Promotion(sha.clone(), Promotion::Live, "adopted from the current link".to_owned())));
    }
    if let Some(sha) = old.previous.as_ref().filter(|sha| Some(*sha) != old.current.as_ref()) {
        adopted.push(Ok(Adopted::Promotion(sha.clone(), Promotion::Superseded, format!("adopted from {PREVIOUS}"))));
    }
    for (sha, state, reason) in &old.failed {
        adopted.push(match Some(sha) == old.current.as_ref() {
            true => Err(Impossible(format!("{sha} is on hold in {FAILED} ({}) and is also what current points at", state.name()))),
            false => Ok(Adopted::Promotion(sha.clone(), *state, format!("adopted from {FAILED}: {reason}"))),
        });
    }
    adopted.extend(old.canaried.iter().map(|(host, sha)| Ok(Adopted::Canary(format!("{host}/{sha}"), Canary::Passed, format!("adopted from {CANARIED}")))));
    adopted.extend(old.judged.iter().map(|sha| Ok(Adopted::Judgement(sha.clone(), Judgement::Judged, format!("adopted from {JUDGED}")))));
    adopted
}

pub(super) fn adopt(layout: &Layout, store: &mut Store) -> Result<(), Failure> {
    let paths = files(layout);
    if !paths.iter().any(|path| path.exists()) {
        return Ok(());
    }
    let old = Old::read(layout);
    let held_at = old.held_at().map_or_else(Timestamp::now, |at| Timestamp::from_unix_seconds(at.duration_since(SystemTime::UNIX_EPOCH).map_or(0, |since| since.as_secs() as i64)));
    for adoption in classify(&old) {
        let adoption = match adoption {
            Ok(adoption) => adoption,
            Err(Impossible(why)) => {
                eprintln!("north runtime: not adopting: {why}");
                continue;
            }
        };
        match adoption {
            Adopted::Promotion(sha, state, cause) => take::<Promoting>(store, &sha, state, &cause, if state.holds_back() { held_at } else { Timestamp::now() })?,
            Adopted::Canary(instance, state, cause) => take::<Canarying>(store, &instance, state, &cause, Timestamp::now())?,
            Adopted::Judgement(sha, state, cause) => take::<Judge>(store, &sha, state, &cause, Timestamp::now())?,
        }
    }
    for path in paths {
        let _ = fs::remove_file(path);
    }
    Ok(())
}

fn take<L: Lifecycle>(store: &mut Store, instance: &str, state: L::State, cause: &str, at: Timestamp) -> Result<(), Failure> {
    if store.current::<L>(instance).map_err(super::unreadable)?.is_some() {
        return Ok(());
    }
    super::moved::<L>(store.transition::<L>(instance, state, cause, at).map_err(super::unreadable)?, instance).map(|_| ())
}
