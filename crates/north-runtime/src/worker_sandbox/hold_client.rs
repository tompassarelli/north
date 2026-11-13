//! The client side of `north runtime hold` (north#1022, runtime/review_hold.rs): inside a worker sandbox's domain, which keeps the verdict store unwritable, the outside broker writes the hold for the session the caller's scope names and tells the caller itself (hold_request.rs); outside one, the session is the one this process's own cgroup scope names and the hold is written here.

use super::{MARK, broker, hold_request, lane_grant};
use crate::runtime::review_hold::{self, Ask, Holds, now};

/// `north runtime hold` on a host that `settles` the fleet, or not (runtime/review_hold.rs); its exit code.
pub fn command(arguments: &[String], settles: bool) -> u8 {
    review_hold::run(arguments, settles, hold)
}

/// Places or lifts the hold `ask` names for the caller's own session; the line saying what was done, empty when the broker already told the caller.
fn hold(ask: Ask) -> Result<String, String> {
    if std::env::var_os(MARK).is_some() {
        return match broker::request_check(&ask.words())? {
            0 => Ok(String::new()),
            code => Err(format!("the broker did not record the hold (exit {code})")),
        };
    }
    let cgroup = std::fs::read_to_string("/proc/self/cgroup").map_err(|error| format!("/proc/self/cgroup: {error}"))?;
    let caller = hold_request::caller(&cgroup, &lane_grant::Host::own())?;
    Holds::here().serve(ask, &caller, now())
}
