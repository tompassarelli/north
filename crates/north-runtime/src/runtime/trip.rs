//! A Buck2 trip (north#1115): the record of a red run of main's Buck2 workflow, written by the screen on the host that settles the
//! fleet (main_runs.rs) when the newest completed run on a head's ancestry is red. The trip holds the promotion of that head and of
//! every later head (review_hold.rs `Screen`) until `north runtime trip clear COMMIT --why TEXT` clears it, the clear is kept with
//! who cleared it and why, or a newer completed run on that ancestry is green. A record that names no run is the lander's
//! (north-landing `trip`) and holds until cleared.

use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const DIR: &str = "safe-push/check-timings/buck2-after-landing";
const TRIP: &str = "trip";
const CLEARED: &str = "cleared";
const RUN: &str = "run";
const REVERT_ASKED: &str = "revert-asked";
const REQUESTS: &str = "refs/heads/claude/land-revert-";

/// Seconds a trip holds before the sheriff is asked to revert the commit it names, unless a fix lands first and its run turns main green (north#1508).
pub(crate) const REVERT_AFTER_SECONDS: u64 = 300;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Clearing {
    pub by: String,
    pub why: String,
    pub at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Trip {
    pub head: String,
    pub run: Option<String>,
    pub cases: Vec<(String, String)>,
}

impl Trip {
    /// Why the promotion of a head that contains the tripped head is refused, naming the trip and the verb that clears it.
    pub(crate) fn refusal(&self) -> String {
        let named = if self.cases.is_empty() {
            format!("the hosted run {} failed", self.run.as_deref().unwrap_or_default())
        } else {
            self.cases.iter().map(|(name, case)| format!("{case} in {name}")).collect::<Vec<_>>().join(", ")
        };
        format!("the Buck2 trip on {} holds it: {named}; `north runtime trip clear {} --why TEXT` clears it", self.head, self.head)
    }

    /// The sheriff's revert request for the commit this trip names (north-landing sheriff/request.rs): its cases as failed tests, the commit's parent as the green parent, its tree as the tree the red was read on, and a Live-fix trailer the sheriff's revert carries.
    pub(crate) fn revert_request(&self, parent: &str, tree: &str) -> String {
        let failed: Vec<String> = self.cases.iter().map(|(_, case)| format!("test {case} ... FAILED")).collect();
        format!("Main red at {head}\n\n{}\n\nGreen-parent: {parent}\nMain-red-run: the Buck2 trip on {head}\nMain-red-tree: {tree}\nLive-fix: the Buck2 trip on {head} holds every promotion", failed.join("\n"), head = self.head)
    }
}

fn dir() -> PathBuf {
    let state = std::env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()).map_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state"), PathBuf::from);
    state.join(DIR)
}

fn exists(path: &Path) -> Result<bool, String> {
    path.try_exists().map_err(|error| format!("{} cannot be checked: {error}", path.display()))
}

pub(crate) struct Trips(pub PathBuf);

impl Trips {
    pub(crate) fn here() -> Self {
        Self(dir())
    }

    pub(crate) fn cleared(&self, head: &str) -> Result<bool, String> {
        exists(&self.0.join(format!("{head}.{CLEARED}")))
    }

    /// The trip record of `head`, whether or not it is cleared; a record that cannot be read is an error naming it, since a trip that cannot be read must not let its head through.
    pub(crate) fn read(&self, head: &str) -> Result<Option<Trip>, String> {
        let path = self.0.join(format!("{head}.{TRIP}"));
        if !exists(&path)? {
            return Ok(None);
        }
        let text = fs::read_to_string(&path).map_err(|error| format!("{} cannot be read: {error}", path.display()))?;
        let mut run = None;
        let mut cases = Vec::new();
        for line in text.lines() {
            let (name, rest) = line.split_once('\t').ok_or_else(|| format!("{} has a line that is no crate and case: {line:?}", path.display()))?;
            if name == RUN {
                run = Some(rest.to_owned());
            } else {
                cases.push((name.to_owned(), rest.to_owned()));
            }
        }
        Ok(Some(Trip { head: head.to_owned(), run, cases }))
    }

