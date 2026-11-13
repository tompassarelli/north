//! A reviewer's hold on a commit (north#1022): a record in the verdict store, which no worker domain writes, that keeps every head containing the commit, or a landing of the lane that holds it, from going live. `north runtime order` refuses such a head and so do the slot and `Live-fix:` promotions, each naming the commit and the reviewer. `north runtime hold` places and lifts a hold for the fleet session the caller's cgroup scope names, read by the broker for a caller inside a worker sandbox and by the verb itself outside one, never from a variable or a trailer; only that session or the fleet's coordinator (a session whose record names the role `proxy`) lifts it, and the record keeps who lifted it and why. Every read for a standing hold fails closed: a store, a ledger or a git answer that cannot be read refuses the head, while a lifted hold that cannot be screened, such as one on a commit the checkout lacks, is passed over. The broker places no hold on a commit the north checkout lacks. The switch `review_holds` in harness/defaults.toml turns every screen off.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use north_base::platform::finish_record::verdict_store;
use north_base::platform::harness::{DEFAULTS, document};
use serde::{Deserialize, Serialize};

use super::after_landing::{self, AfterLanding, Landing, Ledger};
use super::failure::{Failure, failed};
use super::layout;
use super::main_runs::{self, MainRun, MainRuns};
use super::trip::{Trip, Trips};

pub(crate) const VERB: &str = "review-hold";
pub(crate) const COORDINATOR: &str = "proxy";
pub(crate) const REVIEWER: &str = "reviewer";
const DIR: &str = "holds";
const SWITCH: &str = "review_holds";
const REPOSITORY: &str = "north";
const DESTINATION: &str = "main";
const PUSH_LEDGER: &str = "safe-push/pushes.tsv";
const WHY_LIMIT: usize = 2048;
const SHOWN_FOR: u64 = 24 * 60 * 60;
const USAGE: &str = "usage: north runtime hold COMMIT [--host HOST] --why TEXT | north runtime hold --lift COMMIT --why TEXT  place a reviewer's hold on COMMIT and the lane that holds it, which keeps `north runtime order` and the slot and Live-fix promotions from taking live any head that contains COMMIT or a landing of that lane (with --host, only an order that promotes HOST is refused), or lift it; the hold is the calling fleet session's, and only that session or the coordinator lifts it; run it on the host that settles the fleet";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Lift {
    pub by: String,
    pub why: String,
    pub at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Hold {
    pub commit: String,
    pub lane: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    pub session: String,
    pub why: String,
    pub at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifted: Option<Lift>,
}

/// The fleet session that asks, as its cgroup scope names it, and whether its record names it the coordinator or the reviewer role.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Caller {
    pub session: String,
    pub coordinator: bool,
    pub reviewer: bool,
}

/// What `north runtime hold` asks, in the words the broker takes: `review-hold place COMMIT LANE HOST WHY` (HOST empty for every host) or `review-hold lift COMMIT WHY`.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Ask {
    Place { commit: String, lane: String, host: Option<String>, why: String },
    Lift { commit: String, why: String },
}

pub(super) fn full_sha(commit: &str) -> bool {
    matches!(commit.len(), 40 | 64) && commit.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn named(word: &str) -> bool {
    !word.is_empty() && word.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'/'))
}

fn said(why: &str) -> bool {
    !why.trim().is_empty() && why.len() <= WHY_LIMIT
}

impl Ask {
    pub(crate) fn words(&self) -> Vec<String> {
        match self {
            Ask::Place { commit, lane, host, why } => [VERB, "place", commit, lane, host.as_deref().unwrap_or_default(), why].map(str::to_owned).to_vec(),
            Ask::Lift { commit, why } => [VERB, "lift", commit, why].map(str::to_owned).to_vec(),
        }
    }

    /// The hold request in `arguments`: `None` for any other request, and why one in other words is refused.
    pub(crate) fn asked(arguments: &[String]) -> Option<Result<Self, String>> {
        let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
        let [VERB, rest @ ..] = words.as_slice() else { return None };
        let host = |host: &str| Some(host.to_owned()).filter(|host| !host.is_empty());
        Some(match rest {
            ["place", commit, lane, on, why] if full_sha(commit) && named(lane) && (on.is_empty() || named(on)) && said(why) => Ok(Ask::Place { commit: (*commit).to_owned(), lane: (*lane).to_owned(), host: host(on), why: why.trim().to_owned() }),
            ["lift", commit, why] if full_sha(commit) && said(why) => Ok(Ask::Lift { commit: (*commit).to_owned(), why: why.trim().to_owned() }),
            _ => Err(format!("a hold request is `{VERB} place COMMIT LANE HOST WHY` or `{VERB} lift COMMIT WHY`, with a full commit id and a reason of at most {WHY_LIMIT} bytes")),
        })
    }
}

