//! A reviewer's hold on a commit (north#1022): a record in the verdict store, which no worker domain writes, that keeps every head containing the commit, or a landing of the lane that holds it, from going live. `north runtime order` refuses such a head and so do the slot and `Live-fix:` promotions, each naming the commit and the reviewer. `north runtime hold` places and lifts a hold for the fleet session the caller's cgroup scope names, read by the broker for a caller inside a worker sandbox and by the verb itself outside one, never from a variable or a trailer; only that session lifts it by hand, and the record keeps who lifted it and why. A hold placed with `--until LANE` names the lane whose landing lifts it: the first screen of a head containing a landing of that lane that changes a file the held commit changes and whose patch carries the placer's `north runtime accept` lifts it in its record, reading both only in the trusted mirror beside the checkout, which no worker writes (north#1540). A session in the coordinator role (a record naming the role `coordinator`) is refused both, since the client window it also runs in runs in no session's scope (north#1281). Every read for a standing hold fails closed: a store, a ledger or a git answer that cannot be read refuses the head, while a lifted hold that cannot be screened is passed over. Every screen asks git in the trusted mirror, never in the north checkout, whose refs and replace refs a worker writes (south#138); a held commit the mirror lacks, such as a lane commit that has not landed, is in no head the mirror holds, since a fetch brings every ancestor of what it fetches. The switch `review_holds` in harness/defaults.toml turns every screen off.

use std::fs;
use std::path::{Path, PathBuf};

use north_base::platform::harness::{DEFAULTS, document};
use north_base::platform::review_accept::{Accepts, PATCH_DIFF, patch_id_of};
use north_runtime_follow::layout;
use north_runtime_records::after_landing::{self, AfterLanding, Ledger};
use north_worker_sandbox::hold_ledger::{Hold, Holds, Lift, PUSH_LEDGER, REPOSITORY, SHOWN_FOR, now, state_home};

use super::main_runs::MainRuns;
use super::replay::{self, Replay};
use super::trip::{Trip, Trips};

const SWITCH: &str = "review_holds";

/// Whether the reviewers' holds screen the order and the promotions, read from `review_holds`; only `off` turns them off.
pub(crate) fn enabled() -> bool {
    document(DEFAULTS)[SWITCH]["value"].as_str() != Some("off")
}

type Contains = Box<dyn Fn(&str, &str) -> Result<bool, String>>;
pub(crate) type Revert = Box<dyn Fn(&Trip) -> Result<(), String>>;
pub(crate) type Landed = Box<dyn Fn(&str) -> Result<Vec<String>, String>>;
pub(crate) type Answered = Box<dyn Fn(&Hold, &str) -> bool>;

/// The facts a promotion screens a head by: the holds, `contains(commit, head)` saying whether `head` is `commit` or descends from it, `landed(lane)` naming the commits on main each landing of a lane made, since the lander squashes a lane into a commit of its own, and `answered(hold, landed)` whether the landing `landed` of a hold's `--until` lane answers that hold, `after_landing(head)` the verdict of `head`'s after-landing run (after_landing.rs), none while that read is off,, `revert(trip)` asking the sheriff to revert the commit a trip names (trip.rs), and `replay(trips, asked)` reading a red hosted run's new cases again on a build host (replay.rs).
pub(crate) struct Screen {
    pub holds: Holds,
    pub contains: Contains,
    pub landed: Landed,
    pub answered: Answered,
    pub after_landing: Option<AfterLanding>,
    pub trips: Trips,
    pub runs: MainRuns,
    pub revert: Revert,
    pub replay: Replay,
}

fn unscreened(head: &str, why: String) -> String {
    format!("{} is not screened, so it does not go live: {why}", layout::short(head))
}

impl Screen {
    /// The screen on this host: its holds, git's answer in the trusted mirror `store` (update::trusted_store), the landings of this host's push ledger and the open Buck2 trips.
    pub(crate) fn here(store: &Path) -> Self {
        let ledger = state_home().join(PUSH_LEDGER);
        let after = Ledger::here(store);
        Self { holds: Holds::here(), contains: contains_in(store.to_path_buf()), landed: landed_in(ledger), answered: answered_in(store.to_path_buf(), Accepts::here()), after_landing: after_landing::enabled().then(|| Box::new(move |head: &str| after.landing(head, now())) as AfterLanding), trips: Trips::here(), runs: MainRuns::main(), revert: { let store = store.to_path_buf(); Box::new(move |trip| super::trip::ask_revert(&store, trip)) }, replay: replay::here() }
    }

    /// Why `head` may not go live through an order that promotes the hosts `hosts`: a standing hold that `head` holds and that names no host or one of them, or a fact that cannot be read; else a line for each hold `head` holds that was lifted within the last day, naming who lifted it and why.
    pub(crate) fn order(&self, head: &str, hosts: &[&str], now: u64) -> Result<Vec<String>, String> {
        self.screened(head, &|host| host.is_none_or(|host| hosts.contains(&host)), now)
    }