    /// Writes the trip record of `trip`: the run that concluded red, then its failed cases, which may be none when its log cannot be read.
    pub(crate) fn write(&self, trip: &Trip) -> Result<(), String> {
        let path = self.0.join(format!("{}.{TRIP}", trip.head));
        fs::create_dir_all(&self.0).map_err(|error| format!("{} cannot be created: {error}", self.0.display()))?;
        let mut file = OpenOptions::new().write(true).create_new(true).open(&path).map_err(|error| format!("{} cannot be created: {error}", path.display()))?;
        let mut text = trip.run.as_ref().map_or_else(String::new, |run| format!("{RUN}\t{run}\n"));
        for (name, case) in &trip.cases {
            text.push_str(&format!("{name}\t{case}\n"));
        }
        file.write_all(text.as_bytes()).map_err(|error| format!("{} cannot be written: {error}", path.display()))
    }

    /// The open trips: a `HEAD.trip` record that no `HEAD.cleared` answers.
    pub(crate) fn open(&self) -> Result<Vec<Trip>, String> {
        let entries = match fs::read_dir(&self.0) {
            Ok(entries) => entries,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(format!("the trips in {} cannot be listed: {error}", self.0.display())),
        };
        let mut trips = Vec::new();
        for entry in entries {
            let path = entry.map_err(|error| format!("an entry of {} cannot be read: {error}", self.0.display()))?.path();
            let Some(head) = path.file_name().and_then(|name| name.to_str()).and_then(|name| name.strip_suffix(&format!(".{TRIP}"))) else { continue };
            if self.cleared(head)? {
                continue;
            }
            if let Some(trip) = self.read(head)? {
                trips.push(trip);
            }
        }
        trips.sort_by(|left, right| left.head.cmp(&right.head));
        Ok(trips)
    }

    /// Asks once, through `ask`, for the revert of the commit `trip` names after the trip has stood REVERT_AFTER_SECONDS; true when it asked now.
    pub(crate) fn revert_when_due(&self, trip: &Trip, now: u64, ask: &dyn Fn(&Trip) -> Result<(), String>) -> Result<bool, String> {
        let asked = self.0.join(format!("{}.{REVERT_ASKED}", trip.head));
        if exists(&asked)? {
            return Ok(false);
        }
        let record = self.0.join(format!("{}.{TRIP}", trip.head));
        let written = fs::metadata(&record).and_then(|meta| meta.modified()).map_err(|error| format!("{} has no time: {error}", record.display()))?;
        let at = written.duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_secs());
        if now < at + REVERT_AFTER_SECONDS {
            return Ok(false);
        }
        ask(trip)?;
        fs::write(&asked, "").map_err(|error| format!("{} cannot be written: {error}", asked.display()))?;
        Ok(true)
    }

    /// Clears the open trip on `head`, recording who cleared it and why.
    pub(crate) fn clear(&self, head: &str, by: &str, why: &str, at: u64) -> Result<String, String> {
        if !exists(&self.0.join(format!("{head}.{TRIP}")))? {
            return Err(format!("no Buck2 trip stands on {head}"));
        }
        let cleared = self.0.join(format!("{head}.{CLEARED}"));
        if exists(&cleared)? {
            return Err(format!("the Buck2 trip on {head} is already cleared"));
        }
        let text = serde_json::to_string(&Clearing { by: by.to_owned(), why: why.to_owned(), at }).map_err(|error| error.to_string())?;
        fs::write(&cleared, text).map_err(|error| format!("{} cannot be written: {error}", cleared.display()))?;
        Ok(format!("north runtime trip: {by} cleared the Buck2 trip on {head}: {why}"))
    }
}

/// Files the sheriff's revert request for `trip` on origin from `checkout`, the request north-ci's sheriff and the lander's red after-landing run file, which the lander answers through its landing queue.
pub(crate) fn ask_revert(checkout: &Path, trip: &Trip) -> Result<(), String> {
    let git = |arguments: &[&str]| -> Result<String, String> {
        let output = std::process::Command::new("git").arg("-C").arg(checkout).args(arguments).env("SAFE_PUSH_ACTIVE", "1").output().map_err(|error| error.to_string())?;
        if output.status.success() { Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned()) } else { Err(String::from_utf8_lossy(&output.stderr).trim().to_owned()) }
    };
    let parent = git(&["rev-parse", &format!("{}^", trip.head)])?;
    let tree = git(&["rev-parse", &format!("{}^{{tree}}", trip.head)])?;
    let request = git(&["commit-tree", &tree, "-p", &trip.head, "-m", &trip.revert_request(&parent, &tree)])?;
    git(&["push", "-q", "--no-verify", "origin", &format!("{request}:{REQUESTS}{}", trip.head)]).map(drop)
}
