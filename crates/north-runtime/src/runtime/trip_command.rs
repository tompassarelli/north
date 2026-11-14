//! `north runtime trip clear COMMIT --why TEXT`: clears the open Buck2 trip on a commit, on the host that settles the fleet.

use super::failure::{Failure, failed};
use super::order::orderer;
use super::review_hold::{full_sha, git, now};
use super::trip::Trips;

const USAGE: &str = "usage: north runtime trip clear COMMIT --why TEXT  clear the open Buck2 trip that names COMMIT, so the promotion of that head and of later heads goes live again; the clear keeps who cleared it and why; run it on the host that settles the fleet";

pub(super) fn command(flags: &[String], settles: bool) -> Result<(), Failure> {
    let (commit, why) = match flags {
        [verb, commit, flag, why] if verb == "clear" && flag == "--why" && !why.trim().is_empty() => (commit, why),
        _ => return Err(failed(64, USAGE)),
    };
    if !settles {
        return Err(failed(64, format!("the host that settles the fleet keeps the trips its order and promotions screen by, and this is not it\n{USAGE}")));
    }
    let head = git(&["rev-parse", "--verify", &format!("{commit}^{{commit}}")]).ok().filter(|head| full_sha(head)).ok_or_else(|| failed(1, format!("{commit} names no commit here")))?;
    let line = Trips::here().clear(&head, &orderer(), why.trim(), now()).map_err(|why| failed(1, why))?;
    println!("{line}");
    Ok(())
}
