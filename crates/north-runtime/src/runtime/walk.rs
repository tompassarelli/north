use super::canary::{self, Verdict};
use super::failure::{Failure, failed};
use super::layout::{self, Layout};
use super::lifecycle::{self, Canarying, Canary};
use super::live::Swap;
use super::cadence;
use super::pin::{Event, note, now};
use super::probe::Shell;
use crate::platform::host;
use std::fs;
use std::process::Command;

const UNKNOWN_VERB: i32 = 64;
const PANICKED: i32 = 101;

pub(super) fn judged(layout: &Layout, host: &str, sha: &str, verdict: &Verdict, swaps: &[Swap]) {
    let at = now();
    let walker = host == host::name();
    let event = match verdict {
        Verdict::Passed if walker => Event::Walked { sha: sha.to_owned(), host: host.to_owned(), at },
        Verdict::RolledBack => Event::RolledBack { sha: sha.to_owned(), host: host.to_owned(), at, seconds: at - live_since(swaps, sha).unwrap_or(at), walker },
        _ => return,
    };
    note(layout, &event);
    if *verdict == Verdict::RolledBack {
        let probe = lifecycle::record_of::<Canarying>(layout, &format!("{host}/{sha}")).ok().flatten().map_or_else(|| "no probe recorded".to_owned(), |record| record.cause);
        cadence::note_failure(layout, at, sha, &probe);
    }
}

fn live_since(swaps: &[Swap], sha: &str) -> Option<i64> {
    swaps.iter().rev().find(|swap| swap.runtime == sha).map(|swap| swap.at)
}

/// A promotion run by runtime `own` leaves the walk of the runtime now live to that runtime (`north runtime walk`), so each runtime is judged, shipped and followed by its own code; none when `own` is live itself, is no runtime, or the live one is too old to walk itself or died walking, and the caller walks.
pub(super) fn handed_to_live(layout: &Layout, own: Option<&str>) -> Option<Result<(), Failure>> {
    let own = own?;
    let live = layout.live_sha().filter(|live| live.as_str() != own && layout::has_programs(&layout.runtime(live)))?;
    let short = layout::short(&live);
    match Command::new(layout.runtime(&live).join("north")).args(["runtime", "walk"]).status() {
        Ok(status) if status.success() => Some(Ok(())),
        Ok(status) if status.code() == Some(UNKNOWN_VERB) => {
            println!("north runtime: {short} is older than `north runtime walk`, so the runtime that promoted it walks it");
            None
        }
        Ok(status) if status.code().is_none_or(|code| code == PANICKED) => {
            println!("north runtime: {short} died in its own walk ({status}), so the runtime that promoted it walks it and rolls it back if it fails");
            None
        }
        Ok(status) => Some(Err(failed(status.code().and_then(|code| u8::try_from(code).ok()).unwrap_or(1), format!("the walk of {short}, run by that runtime, failed; its message is above")))),
        Err(error) => {
            println!("north runtime: {short} could not be started to walk itself ({error}), so the runtime that promoted it walks it");
            None
        }
    }
}

pub(super) fn walked_here(layout: &Layout) -> Result<String, String> {
    let sha = layout.live_sha().ok_or("this host has no live runtime")?;
    let name = host::name();
    let shell = Shell::Here(name.clone());
    let verdict = canary::check(layout, &name, &sha, &canary::Live { layout, host: &shell });
    let swaps: Vec<Swap> = fs::read_to_string(layout.swaps()).unwrap_or_default().lines().filter_map(|line| serde_json::from_str(line).ok()).collect();
    judged(layout, &name, &sha, &verdict, &swaps);
    walk_verdict(layout, &name, &sha, &verdict)
}

pub(super) fn walk_verdict(layout: &Layout, host: &str, sha: &str, verdict: &Verdict) -> Result<String, String> {
    let record = lifecycle::record_of::<Canarying>(layout, &format!("{host}/{sha}")).ok().flatten();
    let cause = record.as_ref().map_or_else(String::new, |record| format!(": {}", record.cause));
    match (verdict, record.map(|record| record.state)) {
        (Verdict::Passed, _) | (_, Some(Canary::Passed | Canary::Trusted)) => Ok(sha.to_owned()),
        (Verdict::RolledBack, _) => Err(format!("{} failed the spine walk on {host} and was rolled back{cause}", layout::short(sha))),
        (Verdict::Unrun(why), _) => Err(format!("{} has not walked the spine on {host}: {why}", layout::short(sha))),
        _ => Err(format!("{} has not passed the spine walk on {host}{cause}", layout::short(sha))),
    }
}

/// The walk of the runtime live here after a probe judged `judged` (the sha and its verdict): a rollback leaves another runtime live, whose own canary state decides.
pub(super) fn walk_of(layout: &Layout, host: &str, live: Option<&str>, judged: Option<(&str, &Verdict)>) -> Result<String, String> {
    let live = live.ok_or("this host has no live runtime")?;
    match judged {
        Some((sha, verdict)) if sha == live => walk_verdict(layout, host, live, verdict),
        _ => walk_verdict(layout, host, live, &Verdict::Skipped),
    }
}

/// Ships the walked runtime to the other hosts, or nothing when it has not walked the spine here.
pub(super) fn ship_if_walked<T>(walk: Result<String, String>, ship: impl FnOnce(&str) -> Result<T, Failure>) -> Result<Option<T>, Failure> {
    match walk {
        Ok(sha) => ship(&sha).map(Some),
        Err(why) => {
            println!("north runtime: not shipped to the other hosts: {why}");
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests;