    /// Why the runtime `head`, already live on the host that settles the fleet, may not be shipped to the hosts `hosts` or followed by its lander link: its after-landing verdict and holds as `order` reads them, but no trip, since a red run of main that ends after the promotion took `head` live refuses the next promotion and asks for its revert instead of leaving every other host and the lander link on an older runtime than the one live here (north#1540).
    pub(crate) fn live(&self, head: &str, hosts: &[&str], now: u64) -> Result<Vec<String>, String> {
        self.passed(head)?;
        self.held(head, &|host| host.is_none_or(|host| hosts.contains(&host)), now)
    }

    fn screened(&self, head: &str, applies: &dyn Fn(Option<&str>) -> bool, now: u64) -> Result<Vec<String>, String> {
        self.passed(head)?;
        self.tripped(head, now)?;
        self.held(head, applies, now)
    }

    fn held(&self, head: &str, applies: &dyn Fn(Option<&str>) -> bool, now: u64) -> Result<Vec<String>, String> {
        if !enabled() {
            return Ok(Vec::new());
        }
        let unread = |why: String| unscreened(head, why);
        let mut held = Vec::new();
        for hold in self.holds.all().map_err(unread)? {
            let hold = self.cured(hold, head, now).map_err(unread)?;
            match self.holds_in(&hold, head) {
                Ok(true) => held.push(hold),
                Ok(false) => {}
                Err(_) if hold.lifted.is_some() => {}
                Err(why) => return Err(unread(why)),
            }
        }
        let standing: Vec<String> = held.iter().filter(|hold| hold.lifted.is_none() && applies(hold.host.as_deref())).map(refusal).collect();
        if !standing.is_empty() {
            return Err(format!("{} contains {}", layout::short(head), standing.join("; ")));
        }
        Ok(held.iter().filter_map(|hold| hold.lifted.as_ref().filter(|lift| now.saturating_sub(lift.at) < SHOWN_FOR).map(|lift| format!("{} was held by {} ({}); {} lifted it: {}", hold.commit, hold.session, hold.why, lift.by, lift.why))).collect())
    }

    /// Refuses `head` when its own after-landing run did not end green; nothing while the after-landing read is off.
    fn passed(&self, head: &str) -> Result<(), String> {
        let Some(after_landing) = &self.after_landing else { return Ok(()) };
        match after_landing(head).refusal() {
            Some(why) => Err(format!("{} has not ended its after-landing run green: {why}", layout::short(head))),
            None => Ok(()),
        }
    }

    /// Refuses `head` when a red hosted run on its ancestry since its last green run trips the head it ran on and that head is `head`, or when a lander's trip record (one no run names) stands on `head` itself (trip.rs); a trip on an ancestor asks the sheriff to revert its commit once it has held REVERT_AFTER_SECONDS but refuses no later head, which goes live and is verified after (north#1537); a run list or git that cannot say refuses the head.
    fn tripped(&self, head: &str, now: u64) -> Result<(), String> {
        let unread = |why: String| unscreened(head, why);
        self.hosted_trip(head, now).map_err(unread)??;
        self.lander_trip(head, now).map_err(unread)?
    }

    /// The refusal, if any, of the first lander's trip record (one no run names) on `head` or its ancestry.
    fn lander_trip(&self, head: &str, now: u64) -> Result<Result<(), String>, String> {
        for trip in self.trips.open()? {
            if trip.run.is_none() && (trip.head == head || (self.contains)(&trip.head, head)?) {
                let stood = self.stands(&trip, head, now)?;
                if stood.is_err() {
                    return Ok(stood);
                }
            }
        }
        Ok(Ok(()))
    }

    /// Asks for the revert of a trip on `head` or its ancestry once due, then refuses `head` only when the trip is its own; a failed ask under a head the trip does not refuse is paged and holds nothing.
    fn stands(&self, trip: &Trip, head: &str, now: u64) -> Result<Result<(), String>, String> {
        let refused = trip.head == head;
        match self.trips.revert_when_due(trip, now, &self.revert) {
            Err(why) if !refused => (self.runs.alert)(&format!("north runtime: the revert of tripped {} could not be asked: {why}", trip.head)),
            asked => {
                asked?;
            }
        }
        Ok(match refused {
            true => Err(format!("{} contains {}", layout::short(head), trip.refusal())),
            false => Ok(()),
        })
    }

    /// The first standing hold naming `host` or no host that keeps `head` off `host`, as its refusal reads; a store or git answer that cannot be read names none, so the host counts as late.
    pub(crate) fn holding(&self, head: &str, host: &str) -> Option<String> {
        let mut standing = self.holds.all().ok()?.into_iter().filter(|hold| hold.lifted.is_none() && hold.host.as_deref().is_none_or(|named| named == host));
        standing.find(|hold| self.holds_in(hold, head) == Ok(true)).map(|hold| refusal(&hold))
    }