fn state_home() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()).map_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state"), PathBuf::from)
}

/// The text of `path`, empty when it does not exist yet; any other failure to read it is an error naming it.
fn read_or_empty(path: &Path) -> Result<String, String> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(format!("{} cannot be read: {error}", path.display())),
    }
}

pub(crate) struct Holds(pub PathBuf);

impl Holds {
    pub(crate) fn here() -> Self {
        let environment: Vec<(String, String)> = ["NORTH_VERDICT_CACHE_DIR", "XDG_STATE_HOME"].into_iter().filter_map(|name| Some((name.to_owned(), std::env::var(name).ok()?))).collect();
        Self::of(&environment, &PathBuf::from(std::env::var_os("HOME").unwrap_or_default()))
    }

    pub(crate) fn of(environment: &[(String, String)], home: &Path) -> Self {
        Self(verdict_store(environment, home).join(DIR))
    }

    fn file(hold: &Hold) -> String {
        format!("{}.{}.json", hold.commit, hold.session)
    }

    /// Every hold in the store, or an error naming what could not be read: a store that exists and cannot be listed, or an entry that cannot be read, does not parse as a hold, or is not named for the hold it holds.
    pub(crate) fn all(&self) -> Result<Vec<Hold>, String> {
        let entries = match fs::read_dir(&self.0) {
            Ok(entries) => entries,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(format!("the holds in {} cannot be listed: {error}", self.0.display())),
        };
        let mut holds = Vec::new();
        for entry in entries {
            let path = entry.map_err(|error| format!("an entry of {} cannot be read: {error}", self.0.display()))?.path();
            let text = fs::read_to_string(&path).map_err(|error| format!("{} cannot be read: {error}", path.display()))?;
            let hold = serde_json::from_str::<Hold>(&text).map_err(|error| format!("{} is no hold: {error}", path.display()))?;
            if path.file_name().is_none_or(|name| *name != *Self::file(&hold)) {
                return Err(format!("{} holds {} for {}, which is not the hold its name says", path.display(), hold.commit, hold.session));
            }
            holds.push(hold);
        }
        holds.sort_by_key(|hold| hold.at);
        Ok(holds)
    }

    fn write(&self, hold: &Hold) -> Result<(), String> {
        let failure = |error: std::io::Error| format!("the hold on {} in {}: {error}", hold.commit, self.0.display());
        fs::create_dir_all(&self.0).map_err(failure)?;
        let pending = tempfile::NamedTempFile::new_in(&self.0).map_err(failure)?;
        fs::write(pending.path(), serde_json::to_string(hold).map_err(|error| error.to_string())?).map_err(failure)?;
        pending.persist(self.0.join(Self::file(hold))).map(|_| ()).map_err(|error| failure(error.error))
    }

    fn prune(&self, holds: &[Hold], now: u64) {
        for hold in holds.iter().filter(|hold| hold.lifted.as_ref().is_some_and(|lift| now.saturating_sub(lift.at) >= SHOWN_FOR)) {
            let _ = fs::remove_file(self.0.join(Self::file(hold)));
        }
    }

    /// Places or lifts what `ask` names for `caller` at `now`, and the line that says what was done.
    pub(crate) fn serve(&self, ask: Ask, caller: &Caller, now: u64) -> Result<String, String> {
        if !named(&caller.session) || caller.session.contains('/') {
            return Err(format!("{:?} names no fleet session", caller.session));
        }
        let holds = self.all()?;
        self.prune(&holds, now);
        match ask {
            Ask::Place { commit, lane, host, why } => {
                let on = host.as_deref().map_or(String::new(), |host| format!(" on {host}"));
                self.write(&Hold { commit: commit.clone(), lane: lane.clone(), host, session: caller.session.clone(), why, at: now, lifted: None })?;
                Ok(format!("north runtime hold: {} holds {commit} of the lane {lane}{on}; no head containing it or a landing of {lane} goes live until {} or the coordinator lifts it", caller.session, caller.session))
            }
            Ask::Lift { commit, why } => self.lift(holds, &commit, &why, caller, now),
        }
    }

