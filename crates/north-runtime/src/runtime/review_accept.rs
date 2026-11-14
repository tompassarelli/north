//! A reviewer's accept of a lane's patch (north#1036, base/platform/review_accept.rs): `north runtime accept COMMIT` records, for the fleet session the caller's cgroup scope names, the accept of the patch of COMMIT over its merge-base with origin/main, and `north runtime accept --withdraw COMMIT` removes that session's own accept of it (north#1492). A full sha this store lacks, such as the head of a lane handed in on a build host, is fetched from the trusted mirror beside the main checkout, where the lander publishes every lane it takes from an outbox (north#1485), so a reviewer needs no reach to that host. The session is read by the broker for a caller inside a worker sandbox and by the verb itself outside one, never from a variable or an argument; the request the broker takes is the words `review-accept PATCH-ID` or `review-accept --withdraw PATCH-ID` and nothing else of the caller's. The land scan `review-accept` counts these records.

use north_base::platform::review_accept::{Accept as Record, Accepts, is_patch_id, patch_id};

use super::failure::{Failure, failed};
use super::review_hold::{Caller, full_sha, git};

pub(crate) const VERB: &str = "review-accept";
const WITHDRAW: &str = "--withdraw";
const BASE: &str = "origin/main";
const USAGE: &str = "usage: north runtime accept [--withdraw] COMMIT  record the calling fleet session's accept, which only a session launched in the reviewer role may give, of the patch of COMMIT over its merge-base with origin/main, which a lane that changes a trust-boundary path needs from two sessions (one for a test-only change or an exact revert) before it lands; --withdraw removes the calling session's own accept of that patch, which then no longer counts; the accept is read on the host that settles the fleet";

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Accept {
    pub patch_id: String,
    pub withdraw: bool,
}

impl Accept {
    pub(crate) fn words(&self) -> Vec<String> {
        let words: &[&str] = if self.withdraw { &[VERB, WITHDRAW, &self.patch_id] } else { &[VERB, &self.patch_id] };
        words.iter().map(|word| (*word).to_owned()).collect()
    }

    /// The accept request in `arguments`: `None` for any other request, and why one in other words is refused.
    pub(crate) fn asked(arguments: &[String]) -> Option<Result<Self, String>> {
        let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
        let [VERB, rest @ ..] = words.as_slice() else { return None };
        Some(match rest {
            [id] if is_patch_id(id) => Ok(Self { patch_id: (*id).to_owned(), withdraw: false }),
            [WITHDRAW, id] if is_patch_id(id) => Ok(Self { patch_id: (*id).to_owned(), withdraw: true }),
            _ => Err(format!("an accept request is `{VERB} PATCH-ID` or `{VERB} {WITHDRAW} PATCH-ID`, with the id `git patch-id --stable` printed")),
        })
    }

    /// Records the accept of the patch for `caller` at `now` in `accepts`, or removes the caller's own accept of it, and the line that says what was done.
    pub(crate) fn serve(&self, accepts: &Accepts, caller: &Caller, now: u64) -> Result<String, String> {
        if self.withdraw {
            return Ok(match accepts.withdraw(&self.patch_id, &caller.session)? {
                true => format!("north runtime accept: {} withdraws its accept of the patch {}", caller.session, self.patch_id),
                false => format!("north runtime accept: {} holds no accept of the patch {}", caller.session, self.patch_id),
            });
        }
        if !caller.reviewer {
            return Err(format!("{} was not launched in the reviewer role, and only a session the fleet launched in that role accepts a patch", caller.session));
        }
        accepts.record(&Record { patch_id: self.patch_id.clone(), session: caller.session.clone(), at: now })?;
        Ok(format!("north runtime accept: {} accepts the patch {}", caller.session, self.patch_id))
    }
}

/// `north runtime accept [--withdraw] COMMIT` on a host that `settles` the fleet: names the patch of COMMIT in the working directory and has `serve` record or withdraw the caller's own accept of it (worker_sandbox/hold_client.rs), printing the line it says; its exit code.
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
    let (named, withdraw) = match flags {
        [named] => (named, false),
        [flag, named] if flag == WITHDRAW => (named, true),
        _ => return Err(failed(64, USAGE)),
    };
    if named.starts_with('-') {
        return Err(failed(64, USAGE));
    }
    if !settles {
        return Err(failed(64, format!("the host that settles the fleet reads the accepts its landing counts, and this is not it; accept from a session there\n{USAGE}")));
    }
    let commit = resolved(named).ok_or_else(|| failed(1, format!("{named} names no commit here, nor a lane head the trusted mirror publishes")))?;
    let fork = git(&["merge-base", BASE, &commit])?;
    let patch = patch_id(&std::env::current_dir().unwrap_or_default(), &fork, &commit).map_err(|why| failed(1, why))?;
    let line = serve(Accept { patch_id: patch, withdraw }).map_err(|why| failed(1, why))?;
    if !line.is_empty() {
        println!("{line}");
    }
    Ok(())
}

/// The commit `named` names here, or, for a full sha this store lacks, the lane head the trusted mirror beside the main checkout publishes, fetched into this store.
fn resolved(named: &str) -> Option<String> {
    let here = || git(&["rev-parse", "--verify", &format!("{named}^{{commit}}")]).ok().filter(|commit| full_sha(commit));
    here().or_else(|| {
        let common = git(&["rev-parse", "--path-format=absolute", "--git-common-dir"]).ok()?;
        let main = std::path::Path::new(&common).parent()?;
        (full_sha(named) && north_trusted_mirror::fetch_lane_head(main, &std::env::current_dir().ok()?, named).is_ok()).then(here).flatten()
    })
}

#[cfg(test)]
#[path = "review_accept_tests.rs"]
mod tests;