    /// `hold`, lifted in its record when it stands, names the lane whose landing lifts it, and `head` contains a landing of that lane that answers it.
    fn cured(&self, mut hold: Hold, head: &str, now: u64) -> Result<Hold, String> {
        let Some(until) = hold.until.clone().filter(|_| hold.lifted.is_none()) else { return Ok(hold) };
        for landed in (self.landed)(&until)? {
            if (self.contains)(&landed, head)? && (self.answered)(&hold, &landed) {
                hold.lifted = Some(Lift { by: format!("the landing of {until}"), why: format!("{until} landed as {landed}, which {} contains, changes a file {} changes and carries the patch {} accepted", layout::short(head), hold.commit, hold.session), at: now });
                self.holds.write(&hold)?;
                break;
            }
        }
        Ok(hold)
    }

    /// Whether `head` holds `hold`: it contains the held commit, or a commit a landing of the hold's lane made.
    fn holds_in(&self, hold: &Hold, head: &str) -> Result<bool, String> {
        if (self.contains)(&hold.commit, head)? {
            return Ok(true);
        }
        for landed in (self.landed)(&hold.lane)? {
            if (self.contains)(&landed, head)? {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

fn refusal(hold: &Hold) -> String {
    let on = hold.host.as_deref().map_or(String::new(), |host| format!(" on {host}"));
    format!("{} of the lane {}, held{on} by {}: {}; only {} lifts it (north runtime hold --lift {} --why TEXT)", hold.commit, hold.lane, hold.session, hold.why, hold.session, hold.commit)
}

/// Whether `head` is `commit` or descends from it, as git in `store` answers: a commit the store lacks or an exit 1 is no, and any other failure is an error, never a no.
pub(crate) fn contains_in(store: PathBuf) -> Contains {
    Box::new(move |commit, head| contained(&store, commit, head))
}

/// Whether a landing answers a hold, read only in the trusted mirror's `store` (north_trusted_mirror), since a worker writes the checkout's store and could plant a replace object there that forges either answer: the landing changes a file the held commit changes, and the patch it made over its parent carries an accept in `accepts` by the session that placed the hold. A landing or held commit the mirror lacks, or an answer that cannot be read, answers nothing, so the hold stands until its placer lifts it.
pub(crate) fn answered_in(store: PathBuf, accepts: Accepts) -> Answered {
    Box::new(move |hold, landed| {
        let git = |arguments: &[&str]| north_trusted_mirror::command(&store).args(arguments).output().ok().filter(|output| output.status.success()).map(|output| output.stdout);
        let files = |commit: &str| git(&["diff-tree", "-r", "-z", "--root", "--no-commit-id", "--name-only", "--no-renames", commit]).map(|names| names.split(|byte| *byte == 0).filter(|name| !name.is_empty()).map(<[u8]>::to_vec).collect::<std::collections::BTreeSet<_>>());
        let (Some(held), Some(changed)) = (files(&hold.commit), files(landed)) else { return false };
        let parent = format!("{landed}^");
        let patch = || git(&[&PATCH_DIFF[..], &[parent.as_str(), landed]].concat()).and_then(|diff| patch_id_of(&diff, &parent, landed).ok());
        !held.is_disjoint(&changed) && patch().is_some_and(|patch| accepts.sessions(&patch).is_ok_and(|sessions| sessions.contains(&hold.session)))
    })
}

fn contained(store: &Path, commit: &str, head: &str) -> Result<bool, String> {
    let object = format!("{commit}^{{commit}}");
    let present = north_base::platform::process::run("git", Some(store), &["rev-parse", "--verify", "--quiet", object.as_str()]).map_err(|error| format!("git rev-parse: {error}"))?;
    if present.status.code() == Some(1) {
        return Ok(false);
    }
    let output = north_base::platform::process::run("git", Some(store), &["merge-base", "--is-ancestor", commit, head]).map_err(|error| format!("git merge-base: {error}"))?;
    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        code => Err(format!("git merge-base --is-ancestor {commit} {head} in {} ended {code:?}: {}", store.display(), String::from_utf8_lossy(&output.stderr).trim())),
    }
}

/// The landings of each lane, read from the push ledger at `ledger`, which the lander on the host that settles the fleet writes as it lands: a ledger that does not exist is missing, never empty, since an empty one would miss a landing the lander squashed.
pub(crate) fn landed_in(ledger: PathBuf) -> Landed {
    Box::new(move |lane| fs::read_to_string(&ledger).map(|text| landings(&text, lane)).map_err(|error| format!("the push ledger {} cannot be read, so no landing of {lane} can be told: {error}", ledger.display())))
}

/// The commits on main each landing of `lane` in north made, read from the push ledger's rows (time, repository, branch, lane tip, landed commit, session).
fn landings(ledger: &str, lane: &str) -> Vec<String> {
    ledger.lines().map(|row| row.split('\t').collect::<Vec<_>>()).filter(|fields| fields.get(1) == Some(&REPOSITORY) && fields.get(2) == Some(&lane)).filter_map(|fields| fields.get(4).filter(|landed| !landed.is_empty()).map(|landed| (*landed).to_owned())).collect()
}

mod hosted;
#[cfg(test)]
mod tests;
