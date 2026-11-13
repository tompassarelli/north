//! A reviewer's accept of a lane's patch (north#1036, base/platform/review_accept.rs): `north runtime accept COMMIT` records, for the fleet session the caller's cgroup scope names, the accept of the patch of COMMIT over its merge-base with origin/main. The session is read by the broker for a caller inside a worker sandbox and by the verb itself outside one, never from a variable or an argument; the request the broker takes is the words `review-accept PATCH-ID` and nothing else of the caller's. The land scan `review-accept` counts these records.

use north_base::platform::review_accept::{Accept as Record, Accepts, is_patch_id, patch_id};

use super::failure::{Failure, failed};
use super::review_hold::{Caller, full_sha, git};

pub(crate) const VERB: &str = "review-accept";
const BASE: &str = "origin/main";
const USAGE: &str = "usage: north runtime accept COMMIT  record the calling fleet session's accept, which only a session launched in the reviewer role may give, of the patch of COMMIT over its merge-base with origin/main, which a lane that changes a trust-boundary path needs from two sessions (one for a test-only change or an exact revert) before it lands; the accept is read on the host that settles the fleet";

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Accept {
    pub patch_id: String,
}

impl Accept {
    pub(crate) fn words(&self) -> Vec<String> {
        [VERB, &self.patch_id].map(str::to_owned).to_vec()
    }

    /// The accept request in `arguments`: `None` for any other request, and why one in other words is refused.
    pub(crate) fn asked(arguments: &[String]) -> Option<Result<Self, String>> {
        let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
        let [VERB, rest @ ..] = words.as_slice() else { return None };
        Some(match rest {
            [id] if is_patch_id(id) => Ok(Self { patch_id: (*id).to_owned() }),
            _ => Err(format!("an accept request is `{VERB} PATCH-ID`, with the id `git patch-id --stable` printed")),
        })
    }

    /// Records the accept of the patch for `caller` at `now` in `accepts`, and the line that says what was done.
    pub(crate) fn serve(&self, accepts: &Accepts, caller: &Caller, now: u64) -> Result<String, String> {
        if !caller.reviewer {
            return Err(format!("{} was not launched in the reviewer role, and only a session the fleet launched in that role accepts a patch", caller.session));
        }
        accepts.record(&Record { patch_id: self.patch_id.clone(), session: caller.session.clone(), at: now })?;
        Ok(format!("north runtime accept: {} accepts the patch {}", caller.session, self.patch_id))
    }
}

/// `north runtime accept COMMIT` on a host that `settles` the fleet: names the patch of COMMIT in the working directory and has `serve` record the caller's own accept of it (worker_sandbox/hold_client.rs), printing the line it says; its exit code.
pub(crate) fn run(flags: &[String], settles: bool, serve: impl FnOnce(Accept) -> Result<String, String>) -> u8 {
    command(flags, settles, serve).map_or_else(
        |failure| {
            eprintln!("north runtime accept: {}", failure.message);
            failure.code
        },
        |()| 0,
    )
}

fn command(flags: &[String], settles: bool, serve: impl FnOnce(Accept) -> Result<String, String>) -> Result<(), Failure> {
    let [named] = flags else { return Err(failed(64, USAGE)) };
    if named.starts_with('-') {
        return Err(failed(64, USAGE));
    }
    if !settles {
        return Err(failed(64, format!("the host that settles the fleet reads the accepts its landing counts, and this is not it; accept from a session there\n{USAGE}")));
    }
    let commit = git(&["rev-parse", "--verify", &format!("{named}^{{commit}}")]).ok().filter(|commit| full_sha(commit)).ok_or_else(|| failed(1, format!("{named} names no commit here")))?;
    let fork = git(&["merge-base", BASE, &commit])?;
    let patch = patch_id(&std::env::current_dir().unwrap_or_default(), &fork, &commit).map_err(|why| failed(1, why))?;
    let line = serve(Accept { patch_id: patch }).map_err(|why| failed(1, why))?;
    if !line.is_empty() {
        println!("{line}");
    }
    Ok(())
}

#[cfg(test)]
#[path = "review_accept_tests.rs"]
mod tests;
