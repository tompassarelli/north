//! A reviewer's hold on a commit placed or lifted through the outside broker for a session whose domain keeps the verdict store unwritable (north#1022, runtime/review_hold.rs). The request is the words `review-hold place COMMIT HOST WHY` or `review-hold lift COMMIT WHY` and nothing else of the caller's: no directory and no environment. The hold is written for the fleet session whose scope the caller's cgroup names, as a finish record is (finish_request.rs), and a lift is the coordinator's when that session's record names the role `proxy`; the session and its role are never words of the request.

use std::fs::File;
use std::io::Write;
use std::os::fd::OwnedFd;

use super::lane_grant::{self, Host};
use crate::runtime::review_hold::{Ask, COORDINATOR, Caller, Holds, now};

/// The session whose scope `cgroup` names on `host`, and whether its fleet record names it the coordinator.
pub(super) fn caller(cgroup: &str, host: &Host) -> Result<Caller, String> {
    let (directory, record) = lane_grant::own_session(cgroup, host, "a hold is placed and lifted by a fleet session, and this caller runs in no session's scope")?;
    let session = record["name"].as_str().filter(|name| !name.is_empty()).map(str::to_owned).ok_or_else(|| format!("{} names no session", directory.display()))?;
    Ok(Caller { session, coordinator: record["role"].as_str() == Some(COORDINATOR) })
}

/// Places or lifts what `ask` names for the session `cgroup` names, in the verdict store of the broker's own environment; the line saying what was done.
fn served(ask: Ask, cgroup: &str, host: &Host) -> Result<String, String> {
    let caller = caller(cgroup, host)?;
    Holds::of(&host.environment, &host.home).serve(ask, &caller, now())
}

/// Answers the hold request `asked` from a caller whose `/proc/PID/cgroup` reads `cgroup`, telling the client on its standard output what was done; a refusal is told to the client and returned for the broker's log.
pub(super) fn answer(asked: Result<Ask, String>, streams: Vec<OwnedFd>, cgroup: Option<&str>, host: &Host) -> Result<u8, String> {
    match asked.and_then(|ask| served(ask, cgroup.unwrap_or_default(), host)) {
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
            Err(format!("refused a review hold: {why}"))
        }
    }
}