    fn lift(&self, holds: Vec<Hold>, commit: &str, why: &str, caller: &Caller, now: u64) -> Result<String, String> {
        let standing: Vec<Hold> = holds.into_iter().filter(|hold| hold.commit == commit && hold.lifted.is_none()).collect();
        if standing.is_empty() {
            return Err(format!("no hold stands on {commit}"));
        }
        let (own, others): (Vec<Hold>, Vec<Hold>) = standing.into_iter().partition(|hold| caller.coordinator || hold.session == caller.session);
        if own.is_empty() {
            let placers: Vec<&str> = others.iter().map(|hold| hold.session.as_str()).collect();
            return Err(format!("{commit} is held by {}; only {} or the coordinator lifts it, not {}", placers.join(", "), placers.join(" or "), caller.session));
        }
        for mut hold in own.iter().cloned() {
            hold.lifted = Some(Lift { by: caller.session.clone(), why: why.to_owned(), at: now });
            self.write(&hold)?;
        }
        let placers: Vec<&str> = own.iter().map(|hold| hold.session.as_str()).collect();
        Ok(format!("north runtime hold: {} lifted the hold of {} on {commit}: {why}", caller.session, placers.join(", ")))
    }
}

/// Whether the reviewers' holds screen the order and the promotions, read from `review_holds`; only `off` turns them off.
pub(crate) fn enabled() -> bool {
    document(DEFAULTS)[SWITCH]["value"].as_str() != Some("off")
}

type Contains = Box<dyn Fn(&str, &str) -> Result<bool, String>>;
pub(crate) type Landed = Box<dyn Fn(&str) -> Result<Vec<String>, String>>;

/// The facts a promotion screens a head by: the holds, `contains(commit, head)` saying whether `head` is `commit` or descends from it, `landed(lane)` naming the commits on main each landing of a lane made, since the lander squashes a lane into a commit of its own, and `after_landing(head)` the verdict of `head`'s after-landing run (after_landing.rs).
pub(crate) struct Screen {
    pub holds: Holds,
    pub contains: Contains,
    pub landed: Landed,
    pub after_landing: AfterLanding,
    pub trips: Trips,
    pub runs: MainRuns,
}

fn unscreened(head: &str, why: String) -> String {
    format!("{} is not screened, so it does not go live: {why}", layout::short(head))
}

impl Screen {
    /// The screen on this host: its holds, git's answer in `checkout`, the landings of this host's push ledger and the open Buck2 trips.
    pub(crate) fn here(checkout: &Path) -> Self {
        let checkout = checkout.to_path_buf();
        let ledger = state_home().join(PUSH_LEDGER);
        let after = Ledger::here(&checkout);
        Self { holds: Holds::here(), contains: Box::new(move |commit, head| contained(&checkout, commit, head)), landed: landed_in(ledger), after_landing: if after_landing::enabled() { Box::new(move |head| after.landing(head, now())) } else { Box::new(|_| Landing::Green) }, trips: Trips::here(), runs: MainRuns::main() }
    }

    /// Why `head` may not go live through an order that promotes the hosts `hosts`: a standing hold that `head` holds and that names no host or one of them, or a fact that cannot be read; else a line for each hold `head` holds that was lifted within the last day, naming who lifted it and why.
    pub(crate) fn order(&self, head: &str, hosts: &[&str], now: u64) -> Result<Vec<String>, String> {
        self.screened(head, &|host| host.is_none_or(|host| hosts.contains(&host)), now)
    }

    /// The slot and Live-fix promotions ship to every host after the settling host, so a standing hold naming any host refuses them.
    pub(crate) fn promotion(&self, head: &str, now: u64) -> Result<Vec<String>, String> {
        self.screened(head, &|_| true, now)
    }

