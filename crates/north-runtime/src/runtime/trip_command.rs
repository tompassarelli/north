//! `north runtime trip clear COMMIT --why TEXT [--issue REPO#N]`: clears the open Buck2 trip on a commit, on the host that settles the fleet; the second clear of one failing test within a day names its class-fix issue (north#1577).

use north_unblocks::{Admission, Kind, Ledger};

use super::failure::{Failure, failed};
use super::order::orderer;
use super::review_hold::{full_sha, git, now};
use super::trip::{Trip, Trips};

const USAGE: &str = "usage: north runtime trip clear COMMIT --why TEXT [--issue REPO#N]  clear the open Buck2 trip that names COMMIT, so the promotion of that head and of later heads goes live again; the clear keeps who cleared it and why; the second clear of the same failing tests within 24 h is refused unless --issue names an open issue with an owner that fixes their cause; run it on the host that settles the fleet";

pub(super) fn command(flags: &[String], settles: bool) -> Result<(), Failure> {
    let (commit, why, issue) = match flags {
        [verb, commit, flag, why] if verb == "clear" && flag == "--why" => (commit, why, None),
        [verb, commit, flag, why, named, issue] if verb == "clear" && flag == "--why" && named == "--issue" => (commit, why, Some(issue.as_str())),
        _ => return Err(failed(64, USAGE)),
    };
    if why.trim().is_empty() {
        return Err(failed(64, USAGE));
    }
    if !settles {
        return Err(failed(64, format!("the host that settles the fleet keeps the trips its order and promotions screen by, and this is not it\n{USAGE}")));
    }
    let head = git(&["rev-parse", "--verify", &format!("{commit}^{{commit}}")]).ok().filter(|head| full_sha(head)).ok_or_else(|| failed(1, format!("{commit} names no commit here")))?;
    let admission = Admission { ledger: &Ledger::here(), issues: &north_unblocks::GitHub, now: now() };
    let line = clear(&Trips::here(), &admission, &head, &orderer(), why.trim(), issue).map_err(|why| failed(1, why))?;
    println!("{line}");
    Ok(())
}

/// The class a trip clear unblocks: the failed cases it names, or the run when its log named none.
pub fn class(trip: &Trip) -> String {
    let mut cases: Vec<String> = trip.cases.iter().map(|(name, case)| format!("{case} in {name}")).collect();
    cases.sort();
    if cases.is_empty() { "a hosted run whose log names no failed test".to_owned() } else { cases.join(", ") }
}

/// Clears the open trip on `head` once the unblock ledger admits it: the second clear of one class within a day names an open, owned issue.
pub fn clear(trips: &Trips, admission: &Admission, head: &str, by: &str, why: &str, issue: Option<&str>) -> Result<String, String> {
    let trip = trips.read(head)?.ok_or_else(|| format!("no Buck2 trip stands on {head}"))?;
    if trips.cleared(head)? {
        return Err(format!("the Buck2 trip on {head} is already cleared"));
    }
    admission.admit(Kind::TripClear, &class(&trip), by, issue)?;
    trips.clear(head, by, why, admission.now)
}
