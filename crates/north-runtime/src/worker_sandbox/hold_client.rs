//! The client side of `north runtime hold` (north#1022, runtime/review_hold.rs) and `north runtime accept` (north#1036, runtime/review_accept.rs): inside a worker sandbox's domain, which keeps the verdict store unwritable, the outside broker writes the hold or accept for the session the caller's scope names and tells the caller itself (hold_request.rs); outside one, the session is the one this process's own cgroup scope names and the record is written here.

use north_base::platform::review_accept::Accepts;

use super::peer::Peer;
use super::{MARK, broker, hold_request, lane_grant};
use crate::runtime::review_accept::{self, Accept};
use crate::runtime::review_hold::{self, Ask, Caller, Holds, now};
use crate::runtime::update;

/// `north runtime hold` on a host that `settles` the fleet, or not (runtime/review_hold.rs); its exit code.
pub fn command(arguments: &[String], settles: bool) -> u8 {
    review_hold::run(arguments, settles, hold)
}

/// `north runtime accept` on a host that `settles` the fleet, or not (runtime/review_accept.rs); its exit code.
pub fn accept_command(arguments: &[String], settles: bool) -> u8 {
    review_accept::run(arguments, settles, accept)
}

/// Asks the broker to write the request `words` for the caller's own session; the line saying what was done, empty because the broker already told the caller, or `None` outside a worker sandbox.
fn brokered(words: &[String], what: &str) -> Option<Result<String, String>> {
    std::env::var_os(MARK).map(|_| match broker::request_check(words) {
        Ok(0) => Ok(String::new()),
        Ok(code) => Err(format!("the broker did not record the {what} (exit {code})")),
        Err(why) => Err(why),
    })
}

fn own_caller() -> Result<Caller, String> {
    let peer = Peer::of(std::process::id());
    hold_request::caller(&peer, &lane_grant::Host::own())
}

/// Places or lifts the hold `ask` names for the caller's own session; the line saying what was done, empty when the broker already told the caller.
fn hold(ask: Ask) -> Result<String, String> {
    match brokered(&ask.words(), "hold") {
        Some(told) => told,
        None => Holds::here().serve(ask, &own_caller()?, now(), &update::checkout()),
    }
}

/// Records the accept `accept` names for the caller's own session; the line saying what was done, empty when the broker already told the caller.
fn accept(accept: Accept) -> Result<String, String> {
    match brokered(&accept.words(), "accept") {
        Some(told) => told,
        None => accept.serve(&Accepts::here(), &own_caller()?, now()),
    }
}