    fn screened(&self, head: &str, applies: &dyn Fn(Option<&str>) -> bool, now: u64) -> Result<Vec<String>, String> {
        if let Some(why) = (self.after_landing)(head).refusal() {
            return Err(format!("{} has not ended its after-landing run green: {why}", layout::short(head)));
        }
        self.tripped(head)?;
        if !enabled() {
            return Ok(Vec::new());
        }
        let unread = |why: String| unscreened(head, why);
        let mut held = Vec::new();
        for hold in self.holds.all().map_err(unread)? {
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

    /// The newest completed run of main whose head is on `head`'s ancestry, the one that decides whether `head` is tripped; runs are read page by page until one is found or the list ends.
    fn newest_run(&self, head: &str) -> Result<Option<MainRun>, String> {
        let mut number = 1;
        while let Some(runs) = (self.runs.page)(number)? {
            for run in runs {
                if (self.contains)(&run.head, head)? {
                    return Ok(Some(run));
                }
            }
            number += 1;
        }
        Ok(None)
    }

    /// Refuses `head` when the newest completed Buck2 run on its ancestry is red and no clear answers its trip, or when a lander's trip record (one no run names) stands on an ancestor (trip.rs); a run list or git that cannot say refuses the head.
    fn tripped(&self, head: &str) -> Result<(), String> {
        let unread = |why: String| unscreened(head, why);
        let newest = self.newest_run(head).map_err(unread)?.ok_or_else(|| unread("no completed run of main is on its ancestry".to_owned()))?;
        if newest.red && !self.trips.cleared(&newest.head).map_err(unread)? {
            let trip = match self.trips.read(&newest.head).map_err(unread)? {
                Some(trip) => trip,
                None => self.record(&newest).map_err(unread)?,
            };
            return Err(format!("{} contains {}", layout::short(head), trip.refusal()));
        }
        for trip in self.trips.open().map_err(unread)? {
            if trip.run.is_none() && (self.contains)(&trip.head, head).map_err(unread)? {
                return Err(format!("{} contains {}", layout::short(head), trip.refusal()));
            }
        }
        Ok(())
    }

    /// Writes the trip of a red run of main, with its failed cases read from the run's log, and tells the proxy of it.
    fn record(&self, run: &MainRun) -> Result<Trip, String> {
        let cases = (self.runs.log)(&run.id).map(|text| main_runs::failed_cases(&text)).unwrap_or_default();
        let trip = Trip { head: run.head.clone(), run: Some(run.id.clone()), cases };
        self.trips.write(&trip)?;
        (self.runs.alert)(&format!("north runtime: {}", trip.refusal()));
        Ok(trip)
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
    format!("{} of the lane {}, held{on} by {}: {}; only {} or the coordinator lifts it (north runtime hold --lift {} --why TEXT)", hold.commit, hold.lane, hold.session, hold.why, hold.session, hold.commit)
}

/// Refuses a hold on `commit` unless `checkout` holds it as a commit, since every screen asks git there about each hold and one on a commit git cannot find refuses every head.
pub(crate) fn in_checkout(checkout: &Path, commit: &str) -> Result<(), String> {
    let object = format!("{commit}^{{commit}}");
    let output = crate::platform::process::run("git", Some(checkout), &["cat-file", "-e", object.as_str()]).map_err(|error| format!("git cat-file: {error}"))?;
    match output.status.success() {
        true => Ok(()),
        false => Err(format!("{commit} is no commit in the north checkout {}, and a hold on it would refuse every head the order and the promotions screen: {}", checkout.display(), String::from_utf8_lossy(&output.stderr).trim())),
    }
}

/// The north checkout the order and the promotions screen heads in.
pub(crate) fn checkout() -> PathBuf {
    super::update::checkout()
}

/// Whether `head` is `commit` or descends from it in `checkout`: git's exit 1 is no, and any other failure is an error, never a no.
fn contained(checkout: &Path, commit: &str, head: &str) -> Result<bool, String> {
    let output = crate::platform::process::run("git", Some(checkout), &["merge-base", "--is-ancestor", commit, head]).map_err(|error| format!("git merge-base: {error}"))?;
    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        code => Err(format!("git merge-base --is-ancestor {commit} {head} in {} ended {code:?}: {}", checkout.display(), String::from_utf8_lossy(&output.stderr).trim())),
    }
}

/// The landings of each lane, read from the push ledger at `ledger`, which the lander on the host that settles the fleet writes as it lands: a ledger that does not exist is missing, never empty, since an empty one would miss a landing the lander squashed.
pub(crate) fn landed_in(ledger: PathBuf) -> Landed {
    Box::new(move |lane| fs::read_to_string(&ledger).map(|text| landings(&text, lane)).map_err(|error| format!("the push ledger {} cannot be read, so no landing of {lane} can be told: {error}", ledger.display())))
}

/// The commits on main each landing of `lane` in north made, read from the push ledger's rows (time, repository, branch, lane tip, landed commit, session).
fn landings(ledger: &str, lane: &str) -> Vec<String> {
    ledger.lines().map(|row| row.split('\t').collect::<Vec<_>>()).filter(|fields| fields.get(1) == Some(&REPOSITORY) && fields.get(2) == Some(&lane)).filter_map(|fields| fields.get(4).map(|landed| (*landed).to_owned())).collect()
}

/// The lane that holds `commit`: the branch of the push ledger's row whose lane tip or landed commit it is, else the one branch of `branches` (local branches containing it, the destination aside) that holds it; a commit no lane or more than one holds is refused, since its hold would match no landing.
pub(crate) fn lane_of(commit: &str, ledger: &str, branches: &[String]) -> Result<String, String> {
    let pushed = ledger.lines().map(|row| row.split('\t').collect::<Vec<_>>()).filter(|fields| fields.get(1) == Some(&REPOSITORY) && (fields.get(3) == Some(&commit) || fields.get(4) == Some(&commit))).filter_map(|fields| fields.get(2).map(|lane| (*lane).to_owned())).next_back();
    let held: Vec<&String> = branches.iter().filter(|branch| *branch != DESTINATION).collect();
    let lane = pushed.or_else(|| match held.as_slice() {
        [only] => Some((*only).clone()),
        _ => None,
    });
    match lane {
        Some(lane) if named(&lane) => Ok(lane),
        _ => Err(format!("{commit} is in no one lane: no landing names it and the branches holding it here are [{}]; hold a commit of the lane under review", held.iter().map(|branch| branch.as_str()).collect::<Vec<_>>().join(", "))),
    }
}

pub(crate) fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_secs())
}

