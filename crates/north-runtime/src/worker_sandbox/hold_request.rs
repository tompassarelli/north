//! A reviewer's hold on a commit placed or lifted through the outside broker for a session whose domain keeps the verdict store unwritable (north#1022, runtime/review_hold.rs). The request is the words `review-hold place COMMIT HOST WHY` or `review-hold lift COMMIT WHY` and nothing else of the caller's: no directory and no environment. A reviewer's accept of a patch, `review-accept PATCH-ID` (north#1036, runtime/review_accept.rs), is answered the same way, written for the session the caller's cgroup names when its record names the role `reviewer` and refused otherwise. The hold is written for the fleet session whose scope the caller's cgroup names, as a finish record is (finish_request.rs), and a lift is the coordinator's when that session's record names the role `proxy`; the session and its role are never words of the request.

use std::fs::File;
use std::io::Write;
use std::os::fd::OwnedFd;

use north_base::platform::review_accept::Accepts;

use super::lane_grant::{self, Host};
use crate::runtime::review_accept::Accept;
use crate::runtime::review_hold::{self, Ask, COORDINATOR, Caller, Holds, REVIEWER, now};

/// The session whose scope `cgroup` names on `host`, and whether its fleet record names it the coordinator.
pub(super) fn caller(cgroup: &str, host: &Host) -> Result<Caller, String> {
    let (directory, record) = lane_grant::own_session(cgroup, host, "a hold is placed and lifted by a fleet session, and this caller runs in no session's scope")?;
    let session = record["name"].as_str().filter(|name| !name.is_empty()).map(str::to_owned).ok_or_else(|| format!("{} names no session", directory.display()))?;
    let role = record["role"].as_str();
    Ok(Caller { session, coordinator: role == Some(COORDINATOR), reviewer: role == Some(REVIEWER) })
}

/// Places or lifts what `ask` names for the session `cgroup` names, in the verdict store of the broker's own environment, placing no hold on a commit the north checkout lacks; the line saying what was done.
fn served(ask: Ask, cgroup: &str, host: &Host) -> Result<String, String> {
    let caller = caller(cgroup, host)?;
    if let Ask::Place { commit, .. } = &ask {
        review_hold::in_checkout(&review_hold::checkout(), commit)?;
    }
    Holds::of(&host.environment, &host.home).serve(ask, &caller, now())
}

/// Records the accept `accept` names for the session `cgroup` names, in the verdict store of the broker's own environment; the line saying what was done.
fn accepted(accept: &Accept, cgroup: &str, host: &Host) -> Result<String, String> {
    let caller = caller(cgroup, host)?;
    accept.serve(&Accepts::of(&host.environment, &host.home), &caller, now())
}

/// Answers the hold request `asked` from a caller whose `/proc/PID/cgroup` reads `cgroup`, telling the client on its standard output what was done; a refusal is told to the client and returned for the broker's log.
pub(super) fn answer(asked: Result<Ask, String>, streams: Vec<OwnedFd>, cgroup: Option<&str>, host: &Host) -> Result<u8, String> {
    tell(asked.and_then(|ask| served(ask, cgroup.unwrap_or_default(), host)), streams, "a review hold")
}

/// Answers the accept request `asked` as `answer` answers a hold request.
pub(super) fn answer_accept(asked: Result<Accept, String>, streams: Vec<OwnedFd>, cgroup: Option<&str>, host: &Host) -> Result<u8, String> {
    tell(asked.and_then(|accept| accepted(&accept, cgroup.unwrap_or_default(), host)), streams, "a review accept")
}

pub(super) fn tell(outcome: Result<String, String>, streams: Vec<OwnedFd>, what: &str) -> Result<u8, String> {
    match outcome {
        Ok(line) => {
            eprintln!("north worker-sandbox broker: {line}");
            if let Some(stdout) = streams.into_iter().nth(1) {
                let _ = writeln!(File::from(stdout), "{line}");
            }
            Ok(0)
        }
        Err(why) => {
            if let Some(stderr) = streams.into_iter().nth(2) {
                let _ = writeln!(File::from(stderr), "north worker-sandbox broker: refused: {why}");
            }
            Err(format!("refused {what}: {why}"))
        }
    }
}