/// `north runtime hold`, as `command` below, with its exit code; a failure is printed as the runtime verbs print theirs.
pub(crate) fn run(flags: &[String], settles: bool, serve: impl FnOnce(Ask) -> Result<String, String>) -> u8 {
    command(flags, settles, serve).map_or_else(
        |failure| {
            eprintln!("north runtime hold: {}", failure.message);
            failure.code
        },
        |()| 0,
    )
}

pub(super) fn git(arguments: &[&str]) -> Result<String, Failure> {
    let output = crate::platform::process::run("git", None, arguments).map_err(|error| failed(1, format!("git {}: {error}", arguments.join(" "))))?;
    match output.status.success() {
        true => Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned()),
        false => Err(failed(1, format!("git {}: {}", arguments.join(" "), String::from_utf8_lossy(&output.stderr).trim()))),
    }
}

/// The lane that holds `commit` here, read from this host's push ledger and the local branches containing it.
fn lane_here(commit: &str) -> Result<String, Failure> {
    let ledger = read_or_empty(&state_home().join(PUSH_LEDGER)).map_err(|why| failed(1, why))?;
    let branches = git(&["for-each-ref", "--contains", commit, "--format=%(refname:short)", "refs/heads"])?;
    lane_of(commit, &ledger, &branches.lines().map(str::to_owned).collect::<Vec<_>>()).map_err(|why| failed(1, why))
}

/// `north runtime hold` on a host that `settles` the fleet: resolves the commit and its lane in the working directory and has `serve` place or lift the hold for the caller's own session (worker_sandbox/hold_client.rs: through the broker inside a worker sandbox and here outside one), printing the line it says.
fn command(flags: &[String], settles: bool, serve: impl FnOnce(Ask) -> Result<String, String>) -> Result<(), Failure> {
    let (named, lift, host, why) = parse(flags)?;
    if !settles {
        return Err(failed(64, format!("the host that settles the fleet reads the holds its order and promotions screen by, and this is not it; place the hold from a session there\n{USAGE}")));
    }
    let commit = git(&["rev-parse", "--verify", &format!("{named}^{{commit}}")]).ok().filter(|commit| full_sha(commit)).ok_or_else(|| failed(1, format!("{named} names no commit here")))?;
    let ask = match lift {
        true => Ask::Lift { commit, why },
        false => Ask::Place { lane: lane_here(&commit)?, commit, host, why },
    };
    let line = serve(ask).map_err(|why| failed(1, why))?;
    if !line.is_empty() {
        println!("{line}");
    }
    Ok(())
}

fn parse(flags: &[String]) -> Result<(String, bool, Option<String>, String), Failure> {
    let words: Vec<&str> = flags.iter().map(String::as_str).collect();
    let parsed = match words.as_slice() {
        ["--lift", commit, "--why", why] => Some((*commit, true, None, *why)),
        [commit, "--why", why] => Some((*commit, false, None, *why)),
        [commit, "--host", host, "--why", why] | [commit, "--why", why, "--host", host] if named(host) => Some((*commit, false, Some((*host).to_owned()), *why)),
        _ => None,
    };
    match parsed {
        Some((commit, lift, host, why)) if !commit.starts_with('-') && said(why) => Ok((commit.to_owned(), lift, host, why.trim().to_owned())),
        _ => Err(failed(64, USAGE)),
    }
}
